use super::{
    ConnectionRx,
    ConnectionTx,
    Error,
    VecExt,
};
use async_mutex::Mutex;
use futures::{
    channel::{
        mpsc,
        oneshot,
    },
    executor,
    stream::{
        StreamExt,
        TryStreamExt,
    },
    AsyncReadExt,
    AsyncWriteExt,
    Future,
    FutureExt,
    Sink,
    SinkExt,
    Stream,
};
use rand::{
    rngs::StdRng,
    Rng,
    SeedableRng,
};
use std::{
    collections::VecDeque,
    sync::Arc,
    task::{
        Poll,
        Waker,
    },
    thread,
};

// This is the bit to set in the first octet of a WebSocket frame
// to indicate that the frame is the final one in a message.
const FIN: u8 = 0x80;

// This is the bit to set in the second octet of a WebSocket frame
// to indicate that the payload of the frame is masked, and that
// a masking key is included.
const MASK: u8 = 0x80;

// This is the opcode for a continuation frame.
const OPCODE_CONTINUATION: u8 = 0x00;

// This is the opcode for a text frame.
const OPCODE_TEXT: u8 = 0x01;

// This is the opcode for a binary frame.
const OPCODE_BINARY: u8 = 0x02;

// This is the opcode for a close frame.
const OPCODE_CLOSE: u8 = 0x08;

// This is the opcode for a ping frame.
const OPCODE_PING: u8 = 0x09;

// This is the opcode for a pong frame.
const OPCODE_PONG: u8 = 0x0A;

// This is the maximum length of a control frame payload.
const MAX_CONTROL_FRAME_DATA_LENGTH: usize = 125;

// This is the initial number of bytes to attempt to read from the
// underlying connection for a WebSocket when accepting a new frame.
const INITIAL_READ_CHUNK_SIZE: usize = 65536;

#[derive(Clone, Copy)]
pub enum MaskDirection {
    Transmit,
    Receive,
}

#[derive(Clone, Copy)]
enum SetFin {
    Yes,
    No,
}

#[derive(Clone, Copy)]
pub enum LastFragment {
    Yes,
    No,
}

pub enum StreamMessage {
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Text(String),
    Binary(Vec<u8>),
    Close {
        code: usize,
        reason: String,
    },
}

pub enum SinkMessage {
    Ping(Vec<u8>),
    Text {
        payload: String,
        last_fragment: LastFragment,
    },
    Binary {
        payload: Vec<u8>,
        last_fragment: LastFragment,
    },
    Close {
        code: usize,
        reason: String,
    },
    CloseNoStatus,
}

struct ReceivedMessages {
    fused: bool,
    queue: VecDeque<StreamMessage>,
    waker: Option<Waker>,
}

impl ReceivedMessages {
    fn push(
        &mut self,
        message: StreamMessage,
    ) {
        self.queue.push_back(message);
        self.wake();
    }

    fn fuse(&mut self) {
        self.fused = true;
        self.wake();
    }

    fn wake(&mut self) {
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

enum MessageInProgress {
    None,
    Text,
    Binary,
}

enum WorkerMessage {
    // This tells the worker thread to terminate.
    Exit,

    // This tells the worker thread to send the given frame through
    // the connection.
    Send {
        set_fin: SetFin,
        opcode: u8,
        data: Vec<u8>,
    },
}

fn close_code_and_reason_from_error(error: Error) -> (usize, String) {
    match error {
        Error::BadFrame(reason) => (1002, String::from(reason)),
        Error::FramePayloadTooLarge => (1009, String::from("frame too large")),
        Error::Utf8 {
            context,
            ..
        } => (1007, format!("invalid UTF-8 encoding in {}", context)),
        Error::ConnectionBroken(source) => {
            (1006, format!("error in underlying connection ({})", source))
        },
        Error::ConnectionClosed => {
            (1006, String::from("underlying connection closed gracefully"))
        },
        _ => (1005, String::new()),
    }
}

struct MessageHandler {
    close_sent: Option<oneshot::Sender<()>>,
    connection_tx: Box<dyn ConnectionTx>,
    mask_direction: MaskDirection,
    rng: StdRng,
}

impl MessageHandler {
    fn new(
        close_sent: oneshot::Sender<()>,
        connection_tx: Box<dyn ConnectionTx>,
        mask_direction: MaskDirection,
    ) -> Self {
        Self {
            close_sent: Some(close_sent),
            connection_tx,
            mask_direction,
            rng: StdRng::from_entropy(),
        }
    }

    async fn handle_message(
        &mut self,
        message: WorkerMessage,
    ) -> Result<(), ()> {
        match message {
            WorkerMessage::Exit => Err(()),

            WorkerMessage::Send {
                set_fin,
                opcode,
                data,
            } => self.send_frame(set_fin, opcode, data).await.map_err(|_| ()),
        }
    }

    async fn send_frame(
        &mut self,
        set_fin: SetFin,
        opcode: u8,
        mut payload: Vec<u8>,
    ) -> Result<(), Error> {
        // Do not send anything after sending "CLOSE".
        if self.close_sent.is_none() {
            return Err(Error::Closed);
        }

        // Pessimistically (meaning we might allocate more than we need)
        // allocate enough memory for the frame in one shot.
        let num_payload_bytes = payload.len();
        let mut frame = Vec::with_capacity(num_payload_bytes + 14);

        // Construct the first byte: FIN flag and opcode.
        frame.push(
            match set_fin {
                SetFin::Yes => FIN,
                SetFin::No => 0,
            } | opcode,
        );

        // Determine what to set for the MASK flag.
        let mask = match self.mask_direction {
            MaskDirection::Receive => 0,
            MaskDirection::Transmit => MASK,
        };

        // Encode the payload length differently, depending on which range
        // the length falls into: either 1, 2, or 8 bytes.  For the 2 and 8
        // byte variants, the first byte is a special marker.  Also, the MASK
        // flag is added to the first byte always.
        match num_payload_bytes {
            0..=125 => {
                #[allow(clippy::cast_possible_truncation)]
                frame.push(num_payload_bytes as u8 | mask);
            },
            126..=65535 => {
                frame.push(126 | mask);
                frame.push_word(num_payload_bytes, 16);
            },
            _ => {
                frame.push(127 | mask);
                frame.push_word(num_payload_bytes, 64);
            },
        }

        // Add the payload.  If masking, we need to generate a random mask
        // and XOR the payload with it.
        if mask == 0 {
            frame.append(&mut payload);
        } else {
            // Generate a random mask (4 bytes).
            let mut masking_key = [0; 4];
            self.rng.fill(&mut masking_key);

            // Include the mask in the frame.
            frame.extend(masking_key.iter());

            // Apply the mask while adding the payload; the mask byte to use
            // rotates around (0, 1, 2, 3, 0, 1, 2, 3, 0, ...).
            // The mask is applied by computing XOR (bit-wise exclusive-or)
            // one mask byte for every payload byte.
            for (i, byte) in payload.iter().enumerate() {
                frame.push(byte ^ masking_key[i % 4]);
            }
        }

        // Push the frame out to the underlying connection.  Note that this
        // may yield until the connection is able to receive all the bytes.
        //
        // If any error occurs, consider the connection to be broken.
        let written = self.connection_tx.write_all(&frame).await;
        if opcode == OPCODE_CLOSE || written.is_err() {
            let _ = self
                .close_sent
                .take()
                .expect("should not send CLOSE twice")
                .send(());
        }
        written.map_err(Error::ConnectionBroken)
    }

    async fn send_close(
        &mut self,
        code: usize,
        reason: String,
    ) {
        let payload = if code == 1005 {
            vec![]
        } else {
            let mut payload = Vec::new();
            payload.push_word(code, 16);
            payload.extend(reason.as_bytes());
            payload
        };
        let _ = self.send_frame(SetFin::Yes, OPCODE_CLOSE, payload).await;
    }
}

async fn receive_bytes(
    connection_rx: &mut dyn ConnectionRx,
    frame_reassembly_buffer: &mut Vec<u8>,
    read_chunk_size: usize,
) -> Result<(), Error> {
    let left_over = frame_reassembly_buffer.len();
    frame_reassembly_buffer.resize(left_over + read_chunk_size, 0);
    let received = connection_rx
        .read(&mut frame_reassembly_buffer[left_over..])
        .await
        .map_err(Error::ConnectionBroken)
        .and_then(|received| match received {
            0 => Err(Error::ConnectionClosed),
            received => Ok(received),
        })?;
    frame_reassembly_buffer.truncate(left_over + received);
    Ok(())
}

enum ReceivedCloseFrame {
    Yes,
    No,
}

struct FrameReceiver<'a> {
    mask_direction: MaskDirection,
    message_handler: &'a Mutex<MessageHandler>,
    message_reassembly_buffer: Vec<u8>,
    message_in_progress: MessageInProgress,
    received_messages: &'a Mutex<ReceivedMessages>,
}

impl<'a> FrameReceiver<'a> {
    fn new(
        mask_direction: MaskDirection,
        message_handler: &'a Mutex<MessageHandler>,
        received_messages: &'a Mutex<ReceivedMessages>,
    ) -> Self {
        Self {
            mask_direction,
            message_handler,
            message_in_progress: MessageInProgress::None,
            message_reassembly_buffer: Vec::new(),
            received_messages,
        }
    }

    async fn receive_frame(
        &mut self,
        frame_reassembly_buffer: &[u8],
        header_length: usize,
        payload_length: usize,
    ) -> Result<ReceivedCloseFrame, Error> {
        // Decode the FIN flag.
        let fin = (frame_reassembly_buffer[0] & FIN) != 0;

        // Decode the reserved bits, and reject the frame if any are set.
        let reserved_bits = (frame_reassembly_buffer[0] >> 4) & 0x07;
        if reserved_bits != 0 {
            return Err(Error::BadFrame("reserved bits set"));
        }

        // Decode the MASK flag.
        let mask = (frame_reassembly_buffer[1] & MASK) != 0;

        // Verify the MASK flag is correct.  If we are supposed to have data
        // masked in the receive direction, MASK should be set.  Otherwise,
        // it should be clear.
        match (mask, self.mask_direction) {
            (true, MaskDirection::Transmit) => {
                return Err(Error::BadFrame("masked frame"));
            },
            (false, MaskDirection::Receive) => {
                return Err(Error::BadFrame("unmasked frame"));
            },
            _ => (),
        }

        // Decode the opcode.  This determines:
        // * If this is a continuation of a fragmented message, or the first
        //   (and perhaps only) fragment of a new message.
        // * The type of message.
        let opcode = frame_reassembly_buffer[0] & 0x0F;

        // Recover the payload from the frame, applying the mask if necessary.
        let mut payload = frame_reassembly_buffer
            [header_length..header_length + payload_length]
            .to_vec();
        if mask {
            let mask_bytes =
                &frame_reassembly_buffer[header_length - 4..header_length];
            payload.iter_mut().zip(mask_bytes.iter().cycle()).for_each(
                |(payload_byte, mask_byte)| *payload_byte ^= mask_byte,
            );
        }

        // Interpret the payload depending on the opcode.
        match opcode {
            OPCODE_CONTINUATION => {
                self.receive_frame_continuation(payload, fin).await?;
                Ok(ReceivedCloseFrame::No)
            },

            OPCODE_TEXT => {
                if let MessageInProgress::None = self.message_in_progress {
                    self.receive_frame_text(payload, fin).await?;
                } else {
                    return Err(Error::BadFrame("last message incomplete"));
                }
                Ok(ReceivedCloseFrame::No)
            },

            OPCODE_BINARY => {
                if let MessageInProgress::None = self.message_in_progress {
                    self.receive_frame_binary(payload, fin).await?
                } else {
                    return Err(Error::BadFrame("last message incomplete"));
                }
                Ok(ReceivedCloseFrame::No)
            },

            OPCODE_PING => {
                if !fin {
                    return Err(Error::BadFrame("fragmented control frame"));
                }
                self.receive_frame_ping(payload).await?;
                Ok(ReceivedCloseFrame::No)
            },

            OPCODE_PONG => {
                if !fin {
                    return Err(Error::BadFrame("fragmented control frame"));
                }
                self.received_messages
                    .lock()
                    .await
                    .push(StreamMessage::Pong(payload));
                Ok(ReceivedCloseFrame::No)
            },

            OPCODE_CLOSE => {
                if !fin {
                    return Err(Error::BadFrame("fragmented control frame"));
                }
                self.receive_frame_close(payload).await?;
                Ok(ReceivedCloseFrame::Yes)
            },

            _ => Err(Error::BadFrame("unknown opcode")),
        }
    }

    async fn receive_frame_binary(
        &mut self,
        mut payload: Vec<u8>,
        fin: bool,
    ) -> Result<(), Error> {
        self.message_reassembly_buffer.append(&mut payload);
        self.message_in_progress = if fin {
            let mut message = Vec::new();
            std::mem::swap(&mut message, &mut self.message_reassembly_buffer);
            let mut received_messages = self.received_messages.lock().await;
            received_messages.push(StreamMessage::Binary(message));
            MessageInProgress::None
        } else {
            MessageInProgress::Binary
        };
        Ok(())
    }

    async fn receive_frame_close(
        &mut self,
        payload: Vec<u8>,
    ) -> Result<(), Error> {
        let mut code = 1005;
        let mut reason = String::new();
        if payload.len() >= 2 {
            code = ((payload[0] as usize) << 8) + (payload[1] as usize);
            reason = String::from(std::str::from_utf8(&payload[2..]).map_err(
                |source| Error::Utf8 {
                    source,
                    context: "close reason",
                },
            )?);
        }
        self.received_messages.lock().await.push(StreamMessage::Close {
            code,
            reason,
        });
        Ok(())
    }

    async fn receive_frame_continuation(
        &mut self,
        payload: Vec<u8>,
        fin: bool,
    ) -> Result<(), Error> {
        match self.message_in_progress {
            MessageInProgress::None => {
                Err(Error::BadFrame("unexpected continuation frame"))
            },
            MessageInProgress::Text => {
                self.receive_frame_text(payload, fin).await
            },
            MessageInProgress::Binary => {
                self.receive_frame_binary(payload, fin).await
            },
        }
    }

    async fn receive_frame_ping(
        &mut self,
        payload: Vec<u8>,
    ) -> Result<(), Error> {
        self.message_handler
            .lock()
            .await
            .send_frame(SetFin::Yes, OPCODE_PONG, payload.clone())
            .await?;
        self.received_messages.lock().await.push(StreamMessage::Ping(payload));
        Ok(())
    }

    async fn receive_frame_text(
        &mut self,
        mut payload: Vec<u8>,
        fin: bool,
    ) -> Result<(), Error> {
        self.message_reassembly_buffer.append(&mut payload);
        self.message_in_progress = if fin {
            let message = std::str::from_utf8(&self.message_reassembly_buffer)
                .map_err(|source| Error::Utf8 {
                    source,
                    context: "text message",
                })?;
            let mut received_messages = self.received_messages.lock().await;
            received_messages.push(StreamMessage::Text(String::from(message)));
            self.message_reassembly_buffer.clear();
            MessageInProgress::None
        } else {
            MessageInProgress::Text
        };
        Ok(())
    }
}

fn decode_frame_header_payload_lengths(frame: &[u8]) -> Option<(usize, usize)> {
    match frame[1] & !MASK {
        126 => {
            let header_length = 4;
            if frame.len() < header_length {
                None
            } else {
                let payload_length =
                    ((frame[2] as usize) << 8) + frame[3] as usize;
                Some((header_length, payload_length))
            }
        },
        127 => {
            let header_length = 10;
            if frame.len() < header_length {
                None
            } else {
                let payload_length = ((frame[2] as usize) << 56)
                    + ((frame[3] as usize) << 48)
                    + ((frame[4] as usize) << 40)
                    + ((frame[5] as usize) << 32)
                    + ((frame[6] as usize) << 24)
                    + ((frame[7] as usize) << 16)
                    + ((frame[8] as usize) << 8)
                    + frame[9] as usize;
                Some((header_length, payload_length))
            }
        },
        length_first_octet => {
            let header_length = 2;
            let payload_length = length_first_octet as usize;
            Some((header_length, payload_length))
        },
    }
}

async fn try_receive_frames(
    mut connection_rx: Box<dyn ConnectionRx>,
    received_messages: &Mutex<ReceivedMessages>,
    max_frame_size: Option<usize>,
    mask_direction: MaskDirection,
    message_handler: &Mutex<MessageHandler>,
    close_sent: oneshot::Receiver<()>,
) -> Result<(), Error> {
    let mut frame_receiver =
        FrameReceiver::new(mask_direction, message_handler, received_messages);
    let mut frame_reassembly_buffer = Vec::new();
    let mut need_more_input = true;
    let mut read_chunk_size = INITIAL_READ_CHUNK_SIZE;
    loop {
        // Wait for more data to arrive, if we need more input.
        if need_more_input {
            receive_bytes(
                &mut connection_rx,
                &mut frame_reassembly_buffer,
                read_chunk_size,
            )
            .await?;

            // If we need more data, double the amount of free space to arrange
            // in the frame reassembly buffer for receiving more data.
            read_chunk_size *= 2;
        }

        // Until we recover a whole frame, assume we need to read more data.
        need_more_input = true;

        // Proceed only if we have enough data to recover the first length
        // octet.
        if frame_reassembly_buffer.len() < 2 {
            continue;
        }

        // Figure out the payload length, based on the first length octet.
        // It will be either 1, 2, or 8 bytes long, depending on the value
        // of that first byte.  We may loop back to the top if we find that
        // we haven't received enough bytes to determine the payload length.
        if let Some((mut header_length, payload_length)) =
            decode_frame_header_payload_lengths(&frame_reassembly_buffer)
        {
            if (frame_reassembly_buffer[1] & MASK) != 0 {
                header_length += 4;
            }
            let frame_length = header_length + payload_length;
            if let Some(max_frame_size) = max_frame_size {
                if frame_length > max_frame_size {
                    return Err(Error::FramePayloadTooLarge);
                }
            }
            if frame_reassembly_buffer.len() < frame_length {
                continue;
            }
            if let ReceivedCloseFrame::Yes = frame_receiver
                .receive_frame(
                    &frame_reassembly_buffer[0..frame_length],
                    header_length,
                    payload_length,
                )
                .await?
            {
                let _ = close_sent.await;
                return Ok(());
            }
            frame_reassembly_buffer.drain(0..frame_length);

            // Try to recover more frames from what we already received,
            // before reading more.  Once we need to read more, start over
            // in the estimate of how many bytes need to be arranged
            // in the frame reassembly buffer.
            need_more_input = false;
            read_chunk_size = INITIAL_READ_CHUNK_SIZE;
        } else if let Some(max_frame_size) = max_frame_size {
            if frame_reassembly_buffer.len() >= max_frame_size {
                return Err(Error::FramePayloadTooLarge);
            }
        }
    }
}

async fn receive_frames(
    connection_rx: Box<dyn ConnectionRx>,
    received_messages: Arc<Mutex<ReceivedMessages>>,
    max_frame_size: Option<usize>,
    mask_direction: MaskDirection,
    message_handler: &Mutex<MessageHandler>,
    close_sent: oneshot::Receiver<()>,
) {
    if let Err(error) = try_receive_frames(
        connection_rx,
        &*received_messages,
        max_frame_size,
        mask_direction,
        message_handler,
        close_sent,
    )
    .await
    {
        let (code, reason) = close_code_and_reason_from_error(error);
        received_messages.lock().await.push(StreamMessage::Close {
            code,
            reason: reason.clone(),
        });
        message_handler.lock().await.send_close(code, reason).await;
    }
    received_messages.lock().await.fuse();
}

async fn handle_messages(
    work_in_receiver: mpsc::UnboundedReceiver<WorkerMessage>,
    message_handler: &Mutex<MessageHandler>,
) {
    let _ = work_in_receiver
        .map(Ok)
        .try_for_each(|message| async {
            message_handler.lock().await.handle_message(message).await
        })
        .await;
}

// The use of `futures::select!` seems to trigger this warning somehow
// that isn't yet understood.
#[allow(clippy::mut_mut)]
async fn worker(
    work_in_receiver: mpsc::UnboundedReceiver<WorkerMessage>,
    received_messages: Arc<Mutex<ReceivedMessages>>,
    connection_tx: Box<dyn ConnectionTx>,
    connection_rx: Box<dyn ConnectionRx>,
    mask_direction: MaskDirection,
    max_frame_size: Option<usize>,
) {
    // Drive to completion the stream of messages to the worker thread.
    let (close_sent_sender, close_sent_receiver) = oneshot::channel();
    let message_handler = Mutex::new(MessageHandler::new(
        close_sent_sender,
        connection_tx,
        mask_direction,
    ));
    let handle_messages_future =
        handle_messages(work_in_receiver, &message_handler);
    let receive_frames_future = receive_frames(
        connection_rx,
        received_messages,
        max_frame_size,
        mask_direction,
        &message_handler,
        close_sent_receiver,
    );
    futures::select!(
        _ = handle_messages_future.fuse() => {},
        _ = receive_frames_future.fuse() => {},
    );
}

#[must_use]
pub struct WebSocket {
    close_sent: bool,
    message_in_progress: MessageInProgress,
    received_messages: Arc<Mutex<ReceivedMessages>>,

    // This sender is used to deliver messages to the worker thread.
    work_in: mpsc::UnboundedSender<WorkerMessage>,

    // This is our handle to join the worker thread when dropped.
    worker: Option<std::thread::JoinHandle<()>>,
}

impl WebSocket {
    pub fn close<T>(
        &mut self,
        code: usize,
        reason: T,
    ) -> Result<(), Error>
    where
        T: Into<String>,
    {
        executor::block_on(async {
            self.send(SinkMessage::Close {
                code,
                reason: reason.into(),
            })
            .await
        })
    }

    pub(crate) fn new(
        connection_tx: Box<dyn ConnectionTx>,
        connection_rx: Box<dyn ConnectionRx>,
        mask_direction: MaskDirection,
        max_frame_size: Option<usize>,
    ) -> Self {
        // Make the channel used to communicate with the worker thread.
        let (sender, receiver) = mpsc::unbounded();

        // Make storage for a queue with waker used to deliver received
        // messages back to the user.
        let received_messages = Arc::new(Mutex::new(ReceivedMessages {
            fused: false,
            queue: VecDeque::new(),
            waker: None,
        }));

        // Store the sender end of the channel and spawn the worker thread,
        // giving it the receiver end as well as the connection.
        Self {
            close_sent: false,
            message_in_progress: MessageInProgress::None,
            received_messages: received_messages.clone(),
            work_in: sender,
            worker: Some(thread::spawn(move || {
                executor::block_on(worker(
                    receiver,
                    received_messages,
                    connection_tx,
                    connection_rx,
                    mask_direction,
                    max_frame_size,
                ))
            })),
        }
    }

    pub fn ping<T>(
        &mut self,
        data: T,
    ) -> Result<(), Error>
    where
        T: Into<Vec<u8>>,
    {
        executor::block_on(async {
            self.send(SinkMessage::Ping(data.into())).await
        })
    }

    pub fn text<T>(
        &mut self,
        data: T,
        last_fragment: LastFragment,
    ) -> Result<(), Error>
    where
        T: Into<String>,
    {
        executor::block_on(async {
            self.send(SinkMessage::Text {
                payload: data.into(),
                last_fragment,
            })
            .await
        })
    }

    pub fn binary<T>(
        &mut self,
        data: T,
        last_fragment: LastFragment,
    ) -> Result<(), Error>
    where
        T: Into<Vec<u8>>,
    {
        executor::block_on(async {
            self.send(SinkMessage::Binary {
                payload: data.into(),
                last_fragment,
            })
            .await
        })
    }
}

impl Stream for WebSocket {
    type Item = StreamMessage;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        if let Poll::Ready(mut received_messages) =
            Box::pin(self.received_messages.lock()).as_mut().poll(cx)
        {
            if received_messages.queue.is_empty() {
                if received_messages.fused {
                    Poll::Ready(None)
                } else {
                    received_messages.waker.replace(cx.waker().clone());
                    Poll::Pending
                }
            } else {
                let message = received_messages.queue.pop_front();
                Poll::Ready(message)
            }
        } else {
            Poll::Pending
        }
    }
}

impl Sink<SinkMessage> for WebSocket {
    type Error = Error;

    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.work_in.poll_ready(cx).map_err(|_| Error::Closed)
    }

    // TODO: Needs refactoring
    #[allow(clippy::too_many_lines)]
    fn start_send(
        mut self: std::pin::Pin<&mut Self>,
        item: SinkMessage,
    ) -> Result<(), Self::Error> {
        if self.close_sent {
            return Err(Error::Closed);
        }
        let item = match item {
            SinkMessage::Ping(payload) => {
                if payload.len() > MAX_CONTROL_FRAME_DATA_LENGTH {
                    return Err(Error::FramePayloadTooLarge);
                } else {
                    WorkerMessage::Send {
                        set_fin: SetFin::Yes,
                        opcode: OPCODE_PING,
                        data: payload,
                    }
                }
            },
            SinkMessage::Text {
                payload,
                last_fragment,
            } => {
                let (opcode, set_fin) =
                    match (&self.message_in_progress, last_fragment) {
                        (MessageInProgress::Binary, _) => {
                            Err(Error::LastMessageUnfinished)
                        },
                        (MessageInProgress::None, LastFragment::Yes) => {
                            Ok((OPCODE_TEXT, SetFin::Yes))
                        },
                        (MessageInProgress::None, LastFragment::No) => {
                            Ok((OPCODE_TEXT, SetFin::No))
                        },
                        (MessageInProgress::Text, LastFragment::Yes) => {
                            Ok((OPCODE_CONTINUATION, SetFin::Yes))
                        },
                        (MessageInProgress::Text, LastFragment::No) => {
                            Ok((OPCODE_CONTINUATION, SetFin::No))
                        },
                    }?;
                self.message_in_progress =
                    if let LastFragment::No = last_fragment {
                        MessageInProgress::Text
                    } else {
                        MessageInProgress::None
                    };
                WorkerMessage::Send {
                    set_fin,
                    opcode,
                    data: payload.into(),
                }
            },
            SinkMessage::Binary {
                payload,
                last_fragment,
            } => {
                let (opcode, set_fin) =
                    match (&self.message_in_progress, last_fragment) {
                        (MessageInProgress::Text, _) => {
                            Err(Error::LastMessageUnfinished)
                        },
                        (MessageInProgress::None, LastFragment::Yes) => {
                            Ok((OPCODE_BINARY, SetFin::Yes))
                        },
                        (MessageInProgress::None, LastFragment::No) => {
                            Ok((OPCODE_BINARY, SetFin::No))
                        },
                        (MessageInProgress::Binary, LastFragment::Yes) => {
                            Ok((OPCODE_CONTINUATION, SetFin::Yes))
                        },
                        (MessageInProgress::Binary, LastFragment::No) => {
                            Ok((OPCODE_CONTINUATION, SetFin::No))
                        },
                    }?;
                self.message_in_progress =
                    if let LastFragment::No = last_fragment {
                        MessageInProgress::Binary
                    } else {
                        MessageInProgress::None
                    };
                WorkerMessage::Send {
                    set_fin,
                    opcode,
                    data: payload,
                }
            },
            SinkMessage::Close {
                code,
                reason,
            } => {
                if reason.as_bytes().len() + 2 > MAX_CONTROL_FRAME_DATA_LENGTH {
                    return Err(Error::FramePayloadTooLarge);
                } else {
                    self.close_sent = true;
                    WorkerMessage::Send {
                        set_fin: SetFin::Yes,
                        opcode: OPCODE_CLOSE,
                        data: {
                            let mut data = Vec::new();
                            data.push_word(code, 16);
                            data.extend(reason.as_bytes());
                            data
                        },
                    }
                }
            },
            SinkMessage::CloseNoStatus => {
                self.close_sent = true;
                WorkerMessage::Send {
                    set_fin: SetFin::Yes,
                    opcode: OPCODE_CLOSE,
                    data: vec![],
                }
            },
        };
        self.work_in.start_send(item).map_err(|_| Error::Closed)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl Drop for WebSocket {
    fn drop(&mut self) {
        // Tell the worker thread to stop.
        //
        // This can fail if the worker stopped early (due to the connection
        // being lost, for example).
        let _ = self.work_in.unbounded_send(WorkerMessage::Exit);

        // Join the worker thread.
        //
        // This shouldn't fail unless the worker panics.  If it does, there's
        // no reason why we shouldn't panic as well.
        self.worker.take().expect(
            "somehow the worker thread join handle got lost before we could take it"
        ).join().expect(
            "the worker thread panicked before we could join it"
        );
    }
}

#[cfg(test)]
#[allow(clippy::string_lit_as_bytes)]
#[allow(clippy::non_ascii_literal)]
mod tests {
    use super::*;
    use crate::{
        mock_connection,
        timeout::timeout,
    };
    use std::cell::RefCell;

    const REASONABLE_FAST_OPERATION_TIMEOUT: std::time::Duration =
        std::time::Duration::from_millis(200);

    #[test]
    fn server_send_ping_normal_with_data() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, _connection_back_rx) = mock_connection::Rx::new();
        let mut ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        assert!(ws.ping("Hello").is_ok());
        assert_eq!(
            Some(&b"\x89\x05Hello"[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
    }

    #[test]
    fn send_ping_normal_without_data() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, _connection_back_rx) = mock_connection::Rx::new();
        let mut ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        assert!(ws.ping("").is_ok());
        assert_eq!(
            Some(&b"\x89\x00"[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
    }

    #[test]
    fn send_ping_almost_too_much_data() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, _connection_back_rx) = mock_connection::Rx::new();
        let mut ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        assert!(ws.ping("x".repeat(125)).is_ok());
        let mut expected_output = vec![0x89, 0x7D];
        expected_output.append(&mut [b'x'; 125].to_vec());
        assert_eq!(
            Some(expected_output),
            connection_back_tx.web_socket_output()
        );
    }

    #[test]
    fn send_ping_too_much_data() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, _connection_back_rx) = mock_connection::Rx::new();
        let mut ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        assert!(matches!(
            ws.ping("x".repeat(126)),
            Err(Error::FramePayloadTooLarge)
        ));
        assert_eq!(None, connection_back_tx.web_socket_output());
    }

    #[test]
    fn receive_ping() {
        let (connection_tx, connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, mut connection_back_rx) =
            mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Transmit,
            None,
        );
        let connection_back_tx = &RefCell::new(connection_back_tx);
        let reader = async {
            ws.take(1)
                .for_each(|message| async move {
                    if let StreamMessage::Ping(message) = message {
                        // Check that the message is correct.
                        assert_eq!("Hello,".as_bytes(), message);

                        // Expect to receive back the matching "PONG" message
                        // which the WebSocket should send when it gets the
                        // "PING".
                        let output = connection_back_tx.borrow_mut().web_socket_output_async().await;
                        assert!(output.is_some());
                        let output = output.unwrap();

                        // Verify the size is correct.
                        assert_eq!(12, output.len());

                        // Verify the header is correct.
                        assert_eq!(b"\x8A\x86", &output[0..2]);

                        // Verify masked data is correct (mask is random).
                        for i in 0..6 {
                            assert_eq!(
                                // data    ^      mask
                                message[i] ^ output[2 + (i % 4)], // original input, masked
                                output[6 + i]                     // output
                            );
                        }
                    } else {
                        panic!("we got something that isn't a ping!");
                    }
                })
                .await
        };
        let frame = &b"\x89\x06Hello,"[..];
        connection_back_rx.web_socket_input(frame);
        assert_eq!(
            Ok(()),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn receive_pong() {
        let (connection_tx, _connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, mut connection_back_rx) =
            mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Transmit,
            None,
        );
        let reader = async {
            ws.take(1)
                .for_each(|message| async move {
                    if let StreamMessage::Pong(message) = message {
                        // Check that the message is correct.
                        assert_eq!("World!".as_bytes(), message);
                    } else {
                        panic!("we got something that isn't a pong!");
                    }
                })
                .await
        };
        let frame = &b"\x8A\x06World!"[..];
        connection_back_rx.web_socket_input(frame);
        assert_eq!(
            Ok(()),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn send_text() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, _connection_back_rx) = mock_connection::Rx::new();
        let mut ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        assert!(ws.text("Hello, World!", LastFragment::Yes).is_ok());
        assert_eq!(
            Some(&b"\x81\x0DHello, World!"[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
    }

    #[test]
    fn receive_text() {
        let (connection_tx, _connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, mut connection_back_rx) =
            mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Transmit,
            None,
        );
        let reader = async {
            ws.take(1)
                .for_each(|message| async {
                    if let StreamMessage::Text(message) = message {
                        // Check that the message is correct.
                        assert_eq!("foobar", message);
                    } else {
                        panic!("we got something that isn't a text!");
                    }
                })
                .await
        };
        let frame = &b"\x81\x06foobar"[..];
        connection_back_rx.web_socket_input(frame);
        assert_eq!(
            Ok(()),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn send_binary() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, _connection_back_rx) = mock_connection::Rx::new();
        let mut ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        assert!(ws
            .binary("Hello, World!".as_bytes(), LastFragment::Yes)
            .is_ok());
        assert_eq!(
            Some(&b"\x82\x0DHello, World!"[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
    }

    #[test]
    fn receive_binary() {
        let (connection_tx, _connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, mut connection_back_rx) =
            mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Transmit,
            None,
        );
        let reader = async {
            ws.take(1)
                .for_each(|message| async {
                    if let StreamMessage::Binary(message) = message {
                        // Check that the message is correct.
                        assert_eq!("foobar".as_bytes(), message);
                    } else {
                        panic!("we got something that isn't a binary!");
                    }
                })
                .await
        };
        let frame = &b"\x82\x06foobar"[..];
        connection_back_rx.web_socket_input(frame);
        assert_eq!(
            Ok(()),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn send_masked() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, _connection_back_rx) = mock_connection::Rx::new();
        let mut ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Transmit,
            None,
        );
        let data = "Hello, World!";
        assert!(ws.text(data, LastFragment::Yes).is_ok());
        let output = connection_back_tx.web_socket_output();
        assert!(output.is_some());
        let output = output.unwrap();
        assert_eq!(b"\x81\x8D", &output[0..2]);
        let data = data.as_bytes();
        assert_eq!(data.len() + 6, output.len());
        assert!(data
            .iter()
            .zip(output[2..6].iter().cycle())
            .zip(&output[6..])
            .all(|((&input_byte, &mask_byte), &output_byte)| {
                output_byte == input_byte ^ mask_byte
            }));
    }

    #[test]
    fn receive_masked() {
        let (connection_tx, _connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, mut connection_back_rx) =
            mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        let reader = async {
            ws.take(1)
                .for_each(|message| async {
                    if let StreamMessage::Text(message) = message {
                        // Check that the message is correct.
                        assert_eq!("foobar", message);
                    } else {
                        panic!("we got something that isn't a text!");
                    }
                })
                .await
        };
        let mask = [0x12, 0x34, 0x56, 0x78];
        let data = "foobar";
        let frame = [0x81, 0x86]
            .iter()
            .copied()
            .chain(mask.iter().copied())
            .chain(
                data.as_bytes()
                    .iter()
                    .zip(mask.iter().cycle())
                    .map(|(&data_byte, &mask_byte)| data_byte ^ mask_byte),
            )
            .collect::<Vec<u8>>();
        connection_back_rx.web_socket_input(frame);
        assert_eq!(
            Ok(()),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn send_fragmented_text() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, _connection_back_rx) = mock_connection::Rx::new();
        let mut ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        assert!(ws.text("Hello,", LastFragment::No).is_ok());
        assert_eq!(
            Some(&b"\x01\x06Hello,"[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
        assert!(matches!(
            ws.binary(&b"X"[..], LastFragment::Yes),
            Err(Error::LastMessageUnfinished)
        ));
        assert!(ws.ping("").is_ok());
        assert_eq!(
            Some(&b"\x89\x00"[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
        assert!(matches!(
            ws.binary(&b"X"[..], LastFragment::No),
            Err(Error::LastMessageUnfinished)
        ));
        assert!(ws.text(" ", LastFragment::No).is_ok());
        assert_eq!(
            Some(&b"\x00\x01 "[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
        assert!(ws.text("World!", LastFragment::Yes).is_ok());
        assert_eq!(
            Some(&b"\x80\x06World!"[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
    }

    #[test]
    fn send_fragmented_binary() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, _connection_back_rx) = mock_connection::Rx::new();
        let mut ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        assert!(ws.binary(&b"Hello,"[..], LastFragment::No).is_ok());
        assert_eq!(
            Some(&b"\x02\x06Hello,"[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
        assert!(matches!(
            ws.text("X", LastFragment::Yes),
            Err(Error::LastMessageUnfinished)
        ));
        assert!(ws.ping("").is_ok());
        assert_eq!(
            Some(&b"\x89\x00"[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
        assert!(matches!(
            ws.text("X", LastFragment::No),
            Err(Error::LastMessageUnfinished)
        ));
        assert!(ws.binary(&b" "[..], LastFragment::No).is_ok());
        assert_eq!(
            Some(&b"\x00\x01 "[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
        assert!(ws.binary(&b"World!"[..], LastFragment::Yes).is_ok());
        assert_eq!(
            Some(&b"\x80\x06World!"[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
    }

    #[test]
    fn receive_fragmented_text() {
        let (connection_tx, _connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, mut connection_back_rx) =
            mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Transmit,
            None,
        );
        let reader = async {
            ws.take(1)
                .for_each(|message| async {
                    if let StreamMessage::Text(message) = message {
                        // Check that the message is correct.
                        assert_eq!("foobar", message);
                    } else {
                        panic!("we got something that isn't a text!");
                    }
                })
                .await
        };
        for &frame in
            [&b"\x01\x03foo"[..], &b"\x00\x01b"[..], &b"\x80\x02ar"[..]][..]
                .iter()
        {
            connection_back_rx.web_socket_input(frame);
        }
        assert_eq!(
            Ok(()),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn initiate_close_no_status_returned() {
        let (connection_tx, connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        let (mut sink, stream) = ws.split();
        let reader = async {
            stream
                .for_each(|message| async {
                    if let StreamMessage::Close {
                        code,
                        reason,
                    } = message
                    {
                        // Check that the code and reason are correct.
                        assert_eq!(1005, code);
                        assert_eq!("", reason);
                    } else {
                        panic!("we got something that isn't a text!");
                    }
                })
                .await
        };
        let connection_back_tx = RefCell::new(connection_back_tx);
        let connection_back_rx = RefCell::new(connection_back_rx);
        let writer = async {
            // Verify the WebSocket sends out the "CLOSE" message.
            assert_eq!(
                Some(&b"\x88\x0A\x03\xE8Goodbye!"[..]),
                connection_back_tx
                    .borrow_mut()
                    .web_socket_output_async()
                    .await
                    .as_deref()
            );

            // Mock a "CLOSE" response back to the WebSocket,
            // with no payload.
            connection_back_rx
                .borrow_mut()
                .web_socket_input_async(&b"\x88\x80XXXX"[..])
                .await;
        };
        assert!(executor::block_on(async {
            sink.send(SinkMessage::Close {
                code: 1000,
                reason: String::from("Goodbye!"),
            })
            .await
        })
        .is_ok());
        assert_eq!(
            Ok(((), ())),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                async { futures::join!(reader, writer) }
            ))
        );
    }

    // TODO: Refactor this test?
    #[allow(clippy::too_many_lines)]
    #[test]
    fn initiate_close_status_returned() {
        let (connection_tx, connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        let (mut sink, stream) = ws.split();
        let reader = async {
            stream
                .take(1)
                .for_each(|message| async {
                    if let StreamMessage::Close {
                        code,
                        reason,
                    } = message
                    {
                        // Check that the code and reason are correct.
                        assert_eq!(1000, code);
                        assert_eq!("Bye", reason);
                    } else {
                        panic!("we got something that isn't a close!");
                    }
                })
                .await
        };
        let connection_back_tx = RefCell::new(connection_back_tx);
        let connection_back_rx = RefCell::new(connection_back_rx);
        let writer = async {
            // Verify the WebSocket sends out the "CLOSE" message.
            assert_eq!(
                Some(&b"\x88\x0A\x03\xE8Goodbye!"[..]),
                connection_back_tx
                    .borrow_mut()
                    .web_socket_output_async()
                    .await
                    .as_deref()
            );

            // Mock a "CLOSE" response back to the WebSocket,
            // with a payload.
            let mask = [0x12, 0x34, 0x56, 0x78];
            connection_back_rx
                .borrow_mut()
                .web_socket_input_async(
                    b"\x88\x85"
                        .iter()
                        .copied()
                        .chain(mask.iter().copied())
                        .chain(
                            b"\x03\xe8Bye"
                                .iter()
                                .zip(mask.iter().cycle())
                                .map(|(&data, &mask)| data ^ mask),
                        )
                        .collect::<Vec<u8>>(),
                )
                .await
        };
        assert!(executor::block_on(async {
            sink.send(SinkMessage::Close {
                code: 1000,
                reason: String::from("Goodbye!"),
            })
            .await
        })
        .is_ok());
        assert!(matches!(
            executor::block_on(async {
                sink.send(SinkMessage::Text {
                    payload: "Yo Dawg, I heard you like...".into(),
                    last_fragment: LastFragment::Yes,
                })
                .await
            }),
            Err(Error::Closed)
        ));
        assert!(matches!(
            executor::block_on(async {
                sink.send(SinkMessage::Binary {
                    payload: "Yo Dawg, I heard you like...".into(),
                    last_fragment: LastFragment::Yes,
                })
                .await
            }),
            Err(Error::Closed)
        ));
        assert!(matches!(
            executor::block_on(async {
                sink.send(SinkMessage::Ping(vec![])).await
            }),
            Err(Error::Closed)
        ));
        assert!(matches!(
            executor::block_on(async {
                sink.send(SinkMessage::Close {
                    code: 1000,
                    reason: "Goodbye AGAIN!".into(),
                })
                .await
            }),
            Err(Error::Closed)
        ));
        assert_eq!(
            Ok(((), ())),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                async { futures::join!(reader, writer) }
            ))
        );
    }

    // TODO: Refactor this test?
    #[allow(clippy::too_many_lines)]
    #[test]
    fn receive_close() {
        let (connection_tx, connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        let (sink, stream) = ws.split();
        let connection_back_tx = RefCell::new(connection_back_tx);
        let connection_back_rx = RefCell::new(connection_back_rx);
        let sink = RefCell::new(sink);
        let reader = async {
            stream
                .take(1)
                .for_each(|message| async {
                    match message {
                        StreamMessage::Close {
                            code,
                            reason,
                        } => {
                            // Check that the code and reason are correct.
                            assert_eq!(1005, code);
                            assert_eq!("", reason);

                            // Send a "PING" out before we close our end.
                            assert!(dbg!(
                                sink.borrow_mut()
                                    .send(SinkMessage::Ping(vec![]))
                                    .await
                            )
                            .is_ok());
                        },

                        _ => panic!("we got something that isn't a close!"),
                    }
                })
                .await
        };
        let writer = async {
            // Verify the WebSocket sends out the "PING" we requested
            // after it received the mocked "CLOSE" message.
            assert_eq!(
                Some(&b"\x89\x00"[..]),
                connection_back_tx
                    .borrow_mut()
                    .web_socket_output_async()
                    .await
                    .as_deref()
            );

            // Tell the WebSocket to close this end.
            assert!(sink
                .borrow_mut()
                .send(SinkMessage::CloseNoStatus)
                .await
                .is_ok());

            // Verify the WebSocket sends out the "CLOSE" message.
            assert_eq!(
                Some(&b"\x88\x00"[..]),
                connection_back_tx
                    .borrow_mut()
                    .web_socket_output_async()
                    .await
                    .as_deref()
            );

            // Now the we're done, we can close the connection.
            connection_back_rx.borrow_mut().close().await;
        };
        connection_back_rx.borrow_mut().web_socket_input(&b"\x88\x80XXXX"[..]);
        assert_eq!(
            Ok(((), ())),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                async { futures::join!(reader, writer) }
            ))
        );
    }

    #[test]
    fn violation_reserved_bits_set() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.fold(false, |_, message| async {
                match message {
                    StreamMessage::Close {
                        code,
                        reason,
                    } => {
                        // Check that the code and reason are correct.
                        assert_eq!(1002, code);
                        assert_eq!("reserved bits set", reason);

                        // Now the we're done, we can close the connection.
                        connection_back_rx.borrow_mut().close().await;
                        true
                    },

                    _ => panic!("we got something that isn't a close!"),
                }
            })
            .await
        };
        connection_back_rx.borrow_mut().web_socket_input(&b"\x99\x80XXXX"[..]);
        assert_eq!(
            Some(&b"\x88\x13\x03\xeareserved bits set"[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
        assert_eq!(
            Ok(true),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn violation_unexpected_continuation() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.fold(false, |_, message| async {
                match message {
                    StreamMessage::Close {
                        code,
                        reason,
                    } => {
                        // Check that the code and reason are correct.
                        assert_eq!(1002, code);
                        assert_eq!("unexpected continuation frame", reason);

                        // Now the we're done, we can close the connection.
                        connection_back_rx.borrow_mut().close().await;
                        true
                    },

                    _ => panic!("we got something that isn't a close!"),
                }
            })
            .await
        };
        connection_back_rx.borrow_mut().web_socket_input(&b"\x80\x80XXXX"[..]);
        assert_eq!(
            Some(&b"\x88\x1F\x03\xeaunexpected continuation frame"[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
        assert_eq!(
            Ok(true),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn violation_new_text_message_during_fragmented_message() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.fold(false, |_, message| async {
                match message {
                    StreamMessage::Close {
                        code,
                        reason,
                    } => {
                        // Check that the code and reason are correct.
                        assert_eq!(1002, code);
                        assert_eq!("last message incomplete", reason);

                        // Now the we're done, we can close the connection.
                        connection_back_rx.borrow_mut().close().await;
                        true
                    },

                    _ => panic!("we got something that isn't a close!"),
                }
            })
            .await
        };
        connection_back_rx.borrow_mut().web_socket_input(&b"\x01\x80XXXX"[..]);
        connection_back_rx.borrow_mut().web_socket_input(&b"\x01\x80XXXX"[..]);
        assert_eq!(
            Some(&b"\x88\x19\x03\xealast message incomplete"[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
        assert_eq!(
            Ok(true),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn violation_new_binary_message_during_fragmented_message() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.fold(false, |_, message| async {
                match message {
                    StreamMessage::Close {
                        code,
                        reason,
                    } => {
                        // Check that the code and reason are correct.
                        assert_eq!(1002, code);
                        assert_eq!("last message incomplete", reason);

                        // Now the we're done, we can close the connection.
                        connection_back_rx.borrow_mut().close().await;
                        true
                    },

                    _ => panic!("we got something that isn't a close!"),
                }
            })
            .await
        };
        connection_back_rx.borrow_mut().web_socket_input(&b"\x01\x80XXXX"[..]);
        connection_back_rx.borrow_mut().web_socket_input(&b"\x02\x80XXXX"[..]);
        assert_eq!(
            Some(&b"\x88\x19\x03\xealast message incomplete"[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
        assert_eq!(
            Ok(true),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn violation_unknown_opcode() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.fold(false, |_, message| async {
                match message {
                    StreamMessage::Close {
                        code,
                        reason,
                    } => {
                        // Check that the code and reason are correct.
                        assert_eq!(1002, code);
                        assert_eq!("unknown opcode", reason);

                        // Now the we're done, we can close the connection.
                        connection_back_rx.borrow_mut().close().await;
                        true
                    },

                    _ => panic!("we got something that isn't a close!"),
                }
            })
            .await
        };
        connection_back_rx.borrow_mut().web_socket_input(&b"\x83\x80XXXX"[..]);
        assert_eq!(
            Some(&b"\x88\x10\x03\xeaunknown opcode"[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
        assert_eq!(
            Ok(true),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn violation_client_should_mask_frames() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.fold(false, |_, message| async {
                match message {
                    StreamMessage::Close {
                        code,
                        reason,
                    } => {
                        // Check that the code and reason are correct.
                        assert_eq!(1002, code);
                        assert_eq!("unmasked frame", reason);

                        // Now the we're done, we can close the connection.
                        connection_back_rx.borrow_mut().close().await;
                        true
                    },

                    _ => panic!("we got something that isn't a close!"),
                }
            })
            .await
        };
        connection_back_rx.borrow_mut().web_socket_input(&b"\x89\x00"[..]);
        assert_eq!(
            Some(&b"\x88\x10\x03\xeaunmasked frame"[..]),
            connection_back_tx.web_socket_output().as_deref()
        );
        assert_eq!(
            Ok(true),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn violation_server_should_not_mask_frames() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Transmit,
            None,
        );
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.fold(false, |_, message| async {
                match message {
                    StreamMessage::Close {
                        code,
                        reason,
                    } => {
                        // Check that the code and reason are correct.
                        assert_eq!(1002, code);
                        assert_eq!("masked frame", reason);

                        // Now the we're done, we can close the connection.
                        connection_back_rx.borrow_mut().close().await;
                        true
                    },

                    _ => panic!("we got something that isn't a close!"),
                }
            })
            .await
        };
        connection_back_rx.borrow_mut().web_socket_input(&b"\x89\x80XXXX"[..]);
        assert!(matches!(
            connection_back_tx.web_socket_output().as_deref(),
            Some(frame) if
                frame.len() == 20
                && frame[0..2] == b"\x88\x8E"[..]
                && frame[6..].iter().zip(
                    frame[2..6].iter().cycle()
                )
                .map(|(&byte, &mask)| byte ^ mask)
                .eq(b"\x03\xeamasked frame"[..].iter().copied())
        ));
        assert_eq!(
            Ok(true),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn connection_unexpectedly_broken() {
        let (connection_tx, _connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.fold(false, |_, message| async {
                match message {
                    StreamMessage::Close {
                        code,
                        reason,
                    } => {
                        // Check that the code and reason are correct.
                        assert_eq!(1006, code);
                        assert_eq!(
                            "underlying connection closed gracefully",
                            reason
                        );

                        // Now the we're done, we can close the connection.
                        connection_back_rx.borrow_mut().close().await;
                        true
                    },

                    _ => panic!("we got something that isn't a close!"),
                }
            })
            .await
        };
        executor::block_on(async {
            connection_back_rx.borrow_mut().close().await;
        });
        assert_eq!(
            Ok(true),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn bad_utf8_in_text() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.fold(false, |_, message| async {
                match message {
                    StreamMessage::Close {
                        code,
                        reason,
                    } => {
                        // Check that the code and reason are correct.
                        assert_eq!(1007, code);
                        assert_eq!(
                            "invalid UTF-8 encoding in text message",
                            reason
                        );

                        // Now the we're done, we can close the connection.
                        connection_back_rx.borrow_mut().close().await;
                        true
                    },

                    _ => panic!("we got something that isn't a close!"),
                }
            })
            .await
        };
        connection_back_rx.borrow_mut().web_socket_input(
            b"\x81\x82"[..]
                .iter()
                .copied()
                .chain({
                    let mask = b"\x12\x34\x56\x78";
                    let payload = b"\xc0\xaf";
                    mask.iter().copied().chain(
                        payload
                            .iter()
                            .zip(mask.iter().cycle())
                            .map(|(&byte, &mask)| byte ^ mask),
                    )
                })
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            Some(
                &b"\x88\x28\x03\xefinvalid UTF-8 encoding in text message"[..]
            ),
            connection_back_tx.web_socket_output().as_deref()
        );
        assert_eq!(
            Ok(true),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn good_utf8_in_text_split_into_fragments() {
        let (connection_tx, _connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Transmit,
            None,
        );
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.take(1)
                .for_each(|message| async {
                    match message {
                        StreamMessage::Text(message) => {
                            // Check that the message was decoded correctly.
                            assert_eq!("𣎴", message);
                        },

                        _ => panic!("we got something that isn't a text!"),
                    }
                })
                .await
        };
        connection_back_rx
            .borrow_mut()
            .web_socket_input(&b"\x01\x02\xF0\xA3"[..]);
        connection_back_rx
            .borrow_mut()
            .web_socket_input(&b"\x80\x02\x8E\xB4"[..]);
        assert_eq!(
            Ok(()),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn bad_utf8_truncated_in_text_split_into_fragments() {
        let (connection_tx, _connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Transmit,
            None,
        );
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.fold(false, |_, message| async {
                match message {
                    StreamMessage::Close {
                        code,
                        reason,
                    } => {
                        // Check that the code and reason are correct.
                        assert_eq!(1007, code);
                        assert_eq!(
                            "invalid UTF-8 encoding in text message",
                            reason
                        );

                        // Now the we're done, we can close the connection.
                        connection_back_rx.borrow_mut().close().await;
                        true
                    },

                    _ => panic!("we got something that isn't a close!"),
                }
            })
            .await
        };
        connection_back_rx
            .borrow_mut()
            .web_socket_input(&b"\x01\x02\xF0\xA3"[..]);
        connection_back_rx.borrow_mut().web_socket_input(&b"\x80\x01\x8E"[..]);
        assert_eq!(
            Ok(true),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn receive_close_invalid_utf8_in_reason() {
        let (connection_tx, _connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.fold(false, |_, message| async {
                match message {
                    StreamMessage::Close {
                        code,
                        reason,
                    } => {
                        // Check that the code and reason are correct.
                        assert_eq!(1007, code);
                        assert_eq!(
                            "invalid UTF-8 encoding in close reason",
                            reason
                        );

                        // Now the we're done, we can close the connection.
                        connection_back_rx.borrow_mut().close().await;
                        true
                    },

                    _ => panic!("we got something that isn't a close!"),
                }
            })
            .await
        };
        connection_back_rx.borrow_mut().web_socket_input(
            b"\x88\x84"[..]
                .iter()
                .copied()
                .chain({
                    let mask = b"\x12\x34\x56\x78";
                    let payload = b"\x03\xe8\xc0\xaf";
                    mask.iter().copied().chain(
                        payload
                            .iter()
                            .zip(mask.iter().cycle())
                            .map(|(&byte, &mask)| byte ^ mask),
                    )
                })
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            Ok(true),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn drop_connection_if_frame_too_large() {
        let (connection_tx, _connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Transmit,
            Some(7),
        );
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.fold(false, |_, message| async {
                match message {
                    StreamMessage::Close {
                        code,
                        reason,
                    } => {
                        // Check that the code and reason are correct.
                        assert_eq!(1009, code);
                        assert_eq!("frame too large", reason);

                        // Now the we're done, we can close the connection.
                        connection_back_rx.borrow_mut().close().await;
                        true
                    },

                    _ => panic!("we got something that isn't a close!"),
                }
            })
            .await
        };
        connection_back_rx
            .borrow_mut()
            .web_socket_input(&b"\x81\x06foobar"[..]);
        assert_eq!(
            Ok(true),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn close_frame_fin_clear() {
        let (connection_tx, _connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Transmit,
            Some(7),
        );
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.fold(false, |_, message| async {
                match message {
                    StreamMessage::Close {
                        code,
                        reason,
                    } => {
                        // Check that the code and reason are correct.
                        assert_eq!(1002, code);
                        assert_eq!("fragmented control frame", reason);

                        // Now the we're done, we can close the connection.
                        connection_back_rx.borrow_mut().close().await;
                        true
                    },

                    _ => panic!("we got something that isn't a close!"),
                }
            })
            .await
        };
        connection_back_rx
            .borrow_mut()
            .web_socket_input(&b"\x08\x03\x03\xe8bye"[..]);
        assert_eq!(
            Ok(true),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn ping_frame_fin_clear() {
        let (connection_tx, _connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Transmit,
            Some(7),
        );
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.fold(false, |_, message| async {
                match message {
                    StreamMessage::Close {
                        code,
                        reason,
                    } => {
                        // Check that the code and reason are correct.
                        assert_eq!(1002, code);
                        assert_eq!("fragmented control frame", reason);

                        // Now the we're done, we can close the connection.
                        connection_back_rx.borrow_mut().close().await;
                        true
                    },

                    _ => panic!("we got something that isn't a close!"),
                }
            })
            .await
        };
        connection_back_rx.borrow_mut().web_socket_input(&b"\x09\x02hi"[..]);
        assert_eq!(
            Ok(true),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn pong_frame_fin_clear() {
        let (connection_tx, _connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Transmit,
            Some(7),
        );
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.fold(false, |_, message| async {
                match message {
                    StreamMessage::Close {
                        code,
                        reason,
                    } => {
                        // Check that the code and reason are correct.
                        assert_eq!(1002, code);
                        assert_eq!("fragmented control frame", reason);

                        // Now the we're done, we can close the connection.
                        connection_back_rx.borrow_mut().close().await;
                        true
                    },

                    _ => panic!("we got something that isn't a close!"),
                }
            })
            .await
        };
        connection_back_rx.borrow_mut().web_socket_input(&b"\x0a\x02hi"[..]);
        assert_eq!(
            Ok(true),
            executor::block_on(timeout(
                REASONABLE_FAST_OPERATION_TIMEOUT,
                reader,
            ))
        );
    }

    #[test]
    fn send_close_reason_too_long() {
        let (connection_tx, _connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, _connection_back_rx) = mock_connection::Rx::new();
        let mut ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
            None,
        );
        assert!(matches!(
            executor::block_on(async {
                ws.send(SinkMessage::Close {
                    code: 1000,
                    reason: "X".repeat(124),
                })
                .await
            }),
            Err(Error::FramePayloadTooLarge)
        ));
    }
}

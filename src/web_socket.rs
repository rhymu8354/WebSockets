use super::{
    ConnectionRx,
    ConnectionTx,
    Error,
};
use futures::{
    channel::mpsc,
    executor,
    future::FutureExt,
    stream::{
        StreamExt,
        TryStreamExt,
    },
    AsyncReadExt,
    AsyncWriteExt,
    Stream,
};
use rand::{
    rngs::StdRng,
    Rng,
    SeedableRng,
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    sync::{
        Arc,
        Mutex,
    },
    task::Waker,
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

/// This is the opcode for a pong frame.
const OPCODE_PONG: u8 = 0x0A;

// This is the maximum length of a control frame payload.
const MAX_CONTROL_FRAME_DATA_LENGTH: usize = 125;

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

async fn send_frame(
    connection_tx: &mut dyn ConnectionTx,
    set_fin: SetFin,
    opcode: u8,
    mut payload: Vec<u8>,
    mask_direction: MaskDirection,
    rng: &mut StdRng,
) -> Result<(), Error> {
    // Pessimistically (meaning we might allocate more than we need) allocate
    // enough memory for the frame in one shot.
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
    let mask = match mask_direction {
        MaskDirection::Receive => 0,
        MaskDirection::Transmit => MASK,
    };

    // Encode the payload length differently, depending on which range
    // the length falls into: either 1, 2, or 8 bytes.  For the 2 and 8
    // byte variants, the first byte is a special marker.  Also, the MASK
    // flag is added to the first byte always.
    //
    // TODO: Find out if there's a nicer way to avoid the truncation
    // warning about casting to `u8`.
    #[allow(clippy::cast_possible_truncation)]
    match num_payload_bytes {
        0..=125 => {
            frame.push(num_payload_bytes as u8 | mask);
        },
        126..=65535 => {
            frame.push(126 | mask);
            frame.push((num_payload_bytes >> 8) as u8);
            frame.push((num_payload_bytes & 0xFF) as u8);
        },
        _ => {
            frame.push(127 | mask);
            frame.push((num_payload_bytes >> 56) as u8);
            frame.push(((num_payload_bytes >> 48) & 0xFF) as u8);
            frame.push(((num_payload_bytes >> 40) & 0xFF) as u8);
            frame.push(((num_payload_bytes >> 32) & 0xFF) as u8);
            frame.push(((num_payload_bytes >> 24) & 0xFF) as u8);
            frame.push(((num_payload_bytes >> 16) & 0xFF) as u8);
            frame.push(((num_payload_bytes >> 8) & 0xFF) as u8);
            frame.push((num_payload_bytes & 0xFF) as u8);
        },
    }

    // Add the payload.  If masking, we need to generate a random mask
    // and XOR the payload with it.
    if mask == 0 {
        frame.append(&mut payload);
    } else {
        // Generate a random mask (4 bytes).
        let mut masking_key = [0; 4];
        rng.fill(&mut masking_key);

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
    connection_tx.write_all(&frame).await.map_err(Error::ConnectionBroken)
}

async fn handle_message(
    message: WorkerMessage,
    connection_tx: &RefCell<Box<dyn ConnectionTx>>,
    rng: &RefCell<StdRng>,
    mask_direction: MaskDirection,
) -> Result<(), ()> {
    match message {
        WorkerMessage::Exit => {
            println!("worker: got Exit message");
            Err(())
        },

        WorkerMessage::Send {
            set_fin,
            opcode,
            data,
        } => {
            println!("worker: got Send message");
            send_frame(
                &mut *connection_tx.borrow_mut(),
                set_fin,
                opcode,
                data,
                mask_direction,
                &mut rng.borrow_mut(),
            )
            .await
            .map_err(|_| ())
        },
    }
}

async fn receive_bytes(
    connection_rx: &mut dyn ConnectionRx,
    frame_reassembly_buffer: &mut Vec<u8>,
) -> Result<(), Error> {
    // TODO: We have a magic number here as a guess of how much more
    // memory we will need to hold the next frame.  We can do better
    // than that, perhaps by making it configurable, and/or adapting
    // at run-time based on how far off our previous estimates were.
    let left_over = frame_reassembly_buffer.len();
    frame_reassembly_buffer.resize(left_over + 65536, 0);
    let received = connection_rx
        .read(&mut frame_reassembly_buffer[left_over..])
        .await
        .map_err(Error::ConnectionBroken)
        .and_then(|received| match received {
            0 => Err(Error::ConnectionClosed),
            received => Ok(received),
        })?;
    println!(
        "receive_bytes: Received {} bytes (we now have {})",
        received,
        left_over + received
    );
    frame_reassembly_buffer.truncate(left_over + received);
    Ok(())
}

fn receive_frame(
    message_reassembly_buffer: &mut Vec<u8>,
    frame_reassembly_buffer: &[u8],
    header_length: usize,
    payload_length: usize,
    mask_direction: MaskDirection,
    received_messages: &Arc<Mutex<ReceivedMessages>>,
    work_in_sender: &mut mpsc::UnboundedSender<WorkerMessage>,
) -> Result<(), Error> {
    println!("receive_frame: {} bytes", frame_reassembly_buffer.len());
    // TODO: I don't remember if this is actually necessary.  Receiving
    // data after we have received a `Close` frame *is* illegal, but in
    // this implementation, we may have completed the worker thread's tasks
    // and never ever try to read more data from the connection.
    //
    // if closeReceived {
    //     return;
    // }

    // Decode the FIN flag.
    //
    // let fin = (frame_reassembly_buffer[0] & FIN) != 0;

    // Decode the reserved bits, and reject the frame if any are set.
    let reserved_bits = (frame_reassembly_buffer[0] >> 4) & 0x07;
    if reserved_bits != 0 {
        //        close(1002, "reserved bits set", true);
        return Err(Error::BadFrame("reserved bits set"));
    }

    // Decode the MASK flag.
    let mask = (frame_reassembly_buffer[1] & MASK) != 0;

    // Verify the MASK flag is correct.  If we are supposed to have data
    // masked in the receive direction, MASK should be set.  Otherwise,
    // it should be clear.
    match (mask, mask_direction) {
        (true, MaskDirection::Transmit) => {
            // Close(1002, "masked frame", true);
            return Err(Error::BadFrame("masked frame"));
        },
        (false, MaskDirection::Receive) => {
            // Close(1002, "unmasked frame", true);
            return Err(Error::BadFrame("unmasked frame"));
        },
        _ => (),
    }

    // Recover the payload from the frame, applying the mask if necessary.
    let payload =
        &frame_reassembly_buffer[header_length..header_length + payload_length];
    if mask {
        message_reassembly_buffer.reserve(payload_length);
        let mask_bytes =
            &frame_reassembly_buffer[header_length - 4..header_length];
        message_reassembly_buffer.extend(
            payload
                .iter()
                .zip(mask_bytes.iter().cycle())
                .map(|(payload_byte, mask_byte)| payload_byte ^ mask_byte),
        );
    } else {
        message_reassembly_buffer.extend(payload);
    }

    // Decode the opcode.  This determines:
    // * If this is a continuation of a fragmented message, or the first (and
    //   perhaps only) fragment of a new message.
    // * The type of message.
    let opcode = frame_reassembly_buffer[0] & 0x0F;
    match opcode {
        OPCODE_PING => {
            println!(
                "receive_frame: PING({})",
                String::from_utf8_lossy(payload)
            );
            // TODO: Check to see if we should verify FIN is set.
            // It doesn't make any sense for it to be clear, since a ping
            // frame can never be fragmented.
            let payload = payload.to_vec();
            work_in_sender
                .unbounded_send(WorkerMessage::Send {
                    set_fin: SetFin::Yes,
                    opcode: OPCODE_PONG,
                    data: payload.clone(),
                })
                .expect("worker thread dropped its receiver");
            let mut received_messages = received_messages
                .lock()
                .expect("the last holder of the received messages panicked");
            received_messages.queue.push_back(Message::Ping(payload));
            if let Some(waker) = received_messages.waker.take() {
                waker.wake();
            }
            Ok(())
        },

        _ => {
            // Close(1002, "unknown opcode", true);
            Err(Error::BadFrame("unknown opcode"))
        },
    }
}

async fn receive_frames(
    mut connection_rx: Box<dyn ConnectionRx>,
    received_messages: Arc<Mutex<ReceivedMessages>>,
    max_frame_size: Option<usize>,
    mask_direction: MaskDirection,
    mut work_in_sender: mpsc::UnboundedSender<WorkerMessage>,
) {
    println!("receive_frames: begin");
    let mut frame_reassembly_buffer = Vec::new();
    let mut message_reassembly_buffer = Vec::new();
    loop {
        // Wait for more data to arrive.
        println!("receive_frames: Reading more data");
        if receive_bytes(&mut connection_rx, &mut frame_reassembly_buffer)
            .await
            .map_err(|error| {
                println!("receive_frames: error: {:?}", error);
                error
            })
            .is_err()
        {
            println!("receive_frames: Connection broken");
            break;
        }

        // Proceed only if we have enough data to recover the first length
        // octet.
        if frame_reassembly_buffer.len() < 2 {
            println!("receive_frames: not enough bytes to decode frame length");
            continue;
        }

        // Figure out the payload length, based on the first length octet.
        // It will be either 1, 2, or 8 bytes long, depending on the value
        // of that first byte.  We may loop back to the top if we find that
        // we haven't received enough bytes to determine the payload length.
        if let Some((mut header_length, payload_length)) =
            match frame_reassembly_buffer[1] & !MASK {
                126 => {
                    let header_length = 4;
                    if frame_reassembly_buffer.len() < header_length {
                        None
                    } else {
                        let payload_length =
                            ((frame_reassembly_buffer[2] as usize) << 8)
                                + frame_reassembly_buffer[3] as usize;
                        Some((header_length, payload_length))
                    }
                },
                127 => {
                    let header_length = 10;
                    if frame_reassembly_buffer.len() < header_length {
                        None
                    } else {
                        let payload_length =
                            ((frame_reassembly_buffer[2] as usize) << 56)
                                + ((frame_reassembly_buffer[3] as usize) << 48)
                                + ((frame_reassembly_buffer[4] as usize) << 40)
                                + ((frame_reassembly_buffer[5] as usize) << 32)
                                + ((frame_reassembly_buffer[6] as usize) << 24)
                                + ((frame_reassembly_buffer[7] as usize) << 16)
                                + ((frame_reassembly_buffer[8] as usize) << 8)
                                + frame_reassembly_buffer[9] as usize;
                        Some((header_length, payload_length))
                    }
                },
                length_first_octet => {
                    let header_length = 2;
                    let payload_length = length_first_octet as usize;
                    Some((header_length, payload_length))
                },
            }
        {
            if (frame_reassembly_buffer[1] & MASK) != 0 {
                header_length += 4;
            }
            let frame_length = header_length + payload_length;
            println!(
                "receive_frames: next frame is {} header bytes, {} payload bytes",
                header_length,
                payload_length
            );
            // TODO: We will need a way to configure the maximum frame size.
            if let Some(max_frame_size) = max_frame_size {
                if frame_length > max_frame_size {
                    // close(1009, "frame too large", true);
                    break;
                }
            }
            if frame_reassembly_buffer.len() < frame_length {
                continue;
            }
            if receive_frame(
                &mut message_reassembly_buffer,
                &frame_reassembly_buffer[0..frame_length],
                header_length,
                payload_length,
                mask_direction,
                &received_messages,
                &mut work_in_sender,
            )
            .is_err()
            {
                println!("receive_frames: oops!  bad frame?");
                break;
            }
            // TODO: This is expensive, especially if we get a large number
            // of bytes following the frame we just received.  Look for
            // a better way of handling the left-overs.
            frame_reassembly_buffer.drain(0..frame_length);
        } else {
            // TODO: We will need a way to configure the maximum frame size.
            if let Some(max_frame_size) = max_frame_size {
                if frame_reassembly_buffer.len() >= max_frame_size {
                    // close(1009, "frame too large", true);
                    break;
                }
            }
        }
    }
    println!("receive_frames: end");
}

// The use of `futures::select!` seems to trigger this warning somehow
// that isn't yet understood.
#[allow(clippy::mut_mut)]
async fn worker(
    work_in_sender: mpsc::UnboundedSender<WorkerMessage>,
    work_in_receiver: mpsc::UnboundedReceiver<WorkerMessage>,
    received_messages: Arc<Mutex<ReceivedMessages>>,
    connection_tx: Box<dyn ConnectionTx>,
    connection_rx: Box<dyn ConnectionRx>,
    mask_direction: MaskDirection,
) {
    //    let mut close_sent = false;

    // Drive to completion the stream of messages to the worker thread.
    println!("worker: processing messages");
    let connection_tx = RefCell::new(connection_tx);
    let rng = RefCell::new(StdRng::from_entropy());
    let processing_tx = work_in_receiver.map(Ok).try_for_each(|message| {
        handle_message(message, &connection_tx, &rng, mask_direction)
    });
    let processing_rx = receive_frames(
        connection_rx,
        received_messages.clone(),
        None,
        mask_direction,
        work_in_sender,
    );
    futures::select!(
        _ = processing_tx.fuse() => (),
        _ = processing_rx.fuse() => (),
    );
    let mut received_messages = received_messages
        .lock()
        .expect("the last holder of the received messages panicked");
    received_messages.fused = true;
    if let Some(waker) = received_messages.waker.take() {
        waker.wake();
    }
    println!("worker: exiting");
}

struct ReceivedMessages {
    fused: bool,
    queue: VecDeque<Message>,
    waker: Option<Waker>,
}

#[must_use]
pub struct WebSocket {
    received_messages: Arc<Mutex<ReceivedMessages>>,

    // This sender is used to deliver messages to the worker thread.
    work_in: mpsc::UnboundedSender<WorkerMessage>,

    // This is our handle to join the worker thread when dropped.
    worker: Option<std::thread::JoinHandle<()>>,
}

impl WebSocket {
    pub(crate) fn new(
        connection_tx: Box<dyn ConnectionTx>,
        connection_rx: Box<dyn ConnectionRx>,
        mask_direction: MaskDirection,
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
            received_messages: received_messages.clone(),
            work_in: sender.clone(),
            worker: Some(thread::spawn(move || {
                executor::block_on(worker(
                    sender,
                    receiver,
                    received_messages,
                    connection_tx,
                    connection_rx,
                    mask_direction,
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
        let data = data.into();
        if data.len() > MAX_CONTROL_FRAME_DATA_LENGTH {
            Err(Error::FramePayloadTooLarge)
        } else {
            self.send_frame(SetFin::Yes, OPCODE_PING, data);
            Ok(())
        }
    }

    fn send_frame(
        &mut self,
        set_fin: SetFin,
        opcode: u8,
        data: Vec<u8>,
    ) {
        // This can fail if the connection has been dropped and worker
        // thread has exited before the user tries to send a frame.
        // In this case, right now we simply ignore the request.
        //
        // TODO: We might want to instead return an error.
        let _ = self.work_in.unbounded_send(WorkerMessage::Send {
            set_fin,
            opcode,
            data,
        });
    }
}

pub enum Message {
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Text(String),
    Binary(Vec<u8>),
    Close {
        code: usize,
        reason: String,
    },
}

impl Stream for WebSocket {
    type Item = Message;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        // TODO: Look into using an asynchronous Mutex to avoid blocking
        // at all, even for access to the `received_messages`.
        let mut received_messages = self
            .received_messages
            .lock()
            .expect("the last holder of the received messages panicked");
        if received_messages.queue.is_empty() {
            if received_messages.fused {
                std::task::Poll::Ready(None)
            } else {
                received_messages.waker.replace(cx.waker().clone());
                std::task::Poll::Pending
            }
        } else {
            let message = received_messages.queue.pop_front();
            std::task::Poll::Ready(message)
        }
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

mod tests {
    use super::*;
    use crate::mock_connection;
    use async_std::future::timeout;

    const REASONABLE_FAST_OPERATION_TIMEOUT: std::time::Duration =
        std::time::Duration::from_millis(20);

    #[test]
    fn server_send_ping_normal_with_data() {
        let (connection_tx, mut connection_back_tx) =
            mock_connection::Tx::new();
        let (connection_rx, _connection_back_rx) = mock_connection::Rx::new();
        let mut ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Receive,
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
        );
        assert!(matches!(
            ws.ping("x".repeat(126)),
            Err(Error::FramePayloadTooLarge)
        ));
        let mut expected_output = vec![0x89, 0x7D];
        expected_output.append(&mut [b'x'; 125].to_vec());
        assert_eq!(None, connection_back_tx.web_socket_output());
    }

    #[test]
    fn receive_ping() {
        let (connection_tx, connection_back_tx) = mock_connection::Tx::new();
        let (connection_rx, connection_back_rx) = mock_connection::Rx::new();
        let ws = WebSocket::new(
            Box::new(connection_tx),
            Box::new(connection_rx),
            MaskDirection::Transmit,
        );
        let pings = RefCell::new(Vec::new());
        let connection_back_tx = RefCell::new(connection_back_tx);
        let connection_back_rx = RefCell::new(connection_back_rx);
        let reader = async {
            ws.for_each(|message| async {
                if let Message::Ping(message) = message {
                    // Expect to receive back the matching "PONG" message which
                    // the WebSocket should send when it gets the "PING".
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

                    // Record the PONG we received.
                    pings.borrow_mut().push(message);

                    // Now the we're done, we can close the connection.
                    println!("Closing connection now");
                    connection_back_rx.borrow_mut().close();
                } else {
                    panic!("we got something that isn't a ping!");
                }
            })
            .await;
        };
        let frame = &b"\x89\x06World!"[..];
        connection_back_rx.borrow_mut().web_socket_input(frame);
        assert!(executor::block_on(timeout(
            REASONABLE_FAST_OPERATION_TIMEOUT,
            reader,
        ))
        .is_ok());

        // Verify the ping was delivered back through the WebSocket's Stream
        // interface.
        assert_eq!(vec!["World!".as_bytes()], *pings.borrow());
    }
}

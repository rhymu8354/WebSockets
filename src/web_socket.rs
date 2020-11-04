use super::{
    Connection,
    Error,
};
use futures::{
    channel::mpsc,
    executor,
    stream::{
        StreamExt,
        TryStreamExt,
    },
    AsyncWriteExt,
};
use rand::{
    rngs::StdRng,
    Rng,
    SeedableRng,
};
use std::{
    cell::RefCell,
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
    connection: &mut Box<dyn Connection>,
    set_fin: SetFin,
    opcode: u8,
    mut payload: Vec<u8>,
    mask_direction: MaskDirection,
    rng: &mut StdRng,
) -> Result<(), Error> {
    let num_payload_bytes = payload.len();
    let mut frame = Vec::with_capacity(num_payload_bytes + 14);
    frame.push(
        match set_fin {
            SetFin::Yes => FIN,
            SetFin::No => 0,
        } + opcode,
    );
    let mask = match mask_direction {
        MaskDirection::Receive => 0,
        MaskDirection::Transmit => MASK,
    };
    // TODO: Find out if there's a nicer way to avoid the truncation
    // warning about casting to `u8`.
    #[allow(clippy::cast_possible_truncation)]
    match num_payload_bytes {
        0..=125 => {
            frame.push(num_payload_bytes as u8 + mask);
        },
        126..=65535 => {
            frame.push(0x7E + mask);
            frame.push((num_payload_bytes >> 8) as u8);
            frame.push((num_payload_bytes & 0xFF) as u8);
        },
        _ => {
            frame.push(0x7F + mask);
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
    if mask == 0 {
        frame.append(&mut payload);
    } else {
        let mut masking_key = [0; 4];
        rng.fill(&mut masking_key);
        frame.extend(masking_key.iter());
        for (i, byte) in payload.iter().enumerate() {
            frame.push(byte ^ masking_key[i % 4]);
        }
    }
    connection.write_all(&frame).await.map_err(Error::ConnectionBroken)
}

async fn handle_message(
    message: WorkerMessage,
    connection: &RefCell<Box<dyn Connection>>,
    rng: &RefCell<StdRng>,
    mask_direction: MaskDirection,
) -> Result<(), ()> {
    match message {
        WorkerMessage::Exit => Err(()),

        WorkerMessage::Send {
            set_fin,
            opcode,
            data,
        } => {
            println!("worker: got Send message");
            send_frame(
                &mut connection.borrow_mut(),
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

async fn worker(
    work_in_receiver: mpsc::UnboundedReceiver<WorkerMessage>,
    connection: Box<dyn Connection>,
    mask_direction: MaskDirection,
) {
    //    let mut close_sent = false;

    // Drive to completion the stream of messages to the worker thread.
    println!("worker: processing messages");
    let connection = RefCell::new(connection);
    let rng = RefCell::new(StdRng::from_entropy());
    work_in_receiver
        .map(Ok)
        .try_for_each(|message| {
            handle_message(message, &connection, &rng, mask_direction)
        })
        .await
        .unwrap_or(());
    println!("worker: exiting");
}

#[must_use]
pub struct WebSocket {
    // This sender is used to deliver messages to the worker thread.
    work_in: mpsc::UnboundedSender<WorkerMessage>,

    // This is our handle to join the worker thread when dropped.
    worker: Option<std::thread::JoinHandle<()>>,
}

impl WebSocket {
    pub fn new(
        connection: Box<dyn Connection>,
        mask_direction: MaskDirection,
    ) -> Self {
        // Make the channel used to communicate with the worker thread.
        let (sender, receiver) = mpsc::unbounded();

        // Store the sender end of the channel and spawn the worker thread,
        // giving it the receiver end as well as the connection.
        Self {
            work_in: sender,
            worker: Some(thread::spawn(move || {
                executor::block_on(worker(receiver, connection, mask_direction))
            })),
        }
    }

    pub fn ping<T>(
        &mut self,
        data: T,
    ) where
        T: Into<Vec<u8>>,
    {
        let data = data.into();
        if data.len() > MAX_CONTROL_FRAME_DATA_LENGTH {
            return;
        }
        self.send_frame(SetFin::Yes, OPCODE_PING, data);
    }

    fn send_frame(
        &mut self,
        set_fin: SetFin,
        opcode: u8,
        data: Vec<u8>,
    ) {
        self.work_in
            .unbounded_send(WorkerMessage::Send {
                set_fin,
                opcode,
                data,
            })
            // This fails only if the worker has dropped the receiver,
            // which only happens if the worker has exited, which it shouldn't
            // until the WebSocket is dropped.
            .expect("worker thread dropped its receiver");
    }
}

impl Drop for WebSocket {
    fn drop(&mut self) {
        // Tell the worker thread to stop.
        //
        // It shouldn't be possible for this to fail, since the worker holds
        // the receiver for this channel, and we haven't joined or dropped the
        // worker yet (we will a few lines later).  So if it does fail, we want
        // to know about it since it would mean we have a bug.
        self.work_in
            .unbounded_send(WorkerMessage::Exit)
            .expect("worker message dropped before it could reach the worker");

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
mod tests {
    use super::*;
    use crate::mock_connection::MockConnection;

    #[test]
    fn server_send_ping_normal_with_data() {
        let (connection, mut connection_back) = MockConnection::new();
        let mut ws =
            WebSocket::new(Box::new(connection), MaskDirection::Receive);
        ws.ping("Hello");
        assert_eq!(
            Some(&b"\x89\x05Hello"[..]),
            connection_back.web_socket_output().as_deref()
        );
    }
}

use super::{
    Error,
    FrameSender,
    MaskDirection,
    MessageInProgress,
    ReceivedCloseFrame,
    ReceivedMessages,
    SetFin,
    StreamMessage,
    FIN,
    MASK,
    OPCODE_BINARY,
    OPCODE_CLOSE,
    OPCODE_CONTINUATION,
    OPCODE_PING,
    OPCODE_PONG,
    OPCODE_TEXT,
};
use async_mutex::Mutex;

pub struct FrameReceiver<'a> {
    mask_direction: MaskDirection,
    frame_sender: &'a Mutex<FrameSender>,
    message_reassembly_buffer: Vec<u8>,
    message_in_progress: MessageInProgress,
    received_messages: &'a Mutex<ReceivedMessages>,
}

impl<'a> FrameReceiver<'a> {
    pub fn new(
        mask_direction: MaskDirection,
        frame_sender: &'a Mutex<FrameSender>,
        received_messages: &'a Mutex<ReceivedMessages>,
    ) -> Self {
        Self {
            mask_direction,
            frame_sender,
            message_in_progress: MessageInProgress::None,
            message_reassembly_buffer: Vec::new(),
            received_messages,
        }
    }

    pub async fn receive_frame(
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
        self.frame_sender
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

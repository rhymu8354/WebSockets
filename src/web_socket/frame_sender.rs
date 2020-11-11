use super::{
    ConnectionTx,
    Error,
    MaskDirection,
    SetFin,
    VecExt,
    FIN,
    MASK,
    OPCODE_CLOSE,
};
use futures::{
    channel::oneshot,
    AsyncWriteExt,
};
use rand::{
    rngs::StdRng,
    Rng,
    SeedableRng,
};

pub struct FrameSender {
    close_sent: Option<oneshot::Sender<()>>,
    connection_tx: Box<dyn ConnectionTx>,
    mask_direction: MaskDirection,
    rng: StdRng,
}

impl FrameSender {
    pub fn new(
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

    pub async fn send_frame(
        &mut self,
        set_fin: SetFin,
        opcode: u8,
        payload: &[u8],
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
            frame.extend(payload);
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
            frame.extend(
                payload
                    .iter()
                    .zip(masking_key.iter().cycle())
                    .map(|(&payload_byte, &mask)| payload_byte ^ mask),
            );
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

    pub async fn send_close(
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
        let _ = self.send_frame(SetFin::Yes, OPCODE_CLOSE, &payload).await;
    }
}

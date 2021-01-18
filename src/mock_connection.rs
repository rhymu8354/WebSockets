#![cfg(test)]

use super::timeout::timeout;
use futures::{
    channel::mpsc,
    executor,
    stream::StreamExt,
    AsyncRead,
    AsyncWrite,
};
use std::{
    io::Write,
    task::Poll,
};

pub struct BackEndTx {
    receiver: mpsc::UnboundedReceiver<Vec<u8>>,
}

pub struct BackEndRx {
    sender: mpsc::UnboundedSender<Vec<u8>>,
}

impl BackEndTx {
    pub async fn web_socket_output_async(&mut self) -> Option<Vec<u8>> {
        timeout(std::time::Duration::from_millis(200), self.receiver.next())
            .await
            .unwrap_or(None)
    }

    pub fn web_socket_output(&mut self) -> Option<Vec<u8>> {
        executor::block_on(self.web_socket_output_async())
    }
}

impl BackEndRx {
    pub fn close(&mut self) {
        self.sender.close_channel();
    }

    pub fn web_socket_input<T>(
        &mut self,
        data: T,
    ) where
        T: Into<Vec<u8>>,
    {
        let data = data.into();
        let _ = self.sender.unbounded_send(data);
    }
}

pub struct Tx {
    sender: mpsc::UnboundedSender<Vec<u8>>,
}

pub struct Rx {
    buffer: Vec<u8>,
    receiver: mpsc::UnboundedReceiver<Vec<u8>>,
}

impl Tx {
    pub fn new() -> (Self, BackEndTx) {
        let (sender, receiver) = mpsc::unbounded();
        (
            Self {
                sender,
            },
            BackEndTx {
                receiver,
            },
        )
    }
}

impl Rx {
    pub fn new() -> (Self, BackEndRx) {
        let (sender, receiver) = mpsc::unbounded();
        (
            Self {
                buffer: Vec::new(),
                receiver,
            },
            BackEndRx {
                sender,
            },
        )
    }
}

impl AsyncRead for Rx {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        mut buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.buffer.is_empty() {
            if let Poll::Ready(buffer) = self.receiver.poll_next_unpin(cx) {
                if let Some(buffer) = buffer {
                    self.buffer = buffer;
                } else {
                    return Poll::Ready(Ok(0));
                }
            } else {
                return Poll::Pending;
            }
        }
        let mut total_bytes_read = 0;
        while !buf.is_empty() {
            if self.buffer.is_empty() {
                break;
            }
            let bytes_read = buf.write(&self.buffer).unwrap();
            total_bytes_read += bytes_read;
            if bytes_read == self.buffer.len() {
                self.buffer.clear();
            } else {
                self.buffer.drain(0..bytes_read);
            }
        }
        Poll::Ready(Ok(total_bytes_read))
    }
}

impl AsyncWrite for Tx {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let num_bytes = buf.len();
        self.sender.unbounded_send(buf.into()).unwrap_or(());
        Poll::Ready(Ok(num_bytes))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Pending
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Pending
    }
}

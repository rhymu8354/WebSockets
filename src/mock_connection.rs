#![cfg(test)]

use async_std::future::timeout;
use futures::{
    channel::mpsc,
    executor,
    stream::StreamExt,
    AsyncRead,
    AsyncWrite,
};

pub struct BackEnd {
    receiver: Option<mpsc::UnboundedReceiver<Vec<u8>>>,
}

impl BackEnd {
    pub fn web_socket_output(&mut self) -> Option<Vec<u8>> {
        executor::block_on(async {
            let mut receiver = self.receiver.take().unwrap();
            let result =
                timeout(std::time::Duration::from_millis(200), receiver.next())
                    .await;
            self.receiver.replace(receiver);
            result.unwrap_or(None)
        })
    }
}

pub struct MockConnection {
    sender: mpsc::UnboundedSender<Vec<u8>>,
}

impl MockConnection {
    pub fn new() -> (Self, BackEnd) {
        let (sender, receiver) = mpsc::unbounded();
        (
            Self {
                sender,
            },
            BackEnd {
                receiver: Some(receiver),
            },
        )
    }
}

impl AsyncRead for MockConnection {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        _buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Pending
    }
}

impl AsyncWrite for MockConnection {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let num_bytes = buf.len();
        self.sender.unbounded_send(buf.into()).unwrap_or(());
        std::task::Poll::Ready(Ok(num_bytes))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Pending
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Pending
    }
}

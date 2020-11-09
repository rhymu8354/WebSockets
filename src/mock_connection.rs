#![cfg(test)]

use super::timeout::timeout;
use async_mutex::Mutex;
use futures::{
    channel::mpsc,
    executor,
    stream::StreamExt,
    AsyncRead,
    AsyncWrite,
    Future,
};
use std::{
    io::Write,
    sync::Arc,
    task::{
        Poll,
        Waker,
    },
};

pub struct BackEndTx {
    receiver: Option<mpsc::UnboundedReceiver<Vec<u8>>>,
}

struct SharedRx {
    fused: bool,
    input: Vec<Vec<u8>>,
    input_waker: Option<Waker>,
}

pub struct BackEndRx {
    shared: Arc<Mutex<SharedRx>>,
}

impl BackEndTx {
    pub async fn web_socket_output_async(&mut self) -> Option<Vec<u8>> {
        let mut receiver = self.receiver.take().unwrap();
        let result =
            timeout(std::time::Duration::from_millis(200), receiver.next())
                .await;
        self.receiver.replace(receiver);
        result.unwrap_or(None)
    }

    pub fn web_socket_output(&mut self) -> Option<Vec<u8>> {
        executor::block_on(self.web_socket_output_async())
    }
}

impl BackEndRx {
    pub async fn close(&mut self) {
        let mut shared = self.shared.lock().await;
        shared.fused = true;
        if let Some(waker) = shared.input_waker.take() {
            waker.wake();
        }
    }

    pub async fn web_socket_input_async<T>(
        &mut self,
        data: T,
    ) where
        T: Into<Vec<u8>>,
    {
        let data = data.into();
        let mut shared = self.shared.lock().await;
        shared.input.push(data);
        if let Some(waker) = shared.input_waker.take() {
            waker.wake();
        }
    }

    pub fn web_socket_input<T>(
        &mut self,
        data: T,
    ) where
        T: Into<Vec<u8>>,
    {
        executor::block_on(self.web_socket_input_async(data))
    }
}

pub struct Tx {
    sender: mpsc::UnboundedSender<Vec<u8>>,
}

pub struct Rx {
    shared: Arc<Mutex<SharedRx>>,
}

impl Tx {
    pub fn new() -> (Self, BackEndTx) {
        let (sender, receiver) = mpsc::unbounded();
        (
            Self {
                sender,
            },
            BackEndTx {
                receiver: Some(receiver),
            },
        )
    }
}

impl Rx {
    pub fn new() -> (Self, BackEndRx) {
        let shared = Arc::new(Mutex::new(SharedRx {
            fused: false,
            input: Vec::new(),
            input_waker: None,
        }));
        (
            Self {
                shared: shared.clone(),
            },
            BackEndRx {
                shared,
            },
        )
    }
}

impl AsyncRead for Rx {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        mut buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        if let Poll::Ready(mut shared) =
            Box::pin(self.shared.lock()).as_mut().poll(cx)
        {
            if shared.input.is_empty() {
                if shared.fused {
                    Poll::Ready(Ok(0))
                } else {
                    shared.input_waker.replace(cx.waker().clone());
                    Poll::Pending
                }
            } else {
                let mut total_bytes_read = 0;
                while !buf.is_empty() {
                    if let Some(input_front) = shared.input.first_mut() {
                        let bytes_read = buf.write(&input_front).unwrap();
                        total_bytes_read += bytes_read;
                        if bytes_read == input_front.len() {
                            shared.input.remove(0);
                        } else {
                            input_front.drain(0..bytes_read);
                        }
                    } else {
                        break;
                    }
                }
                Poll::Ready(Ok(total_bytes_read))
            }
        } else {
            Poll::Pending
        }
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

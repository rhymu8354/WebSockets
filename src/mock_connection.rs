#![cfg(test)]

use async_std::future::timeout;
use futures::{
    channel::mpsc,
    executor,
    stream::StreamExt,
    AsyncRead,
    AsyncWrite,
};
use std::{
    io::Write,
    sync::{
        Arc,
        Mutex,
    },
    task::Waker,
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
    pub fn close(&mut self) {
        let mut shared = self.shared.lock().unwrap();
        shared.fused = true;
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
        let data = data.into();
        let mut shared = self.shared.lock().unwrap();
        println!(
            "web_socket_input: feeding the WebSocket {} bytes",
            data.len()
        );
        shared.input.push(data);
        if let Some(waker) = shared.input_waker.take() {
            println!("web_socket_input: hey, we have a customer waiting!");
            waker.wake();
        } else {
            println!("web_socket_input: it's very lonely here with nobody waiting for data");
        }
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
    ) -> std::task::Poll<std::io::Result<usize>> {
        let mut shared = self.shared.lock().unwrap();
        println!("poll_read: we want {} bytes plz kthxbye", buf.len());
        if shared.input.is_empty() {
            println!("poll_read: oh no, it's empty!");
            if shared.fused {
                println!("poll_read: drat, the fuse has been lit!");
                std::task::Poll::Ready(Ok(0))
            } else {
                println!("poll_read: come back tomorrow");
                shared.input_waker.replace(cx.waker().clone());
                std::task::Poll::Pending
            }
        } else {
            let mut total_bytes_read = 0;
            while !buf.is_empty() {
                println!("poll_read: they can take {} more", buf.len());
                if let Some(input_front) = shared.input.first_mut() {
                    let bytes_read = buf.write(&input_front).unwrap();
                    println!("poll_read: we gave them {} more", bytes_read);
                    total_bytes_read += bytes_read;
                    if bytes_read == input_front.len() {
                        println!("poll_read: so much for that buffer!");
                        shared.input.remove(0);
                    } else {
                        input_front.drain(0..bytes_read);
                        println!(
                            "poll_read: there are {} bytes left in the buffer",
                            input_front.len()
                        );
                    }
                } else {
                    println!("poll_read: we ran out of buffers, but they still wanted more!");
                    break;
                }
            }
            println!("poll_read: we got {} bytes for you", total_bytes_read);
            std::task::Poll::Ready(Ok(total_bytes_read))
        }
    }
}

impl AsyncWrite for Tx {
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

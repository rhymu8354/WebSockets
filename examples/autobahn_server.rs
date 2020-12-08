use async_ctrlc::CtrlC;
use futures::{
    channel::mpsc,
    executor,
    future,
    future::LocalBoxFuture,
    AsyncReadExt,
    FutureExt as _,
    SinkExt as _,
    StreamExt as _,
};
use rhymuweb::{
    Request,
    Response,
};
use rhymuweb_server::{
    Connection,
    FetchResults,
    HttpServer,
    ResourceFuture,
};
use std::{
    cell::RefCell,
    error::Error as _,
    sync::Arc,
    thread,
};
use structopt::StructOpt;
use websockets::{
    WebSocket,
    WebSocketServerBuilder,
};

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("unable to start the web server")]
    Http(#[source] rhymuweb_server::Error),
}

enum WorkKind {
    WebSocket,
    Receiver {
        receiver: mpsc::UnboundedReceiver<WebSocket>,
        ws: Option<WebSocket>,
    },
}

async fn serve_websocket(
    ws: WebSocket,
    ws_count: usize,
) -> WorkKind {
    let (sink, stream) = ws.split();
    let sink = &RefCell::new(sink);
    stream
        .for_each(|message| async move {
            match message {
                websockets::StreamMessage::Ping(_)
                | websockets::StreamMessage::Pong(_) => {},
                websockets::StreamMessage::Text(message) => {
                    let _ = sink
                        .borrow_mut()
                        .send(websockets::SinkMessage::Text {
                            payload: message,
                            last_fragment: websockets::LastFragment::Yes,
                        })
                        .await;
                },
                websockets::StreamMessage::Binary(message) => {
                    let _ = sink
                        .borrow_mut()
                        .send(websockets::SinkMessage::Binary {
                            payload: message,
                            last_fragment: websockets::LastFragment::Yes,
                        })
                        .await;
                },
                websockets::StreamMessage::Close {
                    code,
                    reason,
                } => {
                    let _ = sink
                        .borrow_mut()
                        .send(if code == 1005 {
                            websockets::SinkMessage::CloseNoStatus
                        } else {
                            websockets::SinkMessage::Close {
                                code,
                                reason,
                            }
                        })
                        .await;
                    let _ = sink.borrow_mut().close().await;
                },
            }
        })
        .await;
    println!("WebSocket #{} was closed", ws_count);
    WorkKind::WebSocket
}

async fn handle_receiver(
    mut receiver: mpsc::UnboundedReceiver<WebSocket>
) -> WorkKind {
    println!("Waiting for the next connection...");
    let ws = receiver.next().await;
    WorkKind::Receiver {
        receiver,
        ws,
    }
}

async fn worker(receiver: mpsc::UnboundedReceiver<WebSocket>) {
    let mut futures: Vec<LocalBoxFuture<WorkKind>> = Vec::new();
    let mut receiver = Some(receiver);
    let mut ws_count = 0;
    loop {
        if let Some(receiver) = receiver.take() {
            let next_ws = handle_receiver(receiver);
            futures.push(Box::pin(next_ws));
        }
        let futures_in = futures;
        let (work_kind, _, mut futures_remaining) =
            future::select_all(futures_in).await;
        match work_kind {
            WorkKind::Receiver {
                receiver: receiver_out,
                ws,
            } => {
                if let Some(ws) = ws {
                    ws_count += 1;
                    println!("Now serving WebSocket #{}", ws_count);
                    receiver.replace(receiver_out);
                    let ws_future = serve_websocket(ws, ws_count);
                    futures_remaining.push(Box::pin(ws_future));
                } else {
                    println!("All done serving WebSockets.  kthxbye");
                    break;
                }
            },
            WorkKind::WebSocket => {},
        }
        futures = futures_remaining;
    }
}

fn handle_test_factory(
    request: Request,
    connection: Box<dyn Connection>,
    sender: mpsc::UnboundedSender<WebSocket>,
) -> ResourceFuture {
    async move {
        match WebSocketServerBuilder::start_open(&request) {
            Ok(response) => FetchResults {
                response,
                connection,
                on_upgraded: Some(Box::new(move |connection| {
                    let (connection_rx, connection_tx) = connection.split();
                    let ws = WebSocketServerBuilder::finish_open(
                        Box::new(connection_tx),
                        Box::new(connection_rx),
                        None,
                    );
                    let _ = sender.unbounded_send(ws);
                })),
            },

            // TODO: Handle specific errors differently.
            Err(_) => {
                let mut response = Response::new();
                response.status_code = 400;
                response.reason_phrase = "bad request".into();
                FetchResults {
                    response,
                    connection,
                    on_upgraded: None,
                }
            },
        }
    }
    .boxed()
}

async fn run_test(
    port: u16,
    sender: mpsc::UnboundedSender<WebSocket>,
) -> Result<(), Error> {
    let mut server = HttpServer::new("rhymu-websockets");
    let sender_for_factory = sender.clone();
    server.register(
        &[b"".to_vec()][..],
        Arc::new(move |request, connection| {
            handle_test_factory(request, connection, sender_for_factory.clone())
        }),
    );
    let server_result = server.start(port).await.map_err(Error::Http);
    CtrlC::new().unwrap().await;
    sender.close_channel();
    drop(server);
    server_result
}

async fn run_tests(port: u16) -> Result<(), Error> {
    let (sender, receiver) = mpsc::unbounded();
    let worker_handle = thread::spawn(|| executor::block_on(worker(receiver)));
    match run_test(port, sender).await {
        Err(error) => match error.source() {
            Some(source) => eprintln!("error: {} ({})", error, source),
            None => eprintln!("error: {}", error),
        },
        Ok(()) => {},
    }
    worker_handle.join().expect("the worker thread panicked");
    Ok(())
}

#[derive(Clone, StructOpt)]
struct Opts {
    /// TCP port on which to listen for incoming connections from the autobahn
    /// testsuite fuzzclient
    #[structopt(default_value = "9002")]
    port: u16,
}

fn main() -> Result<(), Error> {
    let opts: Opts = Opts::from_args();
    executor::block_on(run_tests(opts.port))
}

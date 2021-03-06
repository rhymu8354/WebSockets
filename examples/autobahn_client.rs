use futures::{
    executor,
    AsyncReadExt,
    SinkExt,
    StreamExt,
};
use rhymuri::Uri;
use rhymuweb::Response;
use rhymuweb_client::{
    ConnectionUse,
    HttpClient,
};
use std::{
    cell::RefCell,
    error::Error as _,
};
use structopt::StructOpt;
use websockets::{
    WebSocket,
    WebSocketClientBuilder,
};

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("unable to connect to the web server")]
    Http(#[source] rhymuweb_client::Error),

    #[error("connection was upgraded but we didn't get upgrade information from the client for some reason")]
    NoUpgrade,

    #[error("error in WebSocket")]
    WebSocket(#[source] websockets::Error),

    #[error("received an unexpected response from the web server")]
    UnexpectedResponse(Response),

    #[error("the WebSocket disconnected prematurely")]
    Disconnected,

    #[error("unable to parse text message which should have been a number")]
    IntegerMessageExpected(#[source] std::num::ParseIntError),
}

async fn connect(
    client: &HttpClient,
    uri: Uri,
) -> Result<WebSocket, Error> {
    let (mut ws_builder, mut request) = WebSocketClientBuilder::start_open();
    request.target = uri;
    match client
        .fetch(request, ConnectionUse::Upgrade {
            protocol: String::from("websocket"),
        })
        .await
        .map_err(Error::Http)?
    {
        rhymuweb_client::FetchResults {
            response:
                response
                @
                Response {
                    status_code: 101,
                    ..
                },
            upgrade,
        } => {
            let upgrade = upgrade.ok_or(Error::NoUpgrade)?;
            let (connection_rx, connection_tx) = upgrade.connection.split();
            let ws = ws_builder
                .finish_open(
                    Box::new(connection_tx),
                    Box::new(connection_rx),
                    &response,
                    upgrade.trailer,
                    None,
                )
                .map_err(Error::WebSocket)?;
            Ok(ws)
        },
        rhymuweb_client::FetchResults {
            response,
            ..
        } => Err(Error::UnexpectedResponse(response)),
    }
}

async fn get_case_count<T>(
    client: &HttpClient,
    base_url: T,
) -> Result<usize, Error>
where
    T: AsRef<str>,
{
    let mut ws = connect(
        client,
        Uri::parse(format!("{}/getCaseCount", base_url.as_ref())).unwrap(),
    )
    .await?;
    if let Some(websockets::StreamMessage::Text(message)) = ws.next().await {
        message.parse::<usize>().map_err(Error::IntegerMessageExpected)
    } else {
        Err(Error::Disconnected)
    }
}

async fn run_test<T>(
    client: &HttpClient,
    base_url: T,
    case: usize,
    cases: usize,
    agent: T,
) -> Result<(), Error>
where
    T: AsRef<str>,
{
    println!("Test case {}/{}", case, cases);
    let (sink, stream) = connect(
        client,
        Uri::parse(format!(
            "{}/runCase?case={}&agent={}",
            base_url.as_ref(),
            case,
            agent.as_ref()
        ))
        .unwrap(),
    )
    .await?
    .split();
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
                    reply_sent,
                } => {
                    if !reply_sent {
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
                    }
                },
            }
        })
        .await;
    Ok(())
}

async fn update_reports<T>(
    client: &HttpClient,
    base_url: T,
    agent: T,
) -> Result<(), Error>
where
    T: AsRef<str>,
{
    println!("Updating reports...");
    let mut ws = connect(
        client,
        Uri::parse(format!(
            "{}/updateReports?agent={}",
            base_url.as_ref(),
            agent.as_ref()
        ))
        .unwrap(),
    )
    .await?;
    let _ = ws
        .send(websockets::SinkMessage::Close {
            code: 1000,
            reason: "kthxbye".into(),
        })
        .await;
    Ok(())
}

async fn run_tests<T1, T2>(
    base_url: T1,
    agent: T2,
) -> Result<(), Error>
where
    T1: AsRef<str>,
    T2: AsRef<str>,
{
    let client = HttpClient::new();
    let base_url = base_url.as_ref();
    let cases = get_case_count(&client, base_url).await?;
    println!("There are {} test cases enabled in the fuzzserver.", cases);
    let agent = agent.as_ref();
    for case in 1..=cases {
        match run_test(&client, base_url, case, cases, agent).await {
            Err(error) => match error.source() {
                Some(source) => eprintln!("error: {} ({})", error, source),
                None => eprintln!("error: {}", error),
            },
            Ok(()) => {},
        }
    }
    update_reports(&client, base_url, agent).await?;
    Ok(())
}

#[derive(Clone, StructOpt)]
struct Opts {
    /// Base URI of the autobahn testsuite fuzzserver
    #[structopt(default_value = "ws://192.168.0.221:9001")]
    base_uri: String,
}

fn main() -> Result<(), Error> {
    let opts: Opts = Opts::from_args();
    executor::block_on(run_tests(opts.base_uri, "rhymu-websocket"))
}

use anyhow::{
    anyhow,
    Context,
};
use async_ctrlc::CtrlC;
use async_std::io::stdin;
use futures::{
    future::FutureExt,
    select,
    AsyncReadExt,
    AsyncWriteExt,
    Sink,
    SinkExt,
    StreamExt,
};
use rand::Rng;
use rhymuri::Uri;
use rhymuweb_client::{
    ConnectionUse as HttpClientConnectionUse,
    HttpClient,
};
use structopt::StructOpt;
use websockets::{
    LastFragment as WebSocketLastFragment,
    SinkMessage as WebSocketSinkMessage,
    StreamMessage as WebSocketStreamMessage,
    WebSocket,
    WebSocketClientBuilder,
};

#[derive(Clone, StructOpt)]
struct Opts {
    #[structopt(default_value = "rhymu8354")]
    channel: String,
}

fn handle_incoming_message(message: WebSocketStreamMessage) -> bool {
    match message {
        WebSocketStreamMessage::Text(message) => {
            println!("*** They said: {}", message.trim());
        },
        WebSocketStreamMessage::Binary(_) => {
            println!("*** They said something in binary.");
        },
        WebSocketStreamMessage::Close {
            ..
        } => {
            println!("*** Close received");
            return true;
        },
        _ => {},
    }
    false
}

// TODO: There are problems using this from `use_websocket` that I'll
// need to figure out to get this working.  For now, this code is duplicated
// inside of `use_websocket`.
//
// [14:27] soruh_c10: try where Sink::Error: Send
// [14:28] soruh_c10: *<T as Sink>:
// [14:28] Serayen: i think you want this bind: `<T as Sink<Type>>::Error: Error
// + Send`
//
// async fn send_websocket_close<T>(ws_sink: &mut T) ->
// anyhow::Result<()> where
//     T: Sink<WebSocketSinkMessage>,
// {
//     println!("Sending close...");
//     Ok(ws_sink
//         .send(WebSocketSinkMessage::Close {
//             code: 1000,
//             reason: String::from("kthxbye"),
//         })
//         .await?)
// }

async fn use_websocket<T>(
    ws: WebSocket,
    channel: T,
) -> anyhow::Result<()>
where
    T: AsRef<str>,
{
    let (mut ws_sink, mut ws_stream) = ws.split();
    ws_sink
        .send(WebSocketSinkMessage::Text {
            payload: format!(
                "NICK justinfan{}",
                rand::thread_rng().gen::<u16>()
            ),
            last_fragment: WebSocketLastFragment::Yes,
        })
        .await?;
    ws_sink
        .send(WebSocketSinkMessage::Text {
            payload: format!("JOIN #{}", channel.as_ref()),
            last_fragment: WebSocketLastFragment::Yes,
        })
        .await?;
    println!("***************************************************************");
    println!("*** Connected to Twitch chat.  Good luck!");
    println!("*** Press Ctrl+C to disconnect and exit the program.  Have fun!");
    println!("***************************************************************");
    println!();
    let mut line = String::new();
    let stdin = stdin();
    loop {
        line.clear();
        let stdin_reader = async { stdin.read_line(&mut line).await }.boxed();
        let close = futures::select! {
            written = stdin_reader.fuse() => {
                let _ = written?;
                let line = line.trim();
                ws_sink
                .send(WebSocketSinkMessage::Text {
                    payload: String::from(line),
                    last_fragment: WebSocketLastFragment::Yes,
                })
                .await?;
                false
            },

            message = ws_stream.next().fuse() => match message {
                Some(message) => handle_incoming_message(message),
                None => break Ok(()),
            },
        };
        if close {
            println!("*** Sending close");
            ws_sink
                .send(WebSocketSinkMessage::Close {
                    code: 1000,
                    reason: String::from("thanks for all the fish"),
                })
                .await?;
        }
    }
}

async fn main_async() -> anyhow::Result<()> {
    let opts: Opts = Opts::from_args();
    println!(
        "*** Connecting to Twitch chat and joining channel \"{}\"",
        opts.channel
    );
    let client = HttpClient::default();
    let (mut ws_builder, mut request): (_, rhymuweb::Request) =
        WebSocketClientBuilder::start_open();
    request.target = Uri::parse("wss://irc-ws.chat.twitch.tv")
        .context("parsing the Twitch chat URI")?;
    match client
        .fetch(request, HttpClientConnectionUse::Upgrade {
            protocol: String::from("websocket"),
        })
        .await?
    {
        rhymuweb_client::FetchResults {
            response,
            upgrade,
        } if response.status_code == 101 => {
            let upgrade = upgrade.context(
                "connection to server lost right after WebSocket protocol upgrade"
            )?;
            let (connection_rx, connection_tx) = upgrade.connection.split();
            let ws = ws_builder
                .finish_open(
                    Box::new(connection_tx),
                    Box::new(connection_rx),
                    &response,
                    upgrade.trailer,
                    None,
                )
                .context("finishing client WebSocket handshake")?;
            use_websocket(ws, opts.channel).await
        },
        rhymuweb_client::FetchResults {
            ..
        } => Err(anyhow!("WebSocket server gave us an unexpected response!")),
    }
}

fn main() {
    futures::executor::block_on(async {
        select!(
            result = main_async().fuse() => {
                if let Err(error) = result {
                    eprintln!("Error: {:?}", error);
                }
            },
            () = CtrlC::new().unwrap().fuse() => {
                println!("(Ctrl+C pressed; aborted)");
            },
        )
    });
}

#![warn(clippy::pedantic)]
// TODO: Remove this once ready to publish.
#![allow(clippy::missing_errors_doc)]
// TODO: Uncomment this once ready to publish.
//#![warn(missing_docs)]

mod error;

pub use error::Error;
use futures::{
    AsyncRead,
    AsyncWrite,
};
use rand::{
    rngs::StdRng,
    Rng,
    SeedableRng,
};
pub use rhymuweb::{
    Request,
    Response,
};

// This is the version of the WebSocket protocol that this class supports.
const CURRENTLY_SUPPORTED_WEBSOCKET_VERSION: &str = "13";

// This is the required length of the Base64 decoding of the
// "Sec-WebSocket-Key" header in HTTP requests that initiate a WebSocket
// opening handshake.
const REQUIRED_WEBSOCKET_KEY_LENGTH: usize = 16;

// This is the string added to the "Sec-WebSocket-Key" before computing
// the SHA-1 hash and Base64 encoding the result to form the
// corresponding "Sec-WebSocket-Accept" value.
const WEBSOCKET_KEY_SALT: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

pub trait Connection: AsyncRead + AsyncWrite + Send + Unpin + 'static {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> Connection for T {}

#[must_use]
#[derive(Default)]
pub struct WebSocket {
    key: Option<String>,
}

impl WebSocket {
    pub fn new() -> Self {
        Self {
            key: None,
        }
    }

    pub fn start_open_as_client(&mut self) -> Request {
        let mut request = Request::new();
        request.headers.set_header(
            "Sec-WebSocket-Version",
            CURRENTLY_SUPPORTED_WEBSOCKET_VERSION,
        );
        let mut rng = StdRng::from_entropy();
        let mut nonce = [0; REQUIRED_WEBSOCKET_KEY_LENGTH];
        rng.fill(&mut nonce);
        let key = base64::encode(nonce);
        request.headers.set_header("Sec-WebSocket-Key", &key);
        self.key = Some(key);
        request.headers.set_header("Upgrade", "websocket");
        let mut connection_tokens = request.headers.header_tokens("Connection");
        connection_tokens.push(String::from("upgrade"));
        request.headers.set_header("Connection", connection_tokens.join(", "));
        request
    }

    fn compute_key_answer(key: Option<&str>) -> Result<String, Error> {
        key.as_deref().map_or(Err(Error::HandshakeNotProperlyStarted), |key| {
            if base64::decode(key).map_err(Error::HandshakeKey)?.len()
                == REQUIRED_WEBSOCKET_KEY_LENGTH
            {
                Ok(base64::encode(
                    sha1::Sha1::from(String::from(key) + WEBSOCKET_KEY_SALT)
                        .digest()
                        .bytes(),
                ))
            } else {
                Err(Error::InvalidHandshakeRequest)
            }
        })
    }

    pub fn finish_open_as_client(
        &mut self,
        _connection: Box<dyn Connection>,
        response: Response,
    ) -> Result<(), Error> {
        match response {
            _ if response.status_code != 101 => Err(Error::ProtocolNotSwitched),
            _ if !response
                .headers
                .has_header_token("Connection", "upgrade") =>
            {
                Err(Error::ConnectionNotUpgraded)
            },
            _ if !matches!(
                response.headers.header_value("Upgrade"),
                Some(value) if value.eq_ignore_ascii_case("websocket")
            ) =>
            {
                Err(Error::ProtocolNotUpgradedToWebSocket)
            },
            _ if !matches!(
                response.headers.header_value("Sec-WebSocket-Accept"),
                Some(value) if value == Self::compute_key_answer(self.key.as_deref())?
            ) =>
            {
                Err(Error::InvalidHandshakeResponse)
            },
            _ if !response
                .headers
                .header_tokens("Sec-WebSocket-Extensions")
                .is_empty() =>
            {
                Err(Error::ExtensionNotRequested)
            },
            _ if !response
                .headers
                .header_tokens("Sec-WebSocket-Protocol")
                .is_empty() =>
            {
                Err(Error::SubprotocolNotRequested)
            },
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MockConnection {}

    impl MockConnection {
        fn new() -> Self {
            Self {}
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
            _buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            std::task::Poll::Pending
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

    fn sha1<T>(bytes: T) -> [u8; 20]
    where
        T: AsRef<[u8]>,
    {
        sha1::Sha1::from(bytes).digest().bytes()
    }

    #[test]
    fn initiate_open_as_client() {
        let mut ws = WebSocket::new();
        let request = ws.start_open_as_client();
        assert_eq!(
            Some("13"),
            request.headers.header_value("Sec-WebSocket-Version").as_deref()
        );
        assert!(request.headers.has_header("Sec-WebSocket-Key"));
        let key = request.headers.header_value("Sec-WebSocket-Key");
        assert!(key.is_some());
        assert!(base64::decode(key.unwrap().as_bytes()).is_ok());
        let upgrade = request.headers.header_value("Upgrade");
        assert!(upgrade.is_some());
        assert_eq!("websocket", upgrade.unwrap().to_ascii_lowercase());
        assert!(request.headers.has_header_token("Connection", "upgrade"));
    }

    #[test]
    fn fail_complete_open_as_client_due_to_missing_upgrade() {
        let mut ws = WebSocket::new();
        let request = ws.start_open_as_client();
        let mut response = Response::new();
        response.status_code = 101;
        response.headers.set_header("Connection", "upgrade");
        response.headers.set_header(
            "Sec-WebSocket-Accept",
            base64::encode(sha1(
                request.headers.header_value("Sec-WebSocket-Key").unwrap()
                    + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11",
            )),
        );
        let connection = MockConnection::new();
        assert!(matches!(
            ws.finish_open_as_client(Box::new(connection), response),
            Err(Error::ProtocolNotUpgradedToWebSocket)
        ));
    }

    #[test]
    fn fail_complete_open_as_client_due_to_wrong_upgrade() {
        let mut ws = WebSocket::new();
        let request = ws.start_open_as_client();
        let mut response = Response::new();
        response.status_code = 101;
        response.headers.set_header("Connection", "upgrade");
        response.headers.set_header("Upgrade", "foobar");
        response.headers.set_header(
            "Sec-WebSocket-Accept",
            base64::encode(sha1(
                request.headers.header_value("Sec-WebSocket-Key").unwrap()
                    + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11",
            )),
        );
        let connection = MockConnection::new();
        assert!(matches!(
            ws.finish_open_as_client(Box::new(connection), response),
            Err(Error::ProtocolNotUpgradedToWebSocket)
        ));
    }

    #[test]
    fn fail_complete_open_as_client_due_to_missing_connection() {
        let mut ws = WebSocket::new();
        let request = ws.start_open_as_client();
        let mut response = Response::new();
        response.status_code = 101;
        response.headers.set_header("Upgrade", "websocket");
        response.headers.set_header(
            "Sec-WebSocket-Accept",
            base64::encode(sha1(
                request.headers.header_value("Sec-WebSocket-Key").unwrap()
                    + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11",
            )),
        );
        let connection = MockConnection::new();
        assert!(matches!(
            ws.finish_open_as_client(Box::new(connection), response),
            Err(Error::ConnectionNotUpgraded)
        ));
    }

    #[test]
    fn fail_complete_open_as_client_due_to_wrong_connection() {
        let mut ws = WebSocket::new();
        let request = ws.start_open_as_client();
        let mut response = Response::new();
        response.status_code = 101;
        response.headers.set_header("Connection", "foobar");
        response.headers.set_header("Upgrade", "websocket");
        response.headers.set_header(
            "Sec-WebSocket-Accept",
            base64::encode(sha1(
                request.headers.header_value("Sec-WebSocket-Key").unwrap()
                    + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11",
            )),
        );
        let connection = MockConnection::new();
        assert!(matches!(
            ws.finish_open_as_client(Box::new(connection), response),
            Err(Error::ConnectionNotUpgraded)
        ));
    }

    #[test]
    fn fail_complete_open_as_client_due_to_missing_accept() {
        let mut ws = WebSocket::new();
        ws.start_open_as_client();
        let mut response = Response::new();
        response.status_code = 101;
        response.headers.set_header("Connection", "upgrade");
        response.headers.set_header("Upgrade", "websocket");
        let connection = MockConnection::new();
        assert!(matches!(
            ws.finish_open_as_client(Box::new(connection), response),
            Err(Error::InvalidHandshakeResponse)
        ));
    }

    #[test]
    fn fail_complete_open_as_client_due_to_wrong_accept() {
        let mut ws = WebSocket::new();
        let request = ws.start_open_as_client();
        let mut response = Response::new();
        response.status_code = 101;
        response.headers.set_header("Connection", "upgrade");
        response.headers.set_header("Upgrade", "websocket");
        response.headers.set_header(
            "Sec-WebSocket-Accept",
            base64::encode(sha1(
                request.headers.header_value("Sec-WebSocket-Key").unwrap()
                    + "258EAFA5-E914-47DA-95CA-C5AB0DC85B12",
            )),
        );
        let connection = MockConnection::new();
        assert!(matches!(
            ws.finish_open_as_client(Box::new(connection), response),
            Err(Error::InvalidHandshakeResponse)
        ));
    }

    #[test]
    fn fail_complete_open_as_client_due_to_unsupported_extension() {
        let mut ws = WebSocket::new();
        let request = ws.start_open_as_client();
        let mut response = Response::new();
        response.status_code = 101;
        response.headers.set_header("Connection", "upgrade");
        response.headers.set_header("Upgrade", "websocket");
        response.headers.set_header(
            "Sec-WebSocket-Accept",
            base64::encode(sha1(
                request.headers.header_value("Sec-WebSocket-Key").unwrap()
                    + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11",
            )),
        );
        response.headers.set_header("Sec-WebSocket-Extensions", "foobar");
        let connection = MockConnection::new();
        assert!(matches!(
            ws.finish_open_as_client(Box::new(connection), response),
            Err(Error::ExtensionNotRequested)
        ));
    }

    #[test]
    fn succeed_complete_open_as_client_blank_extensions() {
        let mut ws = WebSocket::new();
        let request = ws.start_open_as_client();
        let mut response = Response::new();
        response.status_code = 101;
        response.headers.set_header("Connection", "upgrade");
        response.headers.set_header("Upgrade", "websocket");
        response.headers.set_header(
            "Sec-WebSocket-Accept",
            base64::encode(sha1(
                request.headers.header_value("Sec-WebSocket-Key").unwrap()
                    + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11",
            )),
        );
        response.headers.set_header("Sec-WebSocket-Extensions", "");
        let connection = MockConnection::new();
        assert!(ws
            .finish_open_as_client(Box::new(connection), response)
            .is_ok());
    }

    #[test]
    fn fail_complete_open_as_client_due_to_unsupported_protocol() {
        let mut ws = WebSocket::new();
        let request = ws.start_open_as_client();
        let mut response = Response::new();
        response.status_code = 101;
        response.headers.set_header("Connection", "upgrade");
        response.headers.set_header("Upgrade", "websocket");
        response.headers.set_header(
            "Sec-WebSocket-Accept",
            base64::encode(sha1(
                request.headers.header_value("Sec-WebSocket-Key").unwrap()
                    + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11",
            )),
        );
        response.headers.set_header("Sec-WebSocket-Protocol", "foobar");
        let connection = MockConnection::new();
        assert!(matches!(
            ws.finish_open_as_client(Box::new(connection), response),
            Err(Error::SubprotocolNotRequested)
        ));
    }

    #[test]
    fn succeed_complete_open_as_client_blank_protocol() {
        let mut ws = WebSocket::new();
        let request = ws.start_open_as_client();
        let mut response = Response::new();
        response.status_code = 101;
        response.headers.set_header("Connection", "upgrade");
        response.headers.set_header("Upgrade", "websocket");
        response.headers.set_header(
            "Sec-WebSocket-Accept",
            base64::encode(sha1(
                request.headers.header_value("Sec-WebSocket-Key").unwrap()
                    + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11",
            )),
        );
        response.headers.set_header("Sec-WebSocket-Protocol", "");
        let connection = MockConnection::new();
        assert!(ws
            .finish_open_as_client(Box::new(connection), response)
            .is_ok());
    }

    #[test]
    fn complete_open_as_client() {
        let mut ws = WebSocket::new();
        let request = ws.start_open_as_client();
        let mut response = Response::new();
        response.status_code = 101;
        response.headers.set_header("Connection", "upgrade");
        response.headers.set_header("Upgrade", "websocket");
        response.headers.set_header(
            "Sec-WebSocket-Accept",
            base64::encode(sha1(
                request.headers.header_value("Sec-WebSocket-Key").unwrap()
                    + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11",
            )),
        );
        let connection = MockConnection::new();
        assert!(ws
            .finish_open_as_client(Box::new(connection), response)
            .is_ok());

        // const std::string data = "Hello";
        // ws.Ping(data);
        // ASSERT_EQ(data.length() + 6, connection->webSocketOutput.length());
        // ASSERT_EQ("\x89\x85", connection->webSocketOutput.substr(0, 2));
        // for (size_t i = 0; i < data.length(); ++i) {
        //     ASSERT_EQ(
        //         data[i] ^ connection->webSocketOutput[2 + (i % 4)],
        //         connection->webSocketOutput[6 + i]
        //     );
        // }
    }
}

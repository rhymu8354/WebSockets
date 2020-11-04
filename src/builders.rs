use super::{
    Connection,
    Error,
    MaskDirection,
    WebSocket,
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

pub fn open_server(
    connection: Box<dyn Connection>,
    request: &Request,
) -> Result<(WebSocket, Response), Error> {
    match request {
        _ if request.method != "GET" => Err(Error::WrongHttpMethod),
        _ if !request.headers.has_header_token("Connection", "upgrade") => {
            Err(Error::UpgradeNotRequested)
        },
        _ if !matches!(
            request.headers.header_value("Upgrade"),
            Some(value) if value.eq_ignore_ascii_case("websocket")
        ) =>
        {
            Err(Error::ProtocolUpgradeRequstNotAWebSocket)
        },
        _ if !matches!(
            request.headers.header_value("Sec-WebSocket-Version"),
            Some(value) if value == CURRENTLY_SUPPORTED_WEBSOCKET_VERSION
        ) =>
        {
            Err(Error::UnsupportedProtocolVersion)
        },
        _ => {
            let mut response = Response::new();
            response.status_code = 101;
            response.reason_phrase = "Switching Protocols".into();
            response.headers.set_header("Connection", "upgrade");
            response.headers.set_header("Upgrade", "websocket");
            response.headers.set_header(
                "Sec-WebSocket-Accept",
                compute_key_answer(
                    request
                        .headers
                        .header_value("Sec-WebSocket-Key")
                        .as_deref(),
                )?,
            );
            Ok((WebSocket::new(connection, MaskDirection::Receive), response))
        },
    }
}

type HandshakeKey = String;

// WebSocket internally is either a client or server, because
#[derive(Debug)]
#[must_use]
pub struct WebSocketClientBuilder {
    key: HandshakeKey,
}

impl WebSocketClientBuilder {
    pub fn start_open() -> (Self, Request) {
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
        request.headers.set_header("Upgrade", "websocket");
        let mut connection_tokens = request.headers.header_tokens("Connection");
        connection_tokens.push(String::from("upgrade"));
        request.headers.set_header("Connection", connection_tokens.join(", "));
        (
            Self {
                key,
            },
            request,
        )
    }

    pub fn finish_open(
        &mut self,
        connection: Box<dyn Connection>,
        response: &Response,
    ) -> Result<WebSocket, Error> {
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
                Some(value) if value == compute_key_answer(Some(&self.key))?
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
            _ => Ok(WebSocket::new(connection, MaskDirection::Transmit)),
        }
    }
}

pub struct WebSocketServerBuilder {}

impl WebSocketServerBuilder {
    pub fn open(
        connection: Box<dyn Connection>,
        request: &Request,
    ) -> Result<(WebSocket, Response), Error> {
        open_server(connection, request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{
        AsyncRead,
        AsyncWrite,
    };

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
        let (_ws, request) = WebSocketClientBuilder::start_open();
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
        let (mut ws, request) = WebSocketClientBuilder::start_open();
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
            ws.finish_open(Box::new(connection), &response),
            Err(Error::ProtocolNotUpgradedToWebSocket)
        ));
    }

    #[test]
    fn fail_complete_open_as_client_due_to_wrong_upgrade() {
        let (mut ws, request) = WebSocketClientBuilder::start_open();
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
            ws.finish_open(Box::new(connection), &response),
            Err(Error::ProtocolNotUpgradedToWebSocket)
        ));
    }

    #[test]
    fn fail_complete_open_as_client_due_to_missing_connection() {
        let (mut ws, request) = WebSocketClientBuilder::start_open();
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
            ws.finish_open(Box::new(connection), &response),
            Err(Error::ConnectionNotUpgraded)
        ));
    }

    #[test]
    fn fail_complete_open_as_client_due_to_wrong_connection() {
        let (mut ws, request) = WebSocketClientBuilder::start_open();
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
            ws.finish_open(Box::new(connection), &response),
            Err(Error::ConnectionNotUpgraded)
        ));
    }

    #[test]
    fn fail_complete_open_as_client_due_to_missing_accept() {
        let (mut ws, _request) = WebSocketClientBuilder::start_open();
        let mut response = Response::new();
        response.status_code = 101;
        response.headers.set_header("Connection", "upgrade");
        response.headers.set_header("Upgrade", "websocket");
        let connection = MockConnection::new();
        assert!(matches!(
            ws.finish_open(Box::new(connection), &response),
            Err(Error::InvalidHandshakeResponse)
        ));
    }

    #[test]
    fn fail_complete_open_as_client_due_to_wrong_accept() {
        let (mut ws, request) = WebSocketClientBuilder::start_open();
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
            ws.finish_open(Box::new(connection), &response),
            Err(Error::InvalidHandshakeResponse)
        ));
    }

    #[test]
    fn fail_complete_open_as_client_due_to_unsupported_extension() {
        let (mut ws, request) = WebSocketClientBuilder::start_open();
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
            ws.finish_open(Box::new(connection), &response),
            Err(Error::ExtensionNotRequested)
        ));
    }

    #[test]
    fn succeed_complete_open_as_client_blank_extensions() {
        let (mut ws, request) = WebSocketClientBuilder::start_open();
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
        assert!(ws.finish_open(Box::new(connection), &response).is_ok());
    }

    #[test]
    fn fail_complete_open_as_client_due_to_unsupported_protocol() {
        let (mut ws, request) = WebSocketClientBuilder::start_open();
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
            ws.finish_open(Box::new(connection), &response),
            Err(Error::SubprotocolNotRequested)
        ));
    }

    #[test]
    fn succeed_complete_open_as_client_blank_protocol() {
        let (mut ws, request) = WebSocketClientBuilder::start_open();
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
        assert!(ws.finish_open(Box::new(connection), &response).is_ok());
    }

    #[test]
    fn complete_open_as_client() {
        let (mut ws, request) = WebSocketClientBuilder::start_open();
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
        assert!(ws.finish_open(Box::new(connection), &response).is_ok());

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

    #[test]
    fn complete_open_as_server() {
        let mut request = Request::new();
        request.method = "GET".into();
        request.headers.set_header("Connection", "upgrade");
        request.headers.set_header("Upgrade", "websocket");
        request.headers.set_header("Sec-WebSocket-Version", "13");
        request
            .headers
            .set_header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==");
        let connection = MockConnection::new();
        let open_result =
            WebSocketServerBuilder::open(Box::new(connection), &request);
        assert!(open_result.is_ok());
        let (_ws, response) = open_result.unwrap();
        assert_eq!(101, response.status_code);
        assert_eq!("Switching Protocols", response.reason_phrase);
        assert_eq!(
            Some("websocket"),
            response.headers.header_value("Upgrade").as_deref()
        );
        assert!(response.headers.has_header_token("Connection", "upgrade"));
        assert_eq!(
            Some("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="),
            response.headers.header_value("Sec-WebSocket-Accept").as_deref()
        );

        // ws.Ping("Hello");
        // ASSERT_EQ("\x89\x05Hello", connection->webSocketOutput);
    }

    #[test]
    fn complete_open_as_server_connection_token_capitalized() {
        let mut request = Request::new();
        request.method = "GET".into();
        request.headers.set_header("Connection", "Upgrade");
        request.headers.set_header("Upgrade", "websocket");
        request.headers.set_header("Sec-WebSocket-Version", "13");
        request
            .headers
            .set_header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==");
        let connection = MockConnection::new();
        let open_result =
            WebSocketServerBuilder::open(Box::new(connection), &request);
        assert!(open_result.is_ok());
    }

    #[test]
    fn fail_complete_open_as_server_not_get_method() {
        let mut request = Request::new();
        request.method = "POST".into();
        request.headers.set_header("Connection", "Upgrade");
        request.headers.set_header("Upgrade", "websocket");
        request.headers.set_header("Sec-WebSocket-Version", "13");
        request
            .headers
            .set_header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==");
        let connection = MockConnection::new();
        assert!(matches!(
            WebSocketServerBuilder::open(Box::new(connection), &request),
            Err(Error::WrongHttpMethod)
        ));
    }

    #[test]
    fn fail_complete_open_as_server_missing_upgrade() {
        let mut request = Request::new();
        request.method = "GET".into();
        request.headers.set_header("Connection", "Upgrade");
        request.headers.set_header("Sec-WebSocket-Version", "13");
        request
            .headers
            .set_header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==");
        let connection = MockConnection::new();
        assert!(matches!(
            WebSocketServerBuilder::open(Box::new(connection), &request),
            Err(Error::ProtocolUpgradeRequstNotAWebSocket)
        ));
    }

    #[test]
    fn fail_complete_open_as_server_wrong_upgrade() {
        let mut request = Request::new();
        request.method = "GET".into();
        request.headers.set_header("Connection", "Upgrade");
        request.headers.set_header("Upgrade", "foobar");
        request.headers.set_header("Sec-WebSocket-Version", "13");
        request
            .headers
            .set_header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==");
        let connection = MockConnection::new();
        assert!(matches!(
            WebSocketServerBuilder::open(Box::new(connection), &request),
            Err(Error::ProtocolUpgradeRequstNotAWebSocket)
        ));
    }

    #[test]
    fn fail_complete_open_as_server_missing_connection() {
        let mut request = Request::new();
        request.method = "GET".into();
        request.headers.set_header("Upgrade", "websocket");
        request.headers.set_header("Sec-WebSocket-Version", "13");
        request
            .headers
            .set_header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==");
        let connection = MockConnection::new();
        assert!(matches!(
            WebSocketServerBuilder::open(Box::new(connection), &request),
            Err(Error::UpgradeNotRequested)
        ));
    }

    #[test]
    fn fail_complete_open_as_server_wrong_connection() {
        let mut request = Request::new();
        request.method = "GET".into();
        request.headers.set_header("Connection", "foobar");
        request.headers.set_header("Upgrade", "websocket");
        request.headers.set_header("Sec-WebSocket-Version", "13");
        request
            .headers
            .set_header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==");
        let connection = MockConnection::new();
        assert!(matches!(
            WebSocketServerBuilder::open(Box::new(connection), &request),
            Err(Error::UpgradeNotRequested)
        ));
    }

    #[test]
    fn fail_complete_open_as_server_missing_key() {
        let mut request = Request::new();
        request.method = "GET".into();
        request.headers.set_header("Connection", "upgrade");
        request.headers.set_header("Upgrade", "websocket");
        request.headers.set_header("Sec-WebSocket-Version", "13");
        let connection = MockConnection::new();
        assert!(matches!(
            WebSocketServerBuilder::open(Box::new(connection), &request),
            Err(Error::HandshakeNotProperlyStarted)
        ));
    }

    #[test]
    fn fail_complete_open_as_server_bad_key() {
        // Perform setup that's common for the three sub-cases.
        let mut request = Request::new();
        request.method = "GET".into();
        request.headers.set_header("Connection", "upgrade");
        request.headers.set_header("Upgrade", "websocket");
        request.headers.set_header("Sec-WebSocket-Version", "13");

        // First try with a key that is one byte too short.
        request
            .headers
            .set_header("Sec-WebSocket-Key", base64::encode("abcdefghijklmno"));
        let connection = MockConnection::new();
        assert!(matches!(
            WebSocketServerBuilder::open(Box::new(connection), &request),
            Err(Error::InvalidHandshakeRequest)
        ));

        // Next try with a key that is one byte too long.
        request.headers.set_header(
            "Sec-WebSocket-Key",
            base64::encode("abcdefghijklmnopq"),
        );
        let connection = MockConnection::new();
        assert!(matches!(
            WebSocketServerBuilder::open(Box::new(connection), &request),
            Err(Error::InvalidHandshakeRequest)
        ));

        // Finally try with a key that is just right.
        request.headers.set_header(
            "Sec-WebSocket-Key",
            base64::encode("abcdefghijklmnop"),
        );
        let connection = MockConnection::new();
        assert!(WebSocketServerBuilder::open(Box::new(connection), &request)
            .is_ok());
    }

    #[test]
    fn fail_complete_open_as_server_missing_version() {
        let mut request = Request::new();
        request.method = "GET".into();
        request.headers.set_header("Connection", "upgrade");
        request.headers.set_header("Upgrade", "websocket");
        request
            .headers
            .set_header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==");
        let connection = MockConnection::new();
        assert!(matches!(
            WebSocketServerBuilder::open(Box::new(connection), &request),
            Err(Error::UnsupportedProtocolVersion)
        ));
    }

    #[test]
    fn fail_complete_open_as_server_bad_version() {
        // Perform setup that's common for the three sub-cases.
        let mut request = Request::new();
        request.method = "GET".into();
        request.headers.set_header("Connection", "upgrade");
        request.headers.set_header("Upgrade", "websocket");
        request
            .headers
            .set_header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==");

        // First, try a version that's too old.
        request.headers.set_header("Sec-WebSocket-Version", "12");
        let connection = MockConnection::new();
        assert!(matches!(
            WebSocketServerBuilder::open(Box::new(connection), &request),
            Err(Error::UnsupportedProtocolVersion)
        ));

        // Next, try a version that's too new.
        request.headers.set_header("Sec-WebSocket-Version", "14");
        let connection = MockConnection::new();
        assert!(matches!(
            WebSocketServerBuilder::open(Box::new(connection), &request),
            Err(Error::UnsupportedProtocolVersion)
        ));

        // Finally, try a version that's supported.
        request.headers.set_header("Sec-WebSocket-Version", "13");
        let connection = MockConnection::new();
        assert!(WebSocketServerBuilder::open(Box::new(connection), &request)
            .is_ok());
    }
}

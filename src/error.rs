/// This is the enumeration of all the different kinds of errors which this
/// crate generates.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The WebSocket could not be opened because the status code in the HTTP
    /// response in the opening handshake was not 101 (Switching Protocols).
    #[error(
        "the upgrade response status code is not 101 (Switching Protocols)"
    )]
    ProtocolNotSwitched,

    /// The WebSocket could not be opened because the HTTP response in the
    /// opening handshake did not indicate that the connection was upgraded at
    /// all.
    #[error("the upgrade response did not indicate that the connection was upgraded")]
    ConnectionNotUpgraded,

    /// The WebSocket could not be opened because the HTTP response in the
    /// opening handshake did not indicate that the connection was upgraded
    /// to a WebSocket.
    #[error("the upgrade response did not indicate that the connection was upgraded to be a WebSocket")]
    ProtocolNotUpgradedToWebSocket,

    /// The WebSocket could not be opened because the HTTP response in the
    /// opening handshake did not contain the correct value for the
    /// `Sec-WebSocket-Accept` header.
    #[error("the upgrade response did not contain the correct `Sec-WebSocket-Accept` header")]
    InvalidHandshakeResponse,

    /// The WebSocket could not be opened because the HTTP response in the
    /// opening handshake indicated an extension we did not request.
    #[error("the upgrade response indicated an extension we did not request")]
    ExtensionNotRequested,

    /// The WebSocket could not be opened because the HTTP response in the
    /// opening handshake indicated a subprotocol we did not request.
    #[error("the upgrade response indicated a subprotocol we did not request")]
    SubprotocolNotRequested,

    /// For a client WebSocket, this happens if `finish_open_as_client` is
    /// called without first calling `start_open_as_client`.
    ///
    /// For a server WebSocket, this happens if the client did not provide
    /// any value for the `Sec-WebSocket-Key` header.
    #[error("the handshake cannot be finished because it was never started")]
    HandshakeNotProperlyStarted,

    /// The WebSocket could not be opened because the HTTP request in the
    /// opening handshake did not use the `GET` method.
    #[error(
        "the client used the wrong method in the handshake request (not GET)"
    )]
    WrongHttpMethod,

    /// The WebSocket could not be opened because the HTTP request in the
    /// opening handshake did not request upgrading the connection.
    #[error("the upgrade request did not indicate that the connection should be upgraded")]
    UpgradeNotRequested,

    /// The WebSocket could not be opened because the HTTP request in the
    /// opening handshake did not indicate that the connection should be
    /// upgraded to a WebSocket.
    #[error("the upgrade request did not indicate that the connection should be upgraded to be a WebSocket")]
    ProtocolUpgradeRequstNotAWebSocket,

    /// The WebSocket could not be opened because the HTTP request in the
    /// opening handshake requested an unsupported version of the WebSocket
    /// protocol.
    #[error("an unsupported version of the WebSocket protocol was requested")]
    UnsupportedProtocolVersion,

    /// The WebSocket could not be opened because the HTTP request in the
    /// opening handshake did not contain a proper value for the
    /// `Sec-WebSocket-Accept` header.
    #[error("the upgrade request did not contain a proper `Sec-WebSocket-Accept` header")]
    InvalidHandshakeRequest,

    /// The WebSocket could not be opened because the HTTP request in the
    /// opening handshake contained a value for the `Sec-WebSocket-Accept`
    /// header which could not be Base64-decoded.
    #[error(
        "unable to decode the value for the `Sec-WebSocket-Accept` header"
    )]
    HandshakeKey(#[source] base64::DecodeError),

    /// The underlying network connection was broken.
    #[error("the underlying network connection was broken")]
    ConnectionBroken(#[source] std::io::Error),

    /// The underlying network connection was gracefully closed.
    #[error("the underlying network connection was gracefully closed")]
    ConnectionClosed,

    /// This indicates that the last frame reconstructed from the receiver was
    /// bad.  A human-readable explanation of why it was bad is included.
    #[error("the last frame reconstructed from the receiver was bad")]
    BadFrame(&'static str),

    /// A function which generates a control frame (`ping` for example) was
    /// called with a payload that is too large to fit in the frame.
    /// Such messages cannot be fragmented into multiple frames.
    #[error("the payload provided is too large to fit into a frame")]
    FramePayloadTooLarge,

    /// The message cannot be sent, or the start of a new message cannot
    /// be accepted, because the previous message was not finished.
    #[error("last message was not finished")]
    LastMessageUnfinished,

    /// A text message was received that is not valid UTF-8.
    #[error("text message received is not valid UTF-8")]
    Utf8 {
        #[source]
        source: std::str::Utf8Error,
        context: &'static str,
    },

    /// The WebSocket received a continuation frame when no message
    /// reconstruction was in progress.
    #[error("received an unexpected continuation frame")]
    UnexpectedContinuationFrame,

    /// The operation could not be completed because the WebSocket
    /// is already closed.
    #[error("WebSocket is already closed")]
    Closed,
}

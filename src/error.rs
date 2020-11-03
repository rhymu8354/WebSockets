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

    /// This happens if `finish_open_as_client` is called without first
    /// calling `start_open_as_client`.
    #[error("the handshake cannot be finished because it was never started")]
    HandshakeNotProperlyStarted,
}

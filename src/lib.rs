#![warn(clippy::pedantic)]
// TODO: Remove this once ready to publish.
#![allow(clippy::missing_errors_doc)]
// TODO: Uncomment this once ready to publish.
//#![warn(missing_docs)]

mod builders;
mod error;
mod mock_connection;
mod web_socket;

pub use builders::{
    open_server,
    WebSocketClientBuilder,
    WebSocketServerBuilder,
};
pub use error::Error;
use futures::{
    AsyncRead,
    AsyncWrite,
};
use web_socket::MaskDirection;
pub use web_socket::WebSocket;

pub trait ConnectionTx: AsyncWrite + Send + Unpin + 'static {}
impl<T: AsyncWrite + Send + Unpin + 'static> ConnectionTx for T {}

pub trait ConnectionRx: AsyncRead + Send + Unpin + 'static {}
impl<T: AsyncRead + Send + Unpin + 'static> ConnectionRx for T {}

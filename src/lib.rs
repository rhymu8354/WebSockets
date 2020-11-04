#![warn(clippy::pedantic)]
// TODO: Remove this once ready to publish.
#![allow(clippy::missing_errors_doc)]
// TODO: Uncomment this once ready to publish.
//#![warn(missing_docs)]

mod builders;
mod error;
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

pub trait Connection: AsyncRead + AsyncWrite + Send + Unpin + 'static {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> Connection for T {}

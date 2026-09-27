//! Bounded 9P2000.L clients for Sophia's role file exports.
//!
//! [`client`] is a blocking, read-only client. [`pipeline`] is a nonblocking,
//! pipelined, write-capable client for the role clients (shell, WM, output,
//! admin). They share request encoders and reply decoders through the private
//! `client_codec` module, and only the value records of
//! [`sophia_9p_records`] with the server core.

pub mod client;
mod client_codec;
pub mod pipeline;

pub use sophia_9p_records as records;

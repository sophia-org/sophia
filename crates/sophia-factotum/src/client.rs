//! Session's single connection owner for PAM and private launch authentication.
//! The synchronous compatibility API is for the unlock supervisor's thread;
//! `SessionClient::poll` and its begin/take methods never wait on the socket.

mod pam;
mod session;

use sophia_9p::Errno;
use sophia_9p_client::pipeline::PipelineError;

pub use session::SessionClient;
/// Compatibility name for existing callers; both names use the same owner.
pub type UnlockClient = SessionClient;

#[derive(Debug)]
pub enum ClientError {
    Transport(PipelineError),
    Remote(Errno),
    Unexpected(&'static str),
    Launch(crate::launch::client::Error),
    Busy,
    InvalidInput,
    Closed,
}

impl From<PipelineError> for ClientError {
    fn from(error: PipelineError) -> Self {
        Self::Transport(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoginVerdict {
    Accepted,
    Refused(String),
}

/// The first length-prefixed field of 9front AuthInfo: cuid. PAM supplies no
/// secret; the login succeeds only for the requested user, as before.
fn first_field(encoded: &[u8]) -> Option<&[u8]> {
    let [low, high, rest @ ..] = encoded else {
        return None;
    };
    rest.get(..usize::from(u16::from_le_bytes([*low, *high])))
}

//! The frames between the agent and its PAM helper: one request, one reply,
//! then the helper exits. Little-endian; every length is checked before any
//! allocation, and no PAM message text is ever carried.
//!
//! ```text
//! request := u32 len | "SFPM" | u8 version=1 | u8 flags | u16 0
//!            | u16 service_len | service | u16 user_len | user
//!            | u16 secret_len | secret
//! reply   := u32 len=8 | "SFPR" | u8 version=1 | u8 verdict | i16 pam_code
//! ```

use crate::proto::PamVerdict;
use crate::secret::SecretBytes;

const REQUEST_MAGIC: &[u8; 4] = b"SFPM";
const REPLY_MAGIC: &[u8; 4] = b"SFPR";
const VERSION: u8 = 1;
/// Linux-PAM's `PAM_MAX_RESP_SIZE`.
pub const MAX_SECRET: usize = 512;
pub const MAX_USER: usize = 256;
pub const MAX_SERVICE: usize = 64;
/// The largest request the helper reads.
pub const MAX_REQUEST: usize = 4 + 4 + 4 + 2 + MAX_SERVICE + 2 + MAX_USER + 2 + MAX_SECRET;
pub const REPLY_LEN: usize = 12;

/// `flags` bit 0: reinitialise credentials after success.
pub const FLAG_SETCRED: u8 = 1 << 0;
/// `flags` bit 1: refuse an empty secret (`PAM_DISALLOW_NULL_AUTHTOK`).
pub const FLAG_DISALLOW_NULL: u8 = 1 << 1;

/// One attempt as the helper receives it.
#[derive(Debug, Eq, PartialEq)]
pub struct HelperRequest {
    pub flags: u8,
    pub service: String,
    pub user: String,
    pub secret: SecretBytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WireError {
    Truncated,
    BadMagic,
    BadVersion,
    BadLength,
    BadField,
}

impl HelperRequest {
    /// The whole frame. Its bytes carry the secret; the caller zeroes them.
    pub fn encode(&self) -> Result<SecretBytes, WireError> {
        let fields = [
            self.service.as_bytes(),
            self.user.as_bytes(),
            self.secret.as_slice(),
        ];
        check_fields(fields[0], fields[1], fields[2])?;
        let mut frame = Vec::with_capacity(MAX_REQUEST);
        frame.extend_from_slice(&[0; 4]);
        frame.extend_from_slice(REQUEST_MAGIC);
        frame.extend_from_slice(&[VERSION, self.flags, 0, 0]);
        for field in fields {
            let length = u16::try_from(field.len()).map_err(|_| WireError::BadLength)?;
            frame.extend_from_slice(&length.to_le_bytes());
            frame.extend_from_slice(field);
        }
        let total = u32::try_from(frame.len()).map_err(|_| WireError::BadLength)?;
        frame[..4].copy_from_slice(&total.to_le_bytes());
        Ok(SecretBytes::new(frame))
    }

    /// Decodes one whole frame into owned values.
    pub fn decode(frame: &[u8]) -> Result<Self, WireError> {
        let view = RequestView::decode(frame)?;
        Ok(Self {
            flags: view.flags,
            service: view.service.to_owned(),
            user: view.user.to_owned(),
            secret: SecretBytes::from_slice(view.secret),
        })
    }
}

/// A request decoded in place, so the helper can hand PAM the secret from
/// the locked page it was read into, with no copy of its own.
#[derive(Debug)]
pub struct RequestView<'a> {
    pub flags: u8,
    pub service: &'a str,
    pub user: &'a str,
    pub secret: &'a [u8],
}

impl<'a> RequestView<'a> {
    /// Decodes one whole frame, refusing anything malformed, oversized or
    /// with trailing bytes.
    pub fn decode(frame: &'a [u8]) -> Result<Self, WireError> {
        if frame.len() < 12 || frame.len() > MAX_REQUEST {
            return Err(WireError::BadLength);
        }
        let total = u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
        if usize::try_from(total).ok() != Some(frame.len()) {
            return Err(WireError::BadLength);
        }
        if &frame[4..8] != REQUEST_MAGIC {
            return Err(WireError::BadMagic);
        }
        if frame[8] != VERSION {
            return Err(WireError::BadVersion);
        }
        if frame[10] != 0 || frame[11] != 0 || frame[9] & !(FLAG_SETCRED | FLAG_DISALLOW_NULL) != 0
        {
            return Err(WireError::BadField);
        }
        let mut rest = &frame[12..];
        let service = take_field(&mut rest)?;
        let user = take_field(&mut rest)?;
        let secret = take_field(&mut rest)?;
        if !rest.is_empty() {
            return Err(WireError::BadLength);
        }
        check_fields(service, user, secret)?;
        let text = |bytes: &'a [u8]| std::str::from_utf8(bytes).map_err(|_| WireError::BadField);
        Ok(Self {
            flags: frame[9],
            service: text(service)?,
            user: text(user)?,
            secret,
        })
    }

    /// The declared length of a frame from its first four bytes, refused
    /// before anything is read past them when it is out of bounds.
    pub fn declared_length(prefix: [u8; 4]) -> Result<usize, WireError> {
        let length =
            usize::try_from(u32::from_le_bytes(prefix)).map_err(|_| WireError::BadLength)?;
        if !(12..=MAX_REQUEST).contains(&length) {
            return Err(WireError::BadLength);
        }
        Ok(length)
    }
}

/// One length-prefixed field from the front of `rest`.
fn take_field<'a>(rest: &mut &'a [u8]) -> Result<&'a [u8], WireError> {
    let [low, high, tail @ ..] = *rest else {
        return Err(WireError::Truncated);
    };
    let length = usize::from(u16::from_le_bytes([*low, *high]));
    if tail.len() < length {
        return Err(WireError::Truncated);
    }
    let (value, after) = tail.split_at(length);
    *rest = after;
    Ok(value)
}

fn check_fields(service: &[u8], user: &[u8], secret: &[u8]) -> Result<(), WireError> {
    let named = |bytes: &[u8], max| !bytes.is_empty() && bytes.len() <= max && !bytes.contains(&0);
    if !named(service, MAX_SERVICE) || !named(user, MAX_USER) {
        return Err(WireError::BadField);
    }
    if secret.len() > MAX_SECRET || secret.contains(&0) {
        return Err(WireError::BadField);
    }
    Ok(())
}

/// The helper's answer. `pam_code` is PAM's return value, for the agent's
/// own diagnostics; it is never shown to a client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HelperReply {
    pub verdict: PamVerdict,
    pub pam_code: i16,
}

impl HelperReply {
    pub fn encode(&self) -> [u8; REPLY_LEN] {
        let verdict = match self.verdict {
            PamVerdict::Accepted => 0,
            PamVerdict::Rejected => 1,
            PamVerdict::Unavailable => 2,
            PamVerdict::ConversationRefused => 3,
            // Only the agent decides these; the helper never sends them.
            PamVerdict::TimedOut | PamVerdict::HelperFailed => 2,
        };
        let mut frame = [0; REPLY_LEN];
        frame[..4].copy_from_slice(&(REPLY_LEN as u32).to_le_bytes());
        frame[4..8].copy_from_slice(REPLY_MAGIC);
        frame[8] = VERSION;
        frame[9] = verdict;
        frame[10..12].copy_from_slice(&self.pam_code.to_le_bytes());
        frame
    }

    pub fn decode(frame: &[u8]) -> Result<Self, WireError> {
        if frame.len() != REPLY_LEN
            || u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize != REPLY_LEN
        {
            return Err(WireError::BadLength);
        }
        if &frame[4..8] != REPLY_MAGIC {
            return Err(WireError::BadMagic);
        }
        if frame[8] != VERSION {
            return Err(WireError::BadVersion);
        }
        let verdict = match frame[9] {
            0 => PamVerdict::Accepted,
            1 => PamVerdict::Rejected,
            2 => PamVerdict::Unavailable,
            3 => PamVerdict::ConversationRefused,
            _ => return Err(WireError::BadField),
        };
        Ok(Self {
            verdict,
            pam_code: i16::from_le_bytes([frame[10], frame[11]]),
        })
    }
}

//! Already-sanitized presentation facts, not application identities.
//! Extracted from Sophia 262eb82bc's `packets/chrome.rs`. Disclosure policy
//! remains with the broker; receiving these values grants no action authority.

pub const MAX_CHROME_LABEL_LEN: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayLabel {
    pub text: String,
    pub redacted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrustLevel {
    Unknown,
    Trusted,
    Untrusted,
    Isolated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttentionState {
    None,
    Notice,
    Critical,
}

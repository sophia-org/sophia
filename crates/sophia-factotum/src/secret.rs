//! Secret bytes: zeroed when dropped, never shown.

/// Bytes that are a secret: a password, a key, a session secret. They are
/// zeroed on drop and their `Debug` names only the length's absence.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct SecretBytes(zeroize::Zeroizing<Vec<u8>>);

impl SecretBytes {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(zeroize::Zeroizing::new(bytes))
    }

    pub fn from_slice(bytes: &[u8]) -> Self {
        Self::new(bytes.to_vec())
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl core::fmt::Debug for SecretBytes {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Neither the bytes nor their length.
        formatter.write_str("SecretBytes(..)")
    }
}

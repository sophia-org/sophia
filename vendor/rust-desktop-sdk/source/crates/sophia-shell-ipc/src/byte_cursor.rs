//! The little-endian cursor and writer shared by every codec in this crate.
//! Its error, [`CursorError`], names no codec; each codec converts it into
//! its own error type.

pub(crate) fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// A cursor-level decode failure: exactly the two ways reading past a byte
/// slice can fail. Every richer error (reserved fields, invalid enums,
/// record shape, ...) is layered on top by the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CursorError {
    Truncated,
    TrailingBytes(usize),
}

pub(crate) struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub(crate) fn finish(&self) -> Result<(), CursorError> {
        let remaining = self.bytes.len().saturating_sub(self.offset);
        if remaining == 0 {
            Ok(())
        } else {
            Err(CursorError::TrailingBytes(remaining))
        }
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], CursorError> {
        let end = self.offset.checked_add(N).ok_or(CursorError::Truncated)?;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or(CursorError::Truncated)?;
        self.offset = end;
        let mut out = [0; N];
        out.copy_from_slice(slice);
        Ok(out)
    }

    pub(crate) fn slice(&mut self, len: usize) -> Result<&'a [u8], CursorError> {
        let end = self.offset.checked_add(len).ok_or(CursorError::Truncated)?;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or(CursorError::Truncated)?;
        self.offset = end;
        Ok(slice)
    }

    pub(crate) fn u16(&mut self) -> Result<u16, CursorError> {
        Ok(u16::from_le_bytes(self.take()?))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, CursorError> {
        Ok(u32::from_le_bytes(self.take()?))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, CursorError> {
        Ok(u64::from_le_bytes(self.take()?))
    }
}

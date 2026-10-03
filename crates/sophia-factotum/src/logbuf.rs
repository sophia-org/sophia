//! The `log` file's ring of recent messages.
//!
//! Ported from 9front `sys/src/cmd/auth/factotum/log.c` (MIT, Copyright (c)
//! 2021 Plan 9 Foundation and 9front authors), with one deliberate change:
//! 9front's writer overwrites slots without moving the reader, so a writer
//! that laps a slow reader leaves it reading empty messages. Here the ring
//! drops its oldest message instead. Messages never carry secret values;
//! callers format attributes with [`crate::attr::AttrList::masked`].

use std::collections::VecDeque;

/// `nelem(Logbuf.msg)`.
const SLOTS: usize = 128;
/// `flog` formats into a 1024-byte buffer.
const MAX_MESSAGE: usize = 1023;
/// A read must hold at least `...\n` and one byte.
const MIN_READ: usize = 5;

#[derive(Debug, Default)]
pub struct LogBuf {
    messages: VecDeque<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogReadTooShort;

impl LogBuf {
    pub fn new() -> Self {
        Self::default()
    }

    /// `flog`: appends one message, cut to 1023 bytes on a UTF-8 boundary.
    pub fn append(&mut self, mut message: String) {
        if message.len() > MAX_MESSAGE {
            let mut end = MAX_MESSAGE;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
        }
        if self.messages.len() == SLOTS {
            self.messages.pop_front();
        }
        self.messages.push_back(message);
    }

    /// One message and a newline, or `None` when there is nothing to read
    /// and the read must wait. A message that does not fit in `count` is cut
    /// to `count - 5` bytes, backed up to a UTF-8 start byte, and ends in
    /// `...\n`.
    pub fn read(&mut self, count: usize) -> Result<Option<Vec<u8>>, LogReadTooShort> {
        if count < MIN_READ {
            return Err(LogReadTooShort);
        }
        let Some(message) = self.messages.pop_front() else {
            return Ok(None);
        };
        let bytes = message.as_bytes();
        if count < bytes.len() + 2 {
            // 9front's `while(n>0 && (data[--n]&0xC0)==0x80)`: step back one
            // byte at a time until one that starts a character, and cut
            // there, so the character the cut fell in is dropped whole.
            let mut end = count - MIN_READ;
            while end > 0 {
                end -= 1;
                if bytes[end] & 0xc0 != 0x80 {
                    break;
                }
            }
            let mut out = bytes[..end].to_vec();
            out.extend_from_slice(b"...\n");
            return Ok(Some(out));
        }
        let mut out = bytes.to_vec();
        out.push(b'\n');
        Ok(Some(out))
    }
}

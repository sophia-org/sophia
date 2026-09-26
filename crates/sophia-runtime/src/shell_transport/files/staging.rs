//! One attach's candidate buffer: append or exact repeat, bounded by the
//! shell transaction cap and the WM file assembly deadline.
use std::time::{Duration, Instant};

use sophia_9p::Errno;
use sophia_protocol::shell_files::{
    SHELL_FILE_ASSEMBLY_TIMEOUT_MILLIS, SHELL_FILE_HEADER_BYTES, SHELL_FILE_MAX_TRANSACTION_BYTES,
};

const ASSEMBLY_DEADLINE: Duration =
    Duration::from_millis(SHELL_FILE_ASSEMBLY_TIMEOUT_MILLIS as u64);

pub(in crate::shell_transport) struct Staging {
    pub handle: u64,
    pub bytes: Vec<u8>,
    deadline: Option<Instant>,
}

impl Staging {
    pub(in crate::shell_transport) fn new(handle: u64) -> Self {
        Self {
            handle,
            bytes: Vec::new(),
            deadline: None,
        }
    }

    pub(in crate::shell_transport) fn expired(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| now >= deadline)
    }

    pub(in crate::shell_transport) fn write(
        &mut self,
        offset: u64,
        bytes: &[u8],
        now: Instant,
    ) -> Result<u32, Errno> {
        if self.expired(now) {
            return Err(Errno::ESTALE);
        }
        let offset = usize::try_from(offset).map_err(|_| Errno::EINVAL)?;
        let end = offset
            .checked_add(bytes.len())
            .filter(|n| *n <= SHELL_FILE_MAX_TRANSACTION_BYTES)
            .ok_or(Errno::EINVAL)?;
        if offset > self.bytes.len() {
            return Err(Errno::EINVAL);
        }
        if offset < self.bytes.len() {
            if end > self.bytes.len() || self.bytes[offset..end] != *bytes {
                return Err(Errno::EINVAL);
            }
            return Ok(bytes.len() as u32);
        }
        // Check a newly completed length before touching the retained prefix.
        if end >= 4 {
            let mut length = [0; 4];
            let retained = self.bytes.len().min(4);
            length[..retained].copy_from_slice(&self.bytes[..retained]);
            if retained < 4 {
                length[retained..].copy_from_slice(&bytes[..4 - retained]);
            }
            let declared = u32::from_le_bytes(length) as usize;
            if !(SHELL_FILE_HEADER_BYTES..=SHELL_FILE_MAX_TRANSACTION_BYTES).contains(&declared)
                || end > declared
            {
                return Err(Errno::EINVAL);
            }
        }
        if !bytes.is_empty() {
            self.deadline.get_or_insert(now + ASSEMBLY_DEADLINE);
            self.bytes.extend_from_slice(bytes);
        }
        Ok(bytes.len() as u32)
    }
}

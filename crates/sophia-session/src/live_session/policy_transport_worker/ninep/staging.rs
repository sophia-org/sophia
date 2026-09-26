use super::*;

pub(super) struct Staging {
    pub handle: u64,
    pub bytes: Vec<u8>,
    deadline: Option<Instant>,
}

impl Staging {
    pub(super) fn new(handle: u64) -> Self {
        Self {
            handle,
            bytes: Vec::new(),
            deadline: None,
        }
    }

    pub(super) fn expired(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| now >= deadline)
    }

    pub(super) fn wait(&self, now: Instant, maximum: Duration) -> Duration {
        self.deadline.map_or(maximum, |deadline| {
            maximum.min(deadline.saturating_duration_since(now))
        })
    }

    pub(super) fn write(&mut self, offset: u64, bytes: &[u8], now: Instant) -> Result<u32, Errno> {
        if self.expired(now) {
            return Err(Errno::ESTALE);
        }
        let offset = usize::try_from(offset).map_err(|_| Errno::EINVAL)?;
        let end = offset
            .checked_add(bytes.len())
            .filter(|n| *n <= WM_FILE_MAX_BYTES)
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
        // Check a newly completed header before touching the retained prefix.
        if end >= 4 {
            let mut length = [0; 4];
            let retained = self.bytes.len().min(4);
            length[..retained].copy_from_slice(&self.bytes[..retained]);
            if retained < 4 {
                length[retained..].copy_from_slice(&bytes[..4 - retained]);
            }
            let declared = u32::from_le_bytes(length) as usize;
            if !(WM_FILE_HEADER_BYTES..=WM_FILE_MAX_BYTES).contains(&declared) || end > declared {
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

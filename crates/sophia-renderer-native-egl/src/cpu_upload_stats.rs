/// CPU texture work across retained EGL contexts. Counters exclude DMA imports.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeCpuUploadStats {
    pub uploads: u64,
    pub bytes: u64,
    pub allocations: u64,
    pub hits: u64,
    pub retained_bytes: u64,
}
impl NativeCpuUploadStats {
    pub fn saturating_add(self, rhs: Self) -> Self {
        Self {
            uploads: self.uploads.saturating_add(rhs.uploads),
            bytes: self.bytes.saturating_add(rhs.bytes),
            allocations: self.allocations.saturating_add(rhs.allocations),
            hits: self.hits.saturating_add(rhs.hits),
            retained_bytes: self.retained_bytes.saturating_add(rhs.retained_bytes),
        }
    }
}

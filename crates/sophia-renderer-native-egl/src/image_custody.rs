//! Retirement state belongs to the captured allocation, independently of
//! the device that created it. Promotion never releases or replaces storage.

pub struct NativeRendererImageCustody<B, P> {
    pub(crate) buffer: B,
    pub(crate) pooled: Option<P>,
    pub(crate) bytes: u64,
    promoted: bool,
}

impl<B, P> NativeRendererImageCustody<B, P> {
    /// Capture has already validated and charged these resources. They remain
    /// staged until the owning Present retires successfully.
    pub fn staged(buffer: B, pooled: Option<P>, bytes: u64) -> Self {
        Self {
            buffer,
            pooled,
            bytes,
            promoted: false,
        }
    }

    pub fn promote(&mut self) -> bool {
        if self.promoted {
            return false;
        }
        self.promoted = true;
        true
    }

    pub fn is_promoted(&self) -> bool {
        self.promoted
    }

    pub fn buffer(&self) -> &B {
        &self.buffer
    }

    pub fn pooled(&self) -> Option<&P> {
        self.pooled.as_ref()
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

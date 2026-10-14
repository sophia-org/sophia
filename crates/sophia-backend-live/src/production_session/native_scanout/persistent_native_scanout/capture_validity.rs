//! Serializes native output withdrawal with consumers of retained pixels.
use std::sync::{Arc, Mutex};

/// A lifetime check, not capture permission. Session still owns admission.
#[derive(Clone, Default)]
pub struct LiveOutputCaptureValidity(Arc<Mutex<bool>>);
impl LiveOutputCaptureValidity {
    /// Run one bounded, nonblocking read unless the native owner withdrew it.
    pub fn with_current<T>(&self, read: impl FnOnce() -> T) -> Option<T> {
        let withdrawn = self.0.lock().ok()?;
        if *withdrawn {
            return None;
        }
        let result = read();
        drop(withdrawn);
        Some(result)
    }
}

#[derive(Default)]
pub(super) struct CaptureValidityOwner(pub LiveOutputCaptureValidity);
impl Drop for CaptureValidityOwner {
    fn drop(&mut self) {
        if let Ok(mut withdrawn) = self.0.0.lock() {
            *withdrawn = true;
        }
    }
}

#[path = "../../../../tests/support/capture_validity.rs"]
mod tests;

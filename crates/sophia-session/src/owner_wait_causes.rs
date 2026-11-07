//! What ended each owner wait, as a histogram over joint causes.
//!
//! The existing wait counters overlap: one poll return can be a ring and a
//! journal notification at once, so their sum does not say how many passes
//! each source caused. Each wait lands in exactly one bucket here, so the
//! buckets partition `owner_waits` and divide by completions into passes per
//! frame by cause. Diagnostic only; no scheduling decision reads it.

/// The owner's own eventfd rang.
#[cfg(feature = "native-session")]
pub(crate) const RING: u8 = 1 << 0;
/// A component 9P server wake reader: journal self-notification.
pub(crate) const JOURNAL: u8 = 1 << 1;
/// A component 9P listener or connection stream: a peer exchange.
pub(crate) const WIRE: u8 = 1 << 2;
/// Any other borrowed descriptor: image, authentication, negotiation, relay.
pub(crate) const FD: u8 = 1 << 3;
/// A native completion descriptor.
#[cfg(feature = "native-session")]
pub(crate) const NATIVE: u8 = 1 << 4;
/// The deadline expired with nothing ready.
#[cfg(feature = "native-session")]
pub(crate) const DEADLINE: u8 = 1 << 5;
/// A bounded service wait rather than an authority wait.
#[cfg(feature = "native-session")]
pub(crate) const SERVICE: u8 = 1 << 6;

/// `flag` when `on`, for composing a wait's cause bits.
pub(crate) fn bit(on: bool, flag: u8) -> u8 {
    if on { flag } else { 0 }
}

#[cfg(feature = "native-session")]
const NAMES: [&str; 7] = ["ring", "journal", "wire", "fd", "native", "deadline", "service"];
#[cfg(feature = "native-session")]
const BUCKETS: usize = 1 << NAMES.len();

#[cfg(feature = "native-session")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WaitCauses([u64; BUCKETS]);

#[cfg(feature = "native-session")]
impl Default for WaitCauses {
    fn default() -> Self {
        Self([0; BUCKETS])
    }
}

#[cfg(feature = "native-session")]
impl WaitCauses {
    pub(crate) fn add(&mut self, causes: u8) {
        let count = &mut self.0[usize::from(causes) % BUCKETS];
        *count = count.saturating_add(1);
    }

    #[cfg(test)]
    pub(crate) fn count(&self, causes: u8) -> u64 {
        self.0[usize::from(causes) % BUCKETS]
    }

    #[cfg(test)]
    pub(crate) fn waits(&self) -> u64 {
        self.0.iter().fold(0, |sum, count| sum.saturating_add(*count))
    }
}

/// One ` cause_<names>=<count>` field per nonempty bucket, names in bit order,
/// so a reducer's key filter admits them and a sum over fields equals `waits`.
#[cfg(feature = "native-session")]
impl std::fmt::Display for WaitCauses {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (bits, count) in self.0.iter().enumerate() {
            if *count == 0 {
                continue;
            }
            f.write_str(" cause")?;
            if bits == 0 {
                f.write_str("_none")?;
            }
            for (i, name) in NAMES.iter().enumerate() {
                if bits & (1 << i) != 0 {
                    write!(f, "_{name}")?;
                }
            }
            write!(f, "={count}")?;
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "native-session"))]
#[path = "../tests/support/owner_wait_causes.rs"]
mod tests;

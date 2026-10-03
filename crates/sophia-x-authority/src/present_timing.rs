//! Present's window counter is independent of the counter of any one head.
//!
//! Pure arithmetic only: Session supplies clock observations and owns wakeups;
//! this module neither predicts hardware vblanks nor reads a host clock.

mod queue;
pub use queue::*;

/// The incarnation must change when a hardware counter can restart. Logical
/// output identity alone is insufficient across modesets, resume or rebind.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum XPresentClockSource {
    Fake,
    Hardware { domain: u64, incarnation: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XPresentClockSample {
    pub source: XPresentClockSource,
    pub ust: u64,
    pub msc: u64,
}

/// Exact retirement evidence. Historical samples must never advance a clock,
/// even when the frontend has not seen the native query that superseded them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XPresentRetirementClock {
    pub sample: XPresentClockSample,
    pub historical: bool,
}

impl From<XPresentClockSample> for XPresentRetirementClock {
    fn from(sample: XPresentClockSample) -> Self {
        Self {
            sample,
            historical: false,
        }
    }
}

impl XPresentClockSample {
    /// One monotonically progressing field per second while no visible head
    /// samples this window. UST stays in CLOCK_MONOTONIC microseconds.
    pub const fn background(monotonic_usec: u64) -> Self {
        Self {
            source: XPresentClockSource::Fake,
            ust: monotonic_usec,
            msc: monotonic_usec / 1_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XPresentTimingError {
    InvalidRemainder,
    MissingClock,
    WrongSource,
    StaleSample,
    CounterExhausted,
}

impl core::fmt::Display for XPresentTimingError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::InvalidRemainder => "invalid Present remainder",
            Self::MissingClock => "Present clock has not been observed",
            Self::WrongSource => "Present clock source does not match the request binding",
            Self::StaleSample => "Present clock observation regressed",
            Self::CounterExhausted => "Present window timeline exhausted",
        })
    }
}

impl std::error::Error for XPresentTimingError {}

/// MSC-valued timing. UST-valued requests must be converted or refused before
/// constructing this value; the units must never be inferred from a number.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XPresentMscTiming {
    target: u64,
    divisor: u64,
    remainder: u64,
    asynchronous: bool,
}

impl XPresentMscTiming {
    pub fn new(
        target: u64,
        divisor: u64,
        remainder: u64,
        asynchronous: bool,
    ) -> Result<Self, XPresentTimingError> {
        if (divisor == 0 && remainder != 0) || (divisor != 0 && remainder >= divisor) {
            return Err(XPresentTimingError::InvalidRemainder);
        }
        Ok(Self {
            target,
            divisor,
            remainder,
            asynchronous,
        })
    }

    /// NotifyMSC with no modulus queries the current field. With a modulus it
    /// requests the next matching field, even if the current field matches.
    pub fn notify(target: u64, divisor: u64, remainder: u64) -> Result<Self, XPresentTimingError> {
        Self::new(target, divisor, remainder, divisor == 0)
    }

    /// Forward distance, kept separately from the wrapping counter. A very
    /// large valid modulus must not become an already-due signed comparison.
    pub fn fields_after(self, current: u64) -> u64 {
        let distance = self.target.wrapping_sub(current);
        if distance != 0 && distance < (1u64 << 63) {
            return distance;
        }
        if self.divisor == 0 {
            return u64::from(!self.asynchronous);
        }
        let residue = current % self.divisor;
        let distance = if residue < self.remainder {
            self.remainder - residue
        } else if residue == self.remainder && self.asynchronous {
            0
        } else {
            self.divisor - (residue - self.remainder)
        };
        if current.checked_add(distance).is_some() {
            distance
        } else {
            // Modulo is on the wire's CARD64 counter. After wrap, the first
            // eligible counter is remainder, rather than a wrapped sum that
            // might no longer satisfy the requested modulus.
            u64::MAX - current + 1 + self.remainder
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XPresentClockTarget {
    pub window_msc: u64,
    position: u128,
    source_anchor: XPresentClockSample,
    fields: u64,
}

/// Freeze this when the request acquires its actual presentation clock. A
/// later source change must not reinterpret an already-submitted completion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XPresentClockBinding {
    pub source: XPresentClockSource,
    offset: u64,
}

impl XPresentClockBinding {
    pub fn window_sample(
        self,
        sample: XPresentClockSample,
    ) -> Result<(u64, u64), XPresentTimingError> {
        if sample.source != self.source {
            return Err(XPresentTimingError::WrongSource);
        }
        Ok((sample.ust, sample.msc.wrapping_sub(self.offset)))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XPresentWindowClock {
    sample: Option<XPresentClockSample>,
    offset: u64,
    position: u128,
}

impl XPresentWindowClock {
    /// On a source change, pass a fresh old-source query when available. If
    /// the old head has gone away, its last observation is the continuity
    /// anchor. Rejected observations leave the entire timeline unchanged.
    pub fn observe(
        &mut self,
        sample: XPresentClockSample,
        previous: Option<XPresentClockSample>,
    ) -> Result<(u64, u64), XPresentTimingError> {
        let mut next = self.clone();
        if let Some(old) = next.sample {
            if old.source != sample.source {
                if let Some(previous) = previous {
                    if previous.source != old.source {
                        return Err(XPresentTimingError::WrongSource);
                    }
                    next.advance(previous)?;
                }
                let old = next.sample.expect("an existing clock remains observed");
                // Each head reports its own most recent vblank timestamp.
                // The new head's observation can therefore be older without
                // either source regressing. Never fabricate a timestamp by
                // clamping one head's UST to the other's.
                let window_msc = old.msc.wrapping_sub(next.offset);
                next.offset = sample.msc.wrapping_sub(window_msc);
                next.sample = Some(sample);
            } else {
                next.advance(sample)?;
            }
        } else {
            next.sample = Some(sample);
        }
        let mapped = next.binding()?.window_sample(sample)?;
        *self = next;
        Ok(mapped)
    }

    fn advance(&mut self, sample: XPresentClockSample) -> Result<(), XPresentTimingError> {
        let old = self.sample.ok_or(XPresentTimingError::MissingClock)?;
        let distance = sample.msc.wrapping_sub(old.msc);
        if sample.ust < old.ust || distance >= (1u64 << 63) {
            return Err(XPresentTimingError::StaleSample);
        }
        self.position = self
            .position
            .checked_add(u128::from(distance))
            .ok_or(XPresentTimingError::CounterExhausted)?;
        self.sample = Some(sample);
        Ok(())
    }

    pub fn binding(&self) -> Result<XPresentClockBinding, XPresentTimingError> {
        Ok(XPresentClockBinding {
            source: self.sample.ok_or(XPresentTimingError::MissingClock)?.source,
            offset: self.offset,
        })
    }

    pub fn target(
        &self,
        timing: XPresentMscTiming,
    ) -> Result<XPresentClockTarget, XPresentTimingError> {
        let sample = self.sample.ok_or(XPresentTimingError::MissingClock)?;
        let current = sample.msc.wrapping_sub(self.offset);
        let distance = timing.fields_after(current);
        Ok(XPresentClockTarget {
            window_msc: current.wrapping_add(distance),
            source_anchor: sample,
            fields: distance,
            position: self
                .position
                .checked_add(u128::from(distance))
                .ok_or(XPresentTimingError::CounterExhausted)?,
        })
    }

    pub const fn target_reached(&self, target: XPresentClockTarget) -> bool {
        self.sample.is_some() && self.position >= target.position
    }

    pub fn sample(&self) -> Result<XPresentClockSample, XPresentTimingError> {
        self.sample.ok_or(XPresentTimingError::MissingClock)
    }

    /// A hardware-synchronised image must be composed before its target
    /// field. Only an actual hardware observation permits this one-field
    /// lead; fake-clock execution and NotifyMSC wait for the target itself.
    pub fn preparation_reached(&self, target: XPresentClockTarget) -> bool {
        if self.sample.is_none() {
            return false;
        }
        let lead = u128::from(matches!(
            self.sample.map(|s| s.source),
            Some(XPresentClockSource::Hardware { .. })
        ));
        self.position.saturating_add(lead) >= target.position
    }

    /// Only fake-clock obligations have a time-derived deadline. Hardware
    /// requests must use actual hardware queries/events. With no request the
    /// owner has nothing to arm and this clock generates no idle polling.
    pub fn background_deadline(&self, target: XPresentClockTarget) -> Option<u64> {
        let sample = self.sample?;
        if sample.source != XPresentClockSource::Fake {
            return None;
        }
        if self.target_reached(target) {
            return Some(sample.ust);
        }
        let remaining = u64::try_from(target.position - self.position).ok()?;
        sample.msc.checked_add(remaining)?.checked_mul(1_000_000)
    }
}

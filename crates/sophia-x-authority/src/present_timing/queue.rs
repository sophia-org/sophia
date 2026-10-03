use super::{XPresentClockSample, XPresentClockSource, XPresentClockTarget, XPresentTimingError};
use sophia_protocol::TransactionId;
use std::num::NonZeroUsize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XPresentScheduledKind {
    Pixmap,
    /// Pixels were superseded and may already be idle. Complete still waits
    /// for this request's own target field.
    Skip,
    NotifyMsc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XPresentScheduledRequest {
    pub request: TransactionId,
    pub kind: XPresentScheduledKind,
    pub target: XPresentClockTarget,
    /// Counter domain, incarnation and offset at scheduling. Equal window
    /// MSCs on different bindings are not the same CRTC/target obligation.
    pub binding: super::XPresentClockBinding,
    /// Only a full-content update may scrap earlier equal-target pixmaps.
    pub replaces_contents: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XPresentScheduleError {
    InvalidRequest,
    DuplicateRequest,
    Capacity,
}

/// One window's bounded ordering, independent of renderer ownership. A later
/// request with an earlier target must be able to execute first. The caller
/// owns connection/global reservations and the retained pixmap/fence state.
#[derive(Debug)]
pub struct XPresentWindowSchedule {
    requests: Vec<QueuedRequest>,
    capacity: usize,
}

#[derive(Clone, Copy, Debug)]
struct QueuedRequest {
    request: XPresentScheduledRequest,
    observed: XPresentClockSample,
    elapsed: u128,
    source_lost: bool,
    waiting_msc: bool,
}

impl QueuedRequest {
    fn observe(mut self, sample: XPresentClockSample) -> Result<Self, XPresentTimingError> {
        if sample.source != self.request.binding.source || self.source_lost {
            return Ok(self);
        }
        let distance = sample.msc.wrapping_sub(self.observed.msc);
        if sample.ust < self.observed.ust || distance >= (1u64 << 63) {
            return Err(XPresentTimingError::StaleSample);
        }
        self.elapsed = self
            .elapsed
            .checked_add(u128::from(distance))
            .ok_or(XPresentTimingError::CounterExhausted)?;
        self.observed = sample;
        Ok(self)
    }

    fn ready(self) -> bool {
        let lead = u128::from(
            self.request.kind == XPresentScheduledKind::Pixmap
                && matches!(
                    self.request.binding.source,
                    XPresentClockSource::Hardware { .. }
                ),
        );
        self.source_lost
            || self.elapsed.saturating_add(lead) >= u128::from(self.request.target.fields)
    }

    fn background_deadline(self) -> Option<u64> {
        if self.request.binding.source != XPresentClockSource::Fake {
            return None;
        }
        if self.ready() {
            return Some(self.observed.ust);
        }
        self.request
            .target
            .source_anchor
            .msc
            .checked_add(self.request.target.fields)?
            .checked_mul(1_000_000)
    }
}

impl XPresentWindowSchedule {
    pub fn new(capacity: NonZeroUsize) -> Self {
        let capacity = capacity.get().min(crate::X_PREPARED_PRESENT_CAPACITY);
        Self {
            requests: Vec::with_capacity(capacity),
            capacity,
        }
    }

    /// Return pixmaps that are now idle. Their queue entries remain as Skip
    /// completions at the original target. Partial updates never scrap an
    /// earlier update; a full update scraps every eligible equal-target one.
    /// Refusal leaves every existing request unchanged, including capacity.
    pub fn insert(
        &mut self,
        request: XPresentScheduledRequest,
    ) -> Result<Vec<XPresentScheduledRequest>, XPresentScheduleError> {
        if !request.request.is_valid() {
            return Err(XPresentScheduleError::InvalidRequest);
        }
        if self
            .requests
            .iter()
            .any(|p| p.request.request == request.request)
        {
            return Err(XPresentScheduleError::DuplicateRequest);
        }
        if self.requests.len() == self.capacity {
            return Err(XPresentScheduleError::Capacity);
        }
        let mut superseded = Vec::new();
        if request.kind == XPresentScheduledKind::Pixmap && request.replaces_contents {
            for earlier in &mut self.requests {
                if earlier.request.kind == XPresentScheduledKind::Pixmap
                    && earlier.waiting_msc
                    && earlier.request.target.window_msc == request.target.window_msc
                    && earlier.request.binding == request.binding
                {
                    superseded.push(earlier.request);
                    earlier.request.kind = XPresentScheduledKind::Skip;
                }
            }
        }
        self.requests.push(QueuedRequest {
            request,
            observed: request.target.source_anchor,
            elapsed: 0,
            source_lost: false,
            waiting_msc: true,
        });
        self.requests
            .sort_by_key(|p| (p.request.target.position, p.request.request.raw()));
        Ok(superseded)
    }

    /// Observe without removing: fence/readiness checks and execution
    /// admission still have to succeed before the caller takes a request.
    pub fn ready(&self) -> impl Iterator<Item = XPresentScheduledRequest> + '_ {
        self.requests
            .iter()
            .copied()
            .filter(|p| p.ready())
            .map(|p| p.request)
    }

    /// Queued requests stay on their original source even if the window's
    /// next request chooses another one. Each source progresses only from
    /// its own observations; a rejected observation changes no entry.
    pub fn observe(&mut self, sample: XPresentClockSample) -> Result<(), XPresentTimingError> {
        self.observe_pair(None, sample).map_err(|(_, error)| error)
    }

    /// Validate both source observations before changing any request. A
    /// rejected new-source sample must not commit the previous-source half.
    pub(crate) fn observe_pair(
        &mut self,
        previous: Option<XPresentClockSample>,
        sample: XPresentClockSample,
    ) -> Result<(), (XPresentClockSource, XPresentTimingError)> {
        let update = |mut entry: QueuedRequest| {
            if let Some(previous) = previous {
                entry = entry.observe(previous).map_err(|e| (previous.source, e))?;
            }
            entry.observe(sample).map_err(|e| (sample.source, e))
        };
        for entry in &self.requests {
            update(*entry)?;
        }
        for entry in &mut self.requests {
            *entry = update(*entry).expect("observations validated without intervening mutation");
        }
        Ok(())
    }

    /// The MSC event was serviced. A pixmap waiting on its acquire fence
    /// afterwards is no longer eligible for equal-target replacement.
    pub(crate) fn begin_execution(&mut self, request: TransactionId) -> bool {
        let Some(entry) = self
            .requests
            .iter_mut()
            .find(|p| p.request.request == request)
        else {
            return false;
        };
        if entry.request.kind != XPresentScheduledKind::Pixmap || !entry.ready() {
            return false;
        }
        entry.waiting_msc = false;
        true
    }

    pub fn clock_sources(&self) -> impl Iterator<Item = XPresentClockSource> + '_ {
        self.requests
            .iter()
            .filter(|p| !p.source_lost)
            .map(|p| p.request.binding.source)
    }

    /// Real fields remaining until service, including the hardware Pixmap
    /// preparation lead. This is scheduling demand, never predicted progress.
    /// Once ready, fences and egress govern execution; they need no more clock
    /// queries. Keep the binding in clock_sources for loss reconciliation.
    pub fn clock_demands(&self) -> impl Iterator<Item = (XPresentClockSource, u64)> + '_ {
        self.requests
            .iter()
            .filter(|p| !p.source_lost && !p.ready())
            .map(|p| {
                let lead = u128::from(
                    p.request.kind == XPresentScheduledKind::Pixmap
                        && matches!(
                            p.request.binding.source,
                            XPresentClockSource::Hardware { .. }
                        ),
                );
                let remaining = u128::from(p.request.target.fields)
                    .saturating_sub(lead)
                    .saturating_sub(p.elapsed);
                (p.request.binding.source, remaining as u64)
            })
    }

    /// A disabled/lost source is never migrated. Sophia deliberately settles
    /// its queued pixmaps as Skip using the last real observation. NotifyMSC
    /// retains its kind. Return the pixmaps whose Idle is now owed.
    pub fn lose_source(&mut self, source: XPresentClockSource) -> Vec<XPresentScheduledRequest> {
        let mut idle = Vec::new();
        for entry in &mut self.requests {
            if entry.request.binding.source != source || entry.source_lost {
                continue;
            }
            entry.source_lost = true;
            if entry.request.kind == XPresentScheduledKind::Pixmap {
                idle.push(entry.request);
                entry.request.kind = XPresentScheduledKind::Skip;
            }
        }
        idle
    }

    pub fn completion_sample(&self, request: TransactionId) -> Option<(u64, u64)> {
        let entry = self
            .requests
            .iter()
            .find(|p| p.request.request == request)?;
        if !entry.source_lost && entry.elapsed < u128::from(entry.request.target.fields) {
            return None;
        }
        entry.request.binding.window_sample(entry.observed).ok()
    }

    pub fn get(&self, request: TransactionId) -> Option<XPresentScheduledRequest> {
        self.requests
            .iter()
            .find(|p| p.request.request == request)
            .map(|p| p.request)
    }

    pub(crate) fn observation(&self, request: TransactionId) -> Option<XPresentClockSample> {
        self.requests
            .iter()
            .find(|p| p.request.request == request)
            .map(|p| p.observed)
    }

    pub(crate) fn latest_observation(
        &self,
        source: XPresentClockSource,
    ) -> Option<XPresentClockSample> {
        self.requests
            .iter()
            .filter(|p| !p.source_lost && p.observed.source == source)
            .map(|p| p.observed)
            .max_by_key(|sample| sample.ust)
    }

    /// Failed execution has no pixels to own. It still owes completion at
    /// its original target, just like an equal-target supersession.
    pub fn scrap(&mut self, request: TransactionId) -> bool {
        let Some(entry) = self
            .requests
            .iter_mut()
            .find(|p| p.request.request == request)
        else {
            return false;
        };
        if entry.request.kind != XPresentScheduledKind::Pixmap {
            return false;
        }
        entry.request.kind = XPresentScheduledKind::Skip;
        true
    }

    pub fn take(&mut self, request: TransactionId) -> Option<XPresentScheduledRequest> {
        let index = self
            .requests
            .iter()
            .position(|p| p.request.request == request)?;
        Some(self.requests.remove(index).request)
    }

    pub fn len(&self) -> usize {
        self.requests.len()
    }
    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    pub fn background_deadline(&self) -> Option<u64> {
        self.requests
            .iter()
            .filter_map(|p| p.background_deadline())
            .min()
    }

    pub(crate) fn future_background_deadline(&self) -> Option<u64> {
        self.requests
            .iter()
            .filter(|p| !p.ready())
            .filter_map(|p| p.background_deadline())
            .min()
    }

    /// Cancellation, unlike equal-target supersession, owes no Complete or
    /// Idle. Return all identities so their reservations/backings can retire.
    pub fn cancel(&mut self) -> impl Iterator<Item = XPresentScheduledRequest> + '_ {
        self.requests.drain(..).map(|p| p.request)
    }
}

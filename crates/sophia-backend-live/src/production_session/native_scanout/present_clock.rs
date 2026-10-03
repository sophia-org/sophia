//! Real CRTC observations and their lifetime. Pure tracker plus a native
//! query adapter; no predicted vblank, global MSC or X protocol dependency.
use sophia_engine::RenderHeadId;
use std::collections::BTreeMap;
use std::num::NonZeroU64;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LiveNativePresentClockSource {
    pub owner: u64,
    pub incarnation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveNativePresentClockSample {
    pub source: LiveNativePresentClockSource,
    pub ust_usec: u64,
    pub msc: u64,
}

/// Observed once per owned card fd. Event attribution must be explicit:
/// drm-rs can otherwise fall back to user_data when crtc_id is absent.
#[derive(Clone, Copy, Debug, Default)]
pub struct LiveNativePresentClockEventSupport {
    pub monotonic: bool,
    pub crtc_id: bool,
}

/// The head remains private to Session/backend. A changed target is a new
/// counter lifetime even when the card and CRTC happen to be the same.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveNativePresentClockKey {
    pub head: RenderHeadId,
    pub target_generation: u64,
    pub card_group: usize,
    pub crtc_id: u32,
}

/// Why an active head has no present clock. Definite for its target
/// lifetime: transient and permission errors are query failures instead.
/// A card without monotonic timestamps is not unclocked: its retirement UST
/// cannot be called monotonic, so it stays `UnsupportedClock`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveNativeUnclockedReason {
    /// GET_SEQUENCE answered EOPNOTSUPP or ENOTTY on a card with monotonic
    /// timestamps; the first errno is kept. EINVAL stays a query failure:
    /// the kernel also gives it while a CRTC's vblank is off.
    SequenceUnsupported { errno: i32 },
}

/// An active head that scans out without a counter: no UST/MSC is ever
/// observed or predicted for it. Its source is stable for the target
/// lifetime and distinct from every clocked source of the same owner; its
/// period is the fastest field of the selected mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveNativeUnclockedPresentClock {
    pub source: LiveNativePresentClockSource,
    pub minimum_period_usec: u64,
    pub reason: LiveNativeUnclockedReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveNativePresentClockStatus {
    Observed,
    Restarted,
    Inactive,
    /// The card fd reports DRM_CAP_TIMESTAMP_MONOTONIC = 0: no clock, and
    /// no unclocked source either.
    UnsupportedClock,
    /// `current` stays `None`: the head is active but has no counter.
    Unclocked(LiveNativeUnclockedPresentClock),
    QueryFailed,
    InvalidTarget,
    Capacity,
    IdentityExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveNativePresentClockObservation {
    /// Notify bound requests of loss before exposing a replacement source.
    pub lost: Option<LiveNativePresentClockSource>,
    pub current: Option<LiveNativePresentClockSample>,
    pub status: LiveNativePresentClockStatus,
}

/// Query one admitted logical output's active mirror members. Prefer its
/// configured primary, then stable head order. A dead or unclocked preferred
/// member does not bind a new request when an active sibling has a clock:
/// querying stops only at the first clocked member. Keep every observation
/// so the caller retires failed old bindings before using the chosen one,
/// and falls back to the first unclocked member when none is clocked.
/// Existing requests never migrate through this function.
#[must_use]
pub fn query_live_present_clock_candidates(
    heads: impl IntoIterator<Item = (RenderHeadId, bool)>,
    primary: Option<RenderHeadId>,
    mut query: impl FnMut(RenderHeadId) -> LiveNativePresentClockObservation,
) -> Vec<(RenderHeadId, LiveNativePresentClockObservation)> {
    let mut heads = heads
        .into_iter()
        .filter_map(|(head, enabled)| (enabled && head.is_valid()).then_some(head))
        .collect::<Vec<_>>();
    heads.sort_unstable_by_key(|head| (Some(*head) != primary, *head));
    heads.dedup();
    let mut observations = Vec::new();
    for head in heads.into_iter().take(sophia_engine::MAX_HEADS_PER_OUTPUT) {
        let observation = query(head);
        observations.push((head, observation));
        if observation.current.is_some() {
            break;
        }
    }
    observations
}

#[derive(Clone, Copy, Debug)]
struct UnclockedRecord {
    key: LiveNativePresentClockKey,
    clock: LiveNativeUnclockedPresentClock,
}

#[derive(Clone, Copy, Debug)]
struct ClockRecord {
    key: LiveNativePresentClockKey,
    sample: LiveNativePresentClockSample,
    timestamp_nsec: u64,
}

/// At most one live record per admitted physical head, clocked or
/// unclocked. Incarnations never recycle within the native owner's domain,
/// including after invalidation. A replacement native owner must supply a
/// different process-unique domain, so its cache of unclocked heads starts
/// empty too.
#[derive(Debug)]
pub struct LiveNativePresentClocks {
    owner: NonZeroU64,
    next_incarnation: u64,
    heads: BTreeMap<RenderHeadId, ClockRecord>,
    unclocked: BTreeMap<RenderHeadId, UnclockedRecord>,
}

impl LiveNativePresentClocks {
    pub fn new(owner: NonZeroU64) -> Self {
        Self {
            owner,
            next_incarnation: 1,
            heads: BTreeMap::new(),
            unclocked: BTreeMap::new(),
        }
    }

    /// Active heads without a counter, each under its own stable source.
    pub fn unclocked(
        &self,
    ) -> impl Iterator<Item = (RenderHeadId, LiveNativeUnclockedPresentClock)> + '_ {
        self.unclocked
            .iter()
            .map(|(head, record)| (*head, record.clock))
    }

    /// The cached unclocked answer for this exact target lifetime: a later
    /// query repeats it without another kernel query.
    pub fn unclocked_for(
        &self,
        key: LiveNativePresentClockKey,
    ) -> Option<LiveNativePresentClockObservation> {
        self.unclocked
            .get(&key.head)
            .filter(|record| record.key == key)
            .map(|record| LiveNativePresentClockObservation {
                lost: None,
                current: None,
                status: LiveNativePresentClockStatus::Unclocked(record.clock),
            })
    }

    fn records(&self) -> usize {
        self.heads.len() + self.unclocked.len()
    }

    fn mint(&mut self) -> Result<LiveNativePresentClockSource, LiveNativePresentClockStatus> {
        if self.records()
            >= sophia_engine::MAX_DRM_KMS_OUTPUTS * sophia_engine::MAX_HEADS_PER_OUTPUT
        {
            return Err(LiveNativePresentClockStatus::Capacity);
        }
        let next = self
            .next_incarnation
            .checked_add(1)
            .ok_or(LiveNativePresentClockStatus::IdentityExhausted)?;
        let source = LiveNativePresentClockSource {
            owner: self.owner.get(),
            incarnation: self.next_incarnation,
        };
        self.next_incarnation = next;
        Ok(source)
    }

    /// A definite answer that this active target has no clock. The first
    /// answer for a target lifetime is kept, errno included; a different
    /// target, or a head that had a counter, loses its old source first.
    pub fn observe_unsupported(
        &mut self,
        key: LiveNativePresentClockKey,
        reason: LiveNativeUnclockedReason,
        minimum_period: std::time::Duration,
    ) -> LiveNativePresentClockObservation {
        if !key.head.is_valid() || key.target_generation == 0 || key.crtc_id == 0 {
            return self.lose_head(key.head, LiveNativePresentClockStatus::InvalidTarget);
        }
        if let Some(cached) = self.unclocked_for(key) {
            return cached;
        }
        let lost = self
            .heads
            .remove(&key.head)
            .map(|r| r.sample.source)
            .or_else(|| self.unclocked.remove(&key.head).map(|r| r.clock.source));
        let source = match self.mint() {
            Ok(source) => source,
            Err(status) => {
                return LiveNativePresentClockObservation {
                    lost,
                    current: None,
                    status,
                };
            }
        };
        let clock = LiveNativeUnclockedPresentClock {
            source,
            minimum_period_usec: u64::try_from(minimum_period.as_micros()).unwrap_or(u64::MAX),
            reason,
        };
        self.unclocked
            .insert(key.head, UnclockedRecord { key, clock });
        LiveNativePresentClockObservation {
            lost,
            current: None,
            status: LiveNativePresentClockStatus::Unclocked(clock),
        }
    }

    pub fn sources(
        &self,
    ) -> impl Iterator<Item = (RenderHeadId, LiveNativePresentClockSource)> + '_ {
        self.heads
            .iter()
            .map(|(head, record)| (*head, record.sample.source))
    }

    pub fn last_sample(
        &self,
        source: LiveNativePresentClockSource,
    ) -> Option<LiveNativePresentClockSample> {
        self.heads
            .values()
            .find(|r| r.sample.source == source)
            .map(|r| r.sample)
    }

    /// The currently queried source for this exact target lifetime.
    pub fn source_for(
        &self,
        key: LiveNativePresentClockKey,
    ) -> Option<LiveNativePresentClockSource> {
        self.heads
            .get(&key.head)
            .filter(|record| record.key == key)
            .map(|record| record.sample.source)
    }

    /// Native submission freezes the source before the event is collected.
    /// A reset/query between submission and collection cannot relabel that
    /// old frame with a newly minted incarnation of the same head.
    pub fn observe_submitted_page_flip(
        &mut self,
        key: LiveNativePresentClockKey,
        submitted: Option<LiveNativePresentClockSource>,
        sequence: u32,
        ust_usec: u64,
        support: LiveNativePresentClockEventSupport,
    ) -> Option<super::LiveNativeRetirementClock> {
        let evidence =
            self.decode_submitted_page_flip(key, submitted, sequence, ust_usec, support)?;
        if !evidence.historical {
            self.observe_page_flip(key, sequence, ust_usec, support)?;
        }
        Some(evidence)
    }

    /// Decode relative to the same submitted incarnation, in either direction
    /// across a low-32 wrap. Exactly half a range is ambiguous. Historical
    /// evidence never changes this tracker or any downstream clock authority.
    pub fn decode_submitted_page_flip(
        &self,
        key: LiveNativePresentClockKey,
        submitted: Option<LiveNativePresentClockSource>,
        sequence: u32,
        ust_usec: u64,
        support: LiveNativePresentClockEventSupport,
    ) -> Option<super::LiveNativeRetirementClock> {
        if !support.monotonic || !support.crtc_id {
            return None;
        }
        let record = self.heads.get(&key.head)?;
        if record.key != key || record.sample.source != submitted? {
            return None;
        }
        let anchor = record.sample;
        let delta = sequence.wrapping_sub(anchor.msc as u32);
        let msc = match delta.cmp(&(1 << 31)) {
            std::cmp::Ordering::Equal => return None,
            std::cmp::Ordering::Less => {
                if delta > 0 && ust_usec <= anchor.ust_usec {
                    return None;
                }
                anchor.msc.checked_add(u64::from(delta))?
            }
            std::cmp::Ordering::Greater => {
                if ust_usec >= anchor.ust_usec {
                    return None;
                }
                anchor
                    .msc
                    .checked_sub(u64::from((anchor.msc as u32).wrapping_sub(sequence)))?
            }
        };
        Some(super::LiveNativeRetirementClock {
            sample: LiveNativePresentClockSample {
                source: anchor.source,
                ust_usec,
                msc,
            },
            historical: ust_usec < anchor.ust_usec,
        })
    }

    /// A page-flip carries only the low 32 bits. Advance an existing 64-bit
    /// GET_SEQUENCE anchor within the same target lifetime; never mint a
    /// clock from an event or from the normalized retirement serial. Delayed
    /// events and ambiguous half-range jumps leave the real query timer intact.
    pub fn observe_page_flip(
        &mut self,
        key: LiveNativePresentClockKey,
        sequence: u32,
        ust_usec: u64,
        support: LiveNativePresentClockEventSupport,
    ) -> Option<LiveNativePresentClockSample> {
        if !support.monotonic || !support.crtc_id {
            return None;
        }
        let record = self.heads.get_mut(&key.head)?;
        if record.key != key || ust_usec < record.sample.ust_usec {
            return None;
        }
        let fields = sequence.wrapping_sub(record.sample.msc as u32);
        if fields >= (1 << 31) || (fields > 0 && ust_usec == record.sample.ust_usec) {
            return None;
        }
        let msc = record.sample.msc.checked_add(u64::from(fields))?;
        let timestamp_nsec = record.timestamp_nsec.max(ust_usec.checked_mul(1_000)?);
        record.sample = LiveNativePresentClockSample {
            msc,
            ust_usec,
            ..record.sample
        };
        record.timestamp_nsec = timestamp_nsec;
        Some(record.sample)
    }

    pub fn lose_head(
        &mut self,
        head: RenderHeadId,
        status: LiveNativePresentClockStatus,
    ) -> LiveNativePresentClockObservation {
        let clocked = self.heads.remove(&head).map(|r| r.sample.source);
        let unclocked = self.unclocked.remove(&head).map(|r| r.clock.source);
        LiveNativePresentClockObservation {
            lost: clocked.or(unclocked),
            current: None,
            status,
        }
    }

    /// Call before any modeset attempt, including one that rolls back to the
    /// old target before another query. Comparing target generations only
    /// would miss that A->B->A sequence. The caller routes each loss to users.
    pub fn invalidate(&mut self) -> Vec<LiveNativePresentClockSource> {
        let clocked = std::mem::take(&mut self.heads)
            .into_values()
            .map(|r| r.sample.source);
        let unclocked = std::mem::take(&mut self.unclocked)
            .into_values()
            .map(|r| r.clock.source);
        clocked.chain(unclocked).collect()
    }

    pub fn observe(
        &mut self,
        key: LiveNativePresentClockKey,
        sample: sophia_drm_clock::CrtcSequence,
    ) -> LiveNativePresentClockObservation {
        use LiveNativePresentClockStatus::*;
        if !key.head.is_valid() || key.target_generation == 0 || key.crtc_id == 0 {
            return self.lose_head(key.head, InvalidTarget);
        }
        if !sample.active {
            return self.lose_head(key.head, Inactive);
        }
        // A counter supersedes an unclocked answer for the same head.
        let was_unclocked = self.unclocked.remove(&key.head).map(|r| r.clock.source);
        let prior = self.heads.get(&key.head).copied();
        if let Some(record) = prior
            && record.key == key
            && sample.sequence == record.sample.msc
            && sample.timestamp_nsec < record.timestamp_nsec
        {
            // Re-observing the same field with a slightly older timestamp
            // provides no progress. Keep the accepted observation rather than
            // inventing a new counter lifetime or publishing a regressed UST.
            return LiveNativePresentClockObservation {
                lost: None,
                current: Some(record.sample),
                status: Observed,
            };
        }
        let restarted = prior.is_some_and(|r| {
            r.key != key
                || sample.sequence < r.sample.msc
                || sample.timestamp_nsec < r.timestamp_nsec
        });
        let lost = if restarted {
            self.heads.remove(&key.head).map(|r| r.sample.source)
        } else {
            None
        }
        .or(was_unclocked);
        let source = if let Some(record) = self.heads.get(&key.head) {
            record.sample.source
        } else {
            match self.mint() {
                Ok(source) => source,
                Err(status) => {
                    return LiveNativePresentClockObservation {
                        lost,
                        current: None,
                        status,
                    };
                }
            }
        };
        let current = LiveNativePresentClockSample {
            source,
            ust_usec: sample.timestamp_nsec / 1_000,
            msc: sample.sequence,
        };
        self.heads.insert(
            key.head,
            ClockRecord {
                key,
                sample: current,
                timestamp_nsec: sample.timestamp_nsec,
            },
        );
        LiveNativePresentClockObservation {
            lost,
            current: Some(current),
            status: if restarted || was_unclocked.is_some() {
                Restarted
            } else {
                Observed
            },
        }
    }
}

/// Only a kernel that cannot answer GET_SEQUENCE for this device at all is
/// definite: EOPNOTSUPP (no vblank support) or ENOTTY (no such ioctl).
/// Permission, transient, stale-object and driver errors stay query
/// failures and are asked again.
pub fn unsupported_sequence_errno(error: &std::io::Error) -> Option<i32> {
    let errno = error.raw_os_error()?;
    [
        rustix::io::Errno::OPNOTSUPP.raw_os_error(),
        rustix::io::Errno::NOTTY.raw_os_error(),
    ]
    .contains(&errno)
    .then_some(errno)
}

/// The card's monotonic-timestamp capability, cached per owned card fd.
/// A definite answer, supported or not, is read once for the fd's lifetime;
/// a failed query is not cached, so the next one asks again. `None` is a
/// failed query.
pub fn cached_monotonic_capability(
    cache: &mut BTreeMap<usize, bool>,
    card_group: usize,
    query: impl FnOnce() -> std::io::Result<u64>,
) -> Option<bool> {
    if let Some(supported) = cache.get(&card_group) {
        return Some(*supported);
    }
    let supported = query().ok()? != 0;
    cache.insert(card_group, supported);
    Some(supported)
}

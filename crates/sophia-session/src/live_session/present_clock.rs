//! Demand-driven Session bridge. Backend counter lifetimes and frontend
//! obligations are reconciled even when ordinary frame service is quarantined.
use sophia_backend_live::{
    LiveNativePresentClockObservation, LiveNativePresentClockSource, LiveProductionNativeScanout,
    LiveProductionVisualRuntime,
};
use sophia_engine::RenderHeadId;
use sophia_protocol::{OutputId, TransactionId};
use sophia_x_authority::{
    X11SetupSocketError, XPresentClockAdmission, XPresentClockSample, XPresentClockSource,
    XServerFrontendPresentClockRouter,
};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

type ClockResult<T> = Result<T, X11SetupSocketError>;

mod admission;

pub(super) trait FrontendClocks {
    fn admissions(&self) -> ClockResult<Vec<XPresentClockAdmission>>;
    fn bind_admission(
        &self,
        request: TransactionId,
        sample: XPresentClockSample,
    ) -> ClockResult<()>;
    fn query_demands(&self) -> ClockResult<Vec<(XPresentClockSource, u64)>>;
    fn bound_sources(&self) -> ClockResult<Vec<XPresentClockSource>>;
    fn lose_source(&self, source: XPresentClockSource) -> ClockResult<()>;
    fn observe_source(&self, sample: XPresentClockSample) -> ClockResult<()>;
}

impl FrontendClocks for XServerFrontendPresentClockRouter {
    fn admissions(&self) -> ClockResult<Vec<XPresentClockAdmission>> {
        self.admissions()
    }
    fn bind_admission(
        &self,
        request: TransactionId,
        sample: XPresentClockSample,
    ) -> ClockResult<()> {
        // A source switch uses the window's last accepted old-source sample.
        // Runtime contains timing/fence refusals; only transport invariants
        // (missing authority or poisoned locks) can escape this call.
        self.bind_admission(request, sample, None).map(|_| ())
    }
    fn query_demands(&self) -> ClockResult<Vec<(XPresentClockSource, u64)>> {
        self.query_demands()
    }
    fn bound_sources(&self) -> ClockResult<Vec<XPresentClockSource>> {
        self.bound_sources()
    }
    fn lose_source(&self, source: XPresentClockSource) -> ClockResult<()> {
        self.lose_source(source)
    }
    fn observe_source(&self, sample: XPresentClockSample) -> ClockResult<()> {
        self.observe_source(sample)
    }
}

fn source(source: LiveNativePresentClockSource) -> XPresentClockSource {
    XPresentClockSource::Hardware {
        domain: source.owner,
        incarnation: source.incarnation,
    }
}

#[derive(Clone, Copy)]
struct ClockHead {
    head: RenderHeadId,
    interval: Duration,
    observed: Option<XPresentClockSample>,
}

struct QueryWait {
    next: Instant,
    observed: XPresentClockSample,
    period: Option<Duration>,
    planned_fields: u64,
}

#[derive(Default)]
pub(super) struct SessionPresentClocks {
    // At most the live hardware sources with outstanding frontend debt.
    // The native head bound applies; no entry survives loss or final settle.
    next: BTreeMap<XPresentClockSource, QueryWait>,
    // Real flips still update ready/executed bindings, without arming queries.
    passive_observed: BTreeMap<XPresentClockSource, XPresentClockSample>,
    queries: u64,
}

impl SessionPresentClocks {
    /// Pass the unfiltered active native owner. None means its authority has
    /// ended (or native scanout was never enabled), even if disposal still
    /// retains the old owner. Quarantine and temporary borrows do not turn
    /// Some into None: modeset loss comes from explicit clock invalidation.
    pub(super) fn service(
        &mut self,
        frontend: &impl FrontendClocks,
        mut native: Option<&mut LiveProductionNativeScanout>,
        runtime: Option<&LiveProductionVisualRuntime>,
        primary_output: Option<OutputId>,
        now: Instant,
    ) -> ClockResult<()> {
        let live = native
            .as_ref()
            .map(|native| {
                native
                    .present_clock_heads()
                    .filter_map(|(id, clock)| {
                        native
                            .heads
                            .iter()
                            .find(|head| head.head == id && head.enabled)
                            .map(|head| {
                                (
                                    source(clock),
                                    ClockHead {
                                        head: id,
                                        interval: head.present_clock_minimum_period(),
                                        observed: native.present_clock_sample(clock).map(
                                            |sample| XPresentClockSample {
                                                source: source(clock),
                                                ust: sample.ust_usec,
                                                msc: sample.msc,
                                            },
                                        ),
                                    },
                                )
                            })
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.service_with(frontend, live, now, |head| {
            native
                .as_mut()
                .expect("only live native heads can be queried")
                .query_present_clock(head)
        })?;
        // New requests must bind even before native startup, during
        // quarantine and after owner release. Only source selection needs a
        // visual runtime/native owner; their absence selects the fake clock.
        self.admit_with(
            frontend,
            admission::monotonic_usec()?,
            |admissions| {
                runtime
                    .map(|runtime| {
                        runtime
                            .present_clock_outputs(
                                admissions.iter().filter_map(|request| request.target),
                                primary_output,
                            )
                            .into_iter()
                            .collect()
                    })
                    .unwrap_or_default()
            },
            |output| {
                native
                    .as_mut()
                    .map(|native| native.query_present_clock_for_output(output))
                    .unwrap_or_default()
            },
        )
    }

    fn service_with(
        &mut self,
        frontend: &impl FrontendClocks,
        live: BTreeMap<XPresentClockSource, ClockHead>,
        now: Instant,
        mut query: impl FnMut(RenderHeadId) -> LiveNativePresentClockObservation,
    ) -> ClockResult<()> {
        let bound = frontend.bound_sources()?;
        let demand = frontend
            .query_demands()?
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        let mut updated = Vec::new();
        self.next
            .retain(|source, _| live.contains_key(source) && demand.contains_key(source));
        self.passive_observed.retain(|source, _| {
            live.contains_key(source) && bound.contains(source) && !demand.contains_key(source)
        });
        for bound in bound {
            if bound == XPresentClockSource::Fake {
                continue;
            }
            let Some(head) = live.get(&bound) else {
                // Mandatory after invalidate/modeset/rollback and owner
                // replacement. A vanished clock emits no bad observations;
                // the frontend's rejection limit cannot detect this loss.
                frontend.lose_source(bound)?;
                continue;
            };
            let Some(&fields) = demand.get(&bound) else {
                if let Some(sample) = head.observed
                    && self.passive_observed.get(&bound) != Some(&sample)
                {
                    frontend.observe_source(sample)?;
                    self.passive_observed.insert(bound, sample);
                }
                continue;
            };
            if let Some(observed) = head.observed
                && self.next.get(&bound).is_some_and(|wait| {
                    let fields = observed.msc.wrapping_sub(wait.observed.msc);
                    observed.source == bound
                        && observed.ust >= wait.observed.ust
                        && fields > 0
                        && fields < (1 << 63)
                })
            {
                // A real flip (or a new request's real query) advanced this
                // head since our last visit. It re-arms the wake without an
                // ioctl, even if the old far-future timer has not expired.
                frontend.observe_source(observed)?;
                self.record_observation(observed, head.interval, fields, now);
                updated.push(bound);
                continue;
            }
            if self
                .next
                .get(&bound)
                .is_some_and(|wait| now < wait.next && fields >= wait.planned_fields)
            {
                continue;
            }
            self.queries = self.queries.saturating_add(1);
            let observation = query(head.head);
            if let Some(lost) = observation.lost {
                frontend.lose_source(source(lost))?;
            }
            if let Some(sample) = observation.current {
                frontend.observe_source(XPresentClockSample {
                    source: source(sample.source),
                    ust: sample.ust_usec,
                    msc: sample.msc,
                })?;
            }
            if observation
                .current
                .is_some_and(|sample| source(sample.source) == bound)
            {
                let sample = observation.current.expect("checked above");
                let observed = XPresentClockSample {
                    source: bound,
                    ust: sample.ust_usec,
                    msc: sample.msc,
                };
                self.record_observation(observed, head.interval, fields, now);
                updated.push(bound);
            } else {
                if observation.lost.map(source) != Some(bound) {
                    frontend.lose_source(bound)?;
                }
                self.next.remove(&bound);
            }
        }
        // Far-future debt uses two real observations to estimate a WAKE time,
        // never an MSC. Wake one field early; cap at 60s to recheck long waits.
        // A newly admitted earlier target forces a query on the next visit.
        // No request/only executed debt means no periodic query timer.
        let demand = frontend
            .query_demands()?
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        self.next.retain(|source, _| demand.contains_key(source));
        for source in updated {
            let Some(wait) = self.next.get_mut(&source) else {
                continue;
            };
            wait.planned_fields = demand[&source];
            if let Some(period) = wait.period {
                // VRR idle samples may describe the slowest rate. Flips can
                // resume at the mode's fastest rate before this wake, so never
                // estimate from a period longer than that legal minimum.
                let period = period.min(live[&source].interval);
                let fields = wait.planned_fields.saturating_sub(1).max(1);
                let nanos = period
                    .as_nanos()
                    .saturating_mul(u128::from(fields))
                    .min(Duration::from_secs(60).as_nanos());
                wait.next = now + Duration::from_nanos(nanos as u64);
            }
        }
        Ok(())
    }

    fn record_observation(
        &mut self,
        observed: XPresentClockSample,
        interval: Duration,
        fields: u64,
        now: Instant,
    ) {
        let period = self.next.get(&observed.source).and_then(|wait| {
            let fields = observed.msc.wrapping_sub(wait.observed.msc);
            let elapsed = observed.ust.saturating_sub(wait.observed.ust);
            if fields > 0 && fields < (1 << 63) && elapsed > 0 {
                Some(Duration::from_micros((elapsed / fields).max(1)))
            } else {
                wait.period
            }
        });
        self.next.insert(
            observed.source,
            QueryWait {
                next: now + interval,
                observed,
                period,
                planned_fields: fields,
            },
        );
    }

    pub(super) fn queries(&self) -> u64 {
        self.queries
    }

    pub(super) fn cap_wait(&self, now: Instant, maximum: Duration) -> Duration {
        self.next
            .values()
            .map(|wait| wait.next.saturating_duration_since(now))
            .min()
            .map_or(maximum, |wait| wait.min(maximum))
    }
}

#[cfg(test)]
#[path = "../../tests/support/session_present_clock.rs"]
mod tests;

#[cfg(test)]
#[path = "../../tests/support/session_present_admission.rs"]
mod admission_tests;

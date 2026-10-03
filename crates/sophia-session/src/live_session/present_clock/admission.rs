//! Source choice is per request, independent of native frame service.
use super::*;
use sophia_protocol::SurfaceId;

pub(super) fn monotonic_usec() -> ClockResult<u64> {
    let time = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    u64::try_from(time.tv_sec)
        .ok()
        .and_then(|seconds| seconds.checked_mul(1_000_000))
        .and_then(|usec| usec.checked_add(u64::try_from(time.tv_nsec).ok()? / 1_000))
        .ok_or_else(|| X11SetupSocketError::new("Present monotonic clock overflow"))
}

impl SessionPresentClocks {
    pub(super) fn admit_with(
        &mut self,
        frontend: &impl FrontendClocks,
        now_usec: u64,
        outputs: impl FnOnce(&[XPresentClockAdmission]) -> BTreeMap<SurfaceId, Option<OutputId>>,
        mut query: impl FnMut(OutputId) -> Vec<(RenderHeadId, LiveNativePresentClockObservation)>,
    ) -> ClockResult<()> {
        let admissions = frontend.admissions()?;
        if admissions.is_empty() {
            return Ok(());
        }
        // Build actual sampling coverage once per batch. A request with no
        // resolvable target is still visited in request order, using Fake.
        let outputs = outputs(&admissions);
        let mut samples = BTreeMap::new();
        for admission in admissions {
            let output = admission
                .target
                .and_then(|(surface, _)| outputs.get(&surface).copied().flatten());
            let sample = if let Some(output) = output {
                if let Some(sample) = samples.get(&output) {
                    *sample
                } else {
                    let mut chosen = None;
                    let mut unclocked = None;
                    // Query only on explicit admission demand. The backend
                    // prefers the mirror primary, then an active member.
                    for (head, observation) in query(output) {
                        self.queries = self.queries.saturating_add(1);
                        if let Some(lost) = observation.lost {
                            lose_native_source(frontend, lost)?;
                        }
                        if let LiveNativePresentClockStatus::Unclocked(clock) = observation.status {
                            // Prefer any working mirror member. Use the
                            // first active clockless member only if none has
                            // a real sample; unsupported is never Fake.
                            unclocked.get_or_insert(XPresentClockSample {
                                source: unclocked_source(clock),
                                ust: now_usec,
                                msc: 0,
                            });
                            if self.unclocked_reported.insert(head, clock.source)
                                != Some(clock.source)
                            {
                                let sophia_backend_live::LiveNativeUnclockedReason::SequenceUnsupported { errno } = clock.reason;
                                let reason = "sequence_unsupported";
                                crate::session_println!(
                                    "sophia_present_unclocked schema=1 head={} owner={} incarnation={} reason={} errno={} minimum_period_usec={}",
                                    head.raw(),
                                    clock.source.owner,
                                    clock.source.incarnation,
                                    reason,
                                    errno,
                                    clock.minimum_period_usec
                                );
                            }
                        }
                        if let Some(sample) = observation.current {
                            let sample = XPresentClockSample {
                                source: source(sample.source),
                                ust: sample.ust_usec,
                                msc: sample.msc,
                            };
                            frontend.observe_source(sample)?;
                            chosen = Some(sample);
                        }
                    }
                    let sample = chosen
                        .or(unclocked)
                        .unwrap_or_else(|| XPresentClockSample::background(now_usec));
                    samples.insert(output, sample);
                    sample
                }
            } else {
                XPresentClockSample::background(now_usec)
            };
            frontend.bind_admission(admission.request, sample)?;
        }
        Ok(())
    }
}

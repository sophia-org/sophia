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
                    // Query only on explicit admission demand. The backend
                    // prefers the mirror primary, then an active member.
                    for (_, observation) in query(output) {
                        self.queries = self.queries.saturating_add(1);
                        if let Some(lost) = observation.lost {
                            frontend.lose_source(source(lost))?;
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
                    let sample =
                        chosen.unwrap_or_else(|| XPresentClockSample::background(now_usec));
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

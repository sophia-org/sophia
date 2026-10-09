use super::*;
use crate::live_session::{LiveControlState, OwnerWake};
use sophia_backend_live::{
    LiveDrmTopologyMonitor, LiveSeatController, LiveSeatDisableOutcome, LiveSeatEvent,
};
use std::time::{Duration, Instant};

/// No WM or application has been launched yet. A missing monitor is a waiting
/// state, with logout and seat release still serviced. Probe retries are finite;
/// after the grace period, only a new topology/seat event starts another series.
pub(in crate::live_session) fn wait_for_startup_output(
    controller: &mut LiveSeatController,
    monitor: &mut LiveDrmTopologyMonitor,
    profile: &DesktopOutputCandidate,
    control: &mut LiveControlState,
    wake: &OwnerWake,
    mapping: sophia_protocol::OutputHeadMapping,
    cursor: &sophia_engine::CursorAsset,
) -> Result<
    Option<(
        sophia_backend_live::LiveProductionNativeScanout,
        DesktopOutputReconciliation,
    )>,
    Box<dyn Error>,
> {
    let mut active = true;
    let mut release_pending = false;
    let mut retry_at = Some(Instant::now());
    let mut attempts = 0;
    let mut waiting_reported = false;
    loop {
        wake.begin_pass()?;
        control.service_before_outputs();
        if control.take_session_requests().logout {
            return Ok(None);
        }
        while let Some(event) = controller.dispatch()? {
            match event {
                LiveSeatEvent::Enable => {
                    active = true;
                    release_pending = false;
                    attempts = 0;
                    retry_at = Some(Instant::now());
                }
                LiveSeatEvent::Disable => {
                    active = false;
                    release_pending = true;
                }
            }
        }
        if release_pending {
            release_pending = !matches!(
                controller.acknowledge_disable()?,
                LiveSeatDisableOutcome::Acknowledged
            );
        }
        if monitor.poll_notice()?.is_some() {
            attempts = 0;
            retry_at = Some(Instant::now());
        }
        if active && retry_at.is_some_and(|at| Instant::now() >= at) {
            let resolution = LiveNativeOutputDiscovery::probe(&controller.device_opener())
                .map_err(|error| Box::new(error) as Box<dyn Error>)
                .and_then(|discovery| resolve_output_replacement(discovery, profile, None));
            match resolution {
                Ok(OutputReplacementDecision::Active(prepared)) => {
                    match sophia_backend_live::LiveProductionNativeScanout::from_resolved_replacement(
                        prepared.native, mapping, cursor.clone(),
                    ) {
                        Ok(native) => return Ok(Some((native, prepared.realization))),
                        Err(error) if profile.availability == sophia_config::DesktopOutputAvailability::Adaptive => {
                            tracing::warn!(target: "sophia_scanout_evidence", "sophia_live_output_resolution schema=1 phase=startup status=construction_refused reason=hardware attempt={}", attempts + 1);
                            tracing::warn!(%error, "startup output construction refused");
                        }
                        Err(error) => return Err(error),
                    }
                }
                Ok(OutputReplacementDecision::Waiting) => {}
                Err(error) if profile.availability == sophia_config::DesktopOutputAvailability::Adaptive
                    && error.downcast_ref::<io::Error>().is_some() => {
                    tracing::warn!(target: "sophia_scanout_evidence", "sophia_live_output_resolution schema=1 phase=startup status=refused reason=hardware attempt={}", attempts + 1);
                    tracing::warn!(%error, "startup output probe refused");
                }
                Err(error) => return Err(error),
            }
            if !waiting_reported {
                crate::session_println!(
                    "sophia_live_output_resolution schema=1 phase=startup status=waiting generation={}",
                    profile.generation.raw()
                );
                waiting_reported = true;
            }
            retry_at = startup_retry_delay(attempts).map(|delay| Instant::now() + delay);
            attempts += 1;
        }
        // The monitor owns its queue without a wake fd. This checks that queue
        // and serves control; it does not rescan or open devices every tick.
        wake.wait_for_service(Duration::from_millis(100), Vec::new(), None)?;
    }
}

fn startup_retry_delay(attempt: usize) -> Option<Duration> {
    [250, 1_000, 4_000]
        .get(attempt)
        .map(|millis| Duration::from_millis(*millis))
}

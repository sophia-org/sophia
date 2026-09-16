//! One exact native owner retained across owner-loop return and error paths.
//! Completion means worker join AND established resource disposition, not None.

use sophia_backend_live::LiveProductionNativeScanout;
use std::time::{Duration, Instant};

pub(super) fn restore_retained_handoff<H, T, E>(
    handoff: &mut Option<H>,
    restore: impl FnOnce(Option<&H>) -> Result<T, E>,
) -> Result<T, E> {
    let result = restore(handoff.as_ref())?;
    drop(handoff.take());
    Ok(result)
}

pub(super) trait RenderRetirement<O> {
    fn drain(&mut self, native: &mut O) -> Result<(), Box<dyn std::error::Error>>;
    fn disposition(&self) -> Result<(), Box<dyn std::error::Error>>;
}

impl RenderRetirement<LiveProductionNativeScanout>
    for sophia_backend_live::LiveProductionVisualRuntime
{
    fn drain(
        &mut self,
        native: &mut LiveProductionNativeScanout,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.drain_native_scanout(native, Duration::from_secs(2))?;
        Ok(())
    }

    fn disposition(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.validate_native_retirement_disposition()?;
        Ok(())
    }
}

/// Final owner-loop exit, including an earlier unrelated failure. The device
/// effect is unavailable after revocation. A failed visual disposition still
/// requests worker shutdown but retains every unresolved owner for the caller's
/// terminal error carrier. Handoff and shell accounting remain caller-owned
/// until this returns the exact completion receipt.
pub(super) fn finish_render_owners<O: RetirementOwner, R: RenderRetirement<O>, S>(
    runtime: &mut Option<R>,
    scene: &mut Option<S>,
    native: &mut Option<O>,
    retirement: &mut NativeRetirement<O>,
    seat_active: bool,
    native_required: bool,
) -> Result<Option<CompletedNativeOwner>, Box<dyn std::error::Error>> {
    if !native_required && (native.is_some() || retirement.latest.is_some()) {
        return Err("disabled native profile has a native owner".into());
    }
    let visual = (|| -> Result<(), Box<dyn std::error::Error>> {
        if let Some(runtime) = runtime.as_mut() {
            if seat_active && let Some(native) = native.as_mut() {
                runtime.drain(native)?;
            }
            runtime.disposition()?;
        }
        Ok(())
    })();
    if visual.is_ok() {
        drop(runtime.take());
        drop(scene.take());
    }
    let begin = retirement.begin(
        native,
        if seat_active {
            RetirementMode::Drained
        } else {
            RetirementMode::DeviceRevoked
        },
        "session_shutdown",
    );
    if let Err(error) = visual {
        return Err(match begin {
            Ok(()) => error,
            Err(begin) => format!("{error}; native retirement: {begin}").into(),
        });
    }
    begin?;
    if native_required {
        retirement.finish_completed().map(Some)
    } else {
        // Explicit headless configuration is not native completion evidence.
        Ok(None)
    }
}

pub(super) trait RetirementOwner {
    fn identity(&self) -> u64;
    fn request_shutdown(&self);
    fn poll_shutdown(&self) -> Result<bool, Box<dyn std::error::Error>>;
    fn disposition(&self) -> Result<(), Box<dyn std::error::Error>>;
}

impl RetirementOwner for LiveProductionNativeScanout {
    fn identity(&self) -> u64 {
        self.retirement_owner_identity()
    }
    fn request_shutdown(&self) {
        self.request_renderer_worker_shutdown();
    }
    fn poll_shutdown(&self) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(self.poll_renderer_worker_shutdown()?)
    }
    fn disposition(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(self.validate_retirement_disposition()?)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RetirementMode {
    Drained,
    DeviceRevoked,
    Abandoned,
}

impl RetirementMode {
    pub fn from_suspend(outcome: sophia_backend_live::LiveProductionNativeSuspendOutcome) -> Self {
        use sophia_backend_live::LiveProductionNativeSuspendOutcome as Outcome;
        match outcome {
            Outcome::Drained => Self::Drained,
            Outcome::ForcedDetachRevoked => Self::DeviceRevoked,
            Outcome::ForcedDetachTimeout | Outcome::ForcedDetachDrainError => Self::Abandoned,
        }
    }
}

struct Retiring<O> {
    owner: O,
    mode: RetirementMode,
    cause: &'static str,
    started: Instant,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CompletedNativeOwner {
    pub identity: u64,
    pub mode: RetirementMode,
}

pub(super) struct NativeRetirement<O = LiveProductionNativeScanout> {
    latest: Option<u64>,
    pending: Option<Retiring<O>>,
    completed: Option<CompletedNativeOwner>,
}

impl<O> Default for NativeRetirement<O> {
    fn default() -> Self {
        Self {
            latest: None,
            pending: None,
            completed: None,
        }
    }
}

impl<O: RetirementOwner> NativeRetirement<O> {
    pub fn admit(&mut self, owner: &O) -> Result<(), &'static str> {
        if self.pending.is_some() {
            return Err("prior native owner is still retiring");
        }
        if let Some(latest) = self.latest
            && latest != owner.identity()
            && self
                .completed
                .is_none_or(|completed| completed.identity != latest)
        {
            return Err("prior native owner has no completed disposition");
        }
        if self.latest != Some(owner.identity()) {
            self.latest = Some(owner.identity());
            self.completed = None;
        }
        Ok(())
    }

    /// Transfer before request/poll or any other fallible operation. Repeated
    /// calls with an empty live slot preserve the existing retirement owner.
    pub fn begin(
        &mut self,
        live: &mut Option<O>,
        mode: RetirementMode,
        cause: &'static str,
    ) -> Result<(), &'static str> {
        let Some(owner) = live.as_ref() else {
            return Ok(());
        };
        self.admit(owner)?;
        self.pending = Some(Retiring {
            owner: live.take().expect("owner checked"),
            mode,
            cause,
            started: Instant::now(),
        });
        self.pending
            .as_ref()
            .expect("just retained")
            .owner
            .request_shutdown();
        Ok(())
    }

    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Neither mode runs KMS here. Active callers must already have drained;
    /// revoked owners with residual scanout custody remain unresolved. The
    /// worker result queue and Mixed owners are dropped only after BOTH checks.
    pub fn poll(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(pending) = &self.pending else {
            return Ok(true);
        };
        if !pending.owner.poll_shutdown()? {
            if pending.started.elapsed() >= Duration::from_secs(2) {
                return Err(format!(
                    "native retirement timed out: owner={} cause={} mode={:?}",
                    pending.owner.identity(),
                    pending.cause,
                    pending.mode
                )
                .into());
            }
            return Ok(false);
        }
        pending.owner.disposition()?;
        let completion = CompletedNativeOwner {
            identity: pending.owner.identity(),
            mode: pending.mode,
        };
        // No completion is recorded until actual owner destruction returned.
        drop(self.pending.take());
        self.completed = Some(completion);
        Ok(true)
    }

    /// For final teardown/reconstruction only, never seat acknowledgement.
    pub fn finish(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        while !self.poll()? {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }

    pub fn completion(&self) -> Result<CompletedNativeOwner, &'static str> {
        self.completed
            .filter(|c| Some(c.identity) == self.latest && !self.pending())
            .ok_or("native ownership has no exact completed retirement")
    }

    /// Associated handoff/accounting owners may be released only after this
    /// exact receipt, including when the live and pending slots are empty.
    pub fn finish_completed(&mut self) -> Result<CompletedNativeOwner, Box<dyn std::error::Error>> {
        self.finish()?;
        Ok(self.completion()?)
    }
}

impl<O: RetirementOwner> std::fmt::Debug for NativeRetirement<O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeRetirement")
            .field("latest", &self.latest)
            .field(
                "pending",
                &self
                    .pending
                    .as_ref()
                    .map(|p| (p.owner.identity(), p.mode, p.cause)),
            )
            .field("completed", &self.completed)
            .finish()
    }
}

/// The terminal error owns unresolved native AND associated accounting/handoff
/// state through the caller chain. Its destruction is terminal failure fallback,
/// never a successful retirement or permission to recycle a resource.
pub(super) struct RetirementFailure<O: RetirementOwner, H> {
    message: String,
    retirement: NativeRetirement<O>,
    _held: H,
}

impl<O: RetirementOwner, H> RetirementFailure<O, H> {
    pub fn new(message: String, retirement: NativeRetirement<O>, held: H) -> Self {
        Self {
            message,
            retirement,
            _held: held,
        }
    }
}

impl<O: RetirementOwner, H> std::fmt::Debug for RetirementFailure<O, H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetirementFailure")
            .field("message", &self.message)
            .field("retirement", &self.retirement)
            .finish_non_exhaustive()
    }
}

impl<O: RetirementOwner, H> std::fmt::Display for RetirementFailure<O, H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl<O: RetirementOwner, H> std::error::Error for RetirementFailure<O, H> {}

#[cfg(test)]
#[path = "../../tests/support/native_retirement.rs"]
mod tests;

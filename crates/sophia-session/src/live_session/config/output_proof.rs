use super::*;

/// One-shot proof control for the boundary between physical KMS acceptance and
/// candidate installation. A non-startup public proposal must never consume it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct OutputProofRollbackAfterApply {
    requested: bool,
    fired: bool,
}

impl OutputProofRollbackAfterApply {
    pub(super) const fn new(requested: bool) -> Self {
        Self {
            requested,
            fired: false,
        }
    }

    pub(super) fn take_for_startup(&mut self, startup: bool) -> bool {
        if !self.requested || self.fired || !startup {
            return false;
        }
        self.fired = true;
        true
    }
}

pub(super) fn parse_output_proof_rollback_after_apply(
    args: &[String],
    native_scanout: bool,
    normal_session: bool,
    wm_process_configured: bool,
    max_runtime: Option<Duration>,
    other_proof_control: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    let requested = args
        .iter()
        .any(|arg| arg == "--output-proof-rollback-after-apply");
    validate_output_proof_rollback_after_apply(
        requested,
        native_scanout,
        normal_session,
        wm_process_configured,
        max_runtime,
        std::env::var_os("SOPHIA_FRAME_FED_OUTPUT_ARM").as_deref()
            == Some(std::ffi::OsStr::new("1")),
        other_proof_control,
    )?;
    Ok(requested)
}

pub(super) fn validate_output_proof_rollback_after_apply(
    requested: bool,
    native_scanout: bool,
    normal_session: bool,
    wm_process_configured: bool,
    max_runtime: Option<Duration>,
    hardware_armed: bool,
    other_proof_control: bool,
) -> Result<(), &'static str> {
    if !requested {
        return Ok(());
    }
    if !native_scanout || !normal_session || !wm_process_configured || max_runtime.is_none() {
        return Err(
            "--output-proof-rollback-after-apply requires --native-scanout, --session-mode=normal, a configured sophia_wm_v1 --wm-process, and --max-runtime-ms",
        );
    }
    if !hardware_armed {
        return Err("set SOPHIA_FRAME_FED_OUTPUT_ARM=1 to arm --output-proof-rollback-after-apply");
    }
    if other_proof_control {
        return Err(
            "--output-proof-rollback-after-apply is mutually exclusive with WM proof controls",
        );
    }
    Ok(())
}

pub(super) fn validate_prepared_output_proof_candidate(
    requested: bool,
    prepared: bool,
) -> Result<(), &'static str> {
    if requested && !prepared {
        Err("--output-proof-rollback-after-apply requires a prepared startup output candidate")
    } else {
        Ok(())
    }
}

/// A physical proof holds publication until ordinary peer-loss cancellation
/// arrives. It never manufactures departure or grants authority to a peer.
// Exceeds the supervisor's two-second TERM-to-KILL grace, allowing reaping
// and worker event delivery. It is not a deadline for physical restoration.
pub(super) const OUTPUT_PEER_LOSS_DEPARTURE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug)]
pub(super) struct OutputPeerLossObservation {
    pub connection_epoch: u64,
    pub transaction: TransactionId,
    pub peer: u32,
    pub disconnected: bool,
    pub terminated: bool,
    pub failed: bool,
}

#[derive(Clone, Debug)]
pub(super) struct OutputPeerLossProof {
    requested: bool,
    target: Option<(u64, TransactionId, Instant)>,
    departure_observed: bool,
    failed: bool,
    restoration_observed: bool,
}

impl OutputPeerLossProof {
    pub(super) const fn new(requested: bool) -> Self {
        Self {
            requested,
            target: None,
            departure_observed: false,
            failed: false,
            restoration_observed: false,
        }
    }

    pub(super) fn arm(&mut self, epoch: u64, transaction: TransactionId, now: Instant) -> bool {
        if !self.requested || self.target.is_some() {
            return false;
        }
        self.requested = false;
        self.target = Some((epoch, transaction, now + OUTPUT_PEER_LOSS_DEPARTURE_TIMEOUT));
        true
    }

    pub(super) fn holds(&self, transaction: TransactionId) -> bool {
        self.target
            .is_some_and(|(_, target, _)| target == transaction)
    }

    pub(super) fn observe(&mut self, observation: Option<OutputPeerLossObservation>) {
        if let Some(observation) = observation
            && self.target.is_some_and(|(epoch, transaction, _)| {
                epoch == observation.connection_epoch && transaction == observation.transaction
            })
        {
            self.departure_observed = observation.disconnected && observation.terminated;
            self.failed |= observation.failed;
        }
    }

    pub(super) fn fail(&mut self, transaction: TransactionId) {
        if self.holds(transaction) {
            self.failed = true;
        }
    }

    pub(super) fn rollback_required(&self, transaction: TransactionId) -> bool {
        self.holds(transaction) && self.failed
    }

    /// The outer Session deadline remains a hard bound. An interrupted proof
    /// cannot qualify, even if ordinary shutdown subsequently restores KMS.
    pub(super) fn interrupt(&mut self) -> bool {
        let incomplete = self.requested || self.target.is_some();
        self.requested = false;
        let newly_failed = incomplete && !self.failed;
        self.failed |= incomplete;
        newly_failed
    }

    pub(super) fn poll(
        &mut self,
        now: Instant,
        observation: Option<OutputPeerLossObservation>,
    ) -> bool {
        // Late observations cannot turn an expired proof into a pass.
        let expired = self
            .target
            .is_some_and(|(_, transaction, _)| self.expire(transaction, now));
        self.observe(observation);
        expired
    }

    /// Expiry requests rollback once; it must never release the commit hold.
    pub(super) fn expire(&mut self, transaction: TransactionId, now: Instant) -> bool {
        if !self.departure_observed
            && !self.failed
            && self
                .target
                .is_some_and(|(_, target, deadline)| target == transaction && now >= deadline)
        {
            self.failed = true;
            return true;
        }
        false
    }

    pub(super) fn restored(&mut self, transaction: TransactionId) {
        if self.holds(transaction) {
            self.restoration_observed = true;
        }
    }

    pub(super) fn finish(&mut self) -> Result<Option<(u64, TransactionId)>, &'static str> {
        if !self.restoration_observed {
            return Ok(None);
        }
        if self.failed {
            return Err("output peer-loss proof failed; physical rollback completed");
        }
        if !self.departure_observed {
            return Ok(None);
        }
        self.restoration_observed = false;
        Ok(self
            .target
            .take()
            .map(|(epoch, transaction, _)| (epoch, transaction)))
    }
}

pub(super) fn validate_output_peer_loss_proof(
    requested: bool,
    native: bool,
    normal: bool,
    output_process: bool,
    runtime: Option<Duration>,
    armed: bool,
    other_proof: bool,
) -> Result<(), &'static str> {
    if !requested {
        return Ok(());
    }
    if !native || !normal || !output_process || runtime.is_none_or(|bound| bound.is_zero()) {
        return Err(
            "--output-proof-peer-loss-after-apply requires native normal Session, --output-process and a positive --max-runtime-ms",
        );
    }
    if !armed {
        return Err(
            "set SOPHIA_FRAME_FED_OUTPUT_ARM=1 to arm --output-proof-peer-loss-after-apply",
        );
    }
    if other_proof {
        return Err(
            "--output-proof-peer-loss-after-apply is mutually exclusive with other output and WM proof controls",
        );
    }
    Ok(())
}

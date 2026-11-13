//! G3-5d d-2b1, test-support only: `turn_combined` split into the steps of
//! the controlled owner turn (REV2 R5, as amended by REV3 section 6), so the
//! Session driver can run content between the image-owner service and the
//! walk. Each step is `turn_combined`'s own code for its half: the images
//! step is its hooked image-owner service for every slot, in every phase;
//! the walk step is its walk and `take_error` for every slot. Nothing here
//! is a production caller; gate 4 decides the production turn.
use super::*;

/// G3-5d d-2b2 (split review, b4 boundary): one admission's exact binding,
/// recorded in `submitted`, the one point where the row ticket, the
/// reservation's validator serial and job, the grant and the issuing head's
/// identity coexist. Nothing reconstructs it later from current state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Binding {
    pub(crate) ticket: u64,
    pub(crate) serial: u64,
    pub(crate) job: u64,
    pub(crate) grant: ContentGrant,
    /// (native owner, head, facade ticket), from `CombinedHead`.
    pub(crate) head: (u64, u64, u64),
}

impl CombinedRow {
    pub(super) fn record_binding(
        &mut self,
        ticket: u64,
        reservation: &CombinedReservation,
        open: &Open,
    ) {
        self.bindings.push(Binding {
            ticket,
            serial: reservation.validator_serial(),
            job: reservation.job(),
            grant: open.grant,
            head: open.head.trace_identity(),
        });
    }

    /// Every binding recorded on this row, in submission order.
    pub(crate) fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// Every collected result's job and disposition, in collection order.
    pub(crate) fn collected(&self) -> &[(u64, CombinedDisposition)] {
        &self.collected
    }

    /// The job's admitted validation deadline, unchanged since admission.
    pub(crate) fn admitted_deadline(&self, job: u64) -> Option<Instant> {
        self.jobs
            .iter()
            .find(|held| held.job == job)
            .map(|held| held.deadline)
    }

    /// REV3 5.1, labelled: the slot's next image-owner service that returns
    /// Ok in the images step fails with `reason` instead, after the service
    /// collected and reconciled what came back. It proves the driver's
    /// failure order on a real collected result, not that the failure
    /// happens there.
    pub(crate) fn declare_image_service_fault(&mut self, reason: &'static str) {
        self.image_fault = Some(reason);
    }
}

impl ShellComponentConnections {
    /// The slot's validation worker (d-2b2), for a control's hold, progress
    /// and serial handles only; None before the slot has an image owner.
    pub(crate) fn combined_validator(&mut self, slot: usize) -> Option<&mut Worker> {
        self.connections[slot]
            .images
            .as_mut()
            .map(|owner| owner.validation_worker())
    }

    /// Step 1: the turn's one hooked image-owner service per slot, through
    /// `turn_with` exactly as `turn_combined` runs it, without the walk.
    pub(crate) fn turn_combined_images<N: ShellContentNative>(
        &mut self,
        turn: &mut CombinedTurn<'_, N>,
    ) -> Failures {
        self.turn_with(|slot, connection, epochs| {
            let instant = turn.instant;
            let Some(owner) = connection.images.as_mut() else {
                return Ok(());
            };
            owner.service_combined(
                &mut connection.transport,
                epochs,
                content_now_msec(),
                instant,
                &mut CombinedHooks {
                    row: &mut connection.combined,
                    turn: &mut *turn,
                    slot,
                },
            )?;
            connection
                .combined
                .image_fault
                .take()
                .map_or(Ok(()), |reason| Err(ShellTransportError::Io(reason.into())))
        })
    }

    /// Step 3: every slot's walk, whatever its phase, then its `take_error`.
    /// An error of the slot's current, unrevoked attempt is that slot's
    /// failure; any other becomes a cleanup diagnostic (R1).
    pub(crate) fn turn_combined_walk<N: ShellContentNative>(
        &mut self,
        native: &mut N,
        instant: Instant,
    ) -> Failures {
        let mut failures: Failures = std::array::from_fn(|_| None);
        for (slot, (connection, failure)) in self
            .connections
            .iter_mut()
            .zip(failures.iter_mut())
            .enumerate()
        {
            let current = connection
                .attempt
                .filter(|(_, phase)| *phase != ComponentConnectionPhase::Revoked)
                .map(|(grant, _)| grant);
            let view = connection.transport.connection(&mut self.epochs);
            connection.combined.walk(
                &mut *native,
                current,
                &|binding: FrameCopyBinding, job: u64| {
                    view.content_frame_candidate_state(binding, job)
                },
                instant,
            );
            let error = connection.combined.take_error(current);
            if let (Some(grant), Some(error)) = (current, error) {
                *failure = Some((ComponentConnectionKey { slot, grant }, error));
            }
        }
        failures
    }
}

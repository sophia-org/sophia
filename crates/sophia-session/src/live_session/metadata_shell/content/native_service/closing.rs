//! Retain exact close and native removal obligations across service visits.
use super::*;
use sophia_backend_live::{LiveShellContentLayer, LiveShellContentRemoval};
use sophia_protocol::{ContentReason, NativeLauncherOpening, TransactionId};

pub(super) struct Closing {
    opening: NativeLauncherOpening,
    transaction: TransactionId,
    reason: ContentReason,
    removal: Option<LiveShellContentRemoval>,
    pixels_absent: bool,
}

impl NativeLauncherContentService {
    /// Retain before transport work. A refusal cannot authorize reopening or
    /// discard an already submitted candidate. Repeated calls must be exact.
    pub fn begin_close(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        opening: NativeLauncherOpening,
        transaction: TransactionId,
        reason: ContentReason,
    ) -> Result<(), ShellTransportError> {
        self.validate(transport)?;
        if opening.grant != self.grant
            || self.opening.is_some_and(|owned| owned != opening)
            || (self.closing.is_none()
                && transport.native_launcher_state().map(|v| v.0) != Some(opening)
                && transport.native_launcher_closed_opening() != Some(opening))
        {
            return Err(ShellTransportError::WrongActivation);
        }
        if let Some(owned) = &self.closing {
            if owned.opening != opening
                || owned.transaction != transaction
                || owned.reason != reason
            {
                return Err(ShellTransportError::WrongActivation);
            }
        } else {
            self.focus_pending = false;
            // Explicit close cancels only untransferred semantic input. The
            // transport retains already-issued event/ACK/activation obligations.
            self.inputs.clear();
            self.input_bytes = 0;
            self.opening = Some(opening);
            self.closing = Some(Closing {
                opening,
                transaction,
                reason,
                removal: None,
                pixels_absent: false,
            });
        }
        if transport.native_launcher_closed_opening() != Some(opening) {
            transport.close_native_launcher(opening, transaction, reason)?;
        }
        Ok(())
    }

    /// Dismiss only the exact transport-recorded admitted opening. Queue
    /// admission survives ordinary dismissal; grant revocation is separate.
    pub fn close_admitted(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        transaction: TransactionId,
    ) -> Result<bool, ShellTransportError> {
        self.validate(transport)?;
        let Some(opening) = transport.native_launcher_admitted_opening() else {
            return Ok(false);
        };
        match self.begin_close(transport, opening, transaction, ContentReason::Cancelled) {
            Ok(()) | Err(ShellTransportError::ContentQueueSaturated) => Ok(true),
            Err(error) => Err(error),
        }
    }

    /// The connected scheduler uses this before any new opening or input.
    /// None means no close; false retains a live removal/resource obligation.
    pub fn service_close_if_requested(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        runtime: &mut sophia_backend_live::LiveProductionVisualRuntime,
        scene: &sophia_backend_live::LiveProductionCpuScene,
        native: Option<&mut sophia_backend_live::LiveProductionNativeScanout>,
        transaction: &mut dyn FnMut() -> ServiceResult<TransactionId>,
    ) -> ServiceResult<Option<bool>> {
        if self.closing.is_none() {
            return Ok(None);
        }
        if !self.service_close_pixels(transport, runtime, scene, native)? {
            return Ok(Some(false));
        }
        self.settle_close_resources(transport, transaction)
            .map(Some)
    }

    /// True establishes only replacement presentation (or no submitted pixels).
    /// It does not release allocations, settle consumers, or permit reopening.
    /// The close owner and exact receipt remain retained even after true.
    pub fn service_close_pixels(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        runtime: &mut sophia_backend_live::LiveProductionVisualRuntime,
        scene: &sophia_backend_live::LiveProductionCpuScene,
        native: Option<&mut sophia_backend_live::LiveProductionNativeScanout>,
    ) -> ServiceResult<bool> {
        self.validate(transport)?;
        let close = self
            .closing
            .as_ref()
            .ok_or(ShellTransportError::WrongActivation)?;
        let (opening, transaction, reason) = (close.opening, close.transaction, close.reason);
        self.begin_close(transport, opening, transaction, reason)?;
        transport.poll_io()?;
        if transport.native_launcher_closed_opening() != Some(opening) {
            return Ok(false);
        }
        transport.service_closed_native_content(opening, self.content.now_msec())?;
        transport.service_closed_native_input(opening)?;
        self.content.observe_presentation(transport, runtime)?;
        let close = self
            .closing
            .as_mut()
            .expect("close retained through service");
        if close.pixels_absent {
            return Ok(true);
        }
        if !self.content.pending.is_empty() {
            return Ok(false);
        }
        let Some(candidate) = self.submitted else {
            close.pixels_absent = true;
            return Ok(true);
        };
        if close.removal.is_none() {
            close.removal = runtime.remove_shell_component_content(
                OutputId::from_raw(opening.output.id),
                LiveShellContentLayer::Launcher,
                self.grant,
                candidate,
                scene,
                native,
            )?;
        }
        close.pixels_absent = close
            .removal
            .is_some_and(|receipt| runtime.shell_component_removal_presented(receipt));
        Ok(close.pixels_absent)
    }
    /// Invalidate exact opening allocations only after replacement presentation,
    /// then inspect actual local owners. Even true is not a peer close barrier;
    /// the retained close continues to reject open service.
    pub fn settle_close_resources(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        transaction: &mut dyn FnMut() -> ServiceResult<TransactionId>,
    ) -> ServiceResult<bool> {
        self.validate(transport)?;
        let close = self
            .closing
            .as_ref()
            .ok_or(ShellTransportError::WrongActivation)?;
        if !close.pixels_absent {
            return Ok(false);
        }
        let opening = close.opening;
        if transport.native_launcher_closed_opening() != Some(opening) {
            return Err(ShellTransportError::WrongActivation.into());
        }
        let allocations = transport.content_allocation_snapshots();
        // Validate the entire set before removing any member. A different
        // opening is not ours to invalidate, even on the same connection.
        if allocations
            .iter()
            .any(|v| v.native_opening != Some(opening.opening) || v.output != opening.output)
        {
            return Err(ShellTransportError::WrongActivation.into());
        }
        for allocation in allocations {
            match transport.invalidate_content_allocation(
                transaction()?,
                allocation.allocation,
                ContentReason::Revoked,
            ) {
                Ok(()) => {}
                Err(ShellTransportError::ContentQueueSaturated) => return Ok(false),
                Err(error) => return Err(error.into()),
            }
        }
        // Store/FIFO own each invalidation after transfer; snapshots on retry
        // contain only still-active allocations, never replaying a terminal.
        transport.service_closed_native_content(opening, self.content.now_msec())?;
        transport.service_closed_native_input(opening)?;
        Ok(transport.closed_native_owners_settled(opening)?)
    }
    /// Open a successor only after exact old pixels and local owners settle.
    /// The transport retains its closed identity and refuses late old records;
    /// local settlement is not used as evidence of peer receipt or silence.
    pub fn reopen(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        transaction: TransactionId,
        next: NativeLauncherOpening,
    ) -> Result<bool, ShellTransportError> {
        self.validate(transport)?;
        let close = self
            .closing
            .as_ref()
            .ok_or(ShellTransportError::WrongActivation)?;
        if next.grant != self.grant || next.opening <= close.opening.opening {
            return Err(ShellTransportError::WrongActivation);
        }
        if !close.pixels_absent
            || !self.content.pending.is_empty()
            || !transport.closed_native_owners_settled(close.opening)?
        {
            return Ok(false);
        }
        transport.publish_native_launcher_opening(transaction, next)?;
        // Nothing fallible after FIFO transfer. Keep connection-wide counters
        // and published output facts; only the closed presentation is obsolete.
        self.content.presented.remove(&close.opening.output);
        self.opening = Some(next);
        self.submitted = None;
        self.closing = None;
        Ok(true)
    }
}

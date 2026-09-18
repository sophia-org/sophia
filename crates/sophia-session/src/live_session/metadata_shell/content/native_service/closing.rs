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
}

//! Existing component owner services capture receipts, including old epochs.
use super::*;
use crate::shell_capture_owner::{Binding, Owner, Publication};

#[cfg(test)]
#[path = "../../tests/support/component_capture_owner.rs"]
mod tests;

impl ShellComponentConnections {
    pub(super) fn capture_maintenance_wait(&self) -> Duration {
        if self.epochs.capture_retirement_active()
            || self
                .capture
                .as_ref()
                .is_some_and(|owner| owner.pending() != 0)
        {
            Duration::from_millis(10)
        } else {
            Duration::MAX
        }
    }
    /// The backend must supply a real native-owner inbox. Policy still exposes
    /// zero capture jobs, so no live admission installs one yet. Never replace
    /// an owner whose exact receipts or source-journal obligations may remain.
    #[allow(dead_code)]
    pub(crate) fn install_capture_owner(
        &mut self,
        inbox: sophia_renderer_live::LiveShellCaptureInbox,
    ) -> Result<(), sophia_renderer_live::LiveShellCaptureInbox> {
        if self.capture.is_some() {
            return Err(inbox);
        }
        self.capture = Some(Owner::new(inbox));
        Ok(())
    }

    pub(super) fn visit_capture_completion<'a>(
        &'a self,
        visit: &mut impl FnMut(rustix::event::PollFd<'a>),
    ) {
        if let Some(owner) = self
            .capture
            .as_ref()
            .filter(|owner| owner.awaiting_completion())
        {
            visit(rustix::event::PollFd::new(
                owner,
                rustix::event::PollFlags::IN,
            ));
        }
    }

    pub(super) fn service_capture_completions(&mut self) -> Vec<(Binding, ShellTransportError)> {
        let Some(owner) = self.capture.as_mut() else {
            return Vec::new();
        };
        let connections = &mut self.connections;
        owner.service(
            &mut self.epochs,
            content_now_msec(),
            std::time::Instant::now(),
            |binding, disposition| {
                let Some(connection) = connections.get_mut(binding.slot).filter(|connection| {
                    connection.attempt == Some((binding.grant, ComponentConnectionPhase::Connected))
                }) else {
                    return Ok(Publication::Disconnected);
                };
                connection.transport.release_capture_image_source(
                    binding.grant,
                    binding.metadata,
                    disposition,
                )?;
                Ok(Publication::Journaled)
            },
        )
    }

    pub(crate) fn retire_capture_imports<E>(
        &mut self,
        remaining: &mut usize,
        evict: &mut impl FnMut(&std::sync::Arc<sophia_runtime::ContentCapturedImage>) -> Result<(), E>,
    ) -> Result<(), E> {
        self.epochs.retire_capture_imports(remaining, evict)
    }
}

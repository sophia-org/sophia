//! Existing component owner services capture receipts, including old epochs.
use super::*;
use crate::shell_capture_owner::{Binding, Owner, Publication};
// The consumer is the native owner loop; feature-only builds retain the types
// for supplied mailbox controls without constructing a native owner.
#[cfg_attr(not(feature = "native-session"), allow(dead_code))]
pub(super) mod endpoint;
#[cfg_attr(not(feature = "native-session"), allow(dead_code))]
pub(super) mod intake;
pub(super) mod source;

#[cfg(test)]
#[path = "../../tests/support/component_capture_owner.rs"]
mod tests;

fn publish_current(
    connections: &mut [Connection],
    binding: Binding,
    publish: impl FnOnce(&mut Connection) -> Result<(), ShellTransportError>,
) -> Result<Publication, ShellTransportError> {
    let Some(connection) = connections.get_mut(binding.slot).filter(|connection| {
        connection.attempt == Some((binding.grant, ComponentConnectionPhase::Connected))
    }) else {
        return Ok(Publication::Disconnected);
    };
    publish(connection)?;
    Ok(Publication::Journaled)
}

impl ShellComponentConnections {
    /// Every admission, from a prepared destination through settlement,
    /// shares the one four-job bound.
    pub(super) fn capture_pending(&self) -> usize {
        self.capture_sources.pending()
            + self.capture.as_ref().map_or(0, Owner::pending)
            + usize::from(self.capture_intake.preparing())
    }

    /// Records the slot's granted render node (or its absence). This alone
    /// authorizes nothing: DMA-BUF also needs a matching live endpoint.
    #[cfg_attr(not(feature = "native-session"), allow(dead_code))]
    pub(crate) fn grant_capture_device(
        &mut self,
        slot: usize,
        device: Option<u64>,
    ) -> Result<(), ComponentConnectionError> {
        self.connections
            .get_mut(slot)
            .ok_or(ComponentConnectionError::UnknownComponent)?
            .capture_device = device;
        Ok(())
    }

    /// DMA-BUF facts for a negotiation, only when this slot's granted device
    /// is the live endpoint's device and that endpoint imports LINEAR XRGB.
    pub(super) fn capture_config(&self, slot: usize) -> Option<images::CaptureConfig> {
        let device = self.connections.get(slot)?.capture_device?;
        let destination = self.capture_link.destination()?;
        (destination.epoch.is_valid()
            && destination.facts.device == device
            && destination.facts.linear_xrgb
            && self
                .capture
                .as_ref()
                .is_some_and(|owner| !owner.is_closed()))
        .then_some(images::CaptureConfig {
            output: destination.output.raw(),
            device,
            max_width: destination.facts.max_width,
            max_height: destination.facts.max_height,
        })
    }

    /// The bounded launch wait for a capture slot: start once its endpoint
    /// is live, or once the caller's wait has elapsed (then negotiation
    /// refuses a required DMA-BUF capability rather than hiding it).
    #[cfg_attr(not(feature = "native-session"), allow(dead_code))]
    pub(crate) fn capture_launch_ready(&self, slot: usize) -> bool {
        self.connections
            .get(slot)
            .is_none_or(|c| c.capture_device.is_none())
            || self.capture_config(slot).is_some()
    }
    pub(super) fn capture_maintenance_wait(&self) -> Duration {
        if self.epochs.capture_retirement_active()
            || self.capture_link.needs_turn()
            || self.capture_pending() != 0
        {
            Duration::from_millis(10)
        } else {
            Duration::MAX
        }
    }
    /// The backend must supply a real native-owner inbox. Policy still exposes
    /// zero capture jobs, so no live admission installs one yet. Replacement
    /// requires a closed owner with no receipt or source-journal obligations.
    #[cfg(test)]
    pub(crate) fn install_capture_owner(
        &mut self,
        inbox: sophia_renderer_live::LiveShellCaptureInbox,
    ) -> Result<(), sophia_renderer_live::LiveShellCaptureInbox> {
        if self
            .capture
            .as_ref()
            .is_some_and(|owner| !owner.replaceable())
        {
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

    pub(crate) fn service_capture_completions(&mut self) -> Vec<(Binding, ShellTransportError)> {
        let mut errors =
            self.service_capture_sources(content_now_msec(), std::time::Instant::now());
        let Some(owner) = self.capture.as_mut() else {
            return errors;
        };
        let connections = &mut self.connections;
        errors.extend(owner.service(
            &mut self.epochs,
            content_now_msec(),
            std::time::Instant::now(),
            |binding, disposition| {
                publish_current(connections, binding, |connection| {
                    connection
                        .transport
                        .release_capture_image_source(binding.grant, binding.metadata, disposition)
                        .map(|_| ())
                })
            },
        ));
        errors
    }

    pub(crate) fn retire_capture_imports<E>(
        &mut self,
        remaining: &mut usize,
        evict: &mut impl FnMut(&std::sync::Arc<sophia_runtime::ContentCapturedImage>) -> Result<(), E>,
    ) -> Result<(), E> {
        self.epochs.retire_capture_imports(remaining, evict)
    }

    pub(crate) fn capture_retirement_pending(&self) -> bool {
        self.epochs.capture_retirement_pending()
    }
}

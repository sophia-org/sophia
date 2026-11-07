//! Diagnostic per-role readiness of the component descriptors in owner waits.
//! Roles only: no descriptor number, slot or component identity is retained.
use sophia_runtime::ShellPollRole;

mod tests;

/// What one borrowed component descriptor serves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FdRole {
    Transport(ShellPollRole),
    ImageWorker,
    #[cfg(feature = "experimental-shell-capture")]
    CaptureCompletion,
}

/// Cumulative owner waits, saturating. Each role adds at most one per wait
/// however many of its descriptors fired; roles overlap within a wait, so
/// their sum may exceed `any`. Error and hangup events count as ready.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ComponentFdReadiness {
    /// Observed waits with at least one component descriptor registered.
    pub waits: u64,
    /// Of those, waits in which any component descriptor was ready.
    pub any: u64,
    /// 9P server wake reader: journal self-notification.
    pub wire_wake: u64,
    /// 9P listener and connection streams: peer exchanges.
    pub wire: u64,
    pub image_listener: u64,
    /// Image channel hello or peer socket.
    pub image_socket: u64,
    /// Export file-authentication custody.
    pub authentication: u64,
    /// Pending negotiation listener or accepted stream.
    pub negotiation: u64,
    /// CPU image-worker completion.
    pub image_worker: u64,
    pub capture_completion: u64,
}

impl ComponentFdReadiness {
    #[cfg(test)]
    pub(super) fn add_wait(self, fds: impl IntoIterator<Item = (FdRole, bool)>) -> Self {
        self.add_observed(fds).0
    }

    /// Also this wait's joint cause bits (`owner_wait_causes`) for the owner's
    /// histogram: journal, wire, or any other component descriptor.
    pub(super) fn add_observed(
        mut self,
        fds: impl IntoIterator<Item = (FdRole, bool)>,
    ) -> (Self, u8) {
        use crate::owner_wait_causes::{bit, FD, JOURNAL, WIRE};
        let mut ready = [false; 8];
        let mut registered = false;
        for (role, fired) in fds {
            registered = true;
            let index = match role {
                FdRole::Transport(ShellPollRole::WireWake) => 0,
                FdRole::Transport(ShellPollRole::Wire) => 1,
                FdRole::Transport(ShellPollRole::ImageListener) => 2,
                FdRole::Transport(ShellPollRole::ImageSocket) => 3,
                FdRole::Transport(ShellPollRole::Authentication) => 4,
                FdRole::Transport(ShellPollRole::Negotiation) => 5,
                FdRole::ImageWorker => 6,
                #[cfg(feature = "experimental-shell-capture")]
                FdRole::CaptureCompletion => 7,
            };
            ready[index] |= fired;
        }
        if !registered {
            return (self, 0);
        }
        let causes =
            bit(ready[0], JOURNAL) | bit(ready[1], WIRE) | bit(ready[2..].contains(&true), FD);
        let add = |count: &mut u64, fired: bool| *count = count.saturating_add(u64::from(fired));
        add(&mut self.waits, true);
        add(&mut self.any, ready.contains(&true));
        for (count, fired) in [
            &mut self.wire_wake,
            &mut self.wire,
            &mut self.image_listener,
            &mut self.image_socket,
            &mut self.authentication,
            &mut self.negotiation,
            &mut self.image_worker,
            &mut self.capture_completion,
        ]
        .into_iter()
        .zip(ready)
        {
            add(count, fired);
        }
        (self, causes)
    }
}

//! Replacing the lock provider's process to follow the render device (t294).
//!
//! The provider's service and transport live as long as the session, so every
//! connection, and every image identity drawn from one, takes the next epoch of
//! one checked counter. A device change replaces only the process. From that
//! moment Session ignores the connection and submission events the old process
//! may still have in flight. The process is asked to exit and is never waited
//! for, and the service is asked to retire it. When the worker executes the
//! retirement it ends the connection and clears the assignee, so nobody is
//! admitted until the next authorization. Its `Retired` marker follows every
//! event the old process sent. The successor starts only once the marker is
//! seen and the old process reaped, in either order, with a launch prepared
//! for the latest device. Changes while a retirement is outstanding coalesce
//! into the latest device. A direct grant with no admitted device starts
//! nothing until one is admitted. A service that stops is terminal for the
//! provider: the cover keeps its fill.
use sophia_engine::SessionLockChord;
use sophia_runtime::lock_files::LockFileServiceEvent;

use crate::session_lock_frames::SessionLockFrames;
use crate::session_lock_input::SessionLockInput;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Child {
    Absent,
    Running,
    /// Asked to exit, not yet reaped.
    Exiting,
}

/// Every decision about the provider's process; the caller only carries it
/// out.
#[derive(Debug)]
pub struct LockProviderSuccession {
    child: Child,
    /// A retirement is owed to the service and not yet queued.
    retire_owed: bool,
    /// The one queued retirement whose marker has not arrived.
    marker_outstanding: bool,
    /// The running process's authorization is owed to the service.
    authorization_owed: bool,
    /// The next launch must be prepared for the latest device. The first
    /// launch is prepared like any other.
    stale: bool,
    awaiting_device: bool,
    failed: bool,
}

impl Default for LockProviderSuccession {
    fn default() -> Self {
        Self {
            child: Child::Absent,
            retire_owed: false,
            marker_outstanding: false,
            authorization_owed: false,
            stale: true,
            awaiting_device: false,
            failed: false,
        }
    }
}

/// What a provider start that has come due may do.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LockProviderStart {
    /// The old process is not yet reaped, the retirement not yet queued or
    /// marked, or the service has stopped.
    Wait,
    /// A direct grant with no admitted device: nothing starts.
    AwaitDevice,
    /// Start. With `regrant`, the launch is first prepared from the ungranted
    /// base for the latest device under the next grant epoch.
    Start { regrant: bool },
}

impl LockProviderSuccession {
    /// The admitted device changed. Returns whether the running process must
    /// be asked to exit. One retirement is outstanding at a time; a change
    /// while one is outstanding only moves the target to the latest device.
    pub fn device_changed(&mut self) -> bool {
        if self.failed {
            return false;
        }
        self.stale = true;
        self.authorization_owed = false;
        if !self.marker_outstanding {
            self.retire_owed = true;
        }
        let terminate = self.child == Child::Running;
        if terminate {
            self.child = Child::Exiting;
        }
        terminate
    }

    /// Whether a retirement is owed to the service.
    pub fn retire_owed(&self) -> bool {
        self.retire_owed && !self.failed
    }

    /// The service accepted the owed retirement; a full queue is not this.
    pub fn retire_queued(&mut self) {
        if std::mem::take(&mut self.retire_owed) {
            self.marker_outstanding = true;
        }
    }

    /// The retirement's `Retired` marker arrived.
    pub fn retired(&mut self) {
        self.marker_outstanding = false;
    }

    /// Whether a connection or submission event belongs to the current
    /// process. While a retirement is owed or unmarked it may be the old
    /// process's, which is never admitted.
    pub fn admits_provider_events(&self) -> bool {
        !self.failed && !self.retire_owed && !self.marker_outstanding
    }

    /// Whether a service event goes on to Session. A `Retired` marker is
    /// consumed here. A connection or submission event that arrives while a
    /// retirement is owed or unmarked may be the old process's and is
    /// dropped; a disconnection always goes on.
    pub fn hands_on(&mut self, event: &LockFileServiceEvent) -> bool {
        match event {
            LockFileServiceEvent::Retired { .. } => {
                self.retired();
                false
            }
            LockFileServiceEvent::Connected { .. } | LockFileServiceEvent::Inbound { .. } => {
                self.admits_provider_events()
            }
            _ => true,
        }
    }

    /// A process was spawned; its authorization is owed.
    pub fn child_started(&mut self) {
        self.child = Child::Running;
        self.authorization_owed = true;
    }

    /// Whether the running process's authorization may be queued now: after
    /// any owed retirement, so the worker executes them in that order.
    pub fn authorization_due(&self) -> bool {
        self.authorization_owed && !self.retire_owed && !self.failed
    }

    /// The service accepted the authorization; a full queue is not this.
    pub fn authorization_queued(&mut self) {
        self.authorization_owed = false;
    }

    /// The running process was asked to exit for a reason of the caller's own,
    /// such as a launch it could not authorize.
    pub fn child_terminating(&mut self) {
        self.authorization_owed = false;
        if self.child == Child::Running {
            self.child = Child::Exiting;
        }
    }

    /// The process was reaped. Returns whether it had been asked to exit;
    /// otherwise it failed on its own and the ordinary backoff applies.
    pub fn child_reaped(&mut self) -> bool {
        let asked = self.child == Child::Exiting;
        self.child = Child::Absent;
        self.authorization_owed = false;
        asked
    }

    /// Whether the next launch must be prepared for the latest device.
    pub fn regrant_pending(&self) -> bool {
        self.stale
    }

    /// Whether the last due start found no admitted device.
    pub fn awaiting_device(&self) -> bool {
        self.awaiting_device
    }

    /// A start has come due. `device_admitted` is whether the grant can be
    /// prepared: a denied grant always can, a direct one only with a device.
    pub fn start(&mut self, device_admitted: bool) -> LockProviderStart {
        if self.failed || self.child != Child::Absent || !self.admits_provider_events() {
            return LockProviderStart::Wait;
        }
        if self.stale && !device_admitted {
            self.awaiting_device = true;
            return LockProviderStart::AwaitDevice;
        }
        self.awaiting_device = false;
        LockProviderStart::Start {
            regrant: self.stale,
        }
    }

    /// The launch was prepared for the latest device. A preparation that
    /// failed leaves it stale, so no older launch is ever used.
    pub fn regranted(&mut self) {
        self.stale = false;
    }

    /// The service stopped. Only the first time, returns whether the running
    /// process must be asked to exit. Nothing starts afterwards.
    pub fn fail(&mut self) -> Option<bool> {
        if std::mem::replace(&mut self.failed, true) {
            return None;
        }
        self.retire_owed = false;
        self.authorization_owed = false;
        let terminate = self.child == Child::Running;
        if terminate {
            self.child = Child::Exiting;
        }
        Some(terminate)
    }

    pub fn failed(&self) -> bool {
        self.failed
    }
}

/// Ends what a replaced or failed provider was granted, at once and before
/// any successor: its chords on the lock keyboard and its images on every
/// head. Returns whether the drawn images changed, so the caller repaints the
/// cover.
pub fn revoke_replaced_lock_provider(
    chords: &mut Vec<SessionLockChord>,
    input: Option<&mut SessionLockInput>,
    frames: &mut SessionLockFrames,
) -> bool {
    chords.clear();
    if let Some(input) = input {
        input.set_chords(Vec::new());
    }
    frames.provider_replaced()
}

//! `sophia_shell_fs_v1`: the file wire for one admitted component epoch.
//! The export owns file custody; this wire turns its 9P server on the
//! owner loop without blocking and moves queued events into the journal.
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use sophia_9p::records::Limits;
use sophia_9p::unix::Server;
use sophia_protocol::shell_files::{
    SHELL_FILE_ACK_PROGRESS_TIMEOUT_MILLIS, SHELL_FILE_MAX_JOURNAL_RECORDS,
    SHELL_FILE_TERMINAL_RESERVE_RECORDS, ShellFileKind,
};

use super::ShellTransportError;
use crate::ContentStoreProfile;

mod export;
mod journal;

pub(super) use export::{CandidateFamily, Inbound, NativeCandidatePart, ShellFiles};
pub(super) use journal::JournalBounds;

/// Nonblocking turns spent flushing a revocation; bounded so a peer that
/// stops reading cannot hold the owner.
const REVOKE_TURNS: usize = 4;

const ACK_PROGRESS_TIMEOUT: Duration =
    Duration::from_millis(SHELL_FILE_ACK_PROGRESS_TIMEOUT_MILLIS as u64);

/// The largest Session-to-client record of each role in its whole file
/// framing: the 32-byte header plus the native body of an `AllocationResult`
/// (168 bytes, bar and dock) or a `NativeInput` (398 bytes, launcher).
pub(super) const ALLOCATION_RESULT_RECORD_BYTES: usize =
    sophia_protocol::shell_files::SHELL_FILE_HEADER_BYTES + 168;
pub(super) const NATIVE_INPUT_RECORD_BYTES: usize =
    sophia_protocol::shell_files::SHELL_FILE_HEADER_BYTES + 398;
/// LauncherRequest is the largest descriptor-family event, including its
/// domain transaction and file header. Combined base content is smaller.
pub(super) const DESCRIPTOR_RECORD_BYTES: usize = 352;

/// The journal byte bounds for a role whose largest record is `record`:
/// 256 of them rounded up to a power of two (at most 1 MiB), with 64 of them,
/// one per terminal-reserve record, kept for credited responses.
pub(super) const fn journal_bounds(record: usize) -> JournalBounds {
    let bytes = (record * SHELL_FILE_MAX_JOURNAL_RECORDS as usize).next_power_of_two();
    JournalBounds {
        bytes: if bytes > 1024 * 1024 {
            1024 * 1024
        } else {
            bytes
        },
        reserve_bytes: record * SHELL_FILE_TERMINAL_RESERVE_RECORDS as usize,
    }
}

/// The role name `api` reports and the journal byte bounds of its profile.
pub(super) fn role_bounds(profile: Option<ContentStoreProfile>) -> (&'static str, JournalBounds) {
    match profile {
        Some(ContentStoreProfile::NativeLauncher) => {
            ("launcher", journal_bounds(NATIVE_INPUT_RECORD_BYTES))
        }
        Some(ContentStoreProfile::PersistentCatalog) => {
            ("dock", journal_bounds(ALLOCATION_RESULT_RECORD_BYTES))
        }
        _ => ("bar", journal_bounds(ALLOCATION_RESULT_RECORD_BYTES)),
    }
}

pub(super) struct ShellFileWire {
    server: Server<ShellFiles>,
}

impl ShellFileWire {
    /// Serves one already accepted and admitted stream. This reactor is not
    /// a listener: its single connection is the component's epoch.
    pub(super) fn adopt(
        stream: UnixStream,
        export: ShellFiles,
    ) -> Result<Self, ShellTransportError> {
        let limits = Limits::new(65536, 512, 16, 32, 131072, 1)
            .map_err(|error| ShellTransportError::Io(format!("9P limits: {error:?}")))?;
        let mut server = Server::new(export, limits).map_err(io_error)?;
        let connection = server
            .adopt(stream)
            .map_err(|refused| io_error(refused.error))?;
        server.export_mut().bind_connection(connection);
        Ok(Self { server })
    }

    pub(super) fn export(&self) -> &ShellFiles {
        self.server.export()
    }

    pub(super) fn export_mut(&mut self) -> &mut ShellFiles {
        self.server.export_mut()
    }

    /// The server's readiness, borrowed for the owner's wait. The next
    /// [`Self::turn`] consumes it.
    pub(super) fn poll_fds(&self) -> Vec<rustix::event::PollFd<'_>> {
        self.server.poll_fds()
    }

    /// One nonblocking turn: whatever requests are ready, no waiting.
    pub(super) fn turn(&mut self) -> Result<(), ShellTransportError> {
        if !self.server.turn(Some(Duration::ZERO)).map_err(io_error)?
            || self.server.connection_count() == 0
        {
            self.server.export_mut().revoke();
            return Err(ShellTransportError::NotConnected);
        }
        self.server.export_mut().expire();
        Ok(())
    }

    /// Appends one queued event if the journal has room for its class, and
    /// wakes waiting reads. `Ok(false)` leaves the event queued.
    pub(super) fn append(
        &mut self,
        kind: ShellFileKind,
        body: &[u8],
        credited: bool,
    ) -> Result<bool, ShellTransportError> {
        match self.server.export_mut().append_event(kind, body, credited) {
            Ok(_) => {
                self.server.wake().wake();
                Ok(true)
            }
            Err(sophia_9p::Errno::EAGAIN) => Ok(false),
            Err(error) => Err(ShellTransportError::Io(format!("shell journal: {error:?}"))),
        }
    }

    /// Publishes one snapshot object with its journaled announcement, and
    /// wakes waiting reads. `Ok(false)` leaves it queued.
    pub(super) fn publish(
        &mut self,
        kind: ShellFileKind,
        body: &[u8],
        credited: bool,
    ) -> Result<bool, ShellTransportError> {
        let published = self
            .server
            .export_mut()
            .publish_object(kind, body, credited)
            .map_err(|error| ShellTransportError::Io(format!("shell object: {error:?}")))?;
        if published {
            self.server.wake().wake();
        }
        Ok(published)
    }

    /// A reader that leaves queued events blocked without acknowledging for
    /// the deadline is closed; the owner revokes only this component.
    pub(super) fn check_ack_progress(
        &self,
        blocked: bool,
        now: Instant,
    ) -> Result<(), ShellTransportError> {
        if blocked
            && now.saturating_duration_since(self.export().journal().last_progress())
                >= ACK_PROGRESS_TIMEOUT
        {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        Ok(())
    }

    /// Ends the epoch. Before the socket closes, a few nonblocking turns
    /// answer every waiting read `ESTALE` and flush replies already owed,
    /// such as the acknowledgement of a terminal event, so the peer observes
    /// the revocation rather than a bare EOF.
    pub(super) fn revoke(&mut self) {
        self.server.export_mut().revoke();
        self.server.wake().wake();
        for _ in 0..REVOKE_TURNS {
            match self.server.turn(Some(Duration::ZERO)) {
                Ok(true) if self.server.connection_count() > 0 => {}
                _ => break,
            }
        }
    }
}

/// The immutable `limits` object of a negotiated content grant.
pub(super) fn encode_limits_object(
    epoch: u64,
    limits: &sophia_protocol::ContentLimits,
) -> Result<Vec<u8>, ShellTransportError> {
    use sophia_protocol::shell_files::{ShellFileHeader, encode_shell_file_limits};
    encode_shell_file_limits(
        ShellFileHeader {
            kind: ShellFileKind::Limits,
            connection_epoch: epoch,
            submission_id: 0,
            sequence: 0,
        },
        limits.clone(),
    )
    .map_err(|_| ShellTransportError::WrongContentRecord)
}

pub(super) fn encode_negotiated(
    welcome: sophia_protocol::ShellV1ServerWelcome,
    limits_published: bool,
) -> Result<Vec<u8>, ShellTransportError> {
    use sophia_protocol::shell_files::{ShellFileNegotiated, encode_shell_file_negotiated_body};
    encode_shell_file_negotiated_body(ShellFileNegotiated {
        welcome,
        limits_published,
    })
    .map_err(|_| ShellTransportError::UnsupportedRevision)
}

pub(super) fn encode_refused(
    refusal: &sophia_protocol::ContentAdmissionRefused,
) -> Result<Vec<u8>, ShellTransportError> {
    sophia_protocol::shell_files::encode_shell_file_refused_body(refusal)
        .map_err(|_| ShellTransportError::ContentAdmissionRefused(refusal.clone()))
}

fn io_error(error: std::io::Error) -> ShellTransportError {
    ShellTransportError::Io(error.to_string())
}

//! `sophia_shell_fs_v1`: the file wire for one admitted component epoch.
//! The export owns file custody; this wire turns its 9P server on the
//! owner loop without blocking and moves queued events into the journal.
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use sophia_9p::records::Limits;
use sophia_9p::unix::Server;
use sophia_protocol::shell_files::{SHELL_FILE_ACK_PROGRESS_TIMEOUT_MILLIS, ShellFileKind};

use super::ShellTransportError;
use crate::ContentStoreProfile;

mod export;
mod journal;
mod staging;

pub(super) use export::{Inbound, ShellFiles};
pub(super) use journal::JournalBounds;

const ACK_PROGRESS_TIMEOUT: Duration =
    Duration::from_millis(SHELL_FILE_ACK_PROGRESS_TIMEOUT_MILLIS as u64);

/// The role name `api` reports and the journal byte bounds of its profile:
/// 256 times the largest Session-to-client record in file framing, rounded
/// up to a power of two, and 64 times it as the terminal reserve.
pub(super) fn role_bounds(profile: Option<ContentStoreProfile>) -> (&'static str, JournalBounds) {
    match profile {
        // Native launcher Input, up to 420 bytes in file framing.
        Some(ContentStoreProfile::NativeLauncher) => (
            "launcher",
            JournalBounds {
                bytes: 131_072,
                reserve_bytes: 26_880,
            },
        ),
        // AllocationResult, 192 bytes in file framing, for bar and dock.
        Some(ContentStoreProfile::PersistentCatalog) => (
            "dock",
            JournalBounds {
                bytes: 65_536,
                reserve_bytes: 12_288,
            },
        ),
        _ => (
            "bar",
            JournalBounds {
                bytes: 65_536,
                reserve_bytes: 12_288,
            },
        ),
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

    pub(super) fn revoke(&mut self) {
        self.server.export_mut().revoke();
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

/// Encodes one server content record as a file event body. Only records the
/// file contract carries so far are accepted; anything else fails closed
/// rather than crossing as an old IPC frame.
pub(super) fn encode_content_event(
    transaction: sophia_protocol::TransactionId,
    record: &sophia_protocol::ShellContentRecord,
) -> Result<(ShellFileKind, Vec<u8>), ShellTransportError> {
    use sophia_protocol::ShellContentRecord;
    use sophia_protocol::shell_files::{
        ShellFileTransactionRecord, encode_shell_file_allocation_result_body,
        encode_shell_file_outputs_body, encode_shell_file_resource_released_body,
        encode_shell_file_resource_status_body,
    };
    let value = ShellFileTransactionRecord {
        transaction,
        record: record.clone(),
    };
    match record {
        ShellContentRecord::ResourceStatus(_) => Ok((
            ShellFileKind::ResourceStatus,
            encode_shell_file_resource_status_body(&value)
                .map_err(|_| ShellTransportError::WrongContentRecord)?,
        )),
        ShellContentRecord::ResourceReleased(_) => Ok((
            ShellFileKind::ResourceReleased,
            encode_shell_file_resource_released_body(&value)
                .map_err(|_| ShellTransportError::WrongContentRecord)?,
        )),
        ShellContentRecord::OutputFacts(_) => {
            let body = encode_shell_file_outputs_body(&ShellFileTransactionRecord {
                transaction,
                record: record.clone(),
            })
            .map_err(|_| ShellTransportError::WrongContentRecord)?;
            Ok((ShellFileKind::Outputs, body))
        }
        ShellContentRecord::AllocationResult(_) => {
            let body = encode_shell_file_allocation_result_body(&ShellFileTransactionRecord {
                transaction,
                record: record.clone(),
            })
            .map_err(|_| ShellTransportError::WrongContentRecord)?;
            Ok((ShellFileKind::AllocationResult, body))
        }
        _ => Err(ShellTransportError::WrongContentRecord),
    }
}

fn io_error(error: std::io::Error) -> ShellTransportError {
    ShellTransportError::Io(error.to_string())
}

//! Nonblocking 9P turns for the separately supervised output authority.
use std::path::Path;
use std::time::{Duration, Instant};

use sophia_9p::{Errno, records::Limits, unix::Server};
use sophia_protocol::output_files::OutputFileLimits;
use sophia_protocol::{OutputAuthoritySnapshot, OutputV1Outcome, TransactionId};

use crate::{
    AdmittedOutputProposal, OutputFileExport, OutputFileQids, OutputFileSubmission, PolicyRole,
    PolicyRoleEndpoint, PolicyRoleEndpointError,
};

pub const SOPHIA_OUTPUT_9P_SOCKET_ENV: &str = "SOPHIA_OUTPUT_9P_SOCKET";

#[derive(Debug)]
pub enum OutputFileTransportError {
    Endpoint(PolicyRoleEndpointError),
    File(Errno),
    Io(std::io::Error),
}

impl std::fmt::Display for OutputFileTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for OutputFileTransportError {}
impl From<Errno> for OutputFileTransportError {
    fn from(error: Errno) -> Self {
        Self::File(error)
    }
}
impl From<std::io::Error> for OutputFileTransportError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<PolicyRoleEndpointError> for OutputFileTransportError {
    fn from(error: PolicyRoleEndpointError) -> Self {
        Self::Endpoint(error)
    }
}

pub struct OutputFileTransport {
    endpoint: PolicyRoleEndpoint,
    qids: OutputFileQids,
    limits: OutputFileLimits,
    next_epoch: u64,
    server: Option<Server<OutputFileExport>>,
    negotiation_deadline: Option<Instant>,
}

impl OutputFileTransport {
    pub fn bind_for_supervised_uid(
        directory: impl AsRef<Path>,
        uid: u32,
        first_epoch: u64,
        limits: OutputFileLimits,
    ) -> Result<Self, OutputFileTransportError> {
        limits.validate().map_err(|_| Errno::EINVAL)?;
        if first_epoch == 0 {
            return Err(Errno::EINVAL.into());
        }
        Ok(Self {
            endpoint: PolicyRoleEndpoint::bind_role_for_supervised_uid(
                directory,
                PolicyRole::Output,
                uid,
            )?,
            qids: OutputFileQids::default(),
            limits,
            next_epoch: first_epoch,
            server: None,
            negotiation_deadline: None,
        })
    }

    pub fn socket_path(&self) -> &Path {
        self.endpoint.socket_path()
    }

    pub fn next_epoch(&self) -> u64 {
        self.next_epoch
    }

    pub fn authorize_supervised_pid(&mut self, pid: u32) -> Result<(), OutputFileTransportError> {
        self.endpoint.authorize_supervised_pid(pid)?;
        Ok(())
    }

    /// One accept attempt. The endpoint verifies credentials before any
    /// bytes can reach 9P; failure leaves the current epoch unassigned.
    pub fn poll_accept(
        &mut self,
        snapshot: &OutputAuthoritySnapshot,
    ) -> Result<bool, OutputFileTransportError> {
        if self.server.is_some() {
            return Err(Errno::EAGAIN.into());
        }
        let next = self.next_epoch.checked_add(1).ok_or(Errno::ENOSPC)?;
        let epoch = self.next_epoch;
        let Some(stream) = self.endpoint.poll_expected()? else {
            return Ok(false);
        };
        // From admission onward, even a failed protocol setup spends the
        // epoch. No candidate from this stream may match a later connection.
        self.next_epoch = next;
        let setup = (|| {
            let export =
                OutputFileExport::new(epoch, self.limits, snapshot.clone(), self.qids.clone())?;
            let limits = Limits::new(65_536, 512, 16, 32, 131_072, 1).map_err(|_| Errno::EINVAL)?;
            let mut server = Server::new(export, limits)?;
            let connection = server.adopt(stream).map_err(|error| error.error)?;
            server.export_mut().bind_connection(connection)?;
            Ok::<_, OutputFileTransportError>(server)
        })();
        match setup {
            Ok(server) => self.server = Some(server),
            Err(error) => {
                if let Some(peer) = self.endpoint.active_peer() {
                    self.endpoint.release_peer(peer)?;
                }
                return Err(error);
            }
        }
        self.negotiation_deadline = Some(
            Instant::now() + Duration::from_millis(self.limits.assembly_timeout_millis.into()),
        );
        Ok(true)
    }

    pub fn export(&self) -> Option<&OutputFileExport> {
        self.server.as_ref().map(Server::export)
    }

    /// No waiting and no unbounded queue. A false result asks the service to
    /// call disconnect and settle its abandoned owner work before accepting.
    pub fn turn(&mut self) -> Result<bool, OutputFileTransportError> {
        let Some(server) = &mut self.server else {
            return Ok(false);
        };
        let now = Instant::now();
        server.export_mut().expire(now);
        if server
            .export()
            .admission()
            .connection()
            .selected_capabilities()
            != 0
        {
            self.negotiation_deadline = None;
        }
        if self
            .negotiation_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            server.export_mut().revoke();
        }
        let running = server.turn(Some(Duration::ZERO))?;
        Ok(running && server.connection_count() != 0 && !server.export().is_revoked())
    }

    pub fn take_delivery(&mut self) -> Option<OutputFileSubmission> {
        self.server.as_mut()?.export_mut().take_delivery()
    }

    pub fn publish(
        &mut self,
        snapshot: &OutputAuthoritySnapshot,
    ) -> Result<u64, OutputFileTransportError> {
        let server = self.server.as_mut().ok_or(Errno::ESTALE)?;
        let qid = server.export_mut().publish(snapshot)?;
        server.wake().wake();
        Ok(qid)
    }

    pub fn settle(
        &mut self,
        transaction: TransactionId,
        outcome: OutputV1Outcome,
    ) -> Result<Option<AdmittedOutputProposal>, OutputFileTransportError> {
        let server = self.server.as_mut().ok_or(Errno::ESTALE)?;
        let promoted = server.export_mut().settle(transaction, outcome)?;
        server.wake().wake();
        Ok(promoted)
    }

    pub fn disconnect(&mut self) -> Result<Vec<AdmittedOutputProposal>, OutputFileTransportError> {
        let mut abandoned = Vec::new();
        if let Some(mut server) = self.server.take() {
            abandoned = server
                .export()
                .admission()
                .connection()
                .clone()
                .disconnect()
                .map_err(|_| Errno::EINVAL)?;
            server.export_mut().revoke();
            server.wake().wake();
            // Flush bounded terminal replies, especially the final Refused
            // acknowledgment. A departed reader cannot hold supervision.
            for _ in 0..4 {
                if !matches!(server.turn(Some(Duration::ZERO)), Ok(true)) {
                    break;
                }
            }
        }
        self.negotiation_deadline = None;
        if let Some(peer) = self.endpoint.active_peer() {
            self.endpoint.release_peer(peer)?;
        }
        Ok(abandoned)
    }
}

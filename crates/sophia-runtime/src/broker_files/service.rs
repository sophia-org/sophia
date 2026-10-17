use super::*;
use crate::{PolicyRole, PolicyRoleEndpoint, ProtectionDomainEvidence};
use sophia_9p::{connection::ConnectionId, export::PeerCredentials, unix::Server};
use std::{io, path::Path};

pub const SOPHIA_BROKER_9P_SOCKET_ENV: &str = "SOPHIA_BROKER_9P_SOCKET";
/// Bind before launching the protected child; its supervisor evidence is supplied
/// to accept. No supervised-PID-only admission or protocol fallback is exposed.
pub struct BrokerFileListener {
    endpoint: PolicyRoleEndpoint,
    uid: u32,
}
impl BrokerFileListener {
    pub fn bind(directory: impl AsRef<Path>, uid: u32) -> io::Result<Self> {
        Ok(Self {
            endpoint: PolicyRoleEndpoint::bind_role_for_supervised_uid(
                directory,
                PolicyRole::Broker,
                uid,
            )
            .map_err(io::Error::other)?,
            uid,
        })
    }
    pub fn socket_path(&self) -> &Path {
        self.endpoint.socket_path()
    }
    pub fn accept<A: BrokerFileEffects>(
        mut self,
        owner: A,
        evidence: &ProtectionDomainEvidence,
        epoch: u64,
        base: u64,
        last_request: u64,
        limits: BrokerFileLimits,
        timeout: Duration,
    ) -> io::Result<BrokerFileService<A>> {
        self.endpoint
            .authorize_protected_peer(evidence)
            .map_err(io::Error::other)?;
        let stream = self
            .endpoint
            .accept_expected_timeout(timeout)
            .map_err(io::Error::other)?;
        let credentials = rustix::net::sockopt::socket_peercred(&stream)?;
        let peer = PeerCredentials {
            pid: credentials.pid.as_raw_pid(),
            uid: credentials.uid.as_raw(),
            gid: credentials.gid.as_raw(),
        };
        let export = BrokerFileExport::new(
            owner,
            ConnectionId(0),
            peer,
            evidence,
            self.uid,
            epoch,
            base,
            last_request,
            limits,
        )
        .map_err(|e| io::Error::other(format!("broker export: {e:?}")))?;
        let mut server = Server::new(export, sophia_9p::Limits::default())?;
        let connection = server.adopt(stream).map_err(|r| r.error)?;
        server
            .export_mut()
            .bind_connection(connection)
            .map_err(|e| io::Error::other(format!("broker connection: {e:?}")))?;
        Ok(BrokerFileService {
            server,
            _endpoint: self.endpoint,
        })
    }
}
/// One admitted connection. The endpoint remains owned for cleanup; the server
/// never listens on it or accepts a replacement behind the supervisor's back.
pub struct BrokerFileService<A: BrokerFileEffects> {
    server: Server<BrokerFileExport<A>>,
    _endpoint: PolicyRoleEndpoint,
}
impl<A: BrokerFileEffects> BrokerFileService<A> {
    pub fn export(&self) -> &BrokerFileExport<A> {
        self.server.export()
    }
    pub fn export_mut(&mut self) -> &mut BrokerFileExport<A> {
        self.server.export_mut()
    }
    /// Close the socket now, retaining the owner's final state for inspection.
    pub fn close(&mut self) -> io::Result<()> {
        self.server.wake().stop();
        self.server.turn(Some(Duration::ZERO))?;
        Ok(())
    }
    /// Caller-driven bounded service, including idle deadline enforcement.
    pub fn turn(&mut self, maximum: Duration) -> io::Result<bool> {
        let now = Instant::now();
        self.server.export_mut().expire(now);
        if self.server.export().custody().closed() {
            self.close()?;
            return Ok(false);
        }
        let wait = self.server.export().wait(now, maximum);
        let running = self.server.turn(Some(wait))?;
        self.server.export_mut().expire(Instant::now());
        let live = running
            && self.server.connection_count() != 0
            && !self.server.export().custody().closed();
        if !live {
            self.close()?;
        }
        Ok(live)
    }
}

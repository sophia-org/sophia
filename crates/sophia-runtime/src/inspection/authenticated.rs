//! One already accepted observer transport. Session owns endpoint selection,
//! the accept filter, launch authentication and grant revocation.
use super::export::InspectionExport;
use super::publication::Shared;
use super::*;
use crate::authenticated_files::{AuthenticatedFiles, FileAuthenticationFactory};
use sophia_9p::records::Limits;
use sophia_9p::unix::Server;
use std::time::{Duration, Instant};

/// Quiet connections must be serviced within this bound so source fences and
/// Session's launch/namespace revocation do not depend on client traffic.
pub const INSPECTION_IDLE_CHECK: Duration = Duration::from_millis(20);
const MAX_AUTHENTICATED_CONNECTIONS: usize = 4;

pub struct InspectionConnection {
    server: Server<AuthenticatedFiles<InspectionExport>>,
    factory: Box<dyn FileAuthenticationFactory>,
    _permit: Permit,
    closed: bool,
}
struct Permit(Arc<Shared>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0
            .authenticated_connections
            .fetch_sub(1, Ordering::SeqCst);
    }
}

/// A captured service generation, suitable for a namespace dispatch worker.
/// Cloning it shares the owner's existing limits and revocation fences.
#[derive(Clone)]
pub struct InspectionAcceptor {
    shared: Arc<Shared>,
    generation: u64,
}
impl InspectionAcceptor {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn is_live(&self) -> bool {
        self.shared.valid(self.generation)
    }
    pub fn adopt(
        &self,
        stream: UnixStream,
        factory: Box<dyn FileAuthenticationFactory>,
    ) -> io::Result<InspectionConnection> {
        adopt(self.shared.clone(), self.generation, stream, factory)
    }
}
impl InspectionService {
    pub fn authenticated_acceptor(&self, generation: u64) -> io::Result<InspectionAcceptor> {
        if !self.shared.valid(generation) {
            return Err(io::Error::other("inspection generation retired"));
        }
        Ok(InspectionAcceptor {
            shared: self.shared.clone(),
            generation,
        })
    }
    /// Adopt only after the caller's launch-specific accept filter succeeds.
    /// A factory is mandatory: this route cannot fall back to HostDomain or
    /// unauthenticated attach. The caller retains its namespace view lease.
    pub fn adopt_authenticated(
        &self,
        stream: UnixStream,
        generation: u64,
        factory: Box<dyn FileAuthenticationFactory>,
    ) -> io::Result<InspectionConnection> {
        adopt(self.shared.clone(), generation, stream, factory)
    }
}
fn adopt(
    shared: Arc<Shared>,
    generation: u64,
    stream: UnixStream,
    mut factory: Box<dyn FileAuthenticationFactory>,
) -> io::Result<InspectionConnection> {
    shared
        .authenticated_connections
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
            (count < MAX_AUTHENTICATED_CONNECTIONS).then_some(count + 1)
        })
        .map_err(|_| io::Error::other("inspection connection capacity"))?;
    let permit = Permit(shared.clone());
    let files = InspectionExport::authenticated(shared, generation).map_err(io::Error::other)?;
    let limits = Limits::new(64 * 1024, 512, 8, 16, 128 * 1024, 1)
        .map_err(|_| io::Error::other("inspection limits"))?;
    let mut server = Server::new(AuthenticatedFiles::pending(files), limits)?;
    let connection = server.adopt(stream).map_err(|refused| refused.error)?;
    server.export_mut().files_mut().bind_connection(connection);
    let owner = factory
        .create(connection, Instant::now(), server.wake())
        .map_err(|_| io::Error::other("inspection authentication unavailable"))?;
    server
        .export_mut()
        .install(owner)
        .map_err(|_| io::Error::other("inspection authentication installation"))?;
    Ok(InspectionConnection {
        server,
        factory,
        _permit: permit,
        closed: false,
    })
}
impl InspectionConnection {
    /// Borrow the common driver's readiness set for a combined owner wait.
    pub fn poll_fds(&self) -> Vec<rustix::event::PollFd<'_>> {
        self.server.poll_fds()
    }
    /// A bounded worker calls this on readiness and at least every idle bound.
    /// Waiting reads are retried only through the normal 9P core and its checks.
    pub fn poll(&mut self) -> io::Result<bool> {
        if self.closed {
            return Ok(false);
        }
        if !self.server.export().files().authorized()
            || self.server.export_mut().poll(Instant::now()).is_err()
        {
            self.close();
            return Ok(false);
        }
        self.server.wake().wake();
        let live = self.server.turn(Some(Duration::ZERO));
        if !matches!(live, Ok(true)) || self.server.connection_count() == 0 {
            self.close();
            return live.map(|_| false);
        }
        Ok(true)
    }
    pub fn admitted(&self) -> bool {
        !self.closed && self.server.export().admitted()
    }
    fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.server.wake().stop();
        let _ = self.server.turn(Some(Duration::ZERO));
        if let Some(owner) = self.server.export_mut().retire(Instant::now()) {
            // The Session cleanup loop retains quota until retirement is
            // acknowledged. A failed handoff drops the owner fail-closed.
            let _ = self.factory.retire(owner);
        }
    }
}
impl Drop for InspectionConnection {
    fn drop(&mut self) {
        self.close();
    }
}

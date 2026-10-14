//! One adopted namespace-client transport, using the shared authentication owner.
use super::*;
use sophia_9p::{Limits, unix::Server};
use sophia_runtime::authenticated_files::{AuthenticatedFiles, FileAuthenticationFactory};
use std::{
    io,
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

pub struct PortalClientConnection {
    server: Server<AuthenticatedFiles<PortalClientFiles>>,
    factory: Box<dyn FileAuthenticationFactory>,
    closed: bool,
}
impl PortalClientConnection {
    /// Borrow the common driver's readiness set; this conveys no authority.
    pub fn poll_fds(&self) -> Vec<rustix::event::PollFd<'_>> {
        self.server.poll_fds()
    }
    pub fn adopt(
        stream: UnixStream,
        namespace: NamespaceId,
        objects: Arc<PortalObjectRecipient>,
        epoch: u64,
        mut factory: Box<dyn FileAuthenticationFactory>,
    ) -> io::Result<Self> {
        let files = PortalClientFiles::new(namespace, objects, epoch).map_err(refused)?;
        let limits = Limits::new(64 * 1024, 512, 8, 16, 128 * 1024, 1)
            .map_err(|_| io::Error::other("invalid portal client limits"))?;
        let mut server = Server::new(AuthenticatedFiles::pending(files), limits)?;
        let connection = server.adopt(stream).map_err(|refused| refused.error)?;
        server
            .export_mut()
            .files_mut()
            .bind_connection(connection)
            .map_err(refused)?;
        let authentication = factory
            .create(connection, Instant::now(), server.wake())
            .map_err(refused)?;
        server
            .export_mut()
            .install(authentication)
            .map_err(refused)?;
        Ok(Self {
            server,
            factory,
            closed: false,
        })
    }
    /// Dispatch calls this on readiness and within its bounded idle cadence.
    /// Retried watch reads go through the same core check as fresh operations.
    pub fn poll(&mut self) -> io::Result<bool> {
        if self.closed {
            return Ok(false);
        }
        if self.server.export_mut().poll(Instant::now()).is_err() {
            self.close();
            return Ok(false);
        }
        self.server.wake().wake();
        let result = self.server.turn(Some(Duration::ZERO));
        if !matches!(result, Ok(true)) || self.server.connection_count() == 0 {
            self.close();
            return result.map(|_| false);
        }
        Ok(true)
    }
    fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.server.wake().stop();
        let _ = self.server.turn(Some(Duration::ZERO));
        if let Some(owner) = self.server.export_mut().retire(Instant::now()) {
            let _ = self.factory.retire(owner);
        }
    }
}
impl Drop for PortalClientConnection {
    fn drop(&mut self) {
        self.close();
    }
}

fn refused(error: Errno) -> io::Error {
    io::Error::from_raw_os_error(error.0 as i32)
}

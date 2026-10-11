#[cfg(unix)]
#[derive(Debug)]
struct FrontendListener {
    id: crate::XServerFrontendListenerId,
    namespace: NamespaceId,
    socket: UnixListener,
    path: Option<BoundListenerPath>,
}

#[cfg(unix)]
#[derive(Debug)]
struct BoundListenerPath {
    path: std::path::PathBuf,
    device: u64,
    inode: u64,
    armed: bool,
}

#[cfg(unix)]
impl BoundListenerPath {
    fn capture(path: &std::path::Path) -> Result<Self, X11SetupSocketError> {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        let metadata = std::fs::symlink_metadata(path).map_err(|error| {
            X11SetupSocketError::new(format!("bound X11 socket metadata: {error}"))
        })?;
        if !metadata.file_type().is_socket() {
            return Err(X11SetupSocketError::new(
                "bound X11 socket path was replaced",
            ));
        }
        Ok(Self {
            path: path.to_owned(),
            device: metadata.dev(),
            inode: metadata.ino(),
            armed: true,
        })
    }
}

#[cfg(unix)]
impl Drop for BoundListenerPath {
    fn drop(&mut self) {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        if self.armed
            && std::fs::symlink_metadata(&self.path).is_ok_and(|metadata| {
                metadata.file_type().is_socket()
                    && metadata.dev() == self.device
                    && metadata.ino() == self.inode
            })
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(unix)]
impl FrontendListener {
    fn new(
        id: crate::XServerFrontendListenerId,
        namespace: NamespaceId,
        path: &std::path::Path,
        socket: UnixListener,
    ) -> Result<Self, X11SetupSocketError> {
        let path = Some(BoundListenerPath::capture(path)?);
        socket
            .set_nonblocking(true)
            .map_err(|error| X11SetupSocketError::new(error.to_string()))?;
        Ok(Self {
            id,
            namespace,
            socket,
            path,
        })
    }
}

#[cfg(unix)]
impl XServerFrontend {
    fn validate_listeners(config: &XServerFrontendConfig) -> Result<(), X11SetupSocketError> {
        if !config.additional_listeners.is_empty() && config.admission_policy().is_none() {
            return Err(X11SetupSocketError::new(
                "multiple X11 listeners require an admission policy",
            ));
        }
        if config.additional_listeners.len() + 1 > config.max_concurrent_clients().get() {
            return Err(X11SetupSocketError::new(
                "each X11 listener requires a reserved worker slot",
            ));
        }
        Ok(())
    }

    /// Fixed shares prevent incomplete setup on one group from occupying
    /// another group's slots. The primary receives any division remainder.
    fn listener_has_capacity(&self, id: crate::XServerFrontendListenerId) -> bool {
        let limit = self.config.max_concurrent_clients().get();
        let count = self.listeners.len();
        let quota = limit / count
            + if id == crate::XServerFrontendListenerId::PRIMARY {
                limit % count
            } else {
                0
            };
        self.workers.len() < limit
            && self
                .workers
                .values()
                .filter(|worker| worker.listener == id)
                .count()
                < quota
    }

    /// One accepting frame scans every listener. Rotating the first candidate
    /// prevents a busy shared listener from starving a confined group.
    fn accept_client(
        &mut self,
        nonblocking: bool,
        concurrent: bool,
    ) -> Result<
        Option<(UnixStream, crate::XServerFrontendListenerId, NamespaceId)>,
        X11SetupSocketError,
    > {
        loop {
            if concurrent {
                self.reap_finished_client_workers()?;
            }
            for offset in 0..self.listeners.len() {
                let index = (self.next_listener + offset) % self.listeners.len();
                let entry = &self.listeners[index];
                if concurrent && !self.listener_has_capacity(entry.id) {
                    continue;
                }
                match entry.socket.accept() {
                    Ok((stream, _)) => {
                        self.next_listener = (index + 1) % self.listeners.len();
                        return Ok(Some((stream, entry.id, entry.namespace)));
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            ErrorKind::WouldBlock | ErrorKind::Interrupted
                        ) => {}
                    Err(error) => {
                        return Err(X11SetupSocketError::new(format!(
                            "failed to accept X11 client: {error}"
                        )));
                    }
                }
            }
            if nonblocking {
                return Ok(None);
            }
            let mut fds = self
                .listeners
                .iter()
                .filter(|entry| !concurrent || self.listener_has_capacity(entry.id))
                .map(|entry| {
                    rustix::event::PollFd::new(&entry.socket, rustix::event::PollFlags::IN)
                })
                .collect::<Vec<_>>();
            // The blocking compatibility API has no installed service wake.
            // Periodically reap workers so a previously full group can reopen.
            let deadline = (concurrent && !self.workers.is_empty())
                .then(|| std::time::Instant::now() + Duration::from_millis(20));
            sophia_wake::wait(&mut fds, deadline)
                .map_err(|error| X11SetupSocketError::new(format!("X11 listener wait: {error}")))?;
        }
    }
}

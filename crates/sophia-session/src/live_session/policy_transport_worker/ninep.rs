//! Bounded file custody on a stream the Session admission owner already
//! authenticated. Scalar bodies stay behind the typed codec boundary.
use super::adapter::{PolicyAdapterEvent, PolicyAdapterStop};
use super::driver::PolicyReceivePermit;
use sophia_9p::{
    export::*,
    records::*,
    unix::{Server, Wake},
};
use sophia_protocol::wm_files::*;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod journal;
mod owner;
mod staging;
use journal::Journal;
pub(super) use owner::WmFiles;
use staging::Staging;

const EBUSY: Errno = Errno(16);
const EALREADY: Errno = Errno(114);
const ASSEMBLY_DEADLINE: Duration = Duration::from_secs(12);
const SEND_DEADLINE: Duration = Duration::from_secs(4);

/// Supplied by the logical Session WM filesystem owner and continued across
/// supervised reconnects. Socket paths and admitted epochs are not qid hashes.
#[derive(Clone)]
pub(super) struct WmQids(Arc<Mutex<u64>>);
impl WmQids {
    pub(super) fn new() -> Self {
        Self(Arc::new(Mutex::new(1)))
    }
    fn allocate(&self, count: u64) -> Result<u64, Errno> {
        let mut next = self.0.lock().map_err(|_| Errno::EIO)?;
        let end = next.checked_add(count).ok_or(Errno::ENOSPC)?;
        let start = *next;
        *next = end;
        Ok(start)
    }
}

pub(super) struct DecodedFileCandidate {
    pub event: PolicyAdapterEvent,
    /// Derived from validated kind/sections by the codec, never a client
    /// assertion of authority. The file owner checks the selected ceiling.
    pub required_capabilities: u64,
}

pub(super) trait PolicyFileCodec {
    fn decode_candidate(
        &self,
        bytes: &[u8],
        selected_capabilities: u64,
    ) -> Result<DecodedFileCandidate, Errno>;
    fn submitted_body(
        &self,
        submission_id: u64,
        candidate_kind: WmFileKind,
    ) -> Result<Vec<u8>, Errno>;
}

pub(super) struct NinePReactor<C: PolicyFileCodec> {
    server: Server<WmFiles<C>>,
}

struct NinePStop(Wake);
impl PolicyAdapterStop for NinePStop {
    fn stop(&self) {
        self.0.stop();
    }
}

impl<C: PolicyFileCodec> NinePReactor<C> {
    pub(super) fn adopt(stream: UnixStream, owner: WmFiles<C>) -> Result<Self, String> {
        // WM-specific bounds: 512-byte minimum permits fragment transport,
        // sixteen waits leave room for event reads plus flush/control traffic.
        // One connection is already admitted; this reactor is not a listener.
        let mut server = Server::new(
            owner,
            Limits::new(65536, 512, 16, 32, 131072, 1).map_err(|e| format!("9P limits: {e:?}"))?,
        )
        .map_err(|e| e.to_string())?;
        let connection = server.adopt(stream).map_err(|e| e.error.to_string())?;
        server.export_mut().bind_connection(connection);
        Ok(Self { server })
    }
    pub(super) fn stop_handle(&self) -> Box<dyn PolicyAdapterStop> {
        Box::new(NinePStop(self.server.wake()))
    }
    pub(super) fn owner_mut(&mut self) -> &mut WmFiles<C> {
        self.server.export_mut()
    }

    fn turn(&mut self, timeout: Duration) -> Result<(), String> {
        // Staging expiry is another wake deadline, not progress-based renewal.
        let timeout = self.server.export_mut().next_wait(timeout);
        if !self.server.turn(Some(timeout)).map_err(|e| e.to_string())?
            || self.server.connection_count() == 0
        {
            self.server.export_mut().revoke();
            return Err("WM file connection ended".into());
        }
        self.server.export_mut().expire();
        Ok(())
    }

    pub(super) fn receive(
        &mut self,
        permit: PolicyReceivePermit,
        timeout: Duration,
    ) -> Result<Option<PolicyAdapterEvent>, String> {
        self.server
            .export_mut()
            .offer(permit)
            .map_err(|e| format!("WM file receive: {e:?}"))?;
        let deadline = Instant::now() + timeout;
        let result = (|| {
            loop {
                // Even zero-time polling gives buffered socket requests a turn.
                self.turn(deadline.saturating_duration_since(Instant::now()))?;
                if let Some(event) = self.server.export_mut().take_delivery() {
                    return Ok(Some(event));
                }
                if Instant::now() >= deadline {
                    return Ok(None);
                }
            }
        })();
        self.server.export_mut().withdraw_permit();
        result
    }

    /// Caller retains its single in-flight semantic command throughout this
    /// borrowed send. No journal bytes or sequence are spent before capacity.
    pub(super) fn send_event(&mut self, kind: WmFileKind, body: &[u8]) -> Result<(), String> {
        let deadline = Instant::now() + SEND_DEADLINE;
        loop {
            if Instant::now() >= deadline {
                return Err("WM file send deadline expired".into());
            }
            match self.server.export_mut().append_event(kind, body) {
                Ok(_) => {
                    self.server.wake().wake();
                    return Ok(());
                }
                Err(Errno::EAGAIN) => {}
                Err(error) => return Err(format!("WM file send: {error:?}")),
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("WM file send deadline expired".into());
            }
            // One reactor thread owns all export mutation; no lock spans this wait.
            self.turn(remaining)?;
        }
    }
}

#[path = "../../../tests/support/policy_file_custody.rs"]
mod custody_tests;

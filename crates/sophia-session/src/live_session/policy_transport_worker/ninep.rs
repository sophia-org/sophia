//! Bounded file custody on a stream the Session admission owner already
//! authenticated. Scalar bodies stay behind the typed codec boundary.
use super::adapter::{PolicyAdapterCommandWake, PolicyAdapterEvent, PolicyAdapterStop};
use super::driver::PolicyReceivePermit;
use sophia_9p::{
    export::*,
    records::*,
    unix::{Server, Wake},
};
use sophia_protocol::wm_files::*;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod journal;
mod owner;
mod pending;
mod runtime_adapter;
mod staging;
mod startup;
mod typed_codec;
use journal::Journal;
pub(super) use owner::WmFiles;
use staging::Staging;

#[path = "../../../tests/support/policy_selection_peer.rs"]
pub(in crate::live_session) mod selection_peer;

impl super::PolicyTransportWorker {
    pub(in crate::live_session) fn new_files(
        endpoint: sophia_runtime::PolicyRoleEndpoint,
        supervisor: &sophia_runtime::ProcessSupervisor,
        epoch: u64,
        limits: WmFileLimits,
        qids: super::PolicyFilesystemQids,
        identity: Option<sophia_protocol::PolicyProfileIdentity>,
    ) -> Result<Self, String> {
        let adapter = runtime_adapter::NinePPolicyAdapter::pending(
            endpoint, supervisor, epoch, limits, qids.0,
        )?;
        let profile = identity.map(|identity| super::adapter::PolicyProfileAdmission {
            connection_epoch: identity.connection_epoch,
            generation: identity.profile_generation,
            digest: identity.profile_digest,
            prepare_transaction: sophia_protocol::TransactionId::from_raw(1),
            activate_transaction: sophia_protocol::TransactionId::from_raw(2),
        });
        Self::spawn(adapter, epoch, profile).map_err(|e| e.to_string())
    }
}

const EBUSY: Errno = Errno(16);
const EALREADY: Errno = Errno(114);
const ASSEMBLY_DEADLINE: Duration = Duration::from_millis(WM_FILE_ASSEMBLY_TIMEOUT_MILLIS as u64);
const SEND_DEADLINE: Duration = Duration::from_millis(WM_FILE_SEND_TIMEOUT_MILLIS as u64);

fn check_publication(stopped: &AtomicBool, deadline: Instant) -> Result<(), Errno> {
    if stopped.load(Ordering::SeqCst) {
        return Err(Errno(125));
    }
    if Instant::now() >= deadline {
        return Err(Errno(110));
    }
    Ok(())
}

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
    stopped: Arc<AtomicBool>,
    cancellation: Arc<NinePCancellation>,
}

/// One cancellation owner spans endpoint accept and reactor adoption. The lock
/// protects only wake registration, never an accept, reactor turn or wait.
struct NinePCancellation {
    stopped: Arc<AtomicBool>,
    wake: Mutex<Option<Wake>>,
}
impl NinePCancellation {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            stopped: Arc::new(AtomicBool::new(false)),
            wake: Mutex::new(None),
        })
    }
    fn install(&self, wake: Wake) -> Result<(), String> {
        *self.wake.lock().map_err(|_| "WM file wake lock poisoned")? = Some(wake.clone());
        // A Stop before or during registration cannot disappear at adoption.
        if self.stopped.load(Ordering::SeqCst) {
            wake.stop();
            return Err("WM file stopped during adoption".into());
        }
        Ok(())
    }
    fn handle(self: &Arc<Self>) -> Box<dyn PolicyAdapterStop> {
        Box::new(NinePStop(self.clone()))
    }
    fn command_handle(self: &Arc<Self>) -> Box<dyn PolicyAdapterCommandWake> {
        Box::new(NinePCommandWake(self.clone()))
    }
}
struct NinePCommandWake(Arc<NinePCancellation>);
impl PolicyAdapterCommandWake for NinePCommandWake {
    fn wake(&self) {
        // Registration lock only; no poll or I/O while held. Before adoption
        // the channel retains the command for the driver's first try_recv.
        let wake = self.0.wake.lock().ok().and_then(|wake| wake.clone());
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}
struct NinePStop(Arc<NinePCancellation>);
impl PolicyAdapterStop for NinePStop {
    fn stop(&self) {
        self.0.stopped.store(true, Ordering::SeqCst);
        let wake = self.0.wake.lock().ok().and_then(|wake| wake.clone());
        if let Some(wake) = wake {
            wake.stop();
        }
    }
}

impl<C: PolicyFileCodec> NinePReactor<C> {
    pub(super) fn adopt(stream: UnixStream, owner: WmFiles<C>) -> Result<Self, String> {
        Self::adopt_with_cancellation(stream, owner, NinePCancellation::new())
    }
    fn adopt_with_cancellation(
        stream: UnixStream,
        owner: WmFiles<C>,
        cancellation: Arc<NinePCancellation>,
    ) -> Result<Self, String> {
        if cancellation.stopped.load(Ordering::SeqCst) {
            return Err("WM file stopped before adoption".into());
        }
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
        cancellation.install(server.wake())?;
        Ok(Self {
            server,
            stopped: cancellation.stopped.clone(),
            cancellation,
        })
    }
    pub(super) fn stop_handle(&self) -> Box<dyn PolicyAdapterStop> {
        self.cancellation.handle()
    }
    pub(super) fn owner_mut(&mut self) -> &mut WmFiles<C> {
        self.server.export_mut()
    }

    fn turn(&mut self, timeout: Duration) -> Result<(), String> {
        if self.stopped.load(Ordering::SeqCst) {
            self.server.export_mut().revoke();
            return Err("WM file stopped".into());
        }
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

    fn idle_receive(
        &mut self,
        permit: PolicyReceivePermit,
        cap: Duration,
    ) -> Result<Option<PolicyAdapterEvent>, String> {
        self.server
            .export_mut()
            .offer(permit)
            .map_err(|e| format!("WM file receive: {e:?}"))?;
        // Unlike active receive, any readiness returns control to the driver.
        // The shared pipe may mean a command, journal append or Stop: never
        // interpret it as a semantic event or renew an active response budget.
        let result = self
            .turn(cap)
            .map(|()| self.server.export_mut().take_delivery());
        self.server.export_mut().withdraw_permit();
        result
    }

    /// Caller retains its single in-flight semantic command throughout this
    /// borrowed send. No journal bytes or sequence are spent before capacity.
    pub(super) fn send_event(&mut self, kind: WmFileKind, body: &[u8]) -> Result<(), String> {
        self.send_encoded(kind, |header| {
            encode_wm_file_record(header, body).map_err(|_| Errno::EINVAL)
        })
    }

    fn send_encoded(
        &mut self,
        kind: WmFileKind,
        encode: impl Fn(WmFileHeader) -> Result<Vec<u8>, Errno>,
    ) -> Result<(), String> {
        self.send_encoded_before(kind, Instant::now() + SEND_DEADLINE, encode)
    }

    fn send_encoded_before(
        &mut self,
        kind: WmFileKind,
        deadline: Instant,
        encode: impl Fn(WmFileHeader) -> Result<Vec<u8>, Errno>,
    ) -> Result<(), String> {
        loop {
            let stopped = &self.stopped;
            match self
                .server
                .export_mut()
                .append_encoded_event_checked(kind, &encode, stopped, deadline)
            {
                Ok(_) => {
                    self.server.wake().wake();
                    return Ok(());
                }
                Err(Errno::EAGAIN) => {}
                Err(Errno(110)) => return Err("WM file send deadline expired".into()),
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

    fn send_cycle(
        &mut self,
        snapshot: &WmFileSnapshot,
        cycle: &WmFileCycle,
        deadline: Instant,
    ) -> Result<(), String> {
        loop {
            match self
                .server
                .export_mut()
                .publish_cycle(snapshot, cycle, &self.stopped, deadline)
            {
                Ok(_) => {
                    self.server.wake().wake();
                    return Ok(());
                }
                Err(Errno::EAGAIN) => {}
                Err(error) => return Err(format!("WM file cycle: {error:?}")),
            }
            self.turn(deadline.saturating_duration_since(Instant::now()))?;
        }
    }
}

#[path = "../../../tests/support/policy_file_custody.rs"]
mod custody_tests;

#[path = "../../../tests/support/policy_file_replay.rs"]
mod replay_tests;

//! Independent wire expectations; no X-authority encoder or private fixture.
mod wire;
pub use wire::{Order, Peer};

use sophia_input_authority::{InstanceId, SeatBinding};
use sophia_protocol::{
    ClientAdmissionContext, DeviceId, NamespaceId, OutputId, OutputTopologyEntry,
    OutputTopologySnapshot, Rect, SeatId, Size,
};
use sophia_session::private_input::{
    PrivateInputConfig, PrivateInputGrantPolicy, PrivateInputHandle, PrivateInputInstanceCookie,
    PrivateInputOutcome, PrivateInputReadiness, PrivateInputRefusal, PrivateInputService,
    PrivateInputThreadJoin,
};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub const WAIT: Duration = Duration::from_secs(8);
pub mod containment;
pub const COOKIE: [u8; 32] = [0x73; 32];
static NEXT: AtomicU64 = AtomicU64::new(1);

pub fn device(value: u64) -> DeviceId {
    DeviceId::from_raw(value)
}

pub fn config(socket: &Path, grants: PrivateInputGrantPolicy) -> PrivateInputConfig {
    let instance = InstanceId::new(731);
    let output = OutputId::from_raw(1);
    PrivateInputConfig {
        session_generation: 1,
        profile: sophia_protocol::NamespaceProfile::Confined,
        capabilities: sophia_protocol::NamespaceCapabilities::NONE,
        frame_clock: sophia_engine::DeterministicFrameClock::new(1, 16),
        socket_path: socket.to_owned(),
        namespace: NamespaceId::from_raw(731),
        binding: SeatBinding::new(instance, SeatId::from_raw(1)),
        cookie: PrivateInputInstanceCookie {
            instance,
            cookie: COOKIE,
        },
        grants,
        max_concurrent_clients: NonZeroUsize::new(4).unwrap(),
        input_capacity: NonZeroUsize::new(8).unwrap(),
        advertised_buttons: 9,
        output_topology: OutputTopologySnapshot {
            generation: 1,
            primary: output,
            outputs: vec![OutputTopologyEntry {
                output,
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: 320,
                    height: 240,
                },
                pixel_size: Size {
                    width: 320,
                    height: 240,
                },
                scale: 1,
                refresh_millihz: 60_000,
                timing: None,
            }],
        },
    }
}

pub struct Instance {
    handle: Option<PrivateInputHandle>,
    directory: PathBuf,
    socket: PathBuf,
}

impl Instance {
    pub fn start(grants: PrivateInputGrantPolicy) -> Self {
        Self::configured(grants, false).unwrap()
    }

    pub fn start_foreign_evidence() -> Result<Self, PrivateInputRefusal> {
        Self::configured(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence, true)
    }

    fn configured(
        grants: PrivateInputGrantPolicy,
        foreign: bool,
    ) -> Result<Self, PrivateInputRefusal> {
        let directory = std::env::temp_dir().join(format!(
            "m4-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let socket = directory.join("private.sock");
        let mut config = config(&socket, grants);
        if foreign {
            config.cookie.instance = InstanceId::new(999);
        }
        let started = PrivateInputService::start(config);
        let handle = match started {
            Ok(handle) => handle,
            Err(error) => {
                std::fs::remove_dir(&directory).unwrap();
                return Err(error);
            }
        };
        assert_eq!(
            handle.await_ready(WAIT).unwrap(),
            PrivateInputReadiness::Ready
        );
        Ok(Self {
            handle: Some(handle),
            directory,
            socket,
        })
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }
    pub fn handle(&self) -> &PrivateInputHandle {
        self.handle.as_ref().unwrap()
    }

    /// The controller, exclusively. `apply_committed` takes `&mut self` so two
    /// callers cannot submit the same committed command twice.
    pub fn handle_mut(&mut self) -> &mut PrivateInputHandle {
        self.handle.as_mut().unwrap()
    }

    pub fn connect(
        &self,
        order: Order,
        cookie: Option<[u8; 32]>,
    ) -> (Peer, ClientAdmissionContext) {
        let before = self
            .handle()
            .admitted()
            .unwrap()
            .into_iter()
            .map(|entry| entry.admission)
            .collect::<Vec<_>>();
        let peer = Peer::connect(self.socket(), order, cookie).unwrap();
        let deadline = Instant::now() + WAIT;
        loop {
            let added = self
                .handle()
                .admitted()
                .unwrap()
                .into_iter()
                .filter(|entry| {
                    !entry.closed && entry.lifecycle_open && !before.contains(&entry.admission)
                })
                .collect::<Vec<_>>();
            if let [entry] = &added[..] {
                let original = self
                    .handle()
                    .admission_record(entry.admission)
                    .unwrap()
                    .unwrap();
                return (peer, original.context);
            }
            assert!(
                Instant::now() < deadline,
                "exact admission did not become observable"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn finish(mut self) -> PrivateInputOutcome {
        self.handle.take().unwrap().stop()
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        drop(self.handle.take());
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[derive(Default)]
pub struct Evidence {
    started: usize,
    collected: usize,
    invocations: usize,
}

impl Evidence {
    pub fn collect(&mut self, outcome: PrivateInputOutcome, failed_setup: bool) {
        assert_eq!(outcome.service_thread, PrivateInputThreadJoin::Joined);
        if !failed_setup {
            assert!(outcome.failure.is_none(), "{outcome:?}");
        }
        let workers = outcome
            .workers
            .as_ref()
            .expect("ordinary invocation returns collection evidence");
        assert!(workers.iter().all(|worker| worker.joined), "{outcome:?}");
        assert!(!outcome.interrupted, "{outcome:?}");
        self.started += 1 + workers.len();
        self.collected += 1 + workers.iter().filter(|worker| worker.joined).count();
        self.invocations += 1;
    }

    pub fn emit(self, case: &str, subcases: &[&str]) {
        let subcases = subcases
            .iter()
            .map(|case| format!("\"{case}\":\"PASS\""))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(self.started, self.collected);
        assert!(self.invocations > 0);
        println!(
            "sophia_m4_acceptance {{\"schema\":1,\"case\":\"M4.{case}\",\"subcases\":{{{subcases}}},\"cleanup\":{{\"actors_started\":{},\"actors_collected\":{},\"pending_actors\":0,\"complete\":true}},\"observations\":{{\"real_session_invocations\":{},\"actor_scope\":\"service_threads_and_registered_ordered_workers\"}}}}",
            self.started, self.collected, self.invocations
        );
    }
}

//! Support shared by every group, which lands one group at a time.
#![allow(dead_code)]

//! Evidence for the M5 gate. Each group test starts real private Session
//! instances, accounts for every actor they started, and prints one record
//! the gate parses; the record's shape is what `cargo xtask check
//! m5-acceptance` requires, so it is pinned here and in the gate's own tests.

pub mod client;
pub mod groups;

use sophia_input_authority::{InstanceId, SeatBinding};
use sophia_protocol::{
    NamespaceCapabilities, NamespaceId, NamespaceProfile, OutputId, OutputTopologyEntry,
    OutputTopologySnapshot, Rect, SeatId, Size,
};
use sophia_session::private_input::{
    PrivateInputConfig, PrivateInputGrantPolicy, PrivateInputInstanceCookie,
    PrivateInputLifetimeOwner, PrivateInputOutcome, PrivateInputReadiness, PrivateInputService,
    PrivateInputThreadJoin,
};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::Duration;

// Re-exported for the groups still to land, which read errors and events
// directly rather than through the reply helpers.
#[allow(unused_imports)]
pub use client::{Answer, Client, Order, XError};

pub const WAIT: Duration = Duration::from_secs(8);
/// The credential an admitted connection presents. One value for the whole
/// target: a group proving a refusal supplies its own wrong one.
pub const COOKIE: [u8; 32] = [0x5b; 32];
/// The opcode XTEST is assigned. Used only where a group must guess it,
/// because a connection refused the extension is never told it.
pub const XTEST_MAJOR: u8 = 146;
/// The screen every group's instance advertises.
pub const SCREEN: (i32, i32) = (320, 240);
const INSTANCE: u64 = 733;

/// One private Session service, its socket, and its ending.
pub struct Instance {
    lifetime: PrivateInputLifetimeOwner,
    /// Behind a lock because `apply_committed` takes the handle exclusively
    /// and a group drives it while its clients are connected.
    handle: std::sync::Mutex<Option<sophia_session::private_input::PrivateInputHandle>>,
    directory: PathBuf,
    socket: PathBuf,
}

impl Instance {
    /// Start a real service and wait for it to be ready.
    ///
    /// The directory is named for the group rather than the process, so a
    /// failed run leaves something that says which group left it.
    pub fn start(group: &str, grants: PrivateInputGrantPolicy) -> Self {
        let directory =
            std::env::temp_dir().join(format!("sophia-m5-{group}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a private directory");
        let socket = directory.join("private.sock");
        let lifetime = PrivateInputLifetimeOwner::reserved();
        let handle = PrivateInputService::start(&lifetime, config(&socket, grants))
            .expect("the private service starts");
        assert_eq!(
            handle.await_ready(WAIT).expect("readiness"),
            PrivateInputReadiness::Ready
        );
        Self {
            lifetime,
            handle: std::sync::Mutex::new(Some(handle)),
            directory,
            socket,
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// The service's own view: admissions, grants, delivery receipts, and
    /// the commit bridge a group has to drive itself. A group reads it beside
    /// the wire so that what a client saw and what the service accounted for
    /// are both in the evidence.
    pub fn with_handle<T>(
        &self,
        read: impl FnOnce(&mut sophia_session::private_input::PrivateInputHandle) -> T,
    ) -> T {
        let mut held = self.handle.lock().expect("the handle is not poisoned");
        read(held.as_mut().expect("a live service"))
    }

    /// Connect a client. A cookie makes it an admitted one; none makes it an
    /// ordinary connection that may not inject.
    pub fn connect(&self, order: Order, cookie: Option<[u8; 32]>) -> Client {
        Client::connect(&self.socket, order, cookie).expect("a connected client")
    }

    /// Stop the service and hand back what it collected.
    pub fn finish(self) -> PrivateInputOutcome {
        let handle = self
            .handle
            .lock()
            .expect("the handle is not poisoned")
            .take()
            .expect("a live service");
        let outcome = handle.stop();
        let _ = std::fs::remove_dir_all(&self.directory);
        let _ = &self.lifetime;
        outcome
    }
}

pub fn config(socket: &Path, grants: PrivateInputGrantPolicy) -> PrivateInputConfig {
    let instance = InstanceId::new(INSTANCE);
    let output = OutputId::from_raw(1);
    let size = Size {
        width: SCREEN.0,
        height: SCREEN.1,
    };
    PrivateInputConfig {
        socket_path: socket.to_owned(),
        namespace: NamespaceId::from_raw(INSTANCE),
        session_generation: 1,
        profile: NamespaceProfile::Confined,
        capabilities: NamespaceCapabilities::NONE,
        frame_clock: sophia_engine::DeterministicFrameClock::new(1, 16),
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
                    width: size.width,
                    height: size.height,
                },
                pixel_size: size,
                scale: 1,
                refresh_millihz: 60_000,
                timing: None,
            }],
        },
    }
}

#[derive(Default)]
pub struct Evidence {
    started: usize,
    collected: usize,
    invocations: usize,
}

impl Evidence {
    /// Account for one finished Session invocation: its service thread and
    /// every registered worker must have been joined. Used by each group test
    /// as it lands; until the first does, nothing in this target calls it.
    #[allow(dead_code)]
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

    /// Print the group's record. Every named subcase is reported passed: a
    /// subcase that did not hold has already failed the test by assertion.
    pub fn emit(self, group: &str, subcases: &[&str]) {
        assert_eq!(self.started, self.collected);
        assert!(self.invocations > 0);
        println!(
            "{}",
            record(
                group,
                subcases,
                self.started,
                self.collected,
                self.invocations
            )
        );
    }
}

/// The record line, as the gate reads it.
pub fn record(
    group: &str,
    subcases: &[&str],
    started: usize,
    collected: usize,
    invocations: usize,
) -> String {
    let subcases = subcases
        .iter()
        .map(|case| format!("\"{case}\":\"PASS\""))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "sophia_m5_acceptance {{\"schema\":1,\"case\":\"M5.{group}\",\"subcases\":{{{subcases}}},\"cleanup\":{{\"actors_started\":{started},\"actors_collected\":{collected},\"pending_actors\":0,\"complete\":true}},\"observations\":{{\"real_session_invocations\":{invocations},\"actor_scope\":\"service_threads_and_registered_ordered_workers\"}}}}"
    )
}

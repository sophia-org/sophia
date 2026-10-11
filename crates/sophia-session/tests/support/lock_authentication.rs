//! Join Session's final admission with the shipped lock file owner. Factotum
//! and protected relay are real; role transport dispatch is called directly.
use super::*;
use sophia_9p::{Errno, OpenFlags, ReadOutcome, export::*};
use sophia_protocol::lock_files::*;
use sophia_runtime::lock_files::*;
use sophia_session::launch_authentication::role::{RoleAuthentication, RoleScope};

const CONNECTION: ConnectionId = ConnectionId(12);
const EPOCH: u64 = 5;

fn context() -> AttachContext<'static> {
    AttachContext {
        connection: CONNECTION,
        peer: None,
        uname: b"lock",
        aname: b"lock-provider",
        n_uname: u32::MAX,
    }
}
fn access(node: &LockFileNode, operation: Operation) -> Access<'_, LockFileNode> {
    Access {
        connection: CONNECTION,
        epoch: Epoch(EPOCH),
        node,
        operation,
    }
}
fn view() -> LockFileExport {
    let mut view = LockFileExport::new(
        LockFileSettings {
            epoch: EPOCH,
            limits: LockFileLimits {
                max_outputs: 1,
                upload_slots: 2,
                max_chords: 0,
                max_width_px: 8,
                max_height_px: 8,
                max_resource_bytes: 256,
                max_live_resources: 4,
                journal_records: 32,
                journal_bytes: 8192,
                assembly_timeout_ms: 1000,
                ack_progress_timeout_ms: 2000,
            },
            reserved_chords: Vec::new(),
            lock: LockObject {
                lock_epoch: 3,
                topology_generation: 1,
                phase: LockPhase::Locked,
                allocations: Vec::new(),
            },
            lock_qid: 0,
        },
        LockFileQids::default(),
    )
    .unwrap();
    view.bind_connection(CONNECTION).unwrap();
    view
}

struct Joined {
    f: Fixture,
    id: LaunchId,
    probe: Probe,
    supervisor: ProcessSupervisor,
    auth: RoleAuthentication,
    attempt: Attempt,
    view: LockFileExport,
}
impl Joined {
    fn new() -> Self {
        let mut f = Fixture::new();
        let id = f
            .book
            .reserve(
                LaunchPolicy {
                    principal: 8,
                    namespace: 9,
                    audience: 10,
                    role: ProtectionDomainRole::LockProvider,
                },
                Instant::now(),
                deadline(),
            )
            .unwrap();
        let end = deadline();
        while f.book.phase(id) != Some(Phase::Ready) {
            f.tick(end);
        }
        let mut auth = RoleAuthentication::new(
            RoleScope {
                server: 11,
                connection: CONNECTION,
                session: 3,
                audience: 10,
                uname: b"lock".to_vec(),
                aname: b"lock-provider".to_vec(),
            },
            f.ingress.clone(),
            Instant::now(),
        )
        .unwrap();
        let attempt = auth.begin(7, Instant::now()).unwrap();
        Self {
            f,
            id,
            probe: Probe::build(),
            supervisor: ProcessSupervisor::new(
                SupervisedProcessKind::LockProvider,
                ProcessLaunchSpec::new("/usr/bin/true"),
            ),
            auth,
            attempt,
            view: view(),
        }
    }
    fn verify(&mut self) {
        let end = deadline();
        let nonce = loop {
            self.f.tick(end);
            match self
                .auth
                .read(
                    self.attempt,
                    0,
                    40,
                    Instant::now(),
                    &mut self.f.book,
                    &mut self.supervisor,
                )
                .unwrap()
            {
                ReadOutcome::Pending => {}
                ReadOutcome::Ready(bytes) => {
                    break sophia_factotum::launch::relay::decode_challenge(&bytes).unwrap();
                }
            }
        };
        let hex = nonce
            .0
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        self.supervisor = ProcessSupervisor::new(
            SupervisedProcessKind::LockProvider,
            ProcessLaunchSpec::new(&self.probe.0)
                .arg(hex)
                .protection_domain(
                    ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::LockProvider]).unwrap(),
                ),
        );
        let channel = self.f.book.take_channel(self.id, Instant::now()).unwrap();
        self.supervisor.start_with_launch_channel(channel).unwrap();
        self.f.activate(self.id, &mut self.supervisor);
        loop {
            self.f.tick(end);
            match self
                .auth
                .read(
                    self.attempt,
                    40,
                    8,
                    Instant::now(),
                    &mut self.f.book,
                    &mut self.supervisor,
                )
                .unwrap()
            {
                ReadOutcome::Pending => {}
                ReadOutcome::Ready(bytes) => {
                    assert_eq!(bytes, [1, 0, 0, 0, 0, 0, 0, 0]);
                    break;
                }
            }
        }
    }
    fn attach(&mut self) -> Result<Attachment<LockFileNode>, Errno> {
        self.auth.attach_lock(
            &mut self.view,
            &context(),
            self.attempt,
            Instant::now(),
            &mut self.f.book,
            &mut self.supervisor,
        )
    }
    fn check(&mut self, node: &LockFileNode, operation: Operation) -> Result<(), Errno> {
        self.auth.check_lock(
            &mut self.view,
            &access(node, operation),
            Instant::now(),
            &mut self.f.book,
            &mut self.supervisor,
        )
    }
}
impl Drop for Joined {
    fn drop(&mut self) {
        let _ = self.supervisor.terminate();
        self.auth.close(Instant::now());
    }
}

#[test]
fn unverified_lock_attach_publishes_neither_root_nor_identity_then_can_retry() {
    let mut j = Joined::new();
    assert!(matches!(j.attach(), Err(Errno::EACCES)));
    assert_eq!(
        j.view.check(&access(&LockFileNode::Root, Operation::Read)),
        Err(Errno::ESTALE),
        "unverified lock attach must not publish its root"
    );
    assert!(
        j.auth.pinned().is_none(),
        "unverified lock attach must not pin identity"
    );
    j.verify();
    let attached = j.attach().unwrap();
    assert_eq!(attached.root, LockFileNode::Root);
    assert_eq!(attached.epoch, Epoch(EPOCH));
    assert_eq!(j.auth.pinned().unwrap().launch, j.id.generation());
    j.check(&LockFileNode::Api, Operation::Read).unwrap();
    let mut api = j.view.open(&LockFileNode::Api, OpenFlags(0)).unwrap();
    assert_eq!(
        j.view.read(&LockFileNode::Api, &mut api, 0, 256).unwrap(),
        ReadOutcome::Ready(b"sophia-lock-files version=1 epoch=5\n".to_vec())
    );
}

#[test]
fn revoked_lock_view_cannot_pin_an_otherwise_verified_identity() {
    let mut j = Joined::new();
    j.verify();
    j.view.revoke();
    assert!(matches!(j.attach(), Err(Errno::ESTALE)));
    assert!(
        j.auth.pinned().is_none(),
        "lock view refusal must precede identity commit"
    );
}

#[test]
fn revoked_launch_cannot_publish_lock_root_after_verified_status() {
    let mut j = Joined::new();
    j.verify();
    j.f.book.revoke(j.id, Instant::now()).unwrap();
    assert!(matches!(j.attach(), Err(Errno::ESTALE)));
    assert_eq!(
        j.view.check(&access(&LockFileNode::Root, Operation::Read)),
        Err(Errno::ESTALE),
        "revoked launch must leave the prepared lock root unpublished"
    );
    assert!(j.auth.pinned().is_none());
}

#[test]
fn retained_lock_object_requires_current_launch_custody() {
    let mut j = Joined::new();
    j.verify();
    j.attach().unwrap();
    j.check(&LockFileNode::Lock, Operation::Read).unwrap();
    let mut object = j.view.open(&LockFileNode::Lock, OpenFlags(0)).unwrap();
    let ReadOutcome::Ready(bytes) = j
        .view
        .read(&LockFileNode::Lock, &mut object, 0, 4096)
        .unwrap()
    else {
        panic!("lock object");
    };
    let record = decode_lock_file_record(&bytes, LockFileClass::Object).unwrap();
    assert_eq!(record.header.kind, LockFileKind::Lock);
    assert_eq!(
        LockObject::decode(record.body).unwrap().phase,
        LockPhase::Locked
    );
    j.f.book.revoke(j.id, Instant::now()).unwrap();
    assert!(
        !j.view.is_revoked(),
        "the control isolates launch custody from file revocation"
    );
    assert_eq!(
        j.check(&LockFileNode::Lock, Operation::Read),
        Err(Errno::ESTALE),
        "retained lock files must recheck launch custody"
    );
}

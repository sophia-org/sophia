//! Both endpoints use the real 9P core; the middle handoff uses portal/files.
//! Admission and source bytes are supplied, not a production app frontend.
#[allow(dead_code)]
#[path = "support/recipient_binding.rs"]
mod wire;
use sophia_9p::{ConnectionId, Errno, OpenFlags, export::*};
use sophia_portal::{
    coordinator::*, execution_fence::PortalExecutionFence, files::PortalFileExecution,
    handlers::ClipboardPayloadHandler, recipient_object::PortalObjectRecipient,
};
use sophia_protocol::{portal_files::PortalFileLimits, *};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Node {
    Root,
    Status,
    Object(u64),
}
enum Content {
    Source,
    Recipient(Arc<PortalObjectRecipient>),
}
struct ExportOwner {
    content: Content,
    ready: bool,
}
impl ExportOwner {
    fn read_object(&self, qid: u64, offset: u64, count: u32) -> Result<Vec<u8>, Errno> {
        match &self.content {
            Content::Source if qid == 1 => Ok(b"secret"
                .iter()
                .skip(offset as usize)
                .take(count as usize)
                .copied()
                .collect()),
            Content::Source => Err(Errno::ESTALE),
            Content::Recipient(owner) => owner.read(qid, offset, count),
        }
    }
}
impl Export for ExportOwner {
    type Node = Node;
    type Handle = ();
    fn attach(&mut self, c: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        if c.connection != ConnectionId(1) || !c.aname.is_empty() {
            return Err(Errno::EACCES);
        }
        Ok(Attachment {
            root: Node::Root,
            epoch: Epoch(1),
        })
    }
    fn check(&mut self, a: &Access<'_, Node>) -> Result<(), Errno> {
        if a.connection != ConnectionId(1) || a.epoch != Epoch(1) {
            return Err(Errno::ESTALE);
        }
        if matches!(a.operation, Operation::Write) {
            return Err(Errno::EACCES);
        }
        if let Node::Object(qid) = a.node {
            self.read_object(*qid, 0, 0)?;
        }
        Ok(())
    }
    fn lookup(&mut self, dir: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
        if *dir != Node::Root {
            return Err(Errno::ENOTDIR);
        }
        match name {
            WalkName::Child(b"status") => Ok(Node::Status),
            WalkName::Child(b"transfer") => Ok(Node::Object(match &self.content {
                Content::Source => 1,
                Content::Recipient(owner) => owner.current()?,
            })),
            _ => Err(Errno::ENOENT),
        }
    }
    fn describe(&self, n: &Node, _: Option<&()>) -> Entry {
        Entry {
            kind: if *n == Node::Root {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path: match n {
                Node::Root => 0,
                Node::Status => u64::MAX,
                Node::Object(qid) => *qid,
            },
            qid_version: 0,
            permissions: 0o444,
            size: 6,
        }
    }
    fn open(&mut self, _: &Node, flags: OpenFlags) -> Result<(), Errno> {
        if flags.0 == 0 {
            Ok(())
        } else {
            Err(Errno::EACCES)
        }
    }
    fn read(
        &mut self,
        node: &Node,
        _: &mut (),
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        match node {
            Node::Root => Err(Errno::EISDIR),
            Node::Status => Ok(ReadOutcome::Ready(b"ready".to_vec())),
            Node::Object(_) if !self.ready => Ok(ReadOutcome::Pending),
            Node::Object(qid) => Ok(ReadOutcome::Ready(self.read_object(*qid, offset, count)?)),
        }
    }
    fn write(&mut self, _: &Node, _: &mut (), _: u64, _: &[u8]) -> Result<u32, Errno> {
        Err(Errno::EACCES)
    }
    fn release(&mut self, _: Node, _: Option<()>) {}
}
struct Source {
    generation: AtomicU64,
    wire: Mutex<wire::Wire<ExportOwner>>,
}
impl PortalSource for Source {
    fn generation(&self, _: &PortalRequest) -> Option<u64> {
        Some(self.generation.load(Ordering::Acquire))
    }
    fn collect(
        &self,
        _: &PortalRequest,
        _: &PortalGrant,
        wait: &PortalCollection,
    ) -> Result<Vec<u8>, PortalTransferFailure> {
        wait.remaining()?;
        Ok(self.wire.lock().unwrap().read(1).data().to_vec())
    }
}
struct Policy {
    source_live: AtomicBool,
    target_live: AtomicBool,
    allowed: AtomicBool,
}
impl PortalPolicy for Policy {
    fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext> {
        let live = match id.raw() {
            1 => &self.source_live,
            2 => &self.target_live,
            _ => return None,
        };
        live.load(Ordering::Acquire).then(|| {
            NamespaceContext::new(id, NamespaceProfile::Confined, NamespaceCapabilities::ALL)
                .unwrap()
        })
    }
    fn allow(&self, r: &PortalRequest) -> bool {
        self.allowed.load(Ordering::Acquire)
            && r.transfer.source_namespace.raw() == 1
            && r.transfer.target_namespace.raw() == 2
    }
}
struct Recipient {
    objects: Arc<PortalObjectRecipient>,
    effects: AtomicU64,
}
impl PortalRecipient for Recipient {
    fn execute(&self, work: &PortalFileExecution, lease: PortalAccessLease) -> bool {
        self.effects.fetch_add(1, Ordering::AcqRel);
        self.objects.execute(work, lease)
    }
    fn failed(&self, _: PortalTransferId, _: PortalTransferFailure) {}
}
struct Fixture {
    coordinator: Option<PortalTransferCoordinator>,
    source: Arc<Source>,
    policy: Arc<Policy>,
    recipient: Arc<Recipient>,
    fence: PortalExecutionFence,
    wire: wire::Wire<ExportOwner>,
}
impl Fixture {
    fn new() -> Self {
        let mut source_wire = wire::Wire::with_owner(ExportOwner {
            content: Content::Source,
            ready: true,
        });
        source_wire.open(1, b"transfer");
        let source = Arc::new(Source {
            generation: AtomicU64::new(9),
            wire: Mutex::new(source_wire),
        });
        let policy = Arc::new(Policy {
            source_live: AtomicBool::new(true),
            target_live: AtomicBool::new(true),
            allowed: AtomicBool::new(true),
        });
        let objects = Arc::new(PortalObjectRecipient::new(NamespaceId::from_raw(2)));
        let recipient = Arc::new(Recipient {
            objects: objects.clone(),
            effects: AtomicU64::new(0),
        });
        let fence = PortalExecutionFence::default();
        let coordinator = Some(
            PortalTransferCoordinator::start(
                policy.clone(),
                source.clone(),
                recipient.clone(),
                Arc::new(ClipboardPayloadHandler),
                fence.clone(),
                PortalFileLimits::defaults(1),
            )
            .unwrap(),
        );
        let wire = wire::Wire::with_owner(ExportOwner {
            content: Content::Recipient(objects),
            ready: true,
        });
        Self {
            coordinator,
            source,
            policy,
            recipient,
            fence,
            wire,
        }
    }
    fn transfer(&self, id: u64, kind: PortalTransferKind) -> Result<(), PortalTransferFailure> {
        self.coordinator
            .as_ref()
            .unwrap()
            .enqueue(PortalTransfer {
                transfer: PortalTransferId::from_raw(id),
                source_namespace: NamespaceId::from_raw(1),
                target_namespace: NamespaceId::from_raw(2),
                kind,
                mime_type: Some("UTF8_STRING".into()),
                byte_size: 6,
                decision: PortalDecision::Pending,
                generation: 9,
            })
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
    }
}

#[test]
fn completed_files_handoff_keeps_authorized_9p_object_readable_and_replacement_stales_old_fids() {
    let mut f = Fixture::new();
    assert_eq!(f.transfer(1, PortalTransferKind::Clipboard), Ok(()));
    let old = f.wire.open(1, b"transfer");
    assert_eq!(
        f.wire.read(1).data(),
        b"secret",
        "Completed does not revoke recipient access"
    );
    assert_eq!(f.recipient.effects.load(Ordering::Acquire), 1);
    assert!(
        f.transfer(1, PortalTransferKind::Clipboard).is_err(),
        "transfer replay cannot install again"
    );
    assert_eq!(f.recipient.effects.load(Ordering::Acquire), 1);
    assert_eq!(f.transfer(2, PortalTransferKind::Clipboard), Ok(()));
    assert_ne!(f.wire.open(2, b"transfer"), old);
    assert_eq!(f.wire.read(1).errno(), Some(116));
    assert_eq!(f.wire.read(2).data(), b"secret");
}

#[test]
fn every_bound_access_rechecks_lifetime_and_waiting_reads_cannot_escape_revocation() {
    for boundary in [
        "generation",
        "source",
        "target",
        "policy",
        "lock",
        "restart",
        "deadline",
        "binding",
    ] {
        let mut f = Fixture::new();
        assert_eq!(f.transfer(1, PortalTransferKind::Clipboard), Ok(()));
        f.wire.open(1, b"transfer");
        f.wire.open(2, b"status");
        f.wire.owner.ready = false;
        assert!(
            f.wire
                .send(116, 42, wire::Wire::<ExportOwner>::read_body(1))
                .is_empty()
        );
        match boundary {
            "generation" => {
                f.source.generation.store(10, Ordering::Release);
            }
            "source" => f.policy.source_live.store(false, Ordering::Release),
            "target" => f.policy.target_live.store(false, Ordering::Release),
            "policy" => f.policy.allowed.store(false, Ordering::Release),
            "lock" => {
                f.fence.block();
                f.fence.unblock();
            }
            "restart" => {
                drop(f.coordinator.take());
            }
            "deadline" => std::thread::sleep(Duration::from_millis(2001)),
            "binding" => f.recipient.objects.revoke(),
            _ => unreachable!(),
        }
        f.wire.owner.ready = true;
        f.wire.connection.retry_waiting(&mut f.wire.owner).unwrap();
        let replies = f.wire.drain();
        assert_eq!(replies.len(), 1, "{boundary}");
        assert_eq!(
            (replies[0].tag, replies[0].errno()),
            (42, Some(116)),
            "{boundary}"
        );
        assert_eq!(f.wire.read(1).errno(), Some(116), "{boundary}");
        assert_eq!(
            f.wire.read(2).data(),
            b"ready",
            "recipient authority survives {boundary}"
        );
    }
}

#[test]
fn unregistered_family_handlers_cannot_collect_or_install_a_payload() {
    let f = Fixture::new();
    for (i, kind) in [
        PortalTransferKind::DragAndDrop,
        PortalTransferKind::FileHandoff,
        PortalTransferKind::ScreenCapture,
        PortalTransferKind::ScreenRecording,
        PortalTransferKind::UriOpen,
        PortalTransferKind::Notification,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            f.transfer(i as u64 + 1, kind),
            Err(PortalTransferFailure::Denied)
        );
    }
    assert_eq!(f.recipient.effects.load(Ordering::Acquire), 0);
    assert_eq!(f.recipient.objects.current(), Err(Errno::ENOENT));
}

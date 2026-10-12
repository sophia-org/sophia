//! Supplied output inventory and bounded records, not renderer capture. The
//! production coordinator mints the lease and the recipient export checks it.
use sophia_9p::{ConnectionId, Errno, OpenFlags, export::*};
use sophia_portal::{
    client_files::{PortalClientFiles, PortalClientNode as Node},
    coordinator::*,
    execution_fence::PortalExecutionFence,
    handlers::PortalPayloadHandler,
    recipient_object::PortalObjectRecipient,
};
use sophia_protocol::{portal_files::PortalFileLimits, *};
use sophia_runtime::authenticated_files::{PreparedFileRoot, ProtectedFiles};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

struct OutputSource {
    output: u32,
    inventory: Arc<Mutex<BTreeMap<u32, u64>>>,
}
fn record(output: u32, generation: u64) -> Vec<u8> {
    [
        output.to_le_bytes().as_slice(),
        generation.to_le_bytes().as_slice(),
    ]
    .concat()
}
impl PortalSource for OutputSource {
    fn generation(&self, _: &PortalRequest) -> Option<u64> {
        self.inventory.lock().unwrap().get(&self.output).copied()
    }
    fn collect(
        &self,
        request: &PortalRequest,
        grant: &PortalGrant,
        wait: &PortalCollection,
    ) -> Result<Vec<u8>, PortalTransferFailure> {
        wait.remaining()?;
        if self.generation(request) != Some(grant.source_generation) {
            return Err(PortalTransferFailure::Source);
        }
        Ok(record(self.output, grant.source_generation))
    }
}
struct SuppliedPolicy;
impl PortalPolicy for SuppliedPolicy {
    fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext> {
        let caps = match id.raw() {
            1 => NamespaceCapabilities::NONE.with_publish(NamespacePortalCapability::ScreenCapture),
            2 => NamespaceCapabilities::NONE.with_request(NamespacePortalCapability::ScreenCapture),
            _ => return None,
        };
        Some(NamespaceContext::new(id, NamespaceProfile::Confined, caps).unwrap())
    }
    fn allow(&self, request: &PortalRequest) -> bool {
        request.transfer.source_namespace.raw() == 1
            && request.transfer.target_namespace.raw() == 2
            && request.transfer.kind == PortalTransferKind::ScreenCapture
    }
}
// Test-only record format: no production capture handler or pixel claim.
struct RecordHandler;
impl PortalPayloadHandler for RecordHandler {
    fn supports(&self, kind: PortalTransferKind) -> bool {
        kind == PortalTransferKind::ScreenCapture
    }
    fn validate(&self, request: &PortalRequest, bytes: &[u8]) -> Result<(), Errno> {
        if self.supports(request.transfer.kind) && bytes.len() == 12 {
            Ok(())
        } else {
            Err(Errno::EINVAL)
        }
    }
}
struct View {
    coordinator: PortalTransferCoordinator,
    files: PortalClientFiles,
}
impl View {
    fn new(output: u32, inventory: Arc<Mutex<BTreeMap<u32, u64>>>) -> Self {
        let objects = Arc::new(PortalObjectRecipient::new(NamespaceId::from_raw(2)));
        let coordinator = PortalTransferCoordinator::start(
            Arc::new(SuppliedPolicy),
            Arc::new(OutputSource { output, inventory }),
            objects.clone(),
            Arc::new(RecordHandler),
            PortalExecutionFence::default(),
            PortalFileLimits::defaults(1),
        )
        .unwrap();
        let mut files = PortalClientFiles::new(NamespaceId::from_raw(2), objects, 7).unwrap();
        files.bind_connection(ConnectionId(1)).unwrap();
        // Authentication is supplied here; independent SDK joins qualify it.
        files
            .prepare_authenticated_root(&AttachContext {
                connection: ConnectionId(1),
                peer: None,
                uname: b"observer",
                aname: b"portal-client",
                n_uname: u32::MAX,
            })
            .unwrap()
            .publish();
        Self { coordinator, files }
    }
    fn transfer(&self, id: u64, generation: u64) {
        assert_eq!(
            self.coordinator
                .enqueue(PortalTransfer {
                    transfer: PortalTransferId::from_raw(id),
                    source_namespace: NamespaceId::from_raw(1),
                    target_namespace: NamespaceId::from_raw(2),
                    kind: PortalTransferKind::ScreenCapture,
                    mime_type: Some("application/x-sophia-test-output-record".into()),
                    byte_size: 12,
                    decision: PortalDecision::Pending,
                    generation,
                })
                .recv_timeout(Duration::from_secs(3))
                .unwrap(),
            Ok(())
        );
    }
    fn node(&mut self, name: &[u8]) -> Node {
        self.files
            .lookup(&Node::Root, WalkName::Child(name))
            .unwrap()
    }
}

#[test]
fn reused_output_number_cannot_revive_a_retained_grant() {
    let inventory = Arc::new(Mutex::new(BTreeMap::from([(7, 9), (8, 12)])));
    let mut first = View::new(7, inventory.clone());
    let mut other = View::new(8, inventory.clone());
    first.transfer(1, 9);
    other.transfer(1, 12);
    let old = first.node(b"transfer");
    let watch = first.node(b"watch");
    let unrelated = other.node(b"transfer");
    let mut old_handle = first.files.open(&old, OpenFlags(0)).unwrap();
    let mut watch_handle = first.files.open(&watch, OpenFlags(0)).unwrap();
    let mut unrelated_handle = other.files.open(&unrelated, OpenFlags(0)).unwrap();
    assert_eq!(
        first.files.read(&old, &mut old_handle, 0, 12).unwrap(),
        ReadOutcome::Ready(record(7, 9))
    );
    assert_eq!(
        first.files.read(&watch, &mut watch_handle, 0, 1).unwrap(),
        ReadOutcome::Pending
    );

    // Keep the role, namespace, endpoint and binding unchanged. Only the
    // supplied physical target incarnation changes; no global fence masks it.
    {
        let mut outputs = inventory.lock().unwrap();
        assert_eq!(outputs.remove(&7), Some(9));
        assert_eq!(outputs.insert(7, 10), None);
        assert_eq!(outputs.get(&8), Some(&12));
    }
    assert_eq!(
        first.files.read(&old, &mut old_handle, 0, 12),
        Err(Errno::ESTALE),
        "reused output number must not preserve the old grant's read authority"
    );
    assert_eq!(
        first.files.read(&watch, &mut watch_handle, 0, 1),
        Err(Errno::ESTALE),
        "output replacement must retire the waiting watch"
    );
    assert_eq!(
        other
            .files
            .read(&unrelated, &mut unrelated_handle, 0, 12)
            .unwrap(),
        ReadOutcome::Ready(record(8, 12)),
        "another output's grant must remain usable"
    );
    first.transfer(2, 10);
    let fresh = first.node(b"transfer");
    assert_ne!(fresh, old);
    let mut fresh_handle = first.files.open(&fresh, OpenFlags(0)).unwrap();
    assert_eq!(
        first.files.read(&fresh, &mut fresh_handle, 0, 12).unwrap(),
        ReadOutcome::Ready(record(7, 10)),
        "new authority must name the replacement generation"
    );
    assert_eq!(
        first.files.read(&old, &mut old_handle, 0, 12),
        Err(Errno::ESTALE)
    );
}

//! Actual response credit/FIFO owners with supplied negotiated state over a
//! real socket pair whose peer does not read: requests arrive through the
//! production read path, drain is the production write path. This is neither
//! socket negotiation nor kernel backpressure.
use super::super::control_budget::CONTROL_RECORD_BYTES;
use super::super::outbound::{Admitted, OutboundRecord};
use super::super::socket::SocketWire;
use super::super::wire::Wire;
use super::*;
use std::io::Write as _;
use std::os::unix::net::UnixStream;
#[path = "catalog_candidate_transport.rs"]
mod candidates;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(1);
const GRANT: ContentGrant = ContentGrant {
    connection_epoch: 7,
    content_grant_epoch: 9,
};
struct Fixture {
    transport: ShellComponentTransport,
    epochs: crate::ContentEpochRegistry,
    peer: UnixStream,
    directory: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        Self::with_control_limit(1)
    }
    fn with_control_limit(control_records: u32) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "sophia-catalog-credit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut transport = ShellComponentTransport::bind_for_supervised_uid(
            &directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        let mut epochs = crate::ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
        let mut limits = ContentLimits::prototype(GRANT);
        limits.max_control_records = control_records;
        transport
            .reserve_content_with_profile(
                &mut epochs,
                limits.clone(),
                crate::ContentStoreProfile::PersistentCatalog,
            )
            .unwrap();
        transport.content_limits = Some(limits);
        transport.content_grant = Some(GRANT);
        transport.connection_epoch = GRANT.connection_epoch;
        transport.capabilities = SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG
            | SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
            | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
            | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;
        let (local, peer) = UnixStream::pair().unwrap();
        local.set_nonblocking(true).unwrap();
        let limits = transport.content_limits.clone();
        transport.wire = Some(Wire::Socket(Box::new(SocketWire::new(
            local,
            limits.as_ref(),
        ))));
        Self {
            transport,
            epochs,
            peer,
            directory,
        }
    }
    /// Delivers one client frame through the socket's production read path.
    fn deliver(&mut self, frame: Vec<u8>) {
        self.peer.write_all(&frame).unwrap();
        let socket = self.transport.socket_mut().unwrap();
        let before = socket.input_accounting().0;
        while socket.input_accounting().0 == before {
            socket.receive(4096).unwrap();
        }
    }
    fn inbox(&self) -> usize {
        self.transport.socket().unwrap().input_accounting().0
    }
    fn front_frame_bytes(&self) -> usize {
        SocketWire::encode(&self.transport.output.front().unwrap().record)
            .unwrap()
            .len()
    }
    /// Writes at most `bytes` of the output order through the socket.
    fn send(&mut self, bytes: usize) {
        let Some(Wire::Socket(socket)) = self.transport.wire.as_mut() else {
            unreachable!("the fixture attaches a socket");
        };
        socket.send(&mut self.transport.output, bytes).unwrap();
    }
    /// One owned typed bulk record: output facts with no rows.
    fn bulk(&self) -> Admitted {
        self.transport
            .admit_record(
                OutboundRecord::Content(
                    TransactionId::from_raw(90),
                    ShellContentRecord::OutputFacts(ContentOutputFacts {
                        grant: GRANT,
                        facts_generation: 1,
                        outputs: Vec::new(),
                    }),
                ),
                super::super::control_budget::Class::Bulk,
            )
            .unwrap()
    }
    fn request(&mut self, event_id: u64) -> (TransactionId, CatalogActivation) {
        let request = CatalogActivation {
            action: ContentAction {
                grant: GRANT,
                output: ContentOutputId {
                    id: 1,
                    generation: 2,
                },
                allocation: ContentAllocationId {
                    id: 1,
                    generation: 1,
                },
                candidate_generation: 3,
                presentation_epoch: 4,
                interaction_generation: 5,
                target_id: 1,
                target_generation: 1,
                action_id: 1,
                event_id,
                kind: 1,
                reason: 0,
            },
            catalog_generation: 6,
        };
        let tx = TransactionId::from_raw(event_id);
        self.deliver(
            encode_shell_catalog_action_frame(
                tx,
                &ShellCatalogActionRecord::Activate(request.clone()),
            )
            .unwrap(),
        );
        (tx, request)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

#[test]
fn persistent_response_credit_transfers_once_and_survives_partial_write() {
    let mut f = Fixture::new();
    let (tx, request) = f.request(1);
    let bulk = f.bulk();
    f.transport.transfer_record(bulk);
    assert!(
        f.transport
            .take_catalog_request(&f.epochs)
            .unwrap()
            .is_none()
    );
    assert_eq!(f.inbox(), 1);
    let bytes = f.front_frame_bytes();
    f.send(bytes);
    assert_eq!(
        f.transport.take_catalog_request(&f.epochs).unwrap(),
        Some((tx, request.clone()))
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        1
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_bytes,
        CONTROL_RECORD_BYTES
    );
    assert!(!f.transport.bulk_capacity_available(&f.epochs, 1));
    assert!(!f.transport.control_capacity_available(&f.epochs, 1));
    let mut wrong = request.clone();
    wrong.catalog_generation += 1;
    assert!(
        f.transport
            .finish_catalog_activation(&f.epochs, tx, &wrong, 1)
            .is_err()
    );
    assert!(
        f.transport
            .catalog_response
            .as_ref()
            .unwrap()
            .status
            .is_none()
    );
    f.transport
        .finish_catalog_activation(&f.epochs, tx, &request, 1)
        .unwrap();
    assert!(f.transport.catalog_response.is_none());
    let OutboundRecord::CatalogAction(actual_tx, record) =
        f.transport.output.front().unwrap().record.clone()
    else {
        panic!("the outcome is queued as a typed catalog record");
    };
    assert_eq!(actual_tx, tx);
    assert_eq!(
        record,
        ShellCatalogActionRecord::ActivationOutcome(CatalogActivationOutcome {
            activation: request.clone(),
            status: 1,
            reason: 0,
        })
    );
    assert!(!f.transport.flush_catalog_response(&f.epochs).unwrap());
    assert!(
        f.transport
            .finish_catalog_activation(&f.epochs, tx, &request, 1)
            .is_err()
    );
    let remaining = f.front_frame_bytes() - 1;
    f.send(1);
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        1
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_bytes,
        CONTROL_RECORD_BYTES
    );
    f.send(remaining);
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        0
    );
    assert_eq!(f.transport.content_accounting(&f.epochs).response_bytes, 0);
}

#[test]
fn refused_transfer_keeps_original_outcome_and_blocks_another_intake() {
    let mut f = Fixture::new();
    let (tx, request) = f.request(1);
    f.transport
        .take_catalog_request(&f.epochs)
        .unwrap()
        .unwrap();
    f.request(2);
    // Defensive returned refusal: production negotiated limits are immutable.
    f.transport
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 0;
    f.transport
        .finish_catalog_activation(&f.epochs, tx, &request, 1)
        .unwrap();
    assert!(f.transport.output.front().is_none());
    assert_eq!(
        f.transport.catalog_response.as_ref().unwrap().status,
        Some(1)
    );
    assert!(
        f.transport
            .take_catalog_request(&f.epochs)
            .unwrap()
            .is_none()
    );
    assert_eq!(f.inbox(), 1);
    assert!(
        f.transport
            .finish_catalog_activation(&f.epochs, tx, &request, 2)
            .is_err()
    );
    assert_eq!(
        f.transport.catalog_response.as_ref().unwrap().status,
        Some(1)
    );
    f.transport
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 1;
    assert!(f.transport.flush_catalog_response(&f.epochs).unwrap());
    assert!(f.transport.catalog_response.is_none());
    assert_eq!(f.transport.output.records(), 1);
    assert!(
        f.transport
            .take_catalog_request(&f.epochs)
            .unwrap()
            .is_none()
    );
}

#[test]
fn wrong_role_or_grant_cannot_consume_request_credit() {
    let mut f = Fixture::new();
    let (tx, mut request) = f.request(1);
    let capabilities = f.transport.capabilities;
    f.transport.capabilities &= !SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG;
    assert_eq!(
        f.transport.take_catalog_request(&f.epochs),
        Err(ShellTransportError::MissingCapability)
    );
    assert!(f.transport.catalog_response.is_none());
    assert_eq!(f.inbox(), 1);
    f.transport.capabilities = capabilities;
    f.transport.content_grant = Some(ContentGrant {
        connection_epoch: 8,
        content_grant_epoch: 10,
    });
    assert_eq!(
        f.transport.take_catalog_request(&f.epochs),
        Err(ShellTransportError::MissingCapability)
    );
    assert!(f.transport.catalog_response.is_none());
    assert_eq!(f.inbox(), 1);
    f.transport.content_grant = Some(GRANT);
    request.action.grant.connection_epoch += 1;
    f.transport.socket_mut().unwrap().take_catalog_activate();
    f.deliver(
        encode_shell_catalog_action_frame(tx, &ShellCatalogActionRecord::Activate(request.clone()))
            .unwrap(),
    );
    assert_eq!(
        f.transport.take_catalog_request(&f.epochs),
        Err(ShellTransportError::WrongContentGrant)
    );
    assert!(f.transport.catalog_response.is_none());
    assert_eq!(f.inbox(), 1);
    request.action.grant = GRANT;
    f.transport.socket_mut().unwrap().take_catalog_activate();
    f.deliver(
        encode_shell_catalog_action_frame(tx, &ShellCatalogActionRecord::Activate(request))
            .unwrap(),
    );
    f.transport
        .take_catalog_request(&f.epochs)
        .unwrap()
        .unwrap();
    f.transport.disconnect(&mut f.epochs).unwrap();
    assert!(f.transport.catalog_response.is_none());
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        0
    );
}

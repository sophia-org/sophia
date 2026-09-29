//! Raw 9P for independent activation/ack ordering; the SDK's combined response
//! would hide the activation-before-ack case. Admission and FIFO are production.
use crate::live_session::tests::shell_file_peer as file_peer;
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
static NEXT: AtomicU64 = AtomicU64::new(1);
pub(super) const GRANT: ContentGrant = ContentGrant {
    connection_epoch: 2,
    content_grant_epoch: 2,
};
pub(super) fn tx(id: u64) -> TransactionId {
    TransactionId::from_raw(id)
}
pub(super) fn limits() -> ContentLimits {
    ContentLimits::prototype(GRANT)
}
pub(super) fn empty() -> ContentEpochRegistry {
    ContentEpochRegistry::new(64 * 1024 * 1024).unwrap()
}
pub(super) fn granted() -> ShellContentAdmissionPolicy {
    ShellContentAdmissionPolicy::Granted {
        discrete_input: true,
    }
}
pub(super) struct Peer {
    pub transport: ShellComponentTransport,
    client: file_peer::Peer,
    submission: u64,
}
impl Peer {
    pub fn new(epochs: &mut ContentEpochRegistry, profile: ContentStoreProfile) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "catalog-action-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut transport = ShellComponentTransport::bind_for_supervised_uid(
            &directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        transport
            .authorize_protected_peer(&ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: std::process::id(),
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            })
            .unwrap();
        transport
            .reserve_content_with_profile(epochs, limits(), profile)
            .unwrap();
        let client = file_peer::Peer::connect(transport.socket_path());
        Self {
            transport,
            client,
            submission: 1,
        }
    }
    pub fn negotiate(
        &mut self,
        epochs: &mut ContentEpochRegistry,
        hello: ShellV1ClientHello,
        policy: ShellContentAdmissionPolicy,
    ) -> Result<ShellV1ServerWelcome, ShellTransportError> {
        self.transport.begin_file_negotiation(
            epochs,
            GRANT.connection_epoch,
            Duration::from_secs(2),
            policy,
        )?;
        std::thread::scope(|scope| {
            let client = &mut self.client;
            let worker = scope.spawn(move || {
                client.setup();
                let bytes = encode_shell_file_negotiate(header(ShellFileKind::Negotiate, 1), hello)
                    .unwrap();
                assert_eq!(client.submit(&bytes).0, 119);
                let submitted = negotiation_io(client.try_next_event())?;
                assert_eq!(
                    decode_shell_file_submitted(&submitted)
                        .unwrap()
                        .submission_id,
                    1
                );
                negotiation_io(client.try_ack(&submitted))?;
                let event = negotiation_io(client.try_next_event())?;
                let kind = decode_shell_file_record(&event, ShellFileClass::Event)
                    .unwrap()
                    .header
                    .kind;
                assert!(matches!(
                    kind,
                    ShellFileKind::Negotiated | ShellFileKind::Refused
                ));
                negotiation_io(client.try_ack(&event))?;
                if kind == ShellFileKind::Refused {
                    return None;
                }
                client.clear();
                let welcome = decode_shell_file_negotiated(&event).unwrap().welcome;
                client.open(6, b"limits", 0);
                let received = decode_shell_file_limits(&client.read(6, 0)).unwrap();
                assert_eq!(received, limits());
                Some(welcome)
            });
            let deadline = Instant::now() + Duration::from_secs(5);
            let result = loop {
                match self.transport.poll_negotiation(epochs, 65536) {
                    Ok(Some(welcome)) => break Ok(welcome),
                    Err(error) => break Err(error),
                    Ok(None) => {}
                }
                assert!(Instant::now() < deadline, "negotiation hung");
                std::thread::yield_now();
            };
            while !worker.is_finished() {
                if result.is_ok() {
                    self.transport.poll_io(epochs).unwrap();
                }
                assert!(Instant::now() < deadline, "handshake observation hung");
                std::thread::yield_now();
            }
            assert_eq!(worker.join().unwrap(), result.as_ref().ok().copied());
            result
        })
    }
    fn drive<R: Send>(
        &mut self,
        epochs: &mut ContentEpochRegistry,
        operation: impl FnOnce(&mut file_peer::Peer) -> R + Send,
    ) -> R {
        std::thread::scope(|scope| {
            let client = &mut self.client;
            let worker = scope.spawn(move || operation(client));
            let deadline = Instant::now() + Duration::from_secs(5);
            while !worker.is_finished() {
                self.transport.poll_io(epochs).unwrap();
                assert!(Instant::now() < deadline, "file operation hung");
                std::thread::yield_now();
            }
            worker.join().unwrap()
        })
    }
    pub fn read(&mut self, epochs: &mut ContentEpochRegistry) -> Vec<u8> {
        self.drive(epochs, |client| {
            let event = client.next_event();
            client.ack(&event);
            event
        })
    }
    pub fn activate(&mut self, epochs: &mut ContentEpochRegistry, activation: CatalogActivation) {
        self.submission += 1;
        let bytes = encode_shell_file_catalog_action(
            header(ShellFileKind::CatalogActivate, self.submission),
            &ShellFileCatalogActionRecord {
                transaction: tx(9),
                record: ShellCatalogActionRecord::Activate(activation),
            },
        )
        .unwrap();
        self.submit(epochs, bytes);
    }
    pub fn send_content(
        &mut self,
        epochs: &mut ContentEpochRegistry,
        transaction: TransactionId,
        record: ShellContentRecord,
    ) {
        self.submission += 1;
        let bytes = encode_shell_file_transaction(
            header(
                shell_file_transaction_kind(&record).unwrap(),
                self.submission,
            ),
            &ShellFileTransactionRecord {
                transaction,
                record,
            },
        )
        .unwrap();
        self.submit(epochs, bytes);
    }
    fn submit(&mut self, epochs: &mut ContentEpochRegistry, bytes: Vec<u8>) {
        let id = self.submission;
        self.drive(epochs, |client| client.submit_acknowledged(&bytes, id));
    }
    pub fn assert_no_event(&mut self, epochs: &mut ContentEpochRegistry) {
        // Empty event reads block. Submit an unrelated demand cancellation
        // without running its owner: Submitted must be the next journal entry,
        // so any extra Action fails the custody decoder. This sends no action ACK.
        self.send_content(
            epochs,
            tx(99),
            ShellContentRecord::FrameDemandCancel(ContentFrameDemandCancel {
                grant: GRANT,
                output: ContentOutputId {
                    id: 1,
                    generation: 1,
                },
                demand_id: 1,
                permit_id: 1,
            }),
        );
    }
}
fn negotiation_io<T>(result: std::io::Result<T>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            assert!(
                matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::UnexpectedEof
                ),
                "unexpected negotiation I/O: {error}"
            );
            // The parent must independently observe a production refusal;
            // a disconnected client alone never satisfies the test.
            None
        }
    }
}
fn header(kind: ShellFileKind, submission_id: u64) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: GRANT.connection_epoch,
        submission_id,
        sequence: 0,
    }
}

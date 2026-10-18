#![cfg(test)]
//! In-crate while-validating cancel for the lock sealed-image path.
//!
//! Production `LockFileTransport::turn` serves the worker before it
//! dispatches wire requests. For a fixture this small the worker
//! finishes almost at once, so by the time a staged cancel is
//! dispatched its completion has normally been collected already;
//! ordinary turn order provides no deterministic gate that journals
//! the cancel first (a worker still scanning is the opposite case,
//! and proves nothing here either). This control is therefore
//! in-crate: as supplied owner scheduling (not production turn-order
//! timing), the owner holds its turns while the worker holds the
//! source, stages the cancel's submit frame, then in one owner turn
//! dispatches the wire BEFORE the worker serves. The cancel journals
//! against a Validating offer and the real completion settles it.
//!
//! Actual handoff is asserted from custody's own facts, not the
//! reservation count: exactly one live offer, and `image_deadline`
//! empty because it excludes Validating offers. Submitted is
//! asserted journaled between dispatch and serve by journal position.
//!
//! The provider child re-executes this test binary at
//! `lock_files::transport::cancel_tests::child`; afid verdicts come
//! from the supplied test authority below, not launch evidence.

use super::*;

use std::io::{Read, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::raw_file_test_peer as raw;

const EPOCH: u64 = 7;
const RELAY: &str = "SOPHIA_TEST_LOCK_CANCEL_RELAY";
const LOCK: &str = "SOPHIA_TEST_LOCK_CANCEL_SOCKET";
const IMAGE: &str = "SOPHIA_TEST_LOCK_CANCEL_ENDPOINT";
const CHILD: &str = "lock_files::transport::cancel_tests::child";

/// Supplied afid verdicts, copied from the integration authority;
/// not launch authentication evidence.
mod authority {
    use sophia_9p::{
        AttachContext, ConnectionId, Errno, Fid, ReadOutcome, authentication::Attempt,
    };
    use sophia_protocol::*;
    use std::time::{Duration, Instant};

    use crate::authenticated_files::{FileAuthentication, FileAuthenticationFactory};

    pub struct Factory;
    struct Owner {
        connection: ConnectionId,
        admitted: bool,
    }
    impl FileAuthenticationFactory for Factory {
        fn create(
            &mut self,
            connection: ConnectionId,
            _: Instant,
            _: sophia_9p::unix::Wake,
        ) -> Result<Box<dyn FileAuthentication>, Errno> {
            Ok(Box::new(Owner {
                connection,
                admitted: false,
            }))
        }
    }
    impl FileAuthentication for Owner {
        fn claims(&self, context: &AttachContext<'_>) -> Result<(), Errno> {
            if context.connection == self.connection {
                Ok(())
            } else {
                Err(Errno::EACCES)
            }
        }
        fn begin(&mut self, afid: Fid, _: Instant) -> Result<Attempt, Errno> {
            Ok(Attempt {
                server: 1,
                connection: self.connection,
                session: 1,
                audience: 1,
                wire_generation: 1,
                attempt_generation: 1,
                afid: afid.0,
            })
        }
        fn read(&mut self, _: Attempt, _: u64, _: u32, _: Instant) -> Result<ReadOutcome, Errno> {
            Err(Errno::EACCES)
        }
        fn commit(&mut self, attempt: Attempt, _: Instant) -> Result<(), Errno> {
            if attempt.connection != self.connection {
                return Err(Errno::EACCES);
            }
            self.admitted = true;
            Ok(())
        }
        fn check(&mut self, connection: ConnectionId, _: Instant) -> Result<(), Errno> {
            if self.admitted && connection == self.connection {
                Ok(())
            } else {
                Err(Errno::EACCES)
            }
        }
        fn release(&mut self, _: Attempt, _: Instant) {}
        fn reset(&mut self, _: Instant) -> Result<(), Errno> {
            self.admitted = false;
            Ok(())
        }
        fn close(&mut self, _: Instant) {
            self.admitted = false;
        }
        fn poll(&mut self, _: Instant) -> Result<(), Errno> {
            Ok(())
        }
        fn drained(&self) -> bool {
            !self.admitted
        }
        fn admitted(&self) -> bool {
            self.admitted
        }
        fn wait(&self, _: Instant) -> Duration {
            Duration::MAX
        }
        fn admission_context(&self) -> Option<ClientAdmissionContext> {
            self.admitted.then(|| {
                ClientAdmissionContext::new(
                    ClientAdmissionId::from_raw(1),
                    NamespaceContext::new(
                        NamespaceId::from_raw(2),
                        NamespaceProfile::Confined,
                        NamespaceCapabilities::NONE,
                    )
                    .unwrap(),
                    ClientAuthProvenance::new(
                        ClientAuthenticationMethod::FactotumLaunch {
                            principal: 3,
                            launch_generation: 4,
                        },
                        5,
                    )
                    .unwrap(),
                )
                .unwrap()
            })
        }
    }
}

fn send_fd(relay: &UnixStream, fd: BorrowedFd<'_>) {
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    let rights = [fd];
    assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
    let sent = sendmsg(
        relay,
        &[std::io::IoSlice::new(b"f")],
        &mut control,
        SendFlags::NOSIGNAL,
    )
    .unwrap();
    assert_eq!(sent, 1);
}

fn receive_fd(relay: &UnixStream) -> OwnedFd {
    let mut ready = [rustix::event::PollFd::new(
        relay,
        rustix::event::PollFlags::IN,
    )];
    let timeout = rustix::event::Timespec {
        tv_sec: 5,
        tv_nsec: 0,
    };
    assert_eq!(rustix::event::poll(&mut ready, Some(&timeout)).unwrap(), 1);
    let mut byte = [0u8; 1];
    let mut message = sophia_linux_peer::receive_rights(relay.as_fd(), &mut byte).unwrap();
    assert_eq!((message.bytes, message.count), (1, 1));
    message.descriptors[0].take().unwrap()
}

/// The supervised provider child. Inert unless launched by
/// `Provider::start` with the relay environment set.
#[test]
fn child() {
    let Ok(relay) = std::env::var(RELAY) else {
        return;
    };
    let mut relay = UnixStream::connect(relay).unwrap();
    let lock = UnixStream::connect(std::env::var(LOCK).unwrap()).unwrap();
    send_fd(&relay, lock.as_fd());
    drop(lock);
    let image = PathBuf::from(std::env::var(IMAGE).unwrap());
    let mut command = [0u8; 1];
    // Each 'i' connects one more image socket; anything else, or EOF, exits.
    while relay.read(&mut command).unwrap_or(0) == 1 && command[0] == b'i' {
        let socket = rustix::net::socket_with(
            rustix::net::AddressFamily::UNIX,
            rustix::net::SocketType::SEQPACKET,
            rustix::net::SocketFlags::CLOEXEC,
            None,
        )
        .unwrap();
        rustix::net::connect(&socket, &rustix::net::SocketAddrUnix::new(&image).unwrap()).unwrap();
        send_fd(&relay, socket.as_fd());
    }
    std::process::exit(0);
}

/// Alive and not yet a zombie.
fn running(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| !stat.contains(") Z "))
}

struct Provider {
    supervisor: crate::ProcessSupervisor,
    relay: UnixStream,
}

impl Provider {
    fn start(transport: &mut LockFileTransport, directory: &std::path::Path) -> (Self, UnixStream) {
        let relay_path = directory.with_extension("relay");
        let _ = std::fs::remove_file(&relay_path);
        let listener = UnixListener::bind(&relay_path).unwrap();
        let spec = crate::ProcessLaunchSpec::new(std::env::current_exe().unwrap())
            .arg(CHILD)
            .arg("--exact")
            .arg("--test-threads=1")
            .arg("-q")
            .env(RELAY, &relay_path)
            .env(LOCK, transport.socket_path())
            .env(IMAGE, directory.join("image.sock"));
        let mut supervisor =
            crate::ProcessSupervisor::new(crate::SupervisedProcessKind::LockProvider, spec);
        supervisor
            .apply(crate::SupervisorCommand::StartProcess {
                process: crate::SupervisedProcessKind::LockProvider,
                delay: Duration::ZERO,
            })
            .unwrap();
        transport
            .authorize_assignee(
                LockFileAssignee::from_supervisor(&supervisor)
                    .unwrap()
                    .with_authentication(Box::new(authority::Factory)),
            )
            .unwrap();
        let (relay, _) = listener.accept().unwrap();
        relay
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let _ = std::fs::remove_file(&relay_path);
        let stream = UnixStream::from(receive_fd(&relay));
        (Self { supervisor, relay }, stream)
    }

    fn connect_image(&mut self) -> OwnedFd {
        self.relay.write_all(b"i").unwrap();
        receive_fd(&self.relay)
    }

    /// Ends the provider process; its pidfd becomes readable.
    fn exit(&mut self) {
        let _ = self.relay.write_all(b"q");
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.supervisor.child_id().is_some_and(running) {
            assert!(Instant::now() < deadline, "provider did not exit");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.exit();
        let _ = self.supervisor.poll();
    }
}

fn policy(hello_timeout_ms: u32) -> LockImagePolicy {
    LockImagePolicy {
        hello_timeout_ms,
        validation_timeout_ms: 5000,
        validation_chunk_bytes: 65536,
        capture_interest_timeout_ms: 500,
        max_sealed_backing_bytes: 2048,
    }
}

fn transport(label: &str) -> (LockFileTransport, PathBuf) {
    let directory =
        std::env::temp_dir().join(format!("lock-image-cancel-{label}-{}", std::process::id()));
    let transport = LockFileTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
        EPOCH,
        LockFileLimits {
            max_outputs: 1,
            upload_slots: 1,
            max_chords: 0,
            max_width_px: 16,
            max_height_px: 16,
            max_resource_bytes: 1024,
            max_live_resources: 2,
            journal_records: 32,
            journal_bytes: 8192,
            assembly_timeout_ms: 1000,
            ack_progress_timeout_ms: 2000,
        },
    )
    .unwrap();
    (transport, directory)
}

fn locked() -> LockObject {
    LockObject {
        lock_epoch: 3,
        topology_generation: 4,
        phase: LockPhase::Locked,
        allocations: vec![LockAllocation {
            output_id: 1,
            output_generation: 1,
            allocation_id: 2,
            allocation_generation: 1,
            pixel_width: 4,
            pixel_height: 4,
            scale_numerator: 1,
            scale_denominator: 1,
        }],
    }
}

/// A 4x4 ISB image for the one allocation of [`locked`].
fn offer_metadata(offer: u64) -> LockImageMetadata {
    LockImageMetadata {
        offer_id: offer,
        resource: 20 + offer,
        resource_generation: 1,
        admission_epoch: 3,
        output_id: 1,
        allocation_id: 2,
        topology_generation: 4,
        source_kind: LOCK_IMAGE_SOURCE_SEALED_MEMORY,
        source_format: LOCK_IMAGE_SEALED_FORMAT,
        width: 4,
        height: 4,
        stride: 16,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 64,
        modifier: 0,
    }
}

fn hello_at(socket: &OwnedFd, epoch: u64, generation: u64) {
    let bytes = LockImagePacket::Hello {
        connection_epoch: epoch,
        channel_generation: generation,
    }
    .encode()
    .unwrap();
    rustix::net::send(socket, &bytes, rustix::net::SendFlags::NOSIGNAL).unwrap();
}

/// A sealed 4x4 source for offer metadata in this file.
fn sealed_source() -> OwnedFd {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    let mut file = std::fs::File::from(
        memfd_create(
            "lock-cancel-source",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    std::io::Write::write_all(&mut file, &[9u8; 64]).unwrap();
    fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    file.into()
}

/// Sends an offer's Source on the image channel with `source`.
fn send_source(socket: &OwnedFd, epoch: u64, offer: u64, source: &OwnedFd) {
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
    let metadata = offer_metadata(offer);
    let unused = LockImagePlane {
        offset: 0,
        stride: 0,
        fd_index: 0,
    };
    let mut planes = [unused; LOCK_IMAGE_MAX_PLANES];
    planes[0].stride = metadata.stride;
    let bytes = LockImagePacket::Source {
        connection_epoch: epoch,
        channel_generation: epoch,
        metadata,
        planes,
    }
    .encode()
    .unwrap();
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    let rights = [source.as_fd()];
    assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
    let sent = sendmsg(
        socket,
        &[std::io::IoSlice::new(&bytes)],
        &mut control,
        SendFlags::NOSIGNAL,
    )
    .unwrap();
    assert_eq!(sent, bytes.len());
}

/// The provider's side of the lock files, at the selected revision.
struct Files {
    peer: raw::Peer,
    offset: u64,
    queued: std::collections::VecDeque<(LockFileKind, u64, Vec<u8>)>,
    submission: u64,
    epoch: u64,
}

impl Files {
    fn attach(stream: UnixStream) -> Self {
        let mut peer = raw::Peer::from_stream(stream);
        let version = [
            65536u32.to_le_bytes().as_slice(),
            &8u16.to_le_bytes(),
            b"9P2000.L",
        ]
        .concat();
        assert_eq!(peer.rpc(100, &version).unwrap().0, 101);
        let auth = [
            10u32.to_le_bytes().as_slice(),
            &[0; 4],
            &u32::MAX.to_le_bytes(),
        ]
        .concat();
        assert_eq!(peer.rpc(102, &auth).unwrap().0, 103);
        let attach = [
            1u32.to_le_bytes().as_slice(),
            &10u32.to_le_bytes(),
            &[0; 4],
            &u32::MAX.to_le_bytes(),
        ]
        .concat();
        assert_eq!(peer.rpc(104, &attach).unwrap().0, 105);
        peer.open(2, b"events", 0);
        peer.open(3, b"submit", 1);
        peer.open(4, b"ack", 1);
        Self {
            peer,
            offset: 0,
            queued: Default::default(),
            submission: 0,
            epoch: EPOCH,
        }
    }

    /// Submits under the next ID. Returns the submit reply's errno, if
    /// any; a refused submission keeps its ID.
    fn submit(&mut self, kind: LockFileKind, body: &[u8], revision: u16) -> Option<u32> {
        let id = self.submission + 1;
        let candidate = encode_lock_file_record_at_revision(
            LockFileHeader {
                kind,
                connection_epoch: self.epoch,
                submission_id: id,
                sequence: 0,
            },
            body,
            revision,
        )
        .unwrap();
        self.peer.open(5, b"transaction", 2);
        assert_eq!(self.peer.write(5, &candidate).0, 119);
        let control = encode_lock_file_submit(LockFileSubmit {
            connection_epoch: self.epoch,
            submission_id: id,
            candidate_bytes: candidate.len() as u32,
        })
        .unwrap();
        let (reply, body) = self.peer.write(3, &control);
        self.peer.clunk(5);
        if reply == 119 {
            self.submission = id;
            None
        } else {
            assert_eq!(reply, 7);
            Some(u32::from_le_bytes(body.try_into().unwrap()))
        }
    }

    fn negotiate(&mut self) -> u16 {
        let body = LockNegotiate {
            minimum_revision: 1,
            maximum_revision: 2,
            requested_capabilities: LOCK_FILE_CAPABILITY_PRESENT
                | LOCK_FILE_CAPABILITY_SEALED_MEMORY
                | LOCK_FILE_CAPABILITY_DMA_BUF,
            chords: Vec::new(),
        }
        .encode()
        .unwrap();
        assert_eq!(self.submit(LockFileKind::Negotiate, &body, 1), None);
        assert_eq!(self.next().0, LockFileKind::Submitted);
        let (kind, body) = self.next();
        assert_eq!(kind, LockFileKind::Negotiated);
        assert_eq!(self.next().0, LockFileKind::ObjectPublished);
        LockNegotiated::decode_selected(&body).unwrap().0
    }

    /// The next event, acknowledged once read.
    fn next(&mut self) -> (LockFileKind, Vec<u8>) {
        if self.queued.is_empty() {
            let bytes = self.peer.read(2, self.offset);
            self.offset += bytes.len() as u64;
            let mut rest = &bytes[..];
            while !rest.is_empty() {
                let size = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
                let record =
                    decode_lock_file_record_at_revision(&rest[..size], LockFileClass::Event, 2)
                        .unwrap();
                self.queued.push_back((
                    record.header.kind,
                    record.header.sequence,
                    record.body.to_vec(),
                ));
                rest = &rest[size..];
            }
        }
        let (kind, sequence, body) = self.queued.pop_front().expect("an event");
        let ack = encode_lock_file_ack(LockFileAck {
            connection_epoch: self.epoch,
            sequence,
        })
        .unwrap();
        assert_eq!(self.peer.write(4, &ack).0, 119);
        (kind, body)
    }

    fn image_begin(&mut self, offer: u64) -> Option<u32> {
        let metadata = offer_metadata(offer);
        self.submit(LockFileKind::ImageBegin, &metadata.encode().unwrap(), 2)
    }

    /// An offer the ready channel took and the owner admitted.
    fn admitted(&mut self, offer: u64) {
        assert_eq!(self.image_begin(offer), None);
        assert_eq!(self.next().0, LockFileKind::Submitted);
        let (kind, body) = self.next();
        assert_eq!(kind, LockFileKind::ImageAdmitted);
        assert_eq!(LockImageMetadata::decode(&body).unwrap().offer_id, offer);
    }

    /// The final status and source release of offer `offer`.
    fn settled(&mut self, offer: u64) -> (LockResourceState, u16, LockSourceDisposition) {
        let (kind, body) = self.next();
        assert_eq!(kind, LockFileKind::ResourceStatus);
        let status = LockResourceStatus::decode(&body).unwrap();
        assert_eq!(status.transaction, offer);
        let (kind, body) = self.next();
        assert_eq!(kind, LockFileKind::SourceReleased);
        let released = LockSourceReleased::decode(&body).unwrap();
        assert_eq!(released.offer_id, offer);
        (status.status, status.reason, released.disposition)
    }

    /// The terminal pair of a cancelled offer: Cancelled with no
    /// reason, released CancelledBeforeImport (4).
    fn cancelled(&mut self, offer: u64) {
        assert_eq!(self.next().0, LockFileKind::Submitted);
        assert_eq!(
            self.settled(offer),
            (
                LockResourceState::Cancelled,
                0,
                LockSourceDisposition::CancelledBeforeImport,
            )
        );
    }

    /// Builds a `ResourceCancel` for `offer`, opens its transaction
    /// fid and stages the candidate bytes on it. Nothing is submitted
    /// yet: only the submit control record submits, and it is staged
    /// separately so it can land as a single frame.
    fn prepare_cancel(&mut self, offer: u64) -> (Vec<u8>, u64) {
        let step = LockResourceStep {
            transaction: offer,
            resource: LockResourceId {
                id: 20 + offer,
                generation: 1,
            },
            total_bytes: None,
        };
        let id = self.submission + 1;
        let candidate = encode_lock_file_record_at_revision(
            LockFileHeader {
                kind: LockFileKind::ResourceCancel,
                connection_epoch: self.epoch,
                submission_id: id,
                sequence: 0,
            },
            &step.encode().unwrap(),
            2,
        )
        .unwrap();
        self.peer.open(5, b"transaction", 2);
        // Stage the candidate now; its write reply is not a submission.
        assert_eq!(self.peer.write(5, &candidate).0, 119);
        let control = encode_lock_file_submit(LockFileSubmit {
            connection_epoch: self.epoch,
            submission_id: id,
            candidate_bytes: candidate.len() as u32,
        })
        .unwrap();
        (control.to_vec(), id)
    }

    /// Stages the prepared cancel's submit control without reading
    /// its reply, while the owner holds its turns. A write reply is
    /// never an event.
    fn stage_cancel(&mut self, control: &[u8]) -> u16 {
        self.peer.write_unanswered(3, control)
    }

    /// Consumes the staged submit's reply once the owner turns again.
    fn consume_cancel(&mut self, tag: u16, id: u64) {
        let reply = self.peer.read_reply(tag);
        assert_eq!(reply.0, 119, "submit write refused: {reply:?}");
        self.submission = id;
        self.peer.clunk(5);
    }
}

/// Disclosure, then EBUSY until the provider's Hello journals ChannelReady.
fn ready(files: &mut Files, provider: &mut Provider) -> OwnedFd {
    assert_eq!(files.negotiate(), 2);
    files.peer.open(6, b"image-channel", 0);
    let text = String::from_utf8(files.peer.read(6, 0)).unwrap();
    let discovery = LockImageDiscovery::decode(&text).unwrap();
    let epoch = files.epoch;
    assert_eq!(
        (discovery.connection_epoch, discovery.channel_generation),
        (epoch, epoch)
    );
    assert_eq!(discovery.formats_generation, 4);
    assert_eq!(files.image_begin(1), Some(16));
    let socket = provider.connect_image();
    hello_at(&socket, epoch, epoch);
    let (kind, body) = files.next();
    assert_eq!(kind, LockFileKind::ChannelReady);
    assert_eq!(
        LockChannelReady::decode(&body).unwrap(),
        LockChannelReady {
            channel_generation: epoch,
            granted_source_kinds: 1,
        }
    );
    socket
}

fn live_offers(transport: &LockFileTransport) -> usize {
    let Some(server) = &transport.server else {
        return 0;
    };
    let custody = server.export().files().custody();
    custody
        .image
        .as_ref()
        .map_or(0, |channel| channel.offers.len())
}

/// Actual worker handoff, from custody's own facts: one live offer
/// and no image deadline, because `image_deadline` excludes
/// Validating offers. The job count alone also counts reservations.
fn offer_is_validating(transport: &LockFileTransport) -> bool {
    let Some(server) = &transport.server else {
        return false;
    };
    let custody = server.export().files().custody();
    live_offers(transport) == 1 && custody.image_deadline().is_none()
}

fn journal_sequence(transport: &LockFileTransport) -> u64 {
    let server = transport.server.as_ref().expect("connected");
    server.export().files().custody().position().next_sequence
}

/// One owner turn with supplied scheduling: the phases of
/// `LockFileTransport::turn`, except the staged wire request is
/// dispatched BEFORE the image service and the worker serve. After
/// the dispatch assertion, collection waits bounded on the worker's
/// own readiness and serves until the real completion lands; its
/// arrival is never inferred from a delay. This is the test acting
/// as owner; it proves nothing about production turn-order timing.
fn dispatch_then_serve(transport: &mut LockFileTransport) {
    let before = journal_sequence(transport);
    {
        let server = transport.server.as_mut().expect("connected");
        let now = Instant::now();
        if server.export_mut().poll(now).is_err() {
            server.export_mut().files_mut().revoke();
        }
        server.export_mut().files_mut().expire(now);
        if server.export().files().custody().is_negotiated() {
            transport.negotiation_deadline = None;
        } else if transport.negotiation_deadline.is_none() && server.export().admitted() {
            transport.negotiation_deadline =
                Some(now + Duration::from_millis(transport.limits.assembly_timeout_ms.into()));
        }
        if transport
            .negotiation_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            server.export_mut().files_mut().revoke();
        }
        // Dispatch: custody journals Submitted and the cancel stop.
        server.turn(Some(Duration::ZERO)).unwrap();
    }
    // Submitted precedes any worker collection: exactly one record
    // journaled, and the offer is still live.
    assert_eq!(
        journal_sequence(transport),
        before + 1,
        "only Submitted journals at dispatch"
    );
    assert_eq!(live_offers(transport), 1, "offer live before collection");
    // Collection: the completion may not be available on the first
    // serve (the worker thread schedules independently), so serve in
    // bounded turns gated on the worker's readiness fd until it
    // lands. The final position and drained job count are required,
    // not inferred from elapsed time.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut journalled;
        {
            let server = transport.server.as_mut().expect("connected");
            let now = Instant::now();
            journalled = service_image(
                &mut transport.image,
                &mut transport.image_bound,
                &transport.endpoint,
                server.export_mut(),
                transport.worker.as_mut(),
                now,
            );
            let custody = server.export_mut().files_mut().custody_mut();
            transport.orphans.poll();
            journalled |= custody.collect_isb_leases().unwrap_or(false);
            if let Some(worker) = &mut transport.worker {
                journalled |= worker.serve(custody, now);
            }
            if journalled {
                server.wake().wake();
            }
        }
        if journal_sequence(transport) == before + 3 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "worker completion never collected"
        );
        if let Some(fd) = transport
            .worker
            .as_ref()
            .and_then(|worker| worker.readiness())
        {
            let mut fds = [rustix::event::PollFd::new(
                &fd,
                rustix::event::PollFlags::IN,
            )];
            let timeout = rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 100_000_000,
            };
            let _ = rustix::event::poll(&mut fds, Some(&timeout));
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    assert_eq!(
        journal_sequence(transport),
        before + 3,
        "the real completion settles after the journaled cancel"
    );
    assert_eq!(transport.image_jobs_pending(), 0, "worker drained");
}

#[test]
fn cancel_while_validating_settles_on_the_real_completion() {
    let (mut transport, directory) = transport("cancel-worker");
    transport.enable_isb_images(policy(2000)).unwrap();
    let (mut provider, stream) = Provider::start(&mut transport, &directory);
    let source_sent = Arc::new(AtomicBool::new(false));
    let job_held = Arc::new(AtomicBool::new(false));
    let cancel_staged = Arc::new(AtomicBool::new(false));
    let (sent_flag, held_flag, staged_flag) =
        (source_sent.clone(), job_held.clone(), cancel_staged.clone());
    let peer = std::thread::spawn(move || {
        let mut files = Files::attach(stream);
        let socket = ready(&mut files, &mut provider);
        files.admitted(1);
        // Pre-stage the cancel candidate on its transaction fid; only
        // the submit control, staged later, submits it.
        let (control, id) = files.prepare_cancel(1);
        send_source(&socket, EPOCH, 1, &sealed_source());
        sent_flag.store(true, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !held_flag.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "owner never held the job");
            std::thread::sleep(Duration::from_millis(1));
        }
        let tag = files.stage_cancel(&control);
        staged_flag.store(true, Ordering::Release);
        files.consume_cancel(tag, id);
        files.cancelled(1);
        drop(socket);
        provider
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut held = false;
    let mut dispatched = false;
    while !peer.is_finished() {
        assert!(Instant::now() < deadline, "validating cancel never settled");
        if transport.export().is_none() {
            let _ = transport.poll_accept(&locked(), &[]);
        } else if held && !cancel_staged.load(Ordering::Acquire) {
            // The held turn: no service, no collection, while the
            // peer stages the submit frame.
            std::thread::sleep(Duration::from_millis(1));
            continue;
        } else if held && !dispatched {
            dispatch_then_serve(&mut transport);
            dispatched = true;
        } else {
            let _ = transport.turn();
        }
        if !held
            && source_sent.load(Ordering::Acquire)
            && transport.image_jobs_pending() == 1
            && offer_is_validating(&transport)
        {
            job_held.store(true, Ordering::Release);
            held = true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let provider = peer.join().unwrap();
    assert!(held, "owner never reached the held point");
    assert!(dispatched, "owner never dispatched before serve");
    assert_eq!(transport.image_jobs_pending(), 0);
    drop(provider);
}

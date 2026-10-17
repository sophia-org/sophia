//! Slice 3 over real sockets. This test binary re-executes itself as the
//! supervised provider: the child connects the lock socket and, on request,
//! the image endpoint, so both carry its credentials, and relays each
//! descriptor back. The test then speaks for the provider on them. Afid
//! verdicts come from the supplied test authority, not launch evidence.
use std::io::{Read, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sophia_protocol::lock_files::*;
use sophia_runtime::lock_files::*;
use sophia_runtime::{
    ProcessLaunchSpec, ProcessSupervisor, SupervisedProcessKind, SupervisorCommand,
};

use super::{authority, raw};

const EPOCH: u64 = 7;
const RELAY: &str = "SOPHIA_TEST_LOCK_IMAGE_RELAY";
const LOCK: &str = "SOPHIA_TEST_LOCK_IMAGE_SOCKET";
const IMAGE: &str = "SOPHIA_TEST_LOCK_IMAGE_ENDPOINT";
const EBUSY: u32 = 16;

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
    // The receive itself does not block; wait for the child's message.
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

/// The supervised provider. Inert unless launched by `Provider::start`.
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

pub struct Provider {
    supervisor: ProcessSupervisor,
    relay: UnixStream,
}

impl Provider {
    fn start(transport: &mut LockFileTransport, directory: &Path) -> (Self, UnixStream) {
        let relay_path = directory.with_extension("relay");
        let _ = std::fs::remove_file(&relay_path);
        let listener = UnixListener::bind(&relay_path).unwrap();
        let spec = ProcessLaunchSpec::new(std::env::current_exe().unwrap())
            .arg("wire::child")
            .arg("--exact")
            .arg("--test-threads=1")
            .arg("-q")
            .env(RELAY, &relay_path)
            .env(LOCK, transport.socket_path())
            .env(IMAGE, directory.join("image.sock"));
        let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::LockProvider, spec);
        supervisor
            .apply(SupervisorCommand::StartProcess {
                process: SupervisedProcessKind::LockProvider,
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
        validation_timeout_ms: 1000,
        validation_chunk_bytes: 65536,
        capture_interest_timeout_ms: 500,
        max_sealed_backing_bytes: 2048,
    }
}

fn transport(label: &str) -> (LockFileTransport, PathBuf) {
    let directory =
        std::env::temp_dir().join(format!("lock-image-wire-{label}-{}", std::process::id()));
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

/// Pumps the transport until `peer` finishes.
fn pump<T>(transport: &mut LockFileTransport, peer: std::thread::JoinHandle<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !peer.is_finished() {
        assert!(Instant::now() < deadline, "peer did not finish");
        if transport.export().is_none() {
            let _ = transport.poll_accept(&locked(), &[]);
        } else {
            let _ = transport.turn();
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    peer.join().unwrap()
}

/// The provider's side of the lock files, at the selected revision.
struct Files {
    peer: raw::Peer,
    offset: u64,
    queued: std::collections::VecDeque<(LockFileKind, u64, Vec<u8>)>,
    submission: u64,
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
        }
    }

    /// Submits under the next ID. Returns the submit reply's errno, if any;
    /// a refused submission keeps its ID.
    fn submit(&mut self, kind: LockFileKind, body: &[u8], revision: u16) -> Option<u32> {
        let id = self.submission + 1;
        let candidate = encode_lock_file_record_at_revision(
            LockFileHeader {
                kind,
                connection_epoch: EPOCH,
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
            connection_epoch: EPOCH,
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
        // The lock object in force follows the selection.
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
            connection_epoch: EPOCH,
            sequence,
        })
        .unwrap();
        assert_eq!(self.peer.write(4, &ack).0, 119);
        (kind, body)
    }

    fn image_begin(&mut self, offer: u64) -> Option<u32> {
        let metadata = LockImageMetadata {
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
        };
        self.submit(LockFileKind::ImageBegin, &metadata.encode().unwrap(), 2)
    }

    /// An offer the ready channel took: its receipt only. This transport
    /// does not admit offers yet, so it stays pending.
    fn pending(&mut self, offer: u64) {
        assert_eq!(self.image_begin(offer), None);
        assert_eq!(self.next().0, LockFileKind::Submitted);
    }

    /// An offer beyond `max_offers` (one, from one upload slot): its
    /// receipt, then Rejected with reason budget.
    fn over_capacity(&mut self, offer: u64) {
        assert_eq!(self.image_begin(offer), None);
        assert_eq!(self.next().0, LockFileKind::Submitted);
        let (kind, body) = self.next();
        assert_eq!(kind, LockFileKind::ResourceStatus);
        let status = LockResourceStatus::decode(&body).unwrap();
        assert_eq!(
            (status.transaction, status.status, status.reason),
            (offer, LockResourceState::Rejected, 6)
        );
    }

    /// A refused offer: its receipt, then Rejected with reason 9.
    fn refused(&mut self, offer: u64) {
        assert_eq!(self.image_begin(offer), None);
        assert_eq!(self.next().0, LockFileKind::Submitted);
        self.unavailable(offer);
    }

    /// Offer `offer` rejected with reason 9.
    fn unavailable(&mut self, offer: u64) {
        let (kind, body) = self.next();
        assert_eq!(kind, LockFileKind::ResourceStatus);
        let status = LockResourceStatus::decode(&body).unwrap();
        assert_eq!(status.transaction, offer);
        assert_eq!(status.status, LockResourceState::Rejected);
        assert_eq!(
            status.reason,
            LOCK_RESOURCE_REASON_IMAGE_CHANNEL_UNAVAILABLE
        );
    }

    fn closed(&mut self, reason: LockChannelClosedReason) {
        let (kind, body) = self.next();
        assert_eq!(kind, LockFileKind::ChannelClosed);
        assert_eq!(
            LockChannelClosed::decode(&body).unwrap(),
            LockChannelClosed {
                channel_generation: EPOCH,
                reason,
            }
        );
    }
}

fn hello(socket: &OwnedFd, generation: u64) {
    let bytes = LockImagePacket::Hello {
        connection_epoch: EPOCH,
        channel_generation: generation,
    }
    .encode()
    .unwrap();
    rustix::net::send(socket, &bytes, rustix::net::SendFlags::NOSIGNAL).unwrap();
}

/// Disclosure, then EBUSY until the provider's Hello journals ChannelReady.
fn ready(files: &mut Files, provider: &mut Provider) -> OwnedFd {
    assert_eq!(files.negotiate(), 2);
    files.peer.open(6, b"image-channel", 0);
    let text = String::from_utf8(files.peer.read(6, 0)).unwrap();
    let discovery = LockImageDiscovery::decode(&text).unwrap();
    assert_eq!(
        (discovery.connection_epoch, discovery.channel_generation),
        (EPOCH, EPOCH)
    );
    assert_eq!(discovery.formats_generation, 4);
    assert_eq!(files.image_begin(1), Some(EBUSY));
    let socket = provider.connect_image();
    hello(&socket, EPOCH);
    let (kind, body) = files.next();
    assert_eq!(kind, LockFileKind::ChannelReady);
    assert_eq!(
        LockChannelReady::decode(&body).unwrap(),
        LockChannelReady {
            channel_generation: EPOCH,
            granted_source_kinds: 1,
        }
    );
    socket
}

#[derive(Clone, Copy, Debug)]
enum Ending {
    StrayPacket,
    PeerDisconnect,
    ProviderExit,
}

fn run_ready(label: &str, ending: Ending) {
    let (mut transport, directory) = transport(label);
    transport.enable_sealed_images(policy(2000)).unwrap();
    let (mut provider, stream) = Provider::start(&mut transport, &directory);
    let (files, socket, provider) = pump(
        &mut transport,
        std::thread::spawn(move || {
            let mut files = Files::attach(stream);
            let socket = ready(&mut files, &mut provider);
            files.pending(1);
            // A second connector is dropped; the serving channel stays.
            let duplicate = provider.connect_image();
            let mut buffer = [0u8; 8];
            let read = rustix::net::recv(&duplicate, &mut buffer, rustix::net::RecvFlags::empty());
            assert!(
                matches!(read, Ok((0, _)) | Err(_)),
                "duplicate was not refused"
            );
            files.over_capacity(2);
            (files, socket, provider)
        }),
    );
    let mut provider = provider;
    let reason = match ending {
        Ending::StrayPacket => {
            hello(&socket, EPOCH);
            LockChannelClosedReason::ProtocolFault
        }
        Ending::PeerDisconnect => {
            drop(socket);
            LockChannelClosedReason::PeerDisconnect
        }
        Ending::ProviderExit => {
            provider.exit();
            LockChannelClosedReason::GrantRevoked
        }
    };
    pump(
        &mut transport,
        std::thread::spawn(move || {
            let mut files = files;
            files.closed(reason);
            // Channel loss refuses the pending offer with it.
            files.unavailable(1);
            // Closed for the epoch: offers are still refused cleanly.
            files.refused(3);
        }),
    );
    transport.disconnect().unwrap();
    assert!(!directory.join("image.sock").exists());
    drop(provider);
}

#[test]
fn a_ready_channel_closes_on_a_stray_packet() {
    run_ready("stray", Ending::StrayPacket);
}

#[test]
fn a_ready_channel_closes_when_its_socket_closes() {
    run_ready("disconnect", Ending::PeerDisconnect);
}

#[test]
fn a_ready_channel_closes_when_the_provider_exits() {
    run_ready("exit", Ending::ProviderExit);
}

fn run_failed_hello(label: &str, bad_generation: bool) {
    let (mut transport, directory) = transport(label);
    transport.enable_sealed_images(policy(100)).unwrap();
    let (mut provider, stream) = Provider::start(&mut transport, &directory);
    pump(
        &mut transport,
        std::thread::spawn(move || {
            let mut files = Files::attach(stream);
            assert_eq!(files.negotiate(), 2);
            let socket = provider.connect_image();
            if bad_generation {
                hello(&socket, EPOCH + 1);
            }
            files.closed(if bad_generation {
                LockChannelClosedReason::ProtocolFault
            } else {
                LockChannelClosedReason::HelloTimeout
            });
            // No ChannelReady preceded it, and offers are refused, not busy.
            files.refused(1);
            drop(socket);
            provider
        }),
    );
}

#[test]
fn a_wrong_hello_closes_without_ready() {
    run_failed_hello("bad-hello", true);
}

#[test]
fn a_missing_hello_times_out() {
    run_failed_hello("no-hello", false);
}

#[test]
fn a_legacy_connection_never_gets_a_channel() {
    let (mut transport, directory) = transport("legacy");
    transport.enable_sealed_images(policy(2000)).unwrap();
    transport
        .authorize_supervised_pid(std::process::id())
        .unwrap();
    let path = transport.socket_path().to_owned();
    pump(
        &mut transport,
        std::thread::spawn(move || {
            let mut peer = raw::Peer::connect(&path);
            peer.setup();
            let mut files = Files {
                peer,
                offset: 0,
                queued: Default::default(),
                submission: 0,
            };
            assert_eq!(files.negotiate(), 1);
            let walk = files.peer.rpc(
                110,
                &[
                    1u32.to_le_bytes().as_slice(),
                    &6u32.to_le_bytes(),
                    &1u16.to_le_bytes(),
                    &13u16.to_le_bytes(),
                    b"image-channel",
                ]
                .concat(),
            );
            assert_eq!(walk.unwrap().0, 7, "image-channel is absent");
        }),
    );
    assert!(!directory.join("image.sock").exists());
}

//! Real child SQ publications through the authenticated adapter. The harness
//! owns 9P setup/control; supplied verdicts do not bind the child to a real grant.
use super::super::process::fixture as process;
use super::*;
use rustix::{
    event::Timespec,
    process::{Pid, PidfdFlags, pidfd_open},
};
use sophia_control_ring_probe::wake::Ready;

const NORMAL: u8 = 0;
const WITHDRAW: u8 = 1;
const EXIT_PENDING: u8 = 2;

#[test]
fn authenticated_child_receives_each_receipt_before_reusing_its_cell() {
    run(NORMAL);
}
#[test]
fn withdrawal_with_pending_receipt_leaves_child_unknown_and_owner_work_retained() {
    run(WITHDRAW);
}
#[test]
fn child_exit_with_pending_receipt_closes_carrier_without_undoing_owner_work() {
    run(EXIT_PENDING);
}
fn run(mode: u8) {
    let mut fixture = Fixture::new();
    let child_name = format!("{}::peer", module_path!().split_once("::").unwrap().1);
    let mut peer = process::spawn(&child_name);
    let pidfd = pidfd_open(
        Pid::from_raw(peer.child.0.id() as i32).unwrap(),
        PidfdFlags::empty(),
    )
    .unwrap();
    let (requests, peer_writer) = wake::pipe().unwrap();
    let (peer_reader, response_writer) = wake::pipe().unwrap();
    process::send_descriptors(&peer.stream, &[peer_writer.as_fd(), peer_reader.as_fd()], 1);
    drop(peer_writer);
    drop(peer_reader);
    peer.stream.write_all(&[mode]).unwrap();
    peer.stream
        .write_all(&fixture.binding.epoch.0.to_le_bytes())
        .unwrap();
    peer.stream
        .write_all(&fixture.binding.writer.to_le_bytes())
        .unwrap();
    process::readable(&peer.stream);
    let mut marker = [0];
    let mut rights = sophia_linux_peer::receive_rights(peer.stream.as_fd(), &mut marker).unwrap();
    assert_eq!((rights.bytes, rights.count, marker), (1, 1, [2]));
    let source = Source::admit(File::from(rights.descriptors[0].take().unwrap())).unwrap();
    let mut slot = Slot::default();
    let mut captured = Captured::buffer();
    let mut responses = Some(response_writer);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut duplicates = 0;
    while slot.last_sequence() < 3 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero());
        assert_eq!(
            wake::wait(
                requests.as_fd(),
                pidfd.as_fd(),
                Timespec {
                    tv_sec: remaining.as_secs() as i64,
                    tv_nsec: remaining.subsec_nanos() as i64,
                }
            )
            .unwrap(),
            Ready::Work
        );
        let Some(hint) = requests.next(slot.last_sequence()).unwrap() else {
            duplicates += 1;
            continue;
        };
        slot.capture_published(hint, &source, &mut captured, ReadPlan::HeaderThenPayload)
            .unwrap();
        fixture
            .binding
            .process(&mut fixture.wire, &mut slot, &captured)
            .unwrap();
        assert!(slot.pending());
        if hint == 3 && mode == EXIT_PENDING {
            peer.stream.write_all(b"E").unwrap();
            assert!(peer.child.0.wait().unwrap().success());
            assert_eq!(
                wake::wait(
                    requests.as_fd(),
                    pidfd.as_fd(),
                    Timespec {
                        tv_sec: 0,
                        tv_nsec: 0
                    }
                )
                .unwrap(),
                Ready::Exited
            );
            slot.close();
            drop(responses.take());
            break;
        }
        if hint == 3 && mode == WITHDRAW {
            fixture.checks.lock().unwrap().deny = true;
            assert_eq!(
                fixture
                    .binding
                    .deliver(&mut fixture.wire, &mut slot, responses.as_ref().unwrap()),
                Err(Errno::EACCES)
            );
            assert!(slot.closed());
            drop(responses.take());
        } else {
            fixture
                .binding
                .deliver(&mut fixture.wire, &mut slot, responses.as_ref().unwrap())
                .unwrap();
        }
    }
    assert_eq!(duplicates, 2, "duplicate hints do not capture or dispatch");
    assert_eq!(fixture.calls(), if mode == EXIT_PENDING { 5 } else { 6 });
    assert!(!slot.pending());
    if mode != EXIT_PENDING {
        peer.stream.read_exact(&mut marker).unwrap();
        assert_eq!(marker, if mode == NORMAL { *b"K" } else { *b"U" });
        peer.stream.write_all(b"Q").unwrap();
        assert!(peer.child.0.wait().unwrap().success());
    }
    // The carrier close owns no rollback of accepted export work.
    slot.close();
    drop(responses);
    drop(source);
    assert!(slot.closed());
    let mut control = Fixture::new();
    for id in 20..23 {
        let bytes = commit(id, 0);
        assert_eq!(control.write(&bytes), Reply::Write(bytes.len() as u32));
    }
    equal(control.wire.export(), fixture.wire.export());
    assert_eq!(fixture.wire.export().inbound.len(), 3);
    assert_eq!(
        control.wire.export_mut().take_inbound(),
        fixture.wire.export_mut().take_inbound()
    );
}

#[test]
#[ignore = "child explicitly invoked by the authenticated persistent parent tests"]
fn peer() {
    let mut stream = process::connect();
    process::readable(&stream);
    let mut marker = [0];
    let mut rights = sophia_linux_peer::receive_rights(stream.as_fd(), &mut marker).unwrap();
    assert_eq!((rights.bytes, rights.count, marker), (1, 2, [1]));
    let requests = Writer::from_owner(rights.descriptors[0].take().unwrap()).unwrap();
    let responses = Reader::from_owner(rights.descriptors[1].take().unwrap()).unwrap();
    stream.read_exact(&mut marker).unwrap();
    let mode = marker[0];
    assert!(mode <= EXIT_PENDING);
    let mut word = [0; 8];
    stream.read_exact(&mut word).unwrap();
    let epoch = u64::from_le_bytes(word);
    stream.read_exact(&mut word).unwrap();
    let writer = u64::from_le_bytes(word);
    let parent = pidfd_open(rustix::process::getppid().unwrap(), PidfdFlags::empty()).unwrap();
    let mut producer = ProducerRing::create(EXTENT).unwrap();
    process::send_descriptors(&stream, &[producer.source_fd()], 2);
    let mut client = Client::default();
    for sequence in 1..=3 {
        if sequence > 1 {
            requests.notify(sequence - 1).unwrap();
        }
        let bytes = commit(sequence + 19, 0);
        let identity = Identity {
            sequence,
            epoch,
            writer,
            submission: sequence + 19,
            ack: 0,
            bytes: bytes.len() as u64,
        };
        producer.bytes_mut()[..HEADER].copy_from_slice(&identity.header());
        producer.bytes_mut()[HEADER..HEADER + bytes.len()].copy_from_slice(&bytes);
        client.begin(identity).unwrap();
        client.publish(&requests).unwrap();
        if sequence == 3 && mode == EXIT_PENDING {
            stream.read_exact(&mut marker).unwrap();
            assert_eq!(marker, *b"E");
            client.disconnect();
            assert!(client.unknown && client.closed);
            assert_eq!(client.known, Some(21));
            return;
        }
        let ready = wake::wait(
            responses.as_fd(),
            parent.as_fd(),
            Timespec {
                tv_sec: 10,
                tv_nsec: 0,
            },
        );
        if sequence == 3 && mode == WITHDRAW {
            assert!(
                ready.is_err(),
                "withdrawal closes receipt writer instead of delivering"
            );
            assert!(responses.receipt().is_err());
            client.disconnect();
            assert!(client.unknown && client.closed);
            assert_eq!(client.known, Some(21));
            stream.write_all(b"U").unwrap();
        } else {
            assert_eq!(ready.unwrap(), Ready::Work);
            let receipt = responses.receipt().unwrap().unwrap();
            assert_eq!(
                client.observe(receipt).unwrap(),
                Outcome::Accepted { applied_ack: 0 }
            );
            assert_eq!(client.known, Some(sequence + 19));
        }
    }
    if mode == NORMAL {
        stream.write_all(b"K").unwrap();
    }
    stream.read_exact(&mut marker).unwrap();
    assert_eq!(marker, *b"Q");
}

//! Persistent raw carrier with the real export; direct calls are the semantic
//! control here, not the future socket-framed performance control.
use super::*;
use rustix::{
    event::Timespec,
    process::{Pid, PidfdFlags, pidfd_open},
};
use sophia_control_ring_probe::wake::{self, Reader, Ready, Writer};
use std::io::{Read, Write};
use std::os::fd::AsFd;
#[path = "../../../../tools/probes/sft_control_ring/tests/support/process.rs"]
mod fixture;

#[test]
fn persistent_process_uses_real_export_and_ignores_duplicate_hints_before_capture() {
    let child_name = format!("{}::peer", module_path!().split_once("::").unwrap().1);
    let mut peer = fixture::spawn(&child_name);
    let pid = Pid::from_raw(peer.child.0.id() as i32).unwrap();
    let pidfd = pidfd_open(pid, PidfdFlags::empty()).unwrap();
    let (requests, peer_writer) = wake::pipe().unwrap();
    let (peer_reader, responses) = wake::pipe().unwrap();
    fixture::send_descriptors(&peer.stream, &[peer_writer.as_fd(), peer_reader.as_fd()], 1);
    drop(peer_writer);
    drop(peer_reader);
    fixture::readable(&peer.stream);
    let mut marker = [0];
    let mut rights = sophia_linux_peer::receive_rights(peer.stream.as_fd(), &mut marker).unwrap();
    assert_eq!((rights.bytes, rights.count, marker), (1, 1, [2]));
    let source = Source::admit(File::from(rights.descriptors[0].take().unwrap())).unwrap();
    let mut files = lean_export();
    let writer = open_writer(&mut files);
    // Give the peer its actual fixture writer binding before publications begin.
    peer.stream.write_all(&writer.to_le_bytes()).unwrap();
    let mut slot = Slot::default();
    let mut captured = Captured::buffer();
    let mut idle = 0;
    let deadline = Instant::now() + Duration::from_secs(10);
    while slot.last_sequence() < 3 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert_eq!(
            wake::wait(
                requests.as_fd(),
                pidfd.as_fd(),
                Timespec {
                    tv_sec: remaining.as_secs() as i64,
                    tv_nsec: remaining.subsec_nanos() as i64
                }
            )
            .unwrap(),
            Ready::Work
        );
        let Some(hint) = requests.next(slot.last_sequence()).unwrap() else {
            idle += 1;
            continue;
        };
        slot.capture_published(hint, &source, &mut captured, ReadPlan::HeaderThenPayload)
            .unwrap();
        let writer = captured.identity.writer;
        slot.process(&captured, |record| {
            files.read_reusable_transaction(writer, 0, 0)?;
            files.write(&Node::FrameSubmit, &mut Handle::Plain, 0, record)
        })
        .unwrap();
        responses.receipt(slot.publish(true).unwrap()).unwrap();
    }
    assert_eq!(idle, 2);
    peer.stream.write_all(b"Q").unwrap();
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
    let mut direct = lean_export();
    open_writer(&mut direct);
    for id in 20..23 {
        control(&mut direct, &commit(id, 0)).unwrap();
    }
    equal(&direct, &files);
    assert_eq!(direct.take_inbound(), files.take_inbound());
}

#[test]
#[ignore = "child explicitly invoked by persistent_process test"]
fn peer() {
    let mut stream = fixture::connect();
    fixture::readable(&stream);
    let mut marker = [0];
    let mut rights = sophia_linux_peer::receive_rights(stream.as_fd(), &mut marker).unwrap();
    assert_eq!((rights.bytes, rights.count, marker), (1, 2, [1]));
    let requests = Writer::from_owner(rights.descriptors[0].take().unwrap()).unwrap();
    let responses = Reader::from_owner(rights.descriptors[1].take().unwrap()).unwrap();
    let parent = pidfd_open(rustix::process::getppid().unwrap(), PidfdFlags::empty()).unwrap();
    let mut producer = ProducerRing::create(EXTENT).unwrap();
    fixture::send_descriptors(&stream, &[producer.source_fd()], 2);
    let mut word = [0; 8];
    stream.read_exact(&mut word).unwrap();
    let writer = u64::from_le_bytes(word);
    let mut client = Client::default();
    for sequence in 1..=3 {
        if sequence > 1 {
            requests.notify(sequence - 1).unwrap();
        }
        let bytes = commit(sequence + 19, 0);
        let identity = Identity {
            sequence,
            epoch: 1,
            writer,
            submission: sequence + 19,
            ack: 0,
            bytes: bytes.len() as u64,
        };
        producer.bytes_mut()[..HEADER].copy_from_slice(&identity.header());
        producer.bytes_mut()[HEADER..HEADER + bytes.len()].copy_from_slice(&bytes);
        client.begin(identity).unwrap();
        requests.notify(sequence).unwrap();
        assert_eq!(
            wake::wait(
                responses.as_fd(),
                parent.as_fd(),
                Timespec {
                    tv_sec: 10,
                    tv_nsec: 0
                }
            )
            .unwrap(),
            Ready::Work
        );
        let receipt = responses.receipt().unwrap().unwrap();
        assert_eq!(
            client.observe(receipt).unwrap(),
            Outcome::Accepted { applied_ack: 0 }
        );
        assert_eq!(client.known, Some(sequence + 19));
        // Only now may the next iteration rewrite the SQ cell.
    }
    stream.read_exact(&mut marker).unwrap();
    assert_eq!(marker, *b"Q");
}

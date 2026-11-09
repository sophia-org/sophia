#[path = "support/process.rs"]
mod process;
use process::send_descriptors;
use sophia_9p::Errno;
use sophia_control_ring_probe::{capture::*, custody::*};
use sophia_linux_peer::ProducerRing;
use sophia_protocol::shell_files::*;
use std::{fs::File, io, os::fd::AsFd};

fn request(seq: u64, id: u64, ack: u64) -> Captured {
    let line = include_str!(
        "../../../../vendor/rust-desktop-sdk/source/spec/golden/sophia-frame-transactions-v1.tsv"
    )
    .lines()
    .find(|l| l.starts_with("commit-retained\t"))
    .unwrap();
    let hex = line.split('\t').nth(2).unwrap();
    let original: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    let mut header = decode_shell_file_record_at_revision(&original, ShellFileClass::Candidate, 13)
        .unwrap()
        .header;
    let value = decode_shell_frame_record(&original, ShellFileClass::Candidate, 13, 8192).unwrap();
    header.submission_id = id;
    let mut bytes = encode_shell_frame_record(header, &value, 13).unwrap();
    bytes[24..32].copy_from_slice(&ack.to_le_bytes());
    Captured {
        identity: Identity {
            sequence: seq,
            epoch: header.connection_epoch,
            writer: 1,
            submission: id,
            ack,
            bytes: bytes.len() as u64,
        },
        bytes,
    }
}

fn stage(r: &Captured) -> ProducerRing {
    let mut p = ProducerRing::create(EXTENT).unwrap();
    p.bytes_mut()[..HEADER].copy_from_slice(&r.identity.header());
    p.bytes_mut()[HEADER..HEADER + r.bytes.len()].copy_from_slice(&r.bytes);
    p
}
#[test]
fn private_capture_survives_source_mutation_and_external_writes_are_sealed() {
    let r = request(1, 10, 0);
    let mut p = stage(&r);
    let fd = p.source_fd().try_clone_to_owned().unwrap();
    let file = File::from(fd);
    use std::os::unix::fs::FileExt;
    assert!(file.write_at(&[0], 0).is_err());
    assert!(file.set_len(0).is_err());
    let s = Source::admit(file).unwrap();
    let c = s.capture(1).unwrap();
    p.bytes_mut().fill(0);
    assert_eq!(c.bytes, r.bytes);
    assert_eq!(c.identity, r.identity);
    assert!(s.capture(1).is_err());
}
#[test]
fn size_seals_and_exact_geometry_required() {
    let fd = rustix::fs::memfd_create(c"test", rustix::fs::MemfdFlags::ALLOW_SEALING).unwrap();
    let f = File::from(fd);
    f.set_len(EXTENT as u64).unwrap();
    assert!(Source::admit(f).is_err());
    let p = ProducerRing::create(EXTENT + 1).unwrap();
    assert!(Source::admit(File::from(p.source_fd().try_clone_to_owned().unwrap())).is_err());
}
#[test]
fn huge_length_is_rejected_before_payload_io_and_sequence_exhaustion_refuses() {
    let mut id = request(1, 10, 0).identity;
    id.bytes = MAX_RECORD as u64 + 1;
    let mut reads = 0;
    assert!(
        capture_with(1, |buf, off| {
            reads += 1;
            assert_eq!(off, 0);
            buf.copy_from_slice(&id.header());
            Ok(buf.len())
        })
        .is_err()
    );
    assert_eq!(reads, 1);
    id.bytes = 32;
    id.sequence = u64::MAX;
    assert!(
        capture_with(u64::MAX, |buf, _| {
            buf.copy_from_slice(&id.header());
            Ok(buf.len())
        })
        .is_err()
    );
}
#[test]
fn short_and_io_errors_close_while_interrupt_retry_is_bounded() {
    let r = request(1, 10, 0);
    for error in [
        None,
        Some(io::ErrorKind::Interrupted),
        Some(io::ErrorKind::Other),
    ] {
        let mut calls = 0;
        assert!(
            capture_with(1, |buf, off| {
                calls += 1;
                if off == 0 {
                    buf.copy_from_slice(&r.identity.header());
                    return Ok(buf.len());
                }
                match error {
                    Some(e) => Err(io::Error::new(e, "injected")),
                    None => Ok(buf.len() - 1),
                }
            })
            .is_err()
        );
        assert_eq!(
            calls,
            if error == Some(io::ErrorKind::Interrupted) {
                3
            } else {
                2
            }
        );
    }
}

#[test]
fn full_receipt_prevents_even_ack_effect() {
    let mut o = Slot::default();
    o.process(&request(1, 10, 0), |bytes| Ok(bytes.len() as u32))
        .unwrap();
    let mut applied = false;
    let result = o.process(&request(2, 11, 1), |bytes| {
        applied = true;
        Ok(bytes.len() as u32)
    });
    assert!(!applied, "full receipt slot must prevent ACK effects");
    assert_eq!(result, Err(EBUSY));
    assert_eq!(o.last_sequence(), 1);
}

#[test]
fn wrong_receipt_identity_or_ack_word_does_not_establish_knowledge() {
    for field in 0..8 {
        let mut c = Client::default();
        let a = request(1, 10, 1);
        c.begin(a.identity).unwrap();
        let mut r = Receipt {
            identity: a.identity,
            outcome: Outcome::Accepted { applied_ack: 1 },
        };
        match field {
            0 => r.identity.epoch += 1,
            1 => r.identity.writer += 1,
            2 => r.identity.sequence += 1,
            3 => r.identity.submission += 1,
            4 => r.identity.bytes += 1,
            5 => r.identity.ack += 1,
            6 => r.outcome = Outcome::Accepted { applied_ack: 2 },
            _ => r.outcome = Outcome::Accepted { applied_ack: 0 },
        }
        assert!(c.observe(r).is_err());
        assert_eq!(c.known, None);
        assert!(c.closed);
    }
}

#[test]
fn peer_exit_without_doorbell_wakes_one_poll_set() {
    use rustix::{
        event::{EventfdFlags, Timespec, eventfd},
        process::{Pid, PidfdFlags, pidfd_open},
    };
    let work = eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).unwrap();
    let mut child = std::process::Command::new("/bin/sleep")
        .arg("0.02")
        .spawn()
        .unwrap();
    let pid = Pid::from_raw(child.id() as i32).unwrap();
    let pidfd = pidfd_open(pid, PidfdFlags::empty()).unwrap();
    let r = sophia_control_ring_probe::wake::wait(
        work.as_fd(),
        pidfd.as_fd(),
        Timespec {
            tv_sec: 2,
            tv_nsec: 0,
        },
    )
    .unwrap();
    child.wait().unwrap();
    assert_eq!(r, sophia_control_ring_probe::wake::Ready::Exited);
}

#[test]
fn answered_refusal_does_not_become_unknown_on_disconnect() {
    let mut c = Client::default();
    let a = request(1, 10, 1);
    c.begin(a.identity).unwrap();
    c.observe(Receipt {
        identity: a.identity,
        outcome: Outcome::Refused(Errno::EAGAIN),
    })
    .unwrap();
    c.disconnect();
    assert!(!c.unknown);
    assert!(c.closed);
}
#[test]
fn consuming_before_wait_keeps_a_later_notification_readable() {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    let (work, sender) = sophia_control_ring_probe::wake::pipe().unwrap();
    assert_eq!(work.next(0).unwrap(), None);
    // Queue inspection found nothing; publication now follows the clear.
    sender.notify(1).unwrap();
    let mut fds = [PollFd::new(&work, PollFlags::IN)];
    assert_eq!(
        poll(
            &mut fds,
            Some(&Timespec {
                tv_sec: 0,
                tv_nsec: 0
            })
        )
        .unwrap(),
        1
    );
    assert!(fds[0].revents().contains(PollFlags::IN));
}

#[test]
fn independent_process_owns_mapping_and_parent_captures_privately() {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use std::io::{Read, Write};
    let mut peer = process::spawn("producer_process_fixture");
    let stream = &mut peer.stream;
    let mut fds = [PollFd::new(&stream, PollFlags::IN)];
    assert_eq!(
        poll(
            &mut fds,
            Some(&Timespec {
                tv_sec: 10,
                tv_nsec: 0
            })
        )
        .unwrap(),
        1
    );
    let mut byte = [0];
    let mut rights = sophia_linux_peer::receive_rights(stream.as_fd(), &mut byte).unwrap();
    assert_eq!(rights.bytes, 1);
    assert_eq!(rights.count, 1);
    assert_eq!(byte, [1]);
    let source = Source::admit(File::from(rights.descriptors[0].take().unwrap())).unwrap();
    let captured = source.capture(1).unwrap();
    assert_eq!(captured.bytes, request(1, 10, 0).bytes);
    stream.write_all(b"M").unwrap();
    stream.read_exact(&mut byte).unwrap();
    assert_eq!(byte, [2]);
    assert!(source.capture(1).is_err());
    assert_eq!(captured.bytes, request(1, 10, 0).bytes);
    // No host mapping: publish the exact receipt directly over an owner-created pipe.
    let mut o = Slot::default();
    o.process(&captured, |bytes| Ok(bytes.len() as u32))
        .unwrap();
    let (reader, sender) = sophia_control_ring_probe::wake::pipe().unwrap();
    send_descriptors(stream, &[reader.as_fd()], 3);
    drop(reader);
    sender.receipt(o.publish(true).unwrap()).unwrap();
    stream.read_exact(&mut byte).unwrap();
    assert_eq!(byte, [4]);
    stream.write_all(b"Q").unwrap();
    assert!(peer.child.0.wait().unwrap().success());
}
#[test]
#[ignore = "child fixture invoked by independent_process test"]
fn producer_process_fixture() {
    use std::io::{Read, Write};
    let mut stream = process::connect();
    let r = request(1, 10, 0);
    let mut p = stage(&r);
    let mut client = Client::default();
    client.begin(r.identity).unwrap();
    send_descriptors(&stream, &[p.source_fd()], 1);
    let mut byte = [0];
    stream.read_exact(&mut byte).unwrap();
    assert_eq!(&byte, b"M");
    p.bytes_mut().fill(0);
    stream.write_all(&[2]).unwrap();
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    let mut fds = [PollFd::new(&stream, PollFlags::IN)];
    let bound = Timespec {
        tv_sec: 10,
        tv_nsec: 0,
    };
    assert_eq!(poll(&mut fds, Some(&bound)).unwrap(), 1);
    let mut reply = sophia_linux_peer::receive_rights(stream.as_fd(), &mut byte).unwrap();
    assert_eq!((reply.bytes, reply.count, byte), (1, 1, [3]));
    let bell =
        sophia_control_ring_probe::wake::Reader::from_owner(reply.descriptors[0].take().unwrap())
            .unwrap();
    let parent = rustix::process::pidfd_open(
        rustix::process::getppid().unwrap(),
        rustix::process::PidfdFlags::empty(),
    )
    .unwrap();
    assert_eq!(
        sophia_control_ring_probe::wake::wait(bell.as_fd(), parent.as_fd(), bound).unwrap(),
        sophia_control_ring_probe::wake::Ready::Work
    );
    let receipt = bell.receipt().unwrap().unwrap();
    client.observe(receipt).unwrap();
    assert_eq!(client.known, Some(10));
    stream.write_all(&[4]).unwrap();
    stream.read_exact(&mut byte).unwrap();
    assert_eq!(&byte, b"Q");
}

#[test]
fn standalone_ack_and_submission_cannot_overlap_in_either_order() {
    let mut c = Client::default();
    c.begin_ack(7).unwrap();
    assert_eq!(c.begin(request(1, 10, 7).identity), Err(EBUSY));
    assert_eq!(c.begin_ack(8), Err(EBUSY));
    c.finish_ack(7).unwrap();
    assert_eq!(c.confirmed_ack, 7);
    c.begin(request(1, 10, 8).identity).unwrap();
    assert_eq!(c.begin_ack(8), Err(EBUSY));
    assert_eq!(c.confirmed_ack, 7);
    c.disconnect();
    assert!(c.unknown);
    assert!(c.begin_ack(8).is_err());
}

#[test]
fn malformed_or_short_header_never_reads_payload() {
    for kind in 0..9 {
        let mut header = request(1, 10, 0).identity.header();
        match kind {
            0 => header[0] ^= 1,
            1 => header[56] = 1,
            2 => header[8..16].fill(0),
            3 => header[16..24].fill(0),
            4 => header[24..32].fill(0),
            5 => header[32..40].fill(0),
            6 => header[48..56].fill(0),
            7 => header[8..16].copy_from_slice(&2u64.to_le_bytes()),
            _ => (),
        }
        let mut calls = 0;
        assert!(
            capture_with(1, |buf, offset| {
                calls += 1;
                assert_eq!(offset, 0);
                buf.copy_from_slice(&header);
                Ok(if kind == 8 { HEADER - 1 } else { HEADER })
            })
            .is_err()
        );
        assert_eq!(calls, 1);
    }
}

#[test]
fn snapshot_header_and_payload_mismatch_refuses_before_ack() {
    let header = request(1, 10, 1).identity;
    let changed = request(1, 11, 1);
    let captured = capture_with(1, |buf, offset| {
        if offset == 0 {
            buf.copy_from_slice(&header.header());
        } else {
            buf.copy_from_slice(&changed.bytes);
        }
        Ok(buf.len())
    })
    .unwrap();
    let mut o = Slot::default();
    o.process(&captured, |_| {
        panic!("mixed identity must refuse before ACK")
    })
    .unwrap_err();
    assert!(o.closed());
    assert!(!o.pending());
}

#[test]
fn closed_work_descriptor_is_not_work_readiness() {
    use rustix::event::{EventfdFlags, Timespec, eventfd};
    use std::os::unix::net::UnixStream;
    let (work, other) = UnixStream::pair().unwrap();
    let quiet = eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).unwrap();
    drop(other);
    assert!(
        sophia_control_ring_probe::wake::wait(
            work.as_fd(),
            quiet.as_fd(),
            Timespec {
                tv_sec: 0,
                tv_nsec: 0
            }
        )
        .is_err()
    );
}

#[test]
fn receipt_bytes_are_exact_and_refusals_cannot_echo_confirmable_ack() {
    for outcome in [
        Outcome::Accepted { applied_ack: 3 },
        Outcome::Refused(Errno::EAGAIN),
        Outcome::Refused(Errno::EINVAL),
        Outcome::Refused(Errno::ESTALE),
        Outcome::Refused(EBUSY),
        Outcome::Refused(EALREADY),
    ] {
        let receipt = Receipt {
            identity: request(1, 10, 3).identity,
            outcome,
        };
        let bytes = receipt.encode();
        assert_eq!(Receipt::decode(&bytes).unwrap(), receipt);
        assert!(Receipt::decode(&bytes[..bytes.len() - 1]).is_err());
        let mut bad = bytes;
        bad[HEADER..HEADER + 8].copy_from_slice(&4096u64.to_le_bytes());
        assert!(Receipt::decode(&bad).is_err());
        let mut bad = bytes;
        bad[HEADER + 8..].copy_from_slice(&99u64.to_le_bytes());
        assert!(Receipt::decode(&bad).is_err());
    }
}

#[test]
fn capture_reuses_capacity_and_distinguishes_idle_from_gaps() {
    for plan in [ReadPlan::HeaderThenPayload, ReadPlan::FullExtent] {
        let request = request(1, 10, 0);
        let mut producer = stage(&request);
        let source = Source::admit(File::from(
            producer.source_fd().try_clone_to_owned().unwrap(),
        ))
        .unwrap();
        let mut buffer = Captured::buffer();
        let pointer = buffer.bytes.as_ptr();
        let capacity = buffer.bytes.capacity();
        let mut slot = Slot::default();
        slot.capture_published(1, &source, &mut buffer, plan)
            .unwrap();
        assert_eq!(buffer.bytes, request.bytes);
        slot.process(&buffer, |bytes| Ok(bytes.len() as u32))
            .unwrap();
        slot.publish(true).unwrap();
        assert!(
            !source
                .scan(slot.last_sequence(), &mut buffer, plan)
                .unwrap()
        );
        assert_eq!(buffer.bytes.as_ptr(), pointer);
        assert_eq!(buffer.bytes.capacity(), capacity);
        let mut identity = request.identity;
        identity.sequence = 3;
        producer.bytes_mut()[..HEADER].copy_from_slice(&identity.header());
        assert!(
            slot.capture_published(2, &source, &mut buffer, plan)
                .is_err()
        );
        assert!(slot.closed());
        assert!(!slot.pending());
    }
}

#[test]
fn opposite_pipe_end_cannot_make_owner_clear_block_or_consume_readiness() {
    use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
    let (reader, writer) = sophia_control_ring_probe::wake::pipe().unwrap();
    let flags = fcntl_getfl(&writer).unwrap();
    fcntl_setfl(&writer, flags & !OFlags::NONBLOCK).unwrap();
    assert!(fcntl_getfl(&reader).unwrap().contains(OFlags::NONBLOCK));
    assert_eq!(reader.next(0).unwrap(), None);
    writer.notify(1).unwrap();
    let mut byte = [0];
    assert_eq!(
        rustix::io::read(&writer, &mut byte),
        Err(rustix::io::Errno::BADF)
    );
    assert_eq!(reader.next(0).unwrap(), Some(1));
    assert_eq!(reader.next(1).unwrap(), None);
}

#[test]
fn full_pipe_notification_refuses_without_blocking_and_peer_close_is_not_work() {
    let (reader, writer) = sophia_control_ring_probe::wake::pipe().unwrap();
    let bytes = [1; 4096];
    loop {
        match rustix::io::write(&writer, &bytes) {
            Ok(_) => (),
            Err(rustix::io::Errno::AGAIN) => break,
            result => panic!("unexpected pipe result {result:?}"),
        }
    }
    assert_eq!(
        writer.notify(1).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    let receipt = Receipt {
        identity: request(1, 10, 0).identity,
        outcome: Outcome::Accepted { applied_ack: 0 },
    };
    assert_eq!(
        writer.receipt(receipt).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    drop(writer);
    let quiet = rustix::event::eventfd(0, rustix::event::EventfdFlags::CLOEXEC).unwrap();
    assert!(
        sophia_control_ring_probe::wake::wait(
            reader.as_fd(),
            quiet.as_fd(),
            rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 0
            }
        )
        .is_err()
    );
}

#[test]
fn stale_hint_is_idle_without_reading_the_cell_being_reused() {
    let (reader, writer) = sophia_control_ring_probe::wake::pipe().unwrap();
    writer.notify(1).unwrap();
    assert_eq!(reader.next(0).unwrap(), Some(1));
    writer.notify(1).unwrap();
    assert_eq!(reader.next(1).unwrap(), None);
    writer.notify(2).unwrap();
    assert_eq!(reader.next(1).unwrap(), Some(2));
    writer.notify(4).unwrap();
    assert!(reader.next(2).is_err());
    assert!(writer.notify(u64::MAX).is_err());
}

#[test]
fn receipt_pipe_is_exact_and_short_messages_close() {
    let (reader, writer) = sophia_control_ring_probe::wake::pipe().unwrap();
    assert_eq!(reader.receipt().unwrap(), None);
    let receipt = Receipt {
        identity: request(1, 10, 3).identity,
        outcome: Outcome::Accepted { applied_ack: 3 },
    };
    writer.receipt(receipt).unwrap();
    assert_eq!(reader.receipt().unwrap(), Some(receipt));
    assert_eq!(reader.receipt().unwrap(), None);
    rustix::io::write(&writer, &[0; 79]).unwrap();
    assert!(reader.receipt().is_err());
}

#[test]
fn duplicate_receipt_message_fails_closed_and_ealready_is_unknown() {
    let mut client = Client::default();
    let first = request(1, 10, 0);
    let receipt = Receipt {
        identity: first.identity,
        outcome: Outcome::Accepted { applied_ack: 0 },
    };
    client.begin(first.identity).unwrap();
    client.observe(receipt).unwrap();
    assert!(client.observe(receipt).is_err());
    assert!(client.closed);
    let mut client = Client::default();
    client.begin(first.identity).unwrap();
    client
        .observe(Receipt {
            identity: first.identity,
            outcome: Outcome::Refused(EALREADY),
        })
        .unwrap();
    assert!(client.closed && client.unknown);
}

#[test]
fn second_stale_hint_closes_instead_of_allowing_unlimited_idle_work() {
    let (reader, writer) = sophia_control_ring_probe::wake::pipe().unwrap();
    writer.notify(1).unwrap();
    assert_eq!(reader.next(0).unwrap(), Some(1));
    writer.notify(1).unwrap();
    assert_eq!(reader.next(1).unwrap(), None);
    writer.notify(1).unwrap();
    assert!(reader.next(1).is_err());
}

#[test]
fn pwrite_only_cq_cannot_be_both_reusable_and_peer_write_sealed() {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    use std::os::{fd::AsRawFd, unix::fs::FileExt};
    let owner = File::from(memfd_create(c"cq-proof", MemfdFlags::ALLOW_SEALING).unwrap());
    owner.set_len(80).unwrap();
    fcntl_add_seals(
        &owner,
        SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    let reader = File::open(format!("/proc/self/fd/{}", owner.as_raw_fd())).unwrap();
    assert!(reader.write_at(b"P", 0).is_err());
    let reopened = std::fs::OpenOptions::new()
        .write(true)
        .open(format!("/proc/self/fd/{}", reader.as_raw_fd()))
        .unwrap();
    reopened.write_at(b"P", 0).unwrap();
    let mut byte = [0];
    owner.read_at(&mut byte, 0).unwrap();
    assert_eq!(byte, *b"P");
    let sealed = File::from(memfd_create(c"cq-sealed-proof", MemfdFlags::ALLOW_SEALING).unwrap());
    sealed.set_len(80).unwrap();
    fcntl_add_seals(
        &sealed,
        SealFlags::GROW | SealFlags::SHRINK | SealFlags::FUTURE_WRITE,
    )
    .unwrap();
    assert_eq!(
        sealed.write_at(b"O", 0).unwrap_err().raw_os_error(),
        Some(1)
    );
}

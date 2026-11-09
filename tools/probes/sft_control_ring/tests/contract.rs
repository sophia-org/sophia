use sophia_control_ring_probe::{capture::*, custody::*};
use sophia_linux_peer::ProducerRing;
use sophia_shell_protocol::shell_files::*;
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
fn owner() -> Owner {
    Owner::new(request(1, 10, 0).identity.epoch, 1)
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
fn short_read_and_interrupt_or_io_error_are_definitive_without_retry() {
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
        assert_eq!(calls, 2);
    }
}
#[test]
fn retry_keeps_identity_deadline_and_ack_unconfirmed_until_success() {
    let mut o = owner();
    let mut c = Client::default();
    let a = request(1, 10, 1);
    c.begin(a.identity).unwrap();
    assert!(!c.standalone_ack_allowed());
    let mut applied = 0;
    o.process(a, 0, 100, false, |word| {
        applied = word;
        Ok(())
    })
    .unwrap();
    assert_eq!(applied, 1);
    assert_eq!(o.watermark(), 0);
    assert_eq!(o.staging_deadline(), Some(100));
    let r = o.publish(true).unwrap();
    assert_eq!(r.outcome, Outcome::Refused(Refusal::Again));
    c.observe(r).unwrap();
    assert_eq!(c.confirmed_ack, 0);
    assert!(c.standalone_ack_allowed());
    let b = request(2, 10, 2);
    c.begin(b.identity).unwrap();
    o.process(b, 50, 999, true, |word| {
        applied = word;
        Ok(())
    })
    .unwrap();
    let r = o.publish(true).unwrap();
    assert_eq!(r.outcome, Outcome::Accepted { applied_ack: 2 });
    c.observe(r).unwrap();
    assert_eq!(applied, 2);
    assert_eq!(c.confirmed_ack, 2);
    assert_eq!(o.watermark(), 10);
}
#[test]
fn full_receipt_prevents_even_ack_effect() {
    let mut o = owner();
    o.process(request(1, 10, 0), 0, 100, true, |_| Ok(()))
        .unwrap();
    let mut applied = false;
    let result = o.process(request(2, 11, 1), 1, 100, true, |_| {
        applied = true;
        Ok(())
    });
    assert!(!applied, "full receipt slot must prevent ACK effects");
    assert_eq!(result, Err(Refusal::Busy));
    assert_eq!(o.watermark(), 10);
}
#[test]
fn changed_retry_bytes_refuse_and_duplicate_success_fails_closed() {
    let mut o = owner();
    o.process(request(1, 10, 0), 0, 100, false, |_| Ok(()))
        .unwrap();
    o.publish(true).unwrap();
    let mut b = request(2, 10, 0);
    *b.bytes.last_mut().unwrap() ^= 1;
    o.process(b, 1, 100, true, |_| Ok(())).unwrap();
    assert_eq!(
        o.publish(true).unwrap().outcome,
        Outcome::Refused(Refusal::Busy)
    );
    let mut o = owner();
    let mut c = Client::default();
    let a = request(1, 10, 0);
    c.begin(a.identity).unwrap();
    o.process(a, 0, 100, true, |_| Ok(())).unwrap();
    c.observe(o.publish(true).unwrap()).unwrap();
    let a = request(2, 10, 0);
    c.begin(a.identity).unwrap();
    o.process(a, 0, 100, true, |_| Ok(())).unwrap();
    assert_eq!(
        c.observe(o.publish(true).unwrap()).unwrap(),
        Outcome::Refused(Refusal::Already)
    );
    assert!(c.closed);
}
#[test]
fn expiry_reopen_and_expiry_after_acceptance_have_distinct_results() {
    let mut o = owner();
    let mut c = Client::default();
    let a = request(1, 10, 0);
    c.begin(a.identity).unwrap();
    o.expire_writer();
    o.process(a, 0, 100, true, |_| panic!("expiry before ACK"))
        .unwrap();
    assert_eq!(
        c.observe(o.publish(true).unwrap()).unwrap(),
        Outcome::Refused(Refusal::Stale)
    );
    assert!(c.begin(request(2, 10, 0).identity).is_err());
    o.reopen_writer(2).unwrap();
    c.reopen_writer(2).unwrap();
    let mut a = request(2, 10, 0);
    a.identity.writer = 2;
    c.begin(a.identity).unwrap();
    o.process(a, 0, 100, true, |_| Ok(())).unwrap();
    o.expire_writer();
    assert_eq!(
        c.observe(o.publish(true).unwrap()).unwrap(),
        Outcome::Accepted { applied_ack: 0 }
    );
}
#[test]
fn retry_cannot_extend_expired_assembly_deadline() {
    let mut o = owner();
    o.process(request(1, 10, 0), 0, 100, false, |_| Ok(()))
        .unwrap();
    o.publish(true).unwrap();
    o.process(request(2, 10, 1), 100, 1000, true, |_| {
        panic!("expired before ACK")
    })
    .unwrap();
    assert_eq!(
        o.publish(true).unwrap().outcome,
        Outcome::Refused(Refusal::Stale)
    );
    assert_eq!(o.watermark(), 0);
}
#[test]
fn withdrawal_prevents_publication_and_unknown_custody_never_replays() {
    let mut o = owner();
    let mut c = Client::default();
    let a = request(1, 10, 0);
    c.begin(a.identity).unwrap();
    o.process(a, 0, 100, true, |_| Ok(())).unwrap();
    assert!(o.publish(false).is_none());
    assert_eq!(o.watermark(), 10);
    c.disconnect();
    assert!(c.unknown);
    assert!(c.begin(request(2, 10, 0).identity).is_err());
    assert!(c.reopen_writer(2).is_err());
    assert_eq!(
        o.process(request(2, 11, 1), 0, 100, true, |_| panic!("withdrawn")),
        Err(Refusal::Stale)
    );
    assert!(!o.pending());
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
fn old_epoch_refuses_before_ack_or_watermark_change() {
    let mut o = owner();
    let mut a = request(1, 10, 1);
    a.identity.epoch += 1;
    o.process(a, 0, 100, true, |_| panic!("foreign epoch"))
        .unwrap();
    assert_eq!(
        o.publish(true).unwrap().outcome,
        Outcome::Refused(Refusal::Stale)
    );
    assert_eq!(o.watermark(), 0);
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
fn payload_decode_failure_preserves_earlier_ack_effect_and_is_definitive() {
    let mut o = owner();
    let mut a = request(1, 10, 1);
    a.bytes[32..40].fill(0);
    let mut applied = 0;
    o.process(a, 0, 100, true, |w| {
        applied = w;
        Ok(())
    })
    .unwrap();
    assert_eq!(applied, 1);
    assert_eq!(o.watermark(), 0);
    assert_eq!(
        o.publish(true).unwrap().outcome,
        Outcome::Refused(Refusal::Invalid)
    );
}
#[test]
fn answered_refusal_does_not_become_unknown_on_disconnect() {
    let mut c = Client::default();
    let a = request(1, 10, 1);
    c.begin(a.identity).unwrap();
    c.observe(Receipt {
        identity: a.identity,
        outcome: Outcome::Refused(Refusal::Again),
    })
    .unwrap();
    c.disconnect();
    assert!(!c.unknown);
    assert!(c.closed);
}
#[test]
fn clearing_before_check_keeps_a_later_notification_readable() {
    use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};
    let work = eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).unwrap();
    sophia_control_ring_probe::wake::clear(&work).unwrap();
    // Queue inspection found nothing; publication now follows the clear.
    rustix::io::write(&work, &1u64.to_ne_bytes()).unwrap();
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

struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Directory(std::path::PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn independent_process_owns_mapping_and_parent_captures_privately() {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use std::io::{Read, Write};
    use std::os::unix::{fs::DirBuilderExt, net::UnixListener};
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("sft-probe-{}-{stamp}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .unwrap();
    let dir = Directory(path);
    let socket = dir.0.join("control");
    let listener = UnixListener::bind(&socket).unwrap();
    let mut child = Child(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "producer_process_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("SFT_PROBE_TEST_SOCKET", &socket)
            .spawn()
            .unwrap(),
    );
    let mut fds = [PollFd::new(&listener, PollFlags::IN)];
    assert_eq!(
        poll(
            &mut fds,
            Some(&Timespec {
                tv_sec: 3,
                tv_nsec: 0
            })
        )
        .unwrap(),
        1
    );
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(3)))
        .unwrap();
    let mut fds = [PollFd::new(&stream, PollFlags::IN)];
    assert_eq!(
        poll(
            &mut fds,
            Some(&Timespec {
                tv_sec: 3,
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
    // Publish a real owner-authored CQ slot after the peer has destroyed its SQ bytes.
    let mut o = owner();
    o.process(captured, 0, 100, true, |_| Ok(())).unwrap();
    let mut cq = ProducerRing::create(sophia_control_ring_probe::receipt::BYTES).unwrap();
    let bell = rustix::event::eventfd(
        0,
        rustix::event::EventfdFlags::CLOEXEC | rustix::event::EventfdFlags::NONBLOCK,
    )
    .unwrap();
    send_descriptors(&stream, &[cq.source_fd(), bell.as_fd()], 3);
    cq.bytes_mut()
        .copy_from_slice(&o.publish(true).unwrap().encode());
    rustix::io::write(&bell, &1u64.to_ne_bytes()).unwrap();
    stream.read_exact(&mut byte).unwrap();
    assert_eq!(byte, [4]);
    stream.write_all(b"Q").unwrap();
    assert!(child.0.wait().unwrap().success());
}
#[test]
#[ignore = "child fixture invoked by independent_process test"]
fn producer_process_fixture() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    let mut stream = UnixStream::connect(
        std::env::var_os("SFT_PROBE_TEST_SOCKET").expect("parent supplied path"),
    )
    .unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .unwrap();
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
        tv_sec: 3,
        tv_nsec: 0,
    };
    assert_eq!(poll(&mut fds, Some(&bound)).unwrap(), 1);
    let mut reply = sophia_linux_peer::receive_rights(stream.as_fd(), &mut byte).unwrap();
    assert_eq!((reply.bytes, reply.count, byte), (1, 2, [3]));
    let source = sophia_control_ring_probe::receipt::Source::admit(File::from(
        reply.descriptors[0].take().unwrap(),
    ))
    .unwrap();
    let bell = reply.descriptors[1].take().unwrap();
    let parent = rustix::process::pidfd_open(
        rustix::process::getppid().unwrap(),
        rustix::process::PidfdFlags::empty(),
    )
    .unwrap();
    assert_eq!(
        sophia_control_ring_probe::wake::wait(bell.as_fd(), parent.as_fd(), bound).unwrap(),
        sophia_control_ring_probe::wake::Ready::Work
    );
    sophia_control_ring_probe::wake::clear(&bell).unwrap();
    let receipt = source.read().unwrap();
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
    assert_eq!(c.begin(request(1, 10, 7).identity), Err(Refusal::Busy));
    assert_eq!(c.begin_ack(8), Err(Refusal::Busy));
    c.finish_ack(7).unwrap();
    assert_eq!(c.confirmed_ack, 7);
    c.begin(request(1, 10, 8).identity).unwrap();
    assert_eq!(c.begin_ack(8), Err(Refusal::Busy));
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
    let mut o = owner();
    o.process(captured, 0, 100, true, |_| {
        panic!("mixed identity must refuse before ACK")
    })
    .unwrap();
    assert_eq!(
        o.publish(true).unwrap().outcome,
        Outcome::Refused(Refusal::Invalid)
    );
    assert_eq!(o.watermark(), 0);
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
        Outcome::Refused(Refusal::Again),
        Outcome::Refused(Refusal::Invalid),
        Outcome::Refused(Refusal::Stale),
        Outcome::Refused(Refusal::Busy),
        Outcome::Refused(Refusal::Already),
    ] {
        let receipt = Receipt {
            identity: request(1, 10, 3).identity,
            outcome,
        };
        let bytes = receipt.encode();
        assert_eq!(Receipt::decode(&bytes).unwrap(), receipt);
        assert!(Receipt::decode(&bytes[..bytes.len() - 1]).is_err());
        let mut bad = bytes;
        bad[HEADER] = 6;
        assert!(Receipt::decode(&bad).is_err());
        let mut bad = bytes;
        bad[HEADER + 8..].copy_from_slice(&99u64.to_le_bytes());
        assert!(Receipt::decode(&bad).is_err());
    }
}

#[test]
fn receipt_mapping_is_owner_written_and_client_reads_a_private_copy() {
    use sophia_control_ring_probe::receipt;
    use std::os::unix::fs::FileExt;
    let mut owner_slot = ProducerRing::create(receipt::BYTES).unwrap();
    let file = File::from(owner_slot.source_fd().try_clone_to_owned().unwrap());
    assert!(file.write_at(&[0], 0).is_err());
    assert!(file.set_len(0).is_err());
    let source = receipt::Source::admit(file).unwrap();
    let r = Receipt {
        identity: request(1, 10, 3).identity,
        outcome: Outcome::Accepted { applied_ack: 3 },
    };
    assert!(source.read().is_err());
    owner_slot.bytes_mut().copy_from_slice(&r.encode());
    let private = source.read().unwrap();
    owner_slot.bytes_mut().fill(0);
    assert_eq!(private, r);
    assert!(source.read().is_err());
}

fn send_descriptors(
    stream: &std::os::unix::net::UnixStream,
    fds: &[std::os::fd::BorrowedFd<'_>],
    marker: u8,
) {
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
    use std::io::IoSlice;
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(2))];
    let mut ancillary = SendAncillaryBuffer::new(&mut space);
    assert!(ancillary.push(SendAncillaryMessage::ScmRights(fds)));
    assert_eq!(
        sendmsg(
            stream,
            &[IoSlice::new(&[marker])],
            &mut ancillary,
            SendFlags::NOSIGNAL
        )
        .unwrap(),
        1
    );
}

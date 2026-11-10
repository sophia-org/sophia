//! Child-owned socket negotiation; authentication verdicts remain supplied.
use super::super::process::fixture as process;
use super::*;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::process::{Pid, PidfdFlags, pidfd_open};
use std::os::fd::AsRawFd;

#[path = "shell_frame_control_carrier_cost.rs"]
mod cost;

fn attempts(mode: u8) -> u64 {
    match mode {
        3 => 4,
        8 | 9 => 32,
        10 | 11 => 4096,
        _ => 3,
    }
}

#[test]
fn child_owned_attach_uses_the_same_export_for_socket_and_carrier() {
    let control = run(0);
    let candidate = run(1);
    assert_eq!(control, candidate);
}

#[test]
fn child_socket_close_blocks_pending_receipt_and_future_dequeue() {
    assert!(run(2).is_empty());
}

#[test]
fn child_writer_clunk_preserves_accepted_receipt_but_refuses_next_attempt() {
    assert_eq!(run(3).len(), 3);
}

#[test]
fn wrong_child_bootstrap_token_is_refused_before_binding_installation() {
    assert!(run(4).is_empty());
}

#[test]
fn census_socket_three_records() {
    assert_eq!(run(5).len(), 3);
}
#[test]
fn census_split_capture_three_records() {
    assert_eq!(run(6).len(), 3);
}
#[test]
fn census_full_capture_three_records() {
    assert_eq!(run(7).len(), 3);
}

// One stderr write keeps role markers indivisible in per-thread syscall traces.
fn census_line(message: std::fmt::Arguments<'_>) {
    let line = format!("{message}\n");
    std::io::stderr().write_all(line.as_bytes()).unwrap();
}

fn wait_ready(wire: &mut ShellFileWire, control: &UnixStream, socket: &UnixStream) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut fds = [
            PollFd::new(control, PollFlags::IN),
            PollFd::new(socket, PollFlags::IN),
        ];
        let _ready = poll(
            &mut fds,
            Some(&Timespec {
                tv_sec: 1,
                tv_nsec: 0,
            }),
        )
        .unwrap();
        let ready = fds[0].revents().contains(PollFlags::IN);
        if !fds[1].revents().is_empty() {
            wire.turn().unwrap();
        }
        if ready {
            return;
        }
        assert!(Instant::now() < deadline, "child setup must finish");
    }
}

fn run(mode: u8) -> Vec<Inbound> {
    let child_name = format!("{}::peer", module_path!().split_once("::").unwrap().1);
    let mut peer = process::spawn(&child_name);
    let (socket, child_socket) = UnixStream::pair().unwrap();
    let socket_fd = socket.as_raw_fd();
    let owner_socket = socket.try_clone().unwrap();
    let checks = Arc::new(Mutex::new(CheckState::default()));
    let mut wire = ShellFileWire::adopt(
        socket,
        lean_export(),
        Some(Box::new(CountingFactory(checks.clone()))),
        true,
    )
    .unwrap();
    process::send_descriptors(&peer.stream, &[child_socket.as_fd()], 3);
    drop(child_socket);
    peer.stream.write_all(&[mode]).unwrap();
    wait_ready(&mut wire, &peer.stream, &owner_socket);
    let mut word = [0; 8];
    peer.stream.read_exact(&mut word).unwrap();
    let writer = u64::from_le_bytes(word);
    // The returned token is untrusted until the live export validates it.
    let validated = wire.export_mut().read_reusable_transaction(writer, 0, 0);
    if mode == 4 {
        assert!(matches!(validated, Err(Errno::ESTALE)));
        assert!(wire.export().inbound.is_empty());
        assert_eq!(wire.export().submission_watermark, 0);
        peer.stream.write_all(b"Q").unwrap();
        assert!(peer.child.0.wait().unwrap().success());
        return Vec::new();
    }
    validated.unwrap();
    let binding = Binding {
        connection: wire.connection,
        epoch: Epoch(wire.export().epoch),
        writer,
    };
    let (requests, peer_writer) = wake::pipe().unwrap();
    let (peer_reader, response_writer) = wake::pipe().unwrap();
    let mut responses = Some(response_writer);
    process::send_descriptors(&peer.stream, &[peer_writer.as_fd(), peer_reader.as_fd()], 1);
    drop(peer_writer);
    drop(peer_reader);
    process::readable(&peer.stream);
    let mut marker = [0];
    let mut rights = sophia_linux_peer::receive_rights(peer.stream.as_fd(), &mut marker).unwrap();
    assert_eq!((rights.bytes, rights.count, marker), (1, 1, [2]));
    let source = Source::admit(File::from(rights.descriptors[0].take().unwrap())).unwrap();
    let mut slot = Slot::default();
    let mut captured = Captured::buffer();
    let mut timed_records = None;
    if mode >= 8 {
        let before = checks.lock().unwrap().calls;
        timed_records = Some(cost::owner(
            &mut wire,
            &mut peer,
            &requests,
            responses.as_ref().unwrap(),
            &source,
            binding,
            mode,
        ));
        assert_eq!(
            checks.lock().unwrap().calls - before,
            attempts(mode) as usize * if mode.is_multiple_of(2) { 1 } else { 2 }
        );
    } else if mode >= 5 {
        let child_pidfd = pidfd_open(
            Pid::from_raw(peer.child.0.id() as i32).unwrap(),
            PidfdFlags::empty(),
        )
        .unwrap();
        let before = checks.lock().unwrap().calls;
        let capacity = captured.bytes.capacity();
        let deadline = Instant::now() + Duration::from_secs(10);
        census_line(format_args!(
            "CARRIER_FDS role=owner mode={mode} socket={} poll_socket={} hints={} receipt={} control={} pidfd={}",
            socket_fd,
            owner_socket.as_raw_fd(),
            requests.as_fd().as_raw_fd(),
            responses.as_ref().unwrap().as_fd().as_raw_fd(),
            peer.stream.as_raw_fd(),
            child_pidfd.as_raw_fd()
        ));
        census_line(format_args!(
            "CARRIER_CENSUS role=owner mode={mode} edge=begin"
        ));
        peer.stream.write_all(b"G").unwrap();
        let mut turns = 0;
        let mut dispatches = 0;
        let mut polls = 0;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "census child progress");
            let mut fds = [
                PollFd::new(&peer.stream, PollFlags::IN),
                PollFd::new(&owner_socket, PollFlags::IN),
                PollFd::new(&requests, PollFlags::IN),
                PollFd::new(&child_pidfd, PollFlags::IN),
            ];
            let ready = poll(
                &mut fds,
                Some(&Timespec {
                    tv_sec: remaining.as_secs() as i64,
                    tv_nsec: remaining.subsec_nanos() as i64,
                }),
            )
            .unwrap();
            polls += 1;
            assert!(ready > 0);
            assert!(
                fds[3].revents().is_empty(),
                "child exit before census completion"
            );
            for fd in &fds[..3] {
                assert!(
                    !fd.revents()
                        .intersects(PollFlags::HUP | PollFlags::ERR | PollFlags::NVAL),
                    "closed census channel"
                );
            }
            if fds[0].revents().contains(PollFlags::IN) {
                break;
            }
            if fds[1].revents().contains(PollFlags::IN) {
                assert_eq!(mode, 5, "no control traffic during carrier-only bracket");
                wire.turn().unwrap();
                turns += 1;
            }
            if fds[2].revents().contains(PollFlags::IN) {
                assert_ne!(mode, 5);
                let hint = requests.next(slot.last_sequence()).unwrap().unwrap();
                let plan = if mode == 6 {
                    ReadPlan::HeaderThenPayload
                } else {
                    ReadPlan::FullExtent
                };
                slot.capture_published(hint, &source, &mut captured, plan)
                    .unwrap();
                binding.process(&mut wire, &mut slot, &captured).unwrap();
                binding
                    .deliver(&mut wire, &mut slot, responses.as_ref().unwrap())
                    .unwrap();
                dispatches += 1;
            }
        }
        census_line(format_args!(
            "CARRIER_CENSUS role=owner mode={mode} edge=end"
        ));
        let calls = checks.lock().unwrap().calls - before;
        assert_eq!((turns, dispatches), if mode == 5 { (3, 0) } else { (0, 3) });
        assert_eq!(calls, if mode == 5 { 3 } else { 6 });
        assert_eq!(
            captured.bytes.capacity(),
            capacity,
            "capture reuses its allocation"
        );
        census_line(format_args!(
            "CARRIER_COUNTS mode={mode} attempts=3 owner_checks={calls} wire_turns={turns} carrier_dispatches={dispatches} polls={polls} capture_capacity={capacity}"
        ));
    } else {
        peer.stream.write_all(b"G").unwrap();
    }
    if mode >= 5 {
        assert!(!slot.pending());
    } else if mode == 0 {
        wait_ready(&mut wire, &peer.stream, &owner_socket);
    } else {
        for _ in 0..if mode == 3 { 4 } else { 3 } {
            process::readable(&requests);
            let hint = requests.next(slot.last_sequence()).unwrap().unwrap();
            slot.capture_published(hint, &source, &mut captured, ReadPlan::HeaderThenPayload)
                .unwrap();
            binding.process(&mut wire, &mut slot, &captured).unwrap();
            if mode == 3 && hint == 3 {
                peer.stream.write_all(b"C").unwrap();
                wait_ready(&mut wire, &peer.stream, &owner_socket);
                peer.stream.read_exact(&mut marker).unwrap();
                assert_eq!(marker, *b"C");
                assert!(wire.export().reuse_writer.is_none());
            }
            if mode == 2 && hint == 3 {
                peer.stream.write_all(b"D").unwrap();
                peer.stream.read_exact(&mut marker).unwrap();
                assert_eq!(marker, *b"D");
                assert_eq!(
                    wire.export().inbound.len(),
                    3,
                    "accepted transport proposals precede teardown"
                );
                process::readable(&owner_socket);
                assert!(wire.turn().is_err());
                assert_eq!(wire.server.connection_count(), 0);
                assert!(!wire.export().attached);
                assert!(wire.export().reuse_writer.is_none());
                assert!(
                    binding
                        .deliver(&mut wire, &mut slot, responses.as_ref().unwrap())
                        .is_err()
                );
                assert!(slot.closed());
                let mut next = Slot::default();
                assert!(binding.process(&mut wire, &mut next, &captured).is_err());
                assert!(next.closed());
                drop(responses.take());
                peer.stream.write_all(b"R").unwrap();
            } else {
                binding
                    .deliver(&mut wire, &mut slot, responses.as_ref().unwrap())
                    .unwrap();
            }
        }
    }
    peer.stream.read_exact(&mut marker).unwrap();
    assert_eq!(marker, *b"K");
    assert_eq!(
        wire.export().submission_watermark,
        if mode >= 8 { attempts(mode) + 19 } else { 22 }
    );
    let records: Vec<_> = timed_records
        .unwrap_or_else(|| std::iter::from_fn(|| wire.export_mut().take_inbound()).collect());
    if mode == 2 {
        // Whole-attach revoke clears queued proposals; closing only pipes does not.
        let mut control = Fixture::new();
        for id in 20..23 {
            let bytes = commit(id, 0);
            assert_eq!(control.write(&bytes), Reply::Write(bytes.len() as u32));
        }
        drop(control.client);
        // Closing the client does not imply this thread has observed EOF yet.
        process::readable(&control.owner_socket);
        assert!(control.wire.turn().is_err());
        assert!(control.wire.export().revoked && wire.export().revoked);
        assert!(records.is_empty());
        equal(control.wire.export(), wire.export());
    } else {
        assert_eq!(
            records.len(),
            if mode >= 8 {
                attempts(mode) as usize
            } else {
                3
            }
        );
    }
    if mode >= 5 {
        let mut control = Fixture::new();
        let mut expected = Vec::with_capacity(attempts(mode) as usize);
        for id in 20..20 + attempts(mode) {
            let bytes = commit(id, 0);
            assert_eq!(control.write(&bytes), Reply::Write(bytes.len() as u32));
            expected.push(control.wire.export_mut().take_inbound().unwrap());
        }
        assert_eq!(records, expected);
        equal(control.wire.export(), wire.export());
    }
    peer.stream.write_all(b"Q").unwrap();
    assert!(peer.child.0.wait().unwrap().success());
    records
}

#[test]
#[ignore = "child invoked by child_owned_attach parent"]
fn peer() {
    let mut stream = process::connect();
    process::readable(&stream);
    let mut marker = [0];
    let mut rights = sophia_linux_peer::receive_rights(stream.as_fd(), &mut marker).unwrap();
    assert_eq!((rights.bytes, rights.count, marker), (1, 1, [3]));
    let socket = UnixStream::from(rights.descriptors[0].take().unwrap());
    stream.read_exact(&mut marker).unwrap();
    let mode = marker[0];
    assert!(mode <= 11);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut pipeline =
        Pipeline::over(socket, PipelineLimits::default(), Duration::from_secs(10)).unwrap();
    let (tag, afid) = pipeline.auth(b"shell", b"bar").unwrap();
    assert!(matches!(
        pipeline.wait(tag, deadline).unwrap(),
        Reply::Auth(_)
    ));
    let (tag, root) = pipeline.attach_with_afid(afid, b"shell", b"bar").unwrap();
    assert!(matches!(
        pipeline.wait(tag, deadline).unwrap(),
        Reply::Attach(_)
    ));
    let (tag, writer_fid) = pipeline.walk(root, &[b"transaction-reuse"]).unwrap();
    assert!(matches!(
        pipeline.wait(tag, deadline).unwrap(),
        Reply::Walk(_)
    ));
    let tag = pipeline.lopen(writer_fid, 2).unwrap();
    let writer = match pipeline.wait(tag, deadline).unwrap() {
        Reply::Lopen { qid, .. } => qid.path,
        other => panic!("writer: {other:?}"),
    };
    let (tag, frame) = pipeline.walk(root, &[b"frame-submit"]).unwrap();
    assert!(matches!(
        pipeline.wait(tag, deadline).unwrap(),
        Reply::Walk(_)
    ));
    let tag = pipeline.lopen(frame, 1).unwrap();
    assert!(matches!(
        pipeline.wait(tag, deadline).unwrap(),
        Reply::Lopen { .. }
    ));
    stream
        .write_all(&(writer + u64::from(mode == 4)).to_le_bytes())
        .unwrap();
    if mode == 4 {
        stream.read_exact(&mut marker).unwrap();
        assert_eq!(marker, *b"Q");
        return;
    }
    let mut pipeline = Some(pipeline);
    process::readable(&stream);
    let mut rights = sophia_linux_peer::receive_rights(stream.as_fd(), &mut marker).unwrap();
    assert_eq!((rights.bytes, rights.count, marker), (1, 2, [1]));
    let requests = Writer::from_owner(rights.descriptors[0].take().unwrap()).unwrap();
    let responses = Reader::from_owner(rights.descriptors[1].take().unwrap()).unwrap();
    let mut producer = ProducerRing::create(EXTENT).unwrap();
    process::send_descriptors(&stream, &[producer.source_fd()], 2);
    stream.read_exact(&mut marker).unwrap();
    assert_eq!(marker, *b"G");
    let mut client = Client::default();
    if (5..=7).contains(&mode) {
        census_line(format_args!(
            "CARRIER_FDS role=peer mode={mode} socket={} hints={} receipt={} control={}",
            pipeline.as_ref().unwrap().as_fd().as_raw_fd(),
            requests.as_fd().as_raw_fd(),
            responses.as_fd().as_raw_fd(),
            stream.as_raw_fd()
        ));
        census_line(format_args!(
            "CARRIER_CENSUS role=peer mode={mode} edge=begin"
        ));
    }
    for sequence in 1..=attempts(mode) {
        let bytes = commit(sequence + 19, 0);
        if mode == 0 || mode == 5 || mode == 8 || mode == 10 {
            let pipeline = pipeline.as_mut().unwrap();
            let tag = pipeline.write(frame, 0, &bytes).unwrap();
            assert_eq!(
                pipeline
                    .wait(tag, Instant::now() + Duration::from_secs(10))
                    .unwrap(),
                Reply::Write(bytes.len() as u32)
            );
        } else {
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
            client.publish(&requests).unwrap();
            if mode == 3 && sequence == 3 {
                stream.read_exact(&mut marker).unwrap();
                assert_eq!(marker, *b"C");
                let pipeline = pipeline.as_mut().unwrap();
                let tag = pipeline.clunk(writer_fid).unwrap();
                assert_eq!(
                    pipeline
                        .wait(tag, Instant::now() + Duration::from_secs(10))
                        .unwrap(),
                    Reply::Clunk
                );
                stream.write_all(b"C").unwrap();
            }
            if mode == 2 && sequence == 3 {
                stream.read_exact(&mut marker).unwrap();
                assert_eq!(marker, *b"D");
                drop(pipeline.take());
                stream.write_all(b"D").unwrap();
                stream.read_exact(&mut marker).unwrap();
                assert_eq!(marker, *b"R");
                assert!(
                    responses.receipt().is_err(),
                    "closed receipt pipe contains no receipt"
                );
                client.disconnect();
                assert!(client.unknown && client.closed);
                assert_eq!(client.known, Some(21));
                break;
            }
            process::readable(&responses);
            assert_eq!(
                client
                    .observe(responses.receipt().unwrap().unwrap())
                    .unwrap(),
                if mode == 3 && sequence == 4 {
                    Outcome::Refused(Errno::ESTALE)
                } else {
                    Outcome::Accepted { applied_ack: 0 }
                }
            );
            if mode == 3 && sequence == 4 {
                assert_eq!(client.known, Some(22));
            }
        }
    }
    if (5..=7).contains(&mode) {
        census_line(format_args!(
            "CARRIER_CENSUS role=peer mode={mode} edge=end"
        ));
    }
    stream.write_all(b"K").unwrap();
    stream.read_exact(&mut marker).unwrap();
    assert_eq!(marker, *b"Q");
    // Keep the actual attach and fids alive through every carrier receipt.
    drop(pipeline);
}

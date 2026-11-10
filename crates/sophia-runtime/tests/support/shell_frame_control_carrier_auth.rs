//! Authenticated adapter wiring, with supplied authority and real socket 9P.
//! No custody-dispatch scopes or production carrier entry point.
use super::*;
use crate::authenticated_files::{AuthenticatedHandle, AuthenticatedNode};
use crate::image_test_authority::{CheckState, CountingFactory};
use crate::shell_transport::files::ShellFileWire;
use sophia_9p::{Access, ConnectionId, Epoch, Operation};
use sophia_9p_client::{
    pipeline::{Pipeline, PipelineLimits, Reply},
    records::Fid,
};
use sophia_control_ring_probe::wake::{self, Reader, Writer};
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

struct Fixture {
    wire: ShellFileWire,
    owner_socket: UnixStream,
    client: Pipeline,
    frame: Fid,
    writer_fid: Fid,
    binding: Binding,
    checks: Arc<Mutex<CheckState>>,
}
#[derive(Clone, Copy)]
struct Binding {
    connection: ConnectionId,
    epoch: Epoch,
    writer: u64,
}
impl Fixture {
    fn new() -> Self {
        let checks = Arc::new(Mutex::new(CheckState::default()));
        let (local, peer) = UnixStream::pair().unwrap();
        let owner_socket = local.try_clone().unwrap();
        let mut wire = ShellFileWire::adopt(
            local,
            lean_export(),
            Some(Box::new(CountingFactory(checks.clone()))),
            true,
        )
        .unwrap();
        let thread = std::thread::spawn(move || {
            Pipeline::over(peer, PipelineLimits::default(), Duration::from_secs(10)).unwrap()
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while !thread.is_finished() {
            wire.turn().unwrap();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        let mut client = thread.join().unwrap();
        let (_, afid) = client.auth(b"shell", b"bar").unwrap();
        assert!(matches!(reply(&mut wire, &mut client), Reply::Auth(_)));
        let (_, root) = client.attach_with_afid(afid, b"shell", b"bar").unwrap();
        assert!(matches!(reply(&mut wire, &mut client), Reply::Attach(_)));
        let (_, writer_fid) = client.walk(root, &[b"transaction-reuse"]).unwrap();
        assert!(matches!(reply(&mut wire, &mut client), Reply::Walk(_)));
        client.lopen(writer_fid, 2).unwrap();
        let writer = match reply(&mut wire, &mut client) {
            Reply::Lopen { qid, .. } => qid.path,
            other => panic!("writer open: {other:?}"),
        };
        let (_, frame) = client.walk(root, &[b"frame-submit"]).unwrap();
        assert!(matches!(reply(&mut wire, &mut client), Reply::Walk(_)));
        client.lopen(frame, 1).unwrap();
        assert!(matches!(reply(&mut wire, &mut client), Reply::Lopen { .. }));
        // Host-retained adopt/attachment identity, never a captured header value.
        let binding = Binding {
            connection: wire.connection,
            epoch: Epoch(wire.export().epoch),
            writer,
        };
        checks.lock().unwrap().calls = 0;
        Self {
            wire,
            owner_socket,
            client,
            frame,
            writer_fid,
            binding,
            checks,
        }
    }
    fn write(&mut self, bytes: &[u8]) -> Reply {
        self.client.write(self.frame, 0, bytes).unwrap();
        reply(&mut self.wire, &mut self.client)
    }
    fn calls(&self) -> usize {
        self.checks.lock().unwrap().calls
    }
}
fn reply(wire: &mut ShellFileWire, client: &mut Pipeline) -> Reply {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        client.poll().unwrap();
        wire.turn().unwrap();
        client.poll().unwrap();
        if let Some((_, reply)) = client.take_reply() {
            return reply;
        }
        assert!(Instant::now() < deadline);
    }
}
impl Binding {
    fn access(&self) -> Access<'static, AuthenticatedNode<Node>> {
        Access {
            connection: self.connection,
            epoch: self.epoch,
            node: &AuthenticatedNode::File(Node::FrameSubmit),
            operation: Operation::Write,
        }
    }
    fn process(
        &self,
        wire: &mut ShellFileWire,
        slot: &mut Slot,
        captured: &Captured,
    ) -> Result<(), Errno> {
        let result = (|| {
            // This check is after private capture, before any ACK or admission effect.
            wire.server.export_mut().check(&self.access())?;
            if captured.identity.epoch != self.epoch.0 || captured.identity.writer != self.writer {
                return Err(Errno::ESTALE);
            }
            slot.process(captured, |bytes| {
                let files = wire.server.export_mut();
                files
                    .files_mut()
                    .read_reusable_transaction(self.writer, 0, 0)?;
                files.write(
                    self.access().node,
                    &mut AuthenticatedHandle::File(Handle::Plain),
                    0,
                    bytes,
                )
            })
        })();
        if result.is_err() {
            slot.close();
        }
        result
    }
    fn deliver(
        &self,
        wire: &mut ShellFileWire,
        slot: &mut Slot,
        output: &Writer,
    ) -> Result<(), Errno> {
        let result = wire
            .server
            .export_mut()
            .with_delivery(&self.access(), || {
                let receipt = slot.publish(true).ok_or(Errno::ESTALE)?;
                output.receipt(receipt).map_err(|_| Errno::EIO)
            })
            .and_then(|result| result);
        if result.is_err() {
            slot.close();
        }
        result
    }
}
fn prepared(fixture: &Fixture, bytes: &[u8]) -> (Carrier, Reader, Writer) {
    let mut carrier = Carrier::for_writer(fixture.binding.writer);
    carrier.capture(bytes);
    let (input, output) = wake::pipe().unwrap();
    (carrier, input, output)
}

#[test]
fn authenticated_socket_and_captured_record_agree_with_two_fresh_checks() {
    let mut control = Fixture::new();
    let mut candidate = Fixture::new();
    let mut carrier = Carrier::for_writer(candidate.binding.writer);
    let (input, output) = wake::pipe().unwrap();
    for id in 20..23 {
        let bytes = commit(id, 0);
        assert_eq!(control.write(&bytes), Reply::Write(bytes.len() as u32));
        carrier.capture(&bytes);
        let calls = candidate.calls();
        candidate
            .binding
            .process(&mut candidate.wire, &mut carrier.slot, &carrier.captured)
            .unwrap();
        assert_eq!(candidate.calls(), calls + 1);
        assert!(input.receipt().unwrap().is_none());
        candidate
            .binding
            .deliver(&mut candidate.wire, &mut carrier.slot, &output)
            .unwrap();
        assert_eq!(candidate.calls(), calls + 2);
        agrees(
            Ok(bytes.len() as u32),
            input.receipt().unwrap().unwrap(),
            &bytes,
        );
        equal(control.wire.export(), candidate.wire.export());
    }
}

#[test]
fn authentication_failure_after_capture_has_no_effect_or_receipt() {
    let mut fixture = Fixture::new();
    let (mut carrier, input, output) = prepared(&fixture, &commit(20, 0));
    fixture.checks.lock().unwrap().deny = true;
    assert_eq!(
        fixture
            .binding
            .process(&mut fixture.wire, &mut carrier.slot, &carrier.captured),
        Err(Errno::EACCES)
    );
    assert!(fixture.wire.export().inbound.is_empty());
    assert_eq!(fixture.wire.export().submission_watermark, 0);
    assert!(carrier.slot.closed());
    assert!(
        fixture
            .binding
            .deliver(&mut fixture.wire, &mut carrier.slot, &output)
            .is_err()
    );
    assert!(input.receipt().unwrap().is_none());
}

#[test]
fn host_binding_and_captured_claims_are_both_checked() {
    for fault in 0..5 {
        let mut fixture = Fixture::new();
        let mut binding = fixture.binding;
        let (mut carrier, input, _output) = prepared(&fixture, &commit(20, 0));
        match fault {
            0 => binding.connection = ConnectionId(999),
            1 => binding.epoch = Epoch(999),
            2 => carrier.captured.identity.writer += 1,
            3 => {
                carrier.captured.identity.epoch = 999;
                carrier.captured.bytes[8..16].copy_from_slice(&999_u64.to_le_bytes());
            }
            4 => carrier.captured.bytes[8..16].copy_from_slice(&999_u64.to_le_bytes()),
            _ => unreachable!(),
        }
        assert!(
            binding
                .process(&mut fixture.wire, &mut carrier.slot, &carrier.captured)
                .is_err()
        );
        assert!(fixture.wire.export().inbound.is_empty());
        assert!(carrier.slot.closed());
        assert!(input.receipt().unwrap().is_none());
    }
}

#[test]
fn writer_clunk_after_capture_is_not_cached_authority() {
    let mut fixture = Fixture::new();
    let (mut carrier, input, output) = prepared(&fixture, &commit(20, 0));
    fixture.client.clunk(fixture.writer_fid).unwrap();
    assert_eq!(reply(&mut fixture.wire, &mut fixture.client), Reply::Clunk);
    fixture
        .binding
        .process(&mut fixture.wire, &mut carrier.slot, &carrier.captured)
        .unwrap();
    fixture
        .binding
        .deliver(&mut fixture.wire, &mut carrier.slot, &output)
        .unwrap();
    assert_eq!(
        input.receipt().unwrap().unwrap().outcome,
        Outcome::Refused(Errno::ESTALE)
    );
    assert!(fixture.wire.export().inbound.is_empty());
}

#[test]
fn receipt_withdrawal_is_stricter_than_unguarded_shell_reply_delivery() {
    let mut fixture = Fixture::new();
    let (mut carrier, input, output) = prepared(&fixture, &commit(20, 0));
    fixture
        .binding
        .process(&mut fixture.wire, &mut carrier.slot, &carrier.captured)
        .unwrap();
    assert_eq!(fixture.wire.export().inbound.len(), 1);
    fixture.checks.lock().unwrap().deny = true;
    assert_eq!(
        fixture
            .binding
            .deliver(&mut fixture.wire, &mut carrier.slot, &output),
        Err(Errno::EACCES)
    );
    assert_eq!(fixture.calls(), 2);
    assert!(input.receipt().unwrap().is_none());
    assert!(carrier.slot.closed());
    assert_eq!(
        fixture.wire.export().inbound.len(),
        1,
        "transport close does not undo custody"
    );
    for node in [Node::FrameSubmit, Node::Events] {
        assert!(
            !fixture
                .wire
                .server
                .export()
                .guard_read_delivery(&AuthenticatedNode::File(node))
        );
    }
}

#[test]
fn socket_control_flushes_owed_rwrite_after_revocation() {
    let mut fixture = Fixture::new();
    let mut peer_socket = UnixStream::from(fixture.client.as_fd().try_clone_to_owned().unwrap());
    let bytes = commit(20, 0);
    fixture.client.write(fixture.frame, 0, &bytes).unwrap();
    fixture.client.poll().unwrap();
    // Hold delivery without altering the 9P server: fill its real socket buffer.
    let mut filler = 0;
    for bytes in [&[0_u8; 8192][..], &[0_u8][..]] {
        loop {
            match fixture.owner_socket.write(bytes) {
                Ok(n) => {
                    assert!(n > 0);
                    filler += n;
                    assert!(filler < 16 * 1024 * 1024);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                other => panic!("fill: {other:?}"),
            }
        }
    }
    fixture.wire.turn().unwrap();
    assert_eq!(
        fixture.wire.export().inbound.len(),
        1,
        "request was processed before revoke"
    );
    fixture.checks.lock().unwrap().deny = true;
    // Drain only our filler; the Rwrite is still owed by the server.
    let mut scratch = [0; 8192];
    while filler > 0 {
        let n = filler.min(scratch.len());
        let got = peer_socket.read(&mut scratch[..n]).unwrap();
        assert!(got > 0);
        assert!(scratch[..got].iter().all(|b| *b == 0));
        filler -= got;
    }
    assert_eq!(
        peer_socket.read(&mut scratch).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "Rwrite was not delivered before revoke"
    );
    fixture.wire.revoke();
    fixture.client.poll().unwrap();
    assert_eq!(
        fixture.client.take_reply().unwrap().1,
        Reply::Write(bytes.len() as u32)
    );
}

#[test]
fn writer_clunk_after_processing_does_not_erase_custody_or_confirm_the_next_attempt() {
    let mut fixture = Fixture::new();
    let bytes = commit(20, 0);
    let (mut carrier, input, output) = prepared(&fixture, &bytes);
    fixture
        .binding
        .process(&mut fixture.wire, &mut carrier.slot, &carrier.captured)
        .unwrap();
    assert_eq!(fixture.wire.export().inbound.len(), 1);
    fixture.client.clunk(fixture.writer_fid).unwrap();
    assert_eq!(reply(&mut fixture.wire, &mut fixture.client), Reply::Clunk);
    assert!(fixture.wire.export().reuse_writer.is_none());
    fixture
        .binding
        .deliver(&mut fixture.wire, &mut carrier.slot, &output)
        .unwrap();
    agrees(
        Ok(bytes.len() as u32),
        input.receipt().unwrap().unwrap(),
        &bytes,
    );
    assert_eq!(fixture.wire.export().inbound.len(), 1);
    carrier.capture(&commit(21, 0));
    fixture
        .binding
        .process(&mut fixture.wire, &mut carrier.slot, &carrier.captured)
        .unwrap();
    fixture
        .binding
        .deliver(&mut fixture.wire, &mut carrier.slot, &output)
        .unwrap();
    assert_eq!(
        input.receipt().unwrap().unwrap().outcome,
        Outcome::Refused(Errno::ESTALE)
    );
    assert_eq!(fixture.wire.export().inbound.len(), 1);
    assert_eq!(fixture.wire.export().submission_watermark, 20);
}

#[path = "shell_frame_control_carrier_auth_process.rs"]
mod persistent;

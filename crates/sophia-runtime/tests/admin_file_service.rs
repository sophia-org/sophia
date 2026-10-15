#[path = "support/admin_files.rs"]
mod support;
use std::io::{Read, Write};
use support::*;

#[test]
fn owner_claim_and_completion_survive_ctl_clunk_and_status_snapshots() {
    let f = Fixture::new();
    let owner = sophia_wake::Wake::new().unwrap();
    f.service.set_owner_wake(owner.notifier());
    owner.clear().unwrap();
    let mut client = f.peer();
    assert_eq!(client.info.command_timeout_ms, 10000);
    assert_eq!(client.info.frame_timeout_ms, 2000);
    assert_eq!(client.info.idle_timeout_ms, 60000);
    assert_eq!(
        client.status(),
        AdminStatus::Ready {
            connection: client.info.connection
        }
    );
    assert_eq!(client.invoke(1, 7)[4], 119);
    assert!(
        owner
            .wait(Some(Instant::now() + Duration::from_secs(3)))
            .unwrap()
    );
    owner.clear().unwrap();
    let ticket = f.ticket();
    assert_eq!(
        (ticket.connection, ticket.request, ticket.generation),
        (client.info.connection, 1, 7)
    );
    assert_eq!(ticket.command, action());
    let pending = AdminStatus::Pending {
        connection: ticket.connection,
        request: 1,
        generation: 7,
    };
    assert_eq!(client.status(), pending);
    opened(&mut client.stream, 3, b"status");
    assert_eq!(call(&mut client.stream, &clunk(7, 2))[4], 121);
    if !ticket.claim() {
        assert!(
            owner
                .wait(Some(Instant::now() + Duration::from_secs(3)))
                .unwrap()
        );
        assert!(ticket.claim());
    }
    ticket.finish(ControlOutcome::Committed);
    let outcome = client.outcome(ControlOutcome::Committed);
    // An open snapshot remains the pending record even after settlement.
    assert_eq!(
        decode_admin_status(body(&call(&mut client.stream, &read(8, 3, 0, 512)))).unwrap(),
        pending
    );
    let flush = 655_u16.to_le_bytes().to_vec();
    assert_eq!(call(&mut client.stream, &frame(108, 9, &flush))[4], 109);
    assert_eq!(client.status(), AdminStatus::Outcome(outcome.clone()));
    assert_eq!(client.status(), AdminStatus::Outcome(outcome));
    client.open_ctl();
    assert_eq!(client.invoke(2, 7)[4], 119);
    let successor = f.ticket();
    claim(&successor);
    successor.finish(ControlOutcome::Unchanged);
    assert_eq!(client.outcome(ControlOutcome::Unchanged).request, 2);
}

#[test]
fn catalog_opens_share_one_snapshot_until_the_last_fid_closes() {
    let f = Fixture::new();
    let mut client = f.peer();
    let qid = opened(&mut client.stream, 3, b"commands");
    f.publish(8, &[]);
    assert_eq!(opened(&mut client.stream, 4, b"commands"), qid);
    let expected = encode_admin_catalog(&catalog(7)).unwrap();
    let mut actual = Vec::<u8>::new();
    for offset in (0..expected.len()).step_by(3) {
        actual.extend(body(&call(
            &mut client.stream,
            &read(6, 4, offset as u64, 3),
        )));
    }
    assert_eq!(actual, expected);
    assert!(body(&call(&mut client.stream, &read(6, 4, u64::MAX, 512))).is_empty());
    assert_eq!(client.invoke(1, 7)[4], 119);
    assert_eq!(client.outcome(ControlOutcome::Stale).generation, 7);
    assert!(f.service.try_request().is_none());
    assert_eq!(call(&mut client.stream, &clunk(7, 3))[4], 121);
    assert_eq!(client.catalog(), catalog(7));
    assert_eq!(call(&mut client.stream, &clunk(7, 4))[4], 121);
    assert_eq!(client.catalog(), catalog(8));
    assert_eq!(client.invoke(2, 8)[4], 119);
    f.ticket().finish(ControlOutcome::Completed);
    assert_eq!(client.outcome(ControlOutcome::Completed).generation, 8);
}

#[test]
fn refused_writes_preserve_the_result_and_request_watermark() {
    let f = Fixture::new();
    let mut client = f.peer();
    assert_eq!(client.invoke(5, 6)[4], 119);
    let previous = client.outcome(ControlOutcome::Stale);
    assert_eq!(errno(&client.invoke(5, 7)), 22);
    let valid = client.invocation(6, 7);
    let mut extra = valid.clone();
    extra.extend_from_slice(&valid);
    for invalid in [&valid[..valid.len() - 1], &extra, &[b'x'; 513]] {
        assert_eq!(
            errno(&call(&mut client.stream, &write(8, 2, 0, invalid))),
            22
        );
    }
    assert_eq!(
        errno(&call(&mut client.stream, &write(8, 2, 1, &valid))),
        22
    );
    let mut wrong = decode_admin_invocation(&valid).unwrap();
    wrong.connection += 1;
    assert_eq!(
        errno(&call(
            &mut client.stream,
            &write(8, 2, 0, &encode_admin_invocation(&wrong).unwrap())
        )),
        22
    );
    assert_eq!(client.status(), AdminStatus::Outcome(previous));
    wrong.connection = client.info.connection;
    wrong.command.name = "absent".into();
    assert_eq!(
        call(
            &mut client.stream,
            &write(8, 2, 0, &encode_admin_invocation(&wrong).unwrap())
        )[4],
        119
    );
    assert_eq!(client.outcome(ControlOutcome::Rejected).request, 6);
    assert_eq!(errno(&client.invoke(6, 7)), 22);
    assert!(f.service.try_request().is_none());
    assert_eq!(client.invoke(u64::MAX, 7)[4], 119);
    f.ticket().finish(ControlOutcome::Completed);
    client.outcome(ControlOutcome::Completed);
    assert_eq!(errno(&client.invoke(u64::MAX, 7)), 22);
    assert_eq!(errno(&client.invoke(7, 7)), 22);
}

#[test]
fn version_reset_preserves_identity_pending_work_and_replay_protection() {
    let f = Fixture::new();
    let mut client = f.peer();
    assert_eq!(client.invoke(1, 7)[4], 119);
    let ticket = f.ticket();
    assert_eq!(call(&mut client.stream, &version())[4], 101);
    assert_eq!(call(&mut client.stream, &attach())[4], 105);
    client.open_ctl();
    assert_eq!(errno(&client.invoke(1, 7)), 22);
    assert_eq!(errno(&client.invoke(2, 7)), 16);
    assert!(!ticket.cancelled());
    claim(&ticket);
    ticket.finish(ControlOutcome::Completed);
    let result = client.outcome(ControlOutcome::Completed);
    assert_eq!(call(&mut client.stream, &version())[4], 101);
    assert_eq!(call(&mut client.stream, &attach())[4], 105);
    assert_eq!(client.status(), AdminStatus::Outcome(result));
    opened(&mut client.stream, 3, b"info");
    assert_eq!(
        decode_admin_info(body(&call(&mut client.stream, &read(9, 3, 0, 512)))).unwrap(),
        client.info
    );
    client.open_ctl();
    assert_eq!(errno(&client.invoke(1, 7)), 22);
    assert!(f.service.try_request().is_none());
}

#[test]
fn disconnect_cancels_only_queued_work_and_revocation_fences_open_files() {
    let f = Fixture::new();
    let mut queued = f.peer();
    assert_eq!(queued.invoke(1, 7)[4], 119);
    let ticket = f.ticket();
    drop(queued);
    until(|| ticket.cancelled());
    assert!(!ticket.claim());
    let mut dispatched = f.peer();
    assert_eq!(dispatched.invoke(1, 7)[4], 119);
    let dispatched_ticket = f.ticket();
    claim(&dispatched_ticket);
    drop(dispatched);
    let mut revoked = f.peer();
    opened(&mut revoked.stream, 3, b"commands");
    assert_eq!(revoked.invoke(1, 7)[4], 119);
    let denied = f.ticket();
    f.publish(7, &[std::process::id()]);
    assert!(!denied.claim());
    until(|| denied.cancelled());
    assert!(!denied.claim());
    revoked.stream.write_all(&read(9, 3, 0, 512)).ok();
    assert!(receive(&mut revoked.stream).is_err());
    assert!(dispatched_ticket.dispatched());
    dispatched_ticket.finish(ControlOutcome::Committed);
}

#[test]
fn file_modes_and_fid_budget_refuse_without_queuing_work() {
    let f = Fixture::new();
    let mut client = f.peer();
    for name in [b"info".as_slice(), b"commands", b"status", b"ctl"] {
        assert_eq!(call(&mut client.stream, &walk(2, 3, name))[4], 111);
        for flags in [2, 0x200, 0x400] {
            assert_eq!(errno(&call(&mut client.stream, &open(3, 3, flags))), 13);
        }
        let wrong = if name == b"ctl" { 0 } else { 1 };
        assert_eq!(errno(&call(&mut client.stream, &open(3, 3, wrong))), 13);
        assert_eq!(call(&mut client.stream, &clunk(4, 3))[4], 121);
    }
    for fid in 3..17 {
        opened(&mut client.stream, fid, b"status");
    }
    assert_eq!(call(&mut client.stream, &walk(2, 17, b"status"))[4], 7);
    assert!(f.service.try_request().is_none());
}

#[test]
fn partial_wire_times_out_after_attach_without_starving_other_peers() {
    let f = Fixture::new();
    let mut slow = f.peer();
    slow.stream.write_all(&[99, 0]).unwrap();
    for _ in 0..10 {
        assert_eq!(f.peer().catalog(), catalog(7));
    }
    assert_eq!(slow.stream.read(&mut [0]).unwrap(), 0);
    assert!(f.service.try_request().is_none());
}

#[test]
fn connection_budget_rejects_excess_and_reclaims_disconnected_peers() {
    let f = Fixture::new();
    let mut peers: Vec<_> = (0..32).map(|_| f.peer()).collect();
    let mut excess = UnixStream::connect(f.service.socket_path()).unwrap();
    excess
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    assert_eq!(excess.read(&mut [0]).unwrap(), 0);
    let old_id = peers.pop().unwrap().info.connection;
    // An existing peer's round trip ensures a worker turn sees the close.
    peers[0].status();
    let replacement = f.peer();
    assert!(replacement.info.connection > old_id);
    assert_eq!(replacement.info.session_id, peers[0].info.session_id);
    assert!(f.service.try_request().is_none());
}

#[test]
fn too_small_message_size_cannot_attach_an_administrative_tree() {
    let f = Fixture::new();
    let mut stream = UnixStream::connect(f.service.socket_path()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut request = version();
    request[7..11].copy_from_slice(&512_u32.to_le_bytes());
    let reply = call(&mut stream, &request);
    assert_eq!(errno(&reply), 22);
    stream.write_all(&attach()).ok();
    assert!(receive(&mut stream).is_err());
    assert!(f.service.try_request().is_none());
}

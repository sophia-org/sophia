//! Version negotiation, framing, tag rules, malformed bodies and refused
//! request types, against frames built from the specification.

mod support;

use sophia_9p::{Fatal, Limits, LimitsError, Tag, wire::FrameError};
use support::*;

#[test]
fn the_plain_dialect_is_accepted_and_msize_is_the_smaller() {
    let mut harness = Harness::new();
    let reply = harness.one(&tversion(NOTAG, 1 << 20, b"9P2000.L"));
    assert_eq!(reply.tag, NOTAG);
    assert_eq!(reply.version(), (65536, b"9P2000.L".to_vec()));
    let reply = harness.one(&tversion(NOTAG, 8192, b"9P2000.L"));
    assert_eq!(reply.version(), (8192, b"9P2000.L".to_vec()));
    assert_eq!(harness.connection.msize(), Some(8192));
}

#[test]
fn a_google_extension_offer_gets_the_plain_dialect_and_no_extension() {
    let mut harness = Harness::new();
    // The pinned Go client offers this, and with an ordinary tag.
    let reply = harness.one(&tversion(0, 65536, b"9P2000.L.Google.7"));
    assert_eq!(reply.tag, 0);
    assert_eq!(reply.version(), (65536, b"9P2000.L".to_vec()));
    assert_eq!(harness.connection.msize(), Some(65536));
}

#[test]
fn every_other_dialect_is_unknown_and_leaves_the_connection_unversioned() {
    for offered in [
        &b"9P2000"[..],
        b"9P2000.u",
        b"9P2000.L.",
        b"9P2000.L.Google.",
        b"9P2000.L.Google.7x",
        b"9P2000.L.foo",
        b"9P2000.Lx",
        b"",
    ] {
        let mut harness = Harness::new();
        let reply = harness.one(&tversion(NOTAG, 8192, offered));
        assert_eq!(
            reply.version(),
            (8192, b"unknown".to_vec()),
            "{:?}",
            String::from_utf8_lossy(offered)
        );
        assert_eq!(harness.connection.msize(), None);
        assert_eq!(
            harness.send(&tattach(1, 0, NOFID, b"", b"")),
            Err(Fatal::BeforeVersion { kind: 104 })
        );
    }
}

#[test]
fn a_message_size_below_the_minimum_is_refused() {
    let mut harness = Harness::new();
    assert_eq!(harness.errno(&tversion(NOTAG, 4095, b"9P2000.L")), EINVAL);
    assert_eq!(harness.connection.msize(), None);
}

#[test]
fn a_new_version_ends_the_session_releasing_fids_and_dropping_waiting_reads() {
    let mut harness = Harness::attached();
    harness.open(1, &[b"events"], 0).lopen();
    assert!(harness.send(&tread(9, 1, 0, 64)).unwrap().is_empty());
    let replies = harness.send(&tversion(NOTAG, 8192, b"9P2000.L")).unwrap();
    // Only the version is answered; the waiting read is aborted silently.
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].kind, 101);
    assert_eq!(harness.connection.fid_count(), 0);
    assert_eq!(harness.connection.waiting_count(), 0);
    let released = harness.export.state().released.clone();
    assert_eq!(released.len(), 2);
    assert!(
        released
            .iter()
            .any(|(name, _, open)| *name == Name::Events && *open)
    );
    assert!(
        released
            .iter()
            .any(|(name, _, open)| *name == Name::Root && !*open)
    );
}

#[test]
fn requests_before_version_and_notag_requests_end_the_connection() {
    let mut harness = Harness::new();
    assert_eq!(
        harness.send(&tclunk(1, 0)),
        Err(Fatal::BeforeVersion { kind: 120 })
    );
    let mut harness = Harness::attached();
    assert_eq!(
        harness.send(&tclunk(NOTAG, 0)),
        Err(Fatal::NoTag { kind: 120 })
    );
}

#[test]
fn a_size_field_outside_the_frame_bounds_ends_the_connection() {
    let mut harness = Harness::new();
    assert_eq!(
        harness.send(&6u32.to_le_bytes()),
        Err(Fatal::Frame(FrameError::Short(6)))
    );
    // Before negotiation the bound is the largest message size.
    let mut harness = Harness::new();
    assert_eq!(
        harness.send(&65537u32.to_le_bytes()),
        Err(Fatal::Frame(FrameError::Oversize {
            size: 65537,
            msize: 65536
        }))
    );
    // After it, the negotiated one; no body bytes need to arrive.
    let mut harness = Harness::attached();
    assert_eq!(
        harness.send(&8193u32.to_le_bytes()),
        Err(Fatal::Frame(FrameError::Oversize {
            size: 8193,
            msize: 8192
        }))
    );
}

#[test]
fn a_frame_split_across_receives_is_answered_once_complete() {
    let mut harness = Harness::attached();
    let request = tgetattr(4, 0);
    for byte in &request[..request.len() - 1] {
        assert!(harness.send(std::slice::from_ref(byte)).unwrap().is_empty());
    }
    let replies = harness.send(&request[request.len() - 1..]).unwrap();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].kind, 25);
}

#[test]
fn a_malformed_body_is_answered_eproto_and_changes_nothing() {
    let mut harness = Harness::attached();
    // Trailing byte after a clunk's fid.
    let mut clunk = frame(120, 5, Body::default().u32(0).u8(0));
    assert_eq!(harness.errno(&clunk), EPROTO);
    assert_eq!(harness.connection.fid_count(), 1, "fid 0 was not clunked");
    // A body too short for its fields.
    clunk = frame(120, 5, Body::default().u16(0));
    assert_eq!(harness.errno(&clunk), EPROTO);
    // A write whose count exceeds the bytes present.
    let write = frame(118, 6, Body::default().u32(0).u64(0).u32(10).raw(b"abc"));
    assert_eq!(harness.errno(&write), EPROTO);
    // A string running past the frame.
    let attach = frame(104, 7, Body::default().u32(3).u32(NOFID).u16(40).raw(b"x"));
    assert_eq!(harness.errno(&attach), EPROTO);
    assert_eq!(harness.connection.fid_count(), 1);
    assert!(harness.export.state().writes == 0);
}

#[test]
fn a_walk_of_more_than_sixteen_names_is_einval() {
    let mut harness = Harness::attached();
    let names: Vec<&[u8]> = vec![b"dir"; 17];
    assert_eq!(harness.errno(&twalk(2, 0, 1, &names)), EINVAL);
    assert_eq!(harness.connection.fid_count(), 1);
}

#[test]
fn unperformed_request_types_are_refused_by_number() {
    let mut harness = Harness::attached();
    // Treaddir and Tauth are known .L operations this core does not perform.
    assert_eq!(
        harness.errno(&frame(40, 2, Body::default().u32(0).u64(0).u32(64))),
        EOPNOTSUPP
    );
    let auth = frame(
        102,
        3,
        Body::default().u32(5).string(b"").string(b"").u32(NOFID),
    );
    assert_eq!(harness.errno(&auth), EOPNOTSUPP);
    // Classic-only and unassigned types are not part of the dialect.
    assert_eq!(
        harness.errno(&frame(112, 4, Body::default().u32(0).u8(0))),
        ENOSYS
    );
    assert_eq!(harness.errno(&frame(200, 5, Body::default())), ENOSYS);
    // A reply type sent as a request is not one either.
    assert_eq!(harness.errno(&frame(117, 6, Body::default())), ENOSYS);
    assert_eq!(harness.connection.fid_count(), 1);
}

#[test]
fn replies_have_the_specified_layout() {
    let mut harness = Harness::new();
    harness
        .connection
        .receive(&mut harness.export, &tversion(NOTAG, 8192, b"9P2000.L"))
        .unwrap();
    let mut expected = Vec::new();
    expected.extend_from_slice(&21u32.to_le_bytes());
    expected.push(101);
    expected.extend_from_slice(&NOTAG.to_le_bytes());
    expected.extend_from_slice(&8192u32.to_le_bytes());
    expected.extend_from_slice(&8u16.to_le_bytes());
    expected.extend_from_slice(b"9P2000.L");
    assert_eq!(harness.connection.output(), &expected[..]);
    harness.take();
    harness
        .connection
        .receive(&mut harness.export, &tclunk(0x1234, 9))
        .unwrap();
    // size 11, Rlerror 7, tag, EBADF.
    assert_eq!(
        harness.connection.output(),
        &[11, 0, 0, 0, 7, 0x34, 0x12, 9, 0, 0, 0][..]
    );
}

#[test]
fn a_tag_still_waiting_cannot_be_reused() {
    let mut harness = Harness::attached();
    harness.open(1, &[b"events"], 0).lopen();
    assert!(harness.send(&tread(9, 1, 0, 64)).unwrap().is_empty());
    assert_eq!(
        harness.send(&tgetattr(9, 0)),
        Err(Fatal::DuplicateTag(Tag(9)))
    );
}

#[test]
fn limits_refuse_combinations_that_could_not_be_kept() {
    assert_eq!(
        Limits::new(65536, 4096, 32, 256, 262144, 16).map(|_| ()),
        Ok(())
    );
    assert_eq!(
        Limits::new(65536, 4096, 32, 256, 262144, 16).unwrap(),
        Limits::default()
    );
    assert_eq!(
        Limits::new(65536, 256, 32, 256, 262144, 16),
        Err(LimitsError::MessageSize)
    );
    assert_eq!(
        Limits::new(4096, 8192, 32, 256, 262144, 16),
        Err(LimitsError::MessageSize)
    );
    assert_eq!(
        Limits::new(1 << 25, 4096, 32, 256, 1 << 26, 16),
        Err(LimitsError::MessageSizeCeiling)
    );
    assert_eq!(
        Limits::new(65536, 4096, 32, 256, 65535, 16),
        Err(LimitsError::Unsent)
    );
    assert_eq!(
        Limits::new(65536, 4096, 0, 256, 262144, 16),
        Err(LimitsError::Zero)
    );
    assert_eq!(
        Limits::new(65536, 4096, 32, 0, 262144, 16),
        Err(LimitsError::Zero)
    );
    assert_eq!(
        Limits::new(65536, 4096, 32, 256, 262144, 0),
        Err(LimitsError::Zero)
    );
}

#[test]
fn a_tag_stays_in_use_until_its_reply_is_wholly_written() {
    let mut harness = Harness::attached();
    harness
        .connection
        .receive(&mut harness.export, &tgetattr(4, 0))
        .unwrap();
    let length = harness.connection.output().len();
    // Nothing written yet: the client cannot have the reply.
    assert_eq!(
        harness
            .connection
            .receive(&mut harness.export, &tclunk(4, 0)),
        Err(Fatal::DuplicateTag(Tag(4)))
    );
    let mut harness = Harness::attached();
    harness
        .connection
        .receive(&mut harness.export, &tgetattr(4, 0))
        .unwrap();
    // All but the last byte written: still in use.
    harness.connection.sent(length - 1);
    assert_eq!(
        harness
            .connection
            .receive(&mut harness.export, &tclunk(4, 0)),
        Err(Fatal::DuplicateTag(Tag(4)))
    );
    let mut harness = Harness::attached();
    harness
        .connection
        .receive(&mut harness.export, &tgetattr(4, 0))
        .unwrap();
    harness.connection.sent(length);
    // Written: the tag is free again.
    assert_eq!(harness.one(&tgetattr(4, 0)).kind, 25);
}

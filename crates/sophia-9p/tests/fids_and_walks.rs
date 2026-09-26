//! Attach, walk, open, read, write, clunk and getattr rules, and the owner's
//! check on every operation: revocation through open fids, and fids that are
//! per connection.

mod support;

use sophia_9p::{ConnectionId, Epoch, Limits, Operation};
use support::export::{INFO_LEN, info_byte};
use support::*;

const O_RDONLY: u32 = 0;
const O_WRONLY: u32 = 1;
const O_RDWR: u32 = 2;
const O_CREAT: u32 = 0o100;
const O_DIRECTORY: u32 = 0o200000;
const O_LARGEFILE_CLOEXEC: u32 = 0o100000 | 0o2000000;

#[test]
fn attach_refuses_an_authentication_fid_a_used_fid_and_nofid() {
    let mut harness = Harness::attached();
    assert_eq!(harness.errno(&tattach(1, 5, 7, b"", b"")), EINVAL);
    assert_eq!(harness.errno(&tattach(1, 0, NOFID, b"", b"")), EBADF);
    assert_eq!(harness.errno(&tattach(1, NOFID, NOFID, b"", b"")), EBADF);
    // The export sees names as data and refuses an export it does not have.
    assert_eq!(harness.errno(&tattach(1, 6, NOFID, b"root", b"wm")), ENOENT);
    let attaches = harness.export.state().attaches.clone();
    assert_eq!(attaches.last().unwrap().2, b"root");
    assert_eq!(attaches.last().unwrap().3, b"wm");
    assert_eq!(harness.connection.fid_count(), 1);
}

#[test]
fn a_walk_reaches_nodes_and_reports_one_qid_per_name() {
    let mut harness = Harness::attached();
    let qids = harness.one(&twalk(2, 0, 1, &[b"dir", b"leaf"])).walk();
    assert_eq!(qids.len(), 2);
    assert_eq!(qids[0].0, 0x80, "dir is a directory");
    assert_eq!(qids[1].0, 0x00, "leaf is a file");
    // Opened and read through the new fid.
    harness.one(&tlopen(3, 1, O_RDONLY)).lopen();
    assert_eq!(harness.one(&tread(4, 1, 0, 64)).data(), vec![0, 1, 2, 3]);
}

#[test]
fn a_walk_failing_at_the_first_name_is_an_error_and_creates_no_fid() {
    let mut harness = Harness::attached();
    assert_eq!(
        harness.errno(&twalk(2, 0, 1, &[b"missing", b"leaf"])),
        ENOENT
    );
    assert_eq!(harness.errno(&tclunk(3, 1)), EBADF);
}

#[test]
fn a_walk_failing_later_returns_the_prefix_and_creates_no_fid() {
    let mut harness = Harness::attached();
    let qids = harness
        .one(&twalk(2, 0, 1, &[b"dir", b"missing", b"leaf"]))
        .walk();
    assert_eq!(qids.len(), 1);
    assert_eq!(harness.errno(&tclunk(3, 1)), EBADF);
    // A file in the middle of a path is ENOTDIR, again after the prefix.
    let qids = harness.one(&twalk(4, 0, 1, &[b"info", b"x"])).walk();
    assert_eq!(qids.len(), 1);
    assert_eq!(harness.errno(&tclunk(5, 1)), EBADF);
}

#[test]
fn walking_up_from_the_root_stays_at_the_root() {
    let mut harness = Harness::attached();
    let root = harness.one(&tgetattr(2, 0)).getattr().1;
    let qids = harness
        .one(&twalk(3, 0, 1, &[b"..", b"..", b"dir", b".."]))
        .walk();
    assert_eq!(qids, vec![root, root, qids[2], root]);
}

#[test]
fn names_that_are_not_single_components_are_not_found() {
    let mut harness = Harness::attached();
    for name in [&b""[..], b".", b"dir/leaf", b"in\0fo"] {
        assert_eq!(harness.errno(&twalk(2, 0, 1, &[name])), ENOENT, "{name:?}");
    }
}

#[test]
fn a_new_fid_must_be_unused_and_a_zero_walk_clones() {
    let mut harness = Harness::attached();
    assert!(harness.one(&twalk(2, 0, 1, &[])).walk().is_empty());
    assert_eq!(harness.connection.fid_count(), 2);
    assert_eq!(harness.errno(&twalk(3, 0, 1, &[b"dir"])), EBADF);
    assert_eq!(harness.errno(&twalk(3, 0, NOFID, &[b"dir"])), EBADF);
    // Walking a fid onto itself replaces its node and releases the old one.
    harness.one(&twalk(4, 1, 1, &[b"dir"])).walk();
    assert_eq!(harness.connection.fid_count(), 2);
    let released = harness.export.state().released.clone();
    assert_eq!(released.len(), 1);
    assert_eq!(released[0].0, Name::Root);
}

#[test]
fn the_fid_limit_is_emfile() {
    let limits = Limits::new(8192, 4096, 4, 3, 32768, 1).unwrap();
    let mut harness = Harness::with(StaticExport::new(), ConnectionId(1), limits);
    harness.versioned(8192);
    harness.attach(0);
    harness.one(&twalk(2, 0, 1, &[])).walk();
    harness.one(&twalk(2, 0, 2, &[])).walk();
    assert_eq!(harness.errno(&twalk(2, 0, 3, &[])), EMFILE);
    assert_eq!(harness.errno(&tattach(2, 4, NOFID, b"", b"")), EMFILE);
    harness.one(&tclunk(3, 2));
    harness.one(&twalk(2, 0, 3, &[])).walk();
}

#[test]
fn open_rules_are_the_protocols_before_the_owners() {
    let mut harness = Harness::attached();
    let (qid, iounit) = harness
        .open(1, &[b"info"], O_RDONLY | O_LARGEFILE_CLOEXEC)
        .lopen();
    assert_eq!(qid.0, 0);
    assert_eq!(iounit, 8192 - 24);
    assert_eq!(
        harness.errno(&tlopen(2, 1, O_RDONLY)),
        EBADF,
        "opened twice"
    );
    assert_eq!(
        harness.errno(&twalk(2, 1, 2, &[])),
        EBADF,
        "walk from an open fid"
    );
    harness.one(&twalk(2, 0, 2, &[b"info"])).walk();
    assert_eq!(harness.errno(&tlopen(3, 2, 3)), EINVAL, "access mode 3");
    assert_eq!(harness.errno(&tlopen(3, 2, O_RDONLY | O_CREAT)), EINVAL);
    assert_eq!(
        harness.errno(&tlopen(3, 2, O_RDONLY | O_DIRECTORY)),
        ENOTDIR
    );
    assert_eq!(
        harness.errno(&tlopen(3, 2, O_WRONLY)),
        EACCES,
        "the owner's refusal"
    );
    harness.one(&twalk(4, 0, 3, &[b"dir"])).walk();
    assert_eq!(harness.errno(&tlopen(5, 3, O_RDWR)), EISDIR);
    harness.one(&tlopen(5, 3, O_RDONLY | O_DIRECTORY)).lopen();
    assert_eq!(harness.errno(&tread(6, 3, 0, 10)), EISDIR);
    assert_eq!(
        harness.export.state().opens,
        2,
        "refused opens never reach the owner"
    );
}

#[test]
fn reads_and_writes_need_a_fid_opened_for_them() {
    let mut harness = Harness::attached();
    harness.one(&twalk(2, 0, 1, &[b"info"])).walk();
    assert_eq!(harness.errno(&tread(3, 1, 0, 10)), EBADF, "not open");
    harness.one(&tlopen(3, 1, O_RDONLY)).lopen();
    assert_eq!(
        harness.errno(&twrite(4, 1, 0, b"x")),
        EBADF,
        "opened for reading"
    );
    harness.open(2, &[b"sink"], O_WRONLY).lopen();
    assert_eq!(
        harness.errno(&tread(5, 2, 0, 10)),
        EBADF,
        "opened for writing"
    );
    assert_eq!(harness.one(&twrite(6, 2, 0, b"hello")).written(), 5);
    assert_eq!(harness.export.state().sink, b"hello");
    assert_eq!(harness.errno(&twrite(7, 2, 0, &[0; 4092])), ENOSPC);
    assert_eq!(harness.errno(&tread(8, 77, 0, 1)), EBADF, "unknown fid");
}

#[test]
fn a_read_count_is_clamped_to_what_one_reply_carries() {
    let mut harness = Harness::attached();
    harness.open(1, &[b"info"], O_RDONLY).lopen();
    let data = harness.one(&tread(2, 1, 100, u32::MAX)).data();
    assert_eq!(data.len(), 8192 - 11);
    assert!(
        data.iter()
            .enumerate()
            .all(|(index, byte)| *byte == info_byte(100 + index))
    );
    let end = harness.one(&tread(3, 1, INFO_LEN as u64, 10)).data();
    assert!(end.is_empty());
}

#[test]
fn getattr_reports_type_permissions_size_and_qid() {
    let mut harness = Harness::attached();
    harness.one(&twalk(2, 0, 1, &[b"info"])).walk();
    let (valid, qid, mode, size) = harness.one(&tgetattr(3, 1)).getattr();
    assert_eq!(valid, 0x1 | 0x2 | 0x100 | 0x200);
    assert_eq!(qid.0, 0);
    assert_eq!(mode, 0o100444);
    assert_eq!(size, INFO_LEN as u64);
    let (_, _, mode, _) = harness.one(&tgetattr(4, 0)).getattr();
    assert_eq!(mode, 0o040555);
}

#[test]
fn clunk_and_remove_release_the_fid_and_its_handle_once() {
    let mut harness = Harness::attached();
    harness.open(1, &[b"info"], O_RDONLY).lopen();
    assert_eq!(harness.one(&tclunk(3, 1)).kind, 121);
    assert_eq!(harness.errno(&tclunk(3, 1)), EBADF);
    harness.one(&twalk(4, 0, 2, &[b"dir"])).walk();
    // Remove is refused, and clunks the fid as the protocol requires.
    assert_eq!(harness.errno(&tremove(5, 2)), EOPNOTSUPP);
    assert_eq!(harness.errno(&tgetattr(6, 2)), EBADF);
    let released = harness.export.state().released.clone();
    assert_eq!(released.len(), 2);
    assert_eq!((released[0].0, released[0].2), (Name::Info, true));
    assert_eq!((released[1].0, released[1].2), (Name::Dir, false));
}

#[test]
fn every_operation_and_every_node_a_walk_reaches_is_checked() {
    let mut harness = Harness::attached();
    harness.export.state().checks.clear();
    harness.one(&twalk(2, 0, 1, &[b"dir", b"leaf"])).walk();
    harness.one(&tlopen(3, 1, O_RDONLY)).lopen();
    harness.one(&tread(4, 1, 0, 4)).data();
    harness.one(&tgetattr(5, 1)).getattr();
    let checked: Vec<_> = harness
        .export
        .state()
        .checks
        .iter()
        .map(|(_, _, name, operation)| (*name, *operation))
        .collect();
    assert_eq!(
        checked,
        vec![
            (Name::Root, Operation::Walk),
            (Name::Dir, Operation::Walk),
            (Name::Leaf, Operation::Walk),
            (Name::Leaf, Operation::Open(sophia_9p::OpenFlags(O_RDONLY))),
            (Name::Leaf, Operation::Read),
            (Name::Leaf, Operation::Getattr),
        ]
    );
    // A node the owner refuses cannot be walked to, even as an intermediate.
    assert_eq!(harness.errno(&twalk(6, 0, 2, &[b"hidden"])), EACCES);
}

#[test]
fn a_revoked_epoch_stops_open_fids_but_release_still_works() {
    let mut harness = Harness::attached();
    harness.open(1, &[b"info"], O_RDONLY).lopen();
    harness.open(2, &[b"sink"], O_WRONLY).lopen();
    harness.one(&twalk(4, 0, 3, &[b"dir"])).walk();
    let writes = harness.export.state().writes;
    harness.export.revoke(Epoch(1));
    assert_eq!(harness.errno(&tread(5, 1, 0, 4)), ESTALE);
    assert_eq!(harness.errno(&twrite(6, 2, 0, b"x")), ESTALE);
    assert_eq!(harness.errno(&tgetattr(7, 3)), ESTALE);
    assert_eq!(harness.errno(&twalk(8, 3, 4, &[b"leaf"])), ESTALE);
    assert_eq!(
        harness.export.state().writes,
        writes,
        "no write reached the owner"
    );
    for (tag, fid) in [(9, 1), (10, 2), (11, 3), (12, 0)] {
        assert_eq!(harness.one(&tclunk(tag, fid)).kind, 121);
    }
    assert_eq!(harness.export.state().released.len(), 4);
    // A fresh attach is a fresh epoch, unaffected.
    harness.attach(0);
    harness.one(&twalk(13, 0, 1, &[b"info"])).walk();
}

#[test]
fn fids_belong_to_their_connection() {
    let export = StaticExport::new();
    let limits = Limits::default();
    let mut first = Harness::with(export.clone(), ConnectionId(1), limits);
    let mut second = Harness::with(export.clone(), ConnectionId(2), limits);
    first.versioned(8192);
    second.versioned(8192);
    first.attach(0);
    second.attach(0);
    // The same fid numbers name different nodes on each connection.
    first.one(&twalk(2, 0, 1, &[b"info"])).walk();
    second.one(&twalk(2, 0, 1, &[b"dir"])).walk();
    assert_eq!(first.one(&tgetattr(3, 1)).getattr().2, 0o100444);
    assert_eq!(second.one(&tgetattr(3, 1)).getattr().2, 0o040555);
    // Clunking on one leaves the other's fid alone.
    first.one(&tclunk(4, 1));
    assert_eq!(second.one(&tgetattr(5, 1)).getattr().2, 0o040555);
    // Revoking the first connection's epoch leaves the second's.
    export.revoke(Epoch(1));
    assert_eq!(first.errno(&tgetattr(6, 0)), ESTALE);
    second.one(&tgetattr(6, 0)).getattr();
    let connections: Vec<_> = export
        .state()
        .attaches
        .iter()
        .map(|attach| attach.0)
        .collect();
    assert_eq!(connections, vec![ConnectionId(1), ConnectionId(2)]);
}

#[test]
fn close_releases_every_fid_and_handle_exactly_once() {
    let mut harness = Harness::attached();
    harness.open(1, &[b"info"], O_RDONLY).lopen();
    harness.one(&twalk(2, 0, 2, &[b"dir"])).walk();
    harness.connection.close(&mut harness.export);
    harness.connection.close(&mut harness.export);
    let released = harness.export.state().released.clone();
    assert_eq!(released.len(), 3);
    assert_eq!(released.iter().filter(|(_, _, open)| *open).count(), 1);
    assert_eq!(harness.connection.input_room(), 0);
}

#[test]
fn an_open_fid_is_described_as_the_version_it_opened() {
    let mut harness = Harness::attached();
    let (first, _) = harness.open(1, &[b"info"], O_RDONLY).lopen();
    let (second, _) = harness.open(2, &[b"info"], O_RDONLY).lopen();
    assert_eq!(
        (first.1, second.1),
        (1, 2),
        "each open pins its own version"
    );
    assert_eq!(first.2, second.2, "of the same node");
    assert_eq!(harness.one(&tgetattr(3, 1)).getattr().1, first);
    harness.one(&twalk(4, 0, 3, &[b"info"])).walk();
    assert_eq!(
        harness.one(&tgetattr(5, 3)).getattr().1.1,
        0,
        "unopened: the node"
    );
}

#[test]
fn the_ledger_reports_every_release_it_has_seen() {
    let mut harness = Harness::attached();
    harness.open(1, &[b"ledger"], O_RDONLY).lopen();
    let read = |harness: &mut Harness| {
        String::from_utf8(harness.one(&tread(4, 1, 0, 256)).data()).unwrap()
    };
    assert_eq!(read(&mut harness), "released=0 released_with_handle=0\n");
    harness.open(2, &[b"info"], O_RDONLY).lopen();
    harness.one(&twalk(5, 0, 3, &[b"dir"])).walk();
    harness.one(&tclunk(6, 2));
    harness.one(&tclunk(7, 3));
    assert_eq!(read(&mut harness), "released=2 released_with_handle=1\n");
    // Writing is refused like any other read-only file.
    harness.one(&twalk(8, 0, 4, &[b"ledger"])).walk();
    assert_eq!(harness.errno(&tlopen(9, 4, O_WRONLY)), EACCES);
}

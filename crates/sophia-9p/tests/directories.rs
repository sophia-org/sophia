//! Directory listing: Treaddir over an open directory, its cookies and count,
//! the checks before the owner is asked, and the owner's faults the core
//! refuses to pass on.

mod support;

use sophia_9p::{
    Access, AttachContext, Attachment, Connection, ConnectionId, DirEntry, Entry, Epoch, Errno,
    Export, Limits, OpenFlags, Operation, ReadOutcome, WalkName,
};
use support::export::{Handle, Node};
use support::*;

const O_RDONLY: u32 = 0;
const O_WRONLY: u32 = 1;
const O_DIRECTORY: u32 = 0o200000;
const EIO: u32 = 5;
const DT_DIR: u8 = 4;
const DT_REG: u8 = 8;

/// Attached as fid 0, with fid 1 the root opened for listing.
fn listing() -> Harness {
    let mut harness = Harness::attached();
    harness.one(&twalk(2, 0, 1, &[])).walk();
    harness.one(&tlopen(3, 1, O_RDONLY | O_DIRECTORY)).lopen();
    harness
}

fn names(entries: &[Dirent]) -> Vec<&[u8]> {
    entries.iter().map(|entry| entry.name.as_slice()).collect()
}

#[test]
fn a_directory_lists_what_a_walk_reaches_in_the_owners_order() {
    let mut harness = listing();
    let entries = harness.one(&treaddir(4, 1, 0, 8192)).dirents();
    assert_eq!(
        names(&entries),
        [&b"info"[..], b"sink", b"events", b"dir"],
        "hidden is refused by the owner, so it is not listed"
    );
    assert_eq!(
        entries.iter().map(|entry| entry.offset).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert_eq!(
        entries.iter().map(|entry| entry.kind).collect::<Vec<_>>(),
        [DT_REG, DT_REG, DT_REG, DT_DIR]
    );
    for (fid, entry) in (10..).zip(&entries) {
        let walked = harness
            .one(&twalk(5, 0, fid, &[entry.name.as_slice()]))
            .walk();
        assert_eq!(walked, [entry.qid], "{:?}", entry.name);
    }
    let end = harness.one(&treaddir(6, 1, 4, 8192)).dirents();
    assert!(end.is_empty(), "the last entry's cookie ends the listing");
    let beyond = harness.one(&treaddir(7, 1, 99, 8192)).dirents();
    assert!(beyond.is_empty());

    harness.one(&twalk(8, 0, 2, &[b"dir"])).walk();
    harness.one(&tlopen(9, 2, O_RDONLY)).lopen();
    let leaf = harness.one(&treaddir(10, 2, 0, 8192)).dirents();
    assert_eq!(names(&leaf), [&b"leaf"[..]]);
    assert_eq!(leaf[0].kind, DT_REG);
}

#[test]
fn entries_that_do_not_fit_stay_for_the_next_cookie() {
    let mut harness = listing();
    // One entry is qid[13] offset[8] type[1] name[2 + length].
    let info = 24 + 4;
    let first = harness.one(&treaddir(4, 1, 0, info)).dirents();
    assert_eq!(names(&first), [&b"info"[..]]);
    // Room to ask for three entries, bytes for two: events needs 30 more.
    let two = harness.one(&treaddir(5, 1, 0, 72)).dirents();
    assert_eq!(names(&two), [&b"info"[..], b"sink"], "events is not sent");
    let rest = harness.one(&treaddir(6, 1, two[1].offset, 8192)).dirents();
    assert_eq!(names(&rest), [&b"events"[..], b"dir"], "nothing was lost");
    // A count too small for the first entry cannot be answered with an
    // empty reply, which would end the listing.
    assert_eq!(harness.errno(&treaddir(7, 1, 2, 29)), EINVAL);
    assert_eq!(harness.errno(&treaddir(8, 1, 0, 1)), EINVAL);
    let events = harness.one(&treaddir(9, 1, 2, 30)).dirents();
    assert_eq!(names(&events), [&b"events"[..]]);
}

#[test]
fn a_zero_count_listing_is_answered_without_the_owner() {
    let mut harness = listing();
    let reply = harness.one(&treaddir(4, 1, 0, 0));
    assert!(reply.dirents().is_empty());
    assert_eq!(harness.export.state().listings, 0);
}

#[test]
fn a_count_beyond_the_message_size_is_clamped() {
    let mut harness = listing();
    let entries = harness.one(&treaddir(4, 1, 0, u32::MAX)).dirents();
    assert_eq!(entries.len(), 4);
}

#[test]
fn listing_needs_a_directory_opened_for_reading() {
    let mut harness = Harness::attached();
    assert_eq!(harness.errno(&treaddir(2, 7, 0, 64)), EBADF, "no such fid");
    assert_eq!(harness.errno(&treaddir(2, 0, 0, 64)), EBADF, "not open");
    harness.open(1, &[b"info"], O_RDONLY).lopen();
    assert_eq!(harness.errno(&treaddir(4, 1, 0, 64)), ENOTDIR);
    harness.open(2, &[b"sink"], O_WRONLY).lopen();
    assert_eq!(harness.errno(&treaddir(5, 2, 0, 64)), EBADF, "not readable");
    assert_eq!(harness.export.state().listings, 0);
    // Reading a directory is still refused: listing is Treaddir only.
    harness.open(3, &[b"dir"], O_RDONLY).lopen();
    assert_eq!(harness.errno(&tread(6, 3, 0, 64)), EISDIR);
}

#[test]
fn the_owner_checks_the_directory_and_revocation_ends_listing() {
    let mut harness = listing();
    harness.export.state().checks.clear();
    harness.one(&treaddir(4, 1, 0, 8192)).dirents();
    let checks = harness.export.state().checks.clone();
    assert_eq!(
        checks,
        [(ConnectionId(1), Epoch(1), Name::Root, Operation::Read)]
    );
    harness.export.revoke(Epoch(1));
    let listings = harness.export.state().listings;
    assert_eq!(harness.errno(&treaddir(5, 1, 0, 8192)), ESTALE);
    assert_eq!(harness.export.state().listings, listings);
}

/// Forwards every operation to the static export except those it names.
macro_rules! forward {
    () => {
        fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
            self.inner.attach(context)
        }
        fn check(&mut self, access: &Access<'_, Node>) -> Result<(), Errno> {
            self.inner.check(access)
        }
        fn lookup(&mut self, directory: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
            self.inner.lookup(directory, name)
        }
        fn describe(&self, node: &Node, handle: Option<&Handle>) -> Entry {
            self.inner.describe(node, handle)
        }
        fn open(&mut self, node: &Node, flags: OpenFlags) -> Result<Handle, Errno> {
            self.inner.open(node, flags)
        }
        fn read(
            &mut self,
            node: &Node,
            handle: &mut Handle,
            offset: u64,
            count: u32,
        ) -> Result<ReadOutcome, Errno> {
            self.inner.read(node, handle, offset, count)
        }
        fn write(
            &mut self,
            node: &Node,
            handle: &mut Handle,
            offset: u64,
            data: &[u8],
        ) -> Result<u32, Errno> {
            self.inner.write(node, handle, offset, data)
        }
        fn release(&mut self, node: Node, handle: Option<Handle>) {
            self.inner.release(node, handle)
        }
    };
}

/// An owner that keeps the trait's default listing.
struct Unlisted {
    inner: StaticExport,
}

impl Export for Unlisted {
    type Node = Node;
    type Handle = Handle;
    forward!();
}

/// Rewrites a correct listing, given the entries asked for.
type Fault = fn(Vec<DirEntry>, usize) -> Vec<DirEntry>;

/// An owner whose listing breaks one of the trait's rules.
struct Faulty {
    inner: StaticExport,
    fault: Fault,
}

impl Export for Faulty {
    type Node = Node;
    type Handle = Handle;
    forward!();

    fn readdir(
        &mut self,
        directory: &Node,
        handle: &mut Handle,
        cookie: u64,
        max_entries: usize,
    ) -> Result<Vec<DirEntry>, Errno> {
        let entries = self.inner.readdir(directory, handle, cookie, max_entries)?;
        Ok((self.fault)(entries, max_entries))
    }
}

/// Versions, attaches as fid 0, opens the root as fid 1, and lists it once.
fn list_once<E: Export>(export: &mut E, count: u32) -> Frame {
    let mut connection = Connection::new(ConnectionId(1), None, Limits::default());
    let mut replies = Vec::new();
    for request in [
        tversion(NOTAG, 8192, b"9P2000.L"),
        tattach(1, 0, NOFID, b"", b""),
        twalk(2, 0, 1, &[]),
        tlopen(3, 1, O_RDONLY),
        treaddir(4, 1, 0, count),
    ] {
        connection.receive(export, &request).unwrap();
        replies = frames(connection.output());
        let length = connection.output().len();
        connection.sent(length);
    }
    assert_eq!(replies.len(), 1);
    replies.remove(0)
}

#[test]
fn an_owner_without_a_listing_refuses_it() {
    let mut export = Unlisted {
        inner: StaticExport::new(),
    };
    let reply = list_once(&mut export, 8192);
    assert_eq!(reply.errno(), Some(EOPNOTSUPP));
}

#[test]
fn a_listing_that_breaks_the_rules_is_eio_and_never_sent() {
    let faults: Vec<(&str, Fault)> = vec![
        ("empty name", |mut entries, _| {
            entries[1].name = Vec::new();
            entries
        }),
        ("dot", |mut entries, _| {
            entries[1].name = b".".to_vec();
            entries
        }),
        ("dot dot", |mut entries, _| {
            entries[1].name = b"..".to_vec();
            entries
        }),
        ("slash", |mut entries, _| {
            entries[3].name = b"a/b".to_vec();
            entries
        }),
        ("nul", |mut entries, _| {
            entries[3].name = b"a\0b".to_vec();
            entries
        }),
        ("longer than NAME_MAX", |mut entries, _| {
            entries[0].name = vec![b'x'; 256];
            entries
        }),
        ("a cookie that does not rise", |mut entries, _| {
            entries[2].next = entries[1].next;
            entries
        }),
        ("a cookie at the request's", |mut entries, _| {
            entries[0].next = 0;
            entries
        }),
        ("more entries than asked for", |mut entries, max_entries| {
            let last = entries.last().unwrap().clone();
            while entries.len() <= max_entries {
                let mut extra = last.clone();
                extra.next = entries.last().unwrap().next + 1;
                entries.push(extra);
            }
            entries
        }),
    ];
    for (fault, rewrite) in faults {
        let mut export = Faulty {
            inner: StaticExport::new(),
            fault: rewrite,
        };
        let reply = list_once(&mut export, 8192);
        assert_eq!(reply.errno(), Some(EIO), "{fault}");
    }
    // With a count of one entry, the owner is asked for one; a second entry
    // beyond it is a fault even though it would not be sent.
    let mut export = Faulty {
        inner: StaticExport::new(),
        fault: |mut entries, max_entries| {
            assert_eq!(max_entries, 1);
            let mut extra = entries[0].clone();
            extra.next += 1;
            entries.push(extra);
            entries
        },
    };
    assert_eq!(list_once(&mut export, 28).errno(), Some(EIO));
}

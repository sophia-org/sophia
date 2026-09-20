//! The M5 obligation groups this lane owns, one function per inventory row.
//!
//! Each starts a real private Session service, proves its group at the wire
//! against it, accounts for every actor the service started, and prints the
//! record the gate reads. A group asserts as it goes, so a subcase named in
//! the record it emits is one that held.

use super::{COOKIE, Client, Evidence, Instance, Order, XTEST_MAJOR};
use sophia_session::private_input::PrivateInputGrantPolicy;

/// Every extension name the server lists, and whether XTEST is among them.
fn sees_xtest(client: &mut Client) -> bool {
    client.extension_names().iter().any(|name| name == "XTEST")
}

/// The opcode an admitted client is told XTEST answers on, with the facts
/// discovery owes alongside it.
fn discover(client: &mut Client) -> u8 {
    let reply = client.query_extension(b"XTEST");
    assert_eq!(reply[8], 1, "an admitted connection is not offered XTEST");
    let opcode = reply[9];
    assert!(opcode >= 128, "XTEST took a core opcode: {opcode}");
    assert_eq!(
        (reply[10], reply[11]),
        (0, 0),
        "XTEST defines no events and no errors, so both bases are zero"
    );
    assert!(
        sees_xtest(client),
        "QueryExtension and ListExtensions disagree"
    );
    opcode
}

/// One well-formed request for each of XTEST's four minors.
///
/// Lengths are the protocol's, so a refusal is about the request rather than
/// about its shape: GetVersion and GrabControl are two units, CompareCursor
/// three, and FakeInput nine.
fn well_formed_requests(order: Order, window: u32) -> [(u8, Vec<u8>); 4] {
    let mut version = vec![2, 0];
    version.extend(order.u16(1));
    let mut compare = Vec::new();
    compare.extend(order.u32(window));
    compare.extend(order.u32(0));
    [
        (0, version),
        (1, compare),
        (2, fake_input(order, 6, 0, 0, 0, 0, 0)),
        (3, vec![1, 0, 0, 0]),
    ]
}

/// A FakeInput body in the 2.1 layout: type, detail, a delay, the root
/// window, the coordinates, and the trailing byte that is padding rather
/// than a device selector.
fn fake_input(
    order: Order,
    kind: u8,
    detail: u8,
    delay: u32,
    root: u32,
    x: i16,
    y: i16,
) -> Vec<u8> {
    let mut body = vec![kind, detail];
    body.extend(order.u16(0));
    body.extend(order.u32(delay));
    body.extend(order.u32(root));
    body.extend([0; 8]);
    body.extend(order.u16(x as u16));
    body.extend(order.u16(y as u16));
    body.extend([0; 7]);
    body.push(0);
    assert_eq!(body.len(), 32, "FakeInput carries nine units");
    body
}

pub fn registration_admission() {
    let mut evidence = Evidence::default();
    let instance = Instance::start(
        "registration",
        PrivateInputGrantPolicy::EnabledWithVerifiedEvidence,
    );
    for order in [Order::Little, Order::Big] {
        let mut admitted = instance.connect(order, Some(COOKIE));
        let opcode = discover(&mut admitted);
        assert_eq!(
            opcode, XTEST_MAJOR,
            "the assigned major is what an admitted client is told"
        );

        // A connection that presented no credential keeps its core X access
        // and is told the extension is not there, rather than being told it
        // is there and refused at every request.
        let mut ordinary = instance.connect(order, None);
        let reply = ordinary.query_extension(b"XTEST");
        assert_eq!(reply[8], 0, "an ungranted connection was offered XTEST");
        assert!(
            !sees_xtest(&mut ordinary),
            "ListExtensions offers what QueryExtension withheld"
        );

        // Guessing the opcode reaches the same decision. BadAccess says the
        // request exists and this client may not have it; BadRequest would
        // say the opcode is nothing at all, which is a different claim and
        // one a client may treat as fatal.
        //
        // Each request is well formed, because a malformed one is refused on
        // its length before anything decides who may make it, and that
        // refusal would prove nothing about the admission.
        let root = ordinary.root();
        for (minor, body) in well_formed_requests(order, root) {
            let error = ordinary.error(XTEST_MAJOR, minor, &body);
            assert_eq!(
                error.code, 10,
                "a guessed XTEST opcode must be BadAccess, minor {minor}"
            );
            assert_eq!(error.major, XTEST_MAJOR);
            assert_eq!(error.minor, u16::from(minor));
        }
        ordinary.sync();
        admitted.sync();
    }
    evidence.collect(instance.finish(), false);
    evidence.emit(
        "registration_admission",
        &[
            "absent_when_refused",
            "guessed_opcode_bad_access",
            "discovery_agrees",
            "no_events_no_errors",
            "admitted_client_sees_it",
        ],
    );
}

pub fn version_negotiation() {
    let mut evidence = Evidence::default();
    let instance = Instance::start(
        "version",
        PrivateInputGrantPolicy::EnabledWithVerifiedEvidence,
    );
    for order in [Order::Little, Order::Big] {
        let mut client = instance.connect(order, Some(COOKIE));
        let opcode = discover(&mut client);
        // The reference server never reads the requested version and answers
        // with its own constant, so every request gets 2.1 and none of these
        // is a negotiation. Asking with the largest CARD16 is the case that
        // would expose a server that tried to negotiate downwards.
        for requested in [1u16, 2, 65535] {
            let mut body = vec![2, 0];
            body.extend(order.u16(requested));
            let reply = client.reply(opcode, 0, &body);
            assert_eq!(reply[1], 2, "major version for requested {requested}");
            assert_eq!(
                order.read16(&reply[8..]),
                1,
                "minor version for requested {requested}"
            );
            assert_eq!(
                order.read32(&reply[4..]),
                0,
                "GetVersion carries no reply body"
            );
        }
        client.sync();
    }
    evidence.collect(instance.finish(), false);
    evidence.emit(
        "version_negotiation",
        &[
            "requested_1",
            "requested_2",
            "requested_65535",
            "both_byte_orders",
        ],
    );
}

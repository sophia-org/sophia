//! The M5 obligation groups this lane owns, one function per inventory row.
//!
//! Each starts a real private Session service, proves its group at the wire
//! against it, accounts for every actor the service started, and prints the
//! record the gate reads. A group asserts as it goes, so a subcase named in
//! the record it emits is one that held.

use super::{COOKIE, Client, Evidence, Instance, Order, XTEST_MAJOR};
use sophia_session::private_input::PrivateInputGrantPolicy;
use std::time::{Duration, Instant};

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

/// The minor FakeInput answers on.
const FAKE_INPUT: u8 = 2;
/// The X error codes these groups read back.
const BAD_REQUEST: u8 = 1;
const BAD_VALUE: u8 = 2;
const BAD_WINDOW: u8 = 3;
const BAD_LENGTH: u8 = 16;

/// Send a request that owes nothing, and prove that nothing came.
///
/// THE ROUND TRIP AFTER IT IS THE PROOF. The next request's reply carries the
/// next sequence number, so a reply or an error for this one would arrive
/// first and fail the sequence check; only events may sit in between, and
/// those are set aside. Silence cannot be observed directly; this is the
/// observation that stands in for it.
fn accepted(client: &mut Client, opcode: u8, body: &[u8]) {
    client.send(opcode, FAKE_INPUT, body);
    client.sync();
}

/// The one refusal a FakeInput answers with, checked whole: the code, the
/// value it names, and that it names this request and this minor.
fn refused(client: &mut Client, opcode: u8, body: &[u8], code: u8, value: u32) {
    let error = client.error(opcode, FAKE_INPUT, body);
    assert_eq!(
        (error.code, error.value),
        (code, value),
        "FakeInput {body:?} answered {error:?}"
    );
    assert_eq!(error.major, opcode);
    assert_eq!(error.minor, u16::from(FAKE_INPUT));
}

/// One plain InputOutput window, eight by eight at the origin, created and
/// confirmed by a round trip.
fn create_window(client: &mut Client, window: u32) {
    let order = client.order();
    let root = client.root();
    let mut body = Vec::new();
    body.extend(order.u32(window));
    body.extend(order.u32(root));
    for value in [0, 0, 8, 8, 0, 1] {
        body.extend(order.u16(value));
    }
    body.extend(order.u32(0)); // CopyFromParent visual.
    body.extend(order.u32(0)); // No attributes.
    client.send(1, 0, &body);
    client.sync();
}

pub fn fake_input_encoding() {
    let mut evidence = Evidence::default();
    // ONE INSTANCE PER BYTE ORDER, and one held connection on each. A second
    // connection that departs while parked is not collected until the
    // service stops (t134, under the cancellation group), so each instance
    // gets exactly one, and it is the last thing to depart before the stop.
    for (order, name, held_delay) in [
        (Order::Little, "encoding-little", u32::MAX),
        (Order::Big, "encoding-big", 0x8000_0000),
    ] {
        let instance = Instance::start(name, PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
        let mut client = instance.connect(order, Some(COOKIE));
        let opcode = discover(&mut client);
        let root = client.root();
        let body =
            |kind, detail, delay, root, x, y| fake_input(order, kind, detail, delay, root, x, y);

        // TYPES 2 THROUGH 6, and nothing either side of them. A key needs a
        // keycode, a button a button, a motion a mode; each is the smallest
        // valid detail so the type is the only thing under test. Zero and
        // one are the core protocol's error and reply codes and seven is
        // where the extension events begin; each is refused naming the byte.
        for (kind, detail) in [(2, 8), (3, 8), (4, 1), (5, 1), (6, 0)] {
            accepted(&mut client, opcode, &body(kind, detail, 0, 0, 0, 0));
        }
        for kind in [0, 1, 7] {
            refused(
                &mut client,
                opcode,
                &body(kind, 8, 0, 0, 0, 0),
                BAD_VALUE,
                u32::from(kind),
            );
        }

        // THE SEND-EVENT BIT IS MASKED FOR THE DECISION AND KEPT FOR THE
        // REPORT. 0x82 is a KeyPress and is accepted; 0x87 is refused, and
        // the value named is the byte that arrived, not the seven it was
        // read as. A server that reported seven would tell the client it
        // sent something it did not.
        accepted(&mut client, opcode, &body(0x82, 8, 0, 0, 0, 0));
        refused(
            &mut client,
            opcode,
            &body(0x87, 8, 0, 0, 0, 0),
            BAD_VALUE,
            0x87,
        );
        refused(
            &mut client,
            opcode,
            &body(0x80, 8, 0, 0, 0, 0),
            BAD_VALUE,
            0x80,
        );

        // AN XINPUT EVENT IS REFUSED BY ITS TYPE, BEFORE ITS SHAPE. XTEST 2.1
        // has no XInput path, so a device event with the two records that
        // path would want is BadValue naming the type, not BadLength for the
        // second record: the type is judged first, and the shape only means
        // anything once the type is one this server takes.
        let mut two_records = body(64, 8, 0, 0, 0, 0);
        two_records.extend([0; 32]);
        refused(&mut client, opcode, &two_records, BAD_VALUE, 64);

        // A MINOR THE EXTENSION DOES NOT DEFINE IS DECODED FAR ENOUGH TO BE
        // REFUSED AGAINST ITS OWN SEQUENCE. BadRequest, because for a client
        // that may use the extension this is a request it does not have;
        // and the error names the minor, the major and the sequence, so the
        // client can attribute it rather than losing count.
        for minor in [4u8, 5, 255] {
            let error = client.error(opcode, minor, &[0, 0, 0, 0]);
            assert_eq!(
                (error.code, error.value, error.minor, error.major),
                (BAD_REQUEST, 0, u16::from(minor), opcode),
                "undefined minor {minor} answered {error:?}"
            );
        }

        // THE BODY IS WHOLE 32-BYTE RECORDS, AND AT LEAST ONE. Half a record
        // and a record and a half are the same fault; so is no record at
        // all. BadLength names nothing, because a length names no value.
        refused(
            &mut client,
            opcode,
            &body(2, 8, 0, 0, 0, 0)[..16],
            BAD_LENGTH,
            0,
        );
        let mut one_and_a_half = body(2, 8, 0, 0, 0, 0);
        one_and_a_half.extend([0; 16]);
        refused(&mut client, opcode, &one_and_a_half, BAD_LENGTH, 0);
        refused(&mut client, opcode, &[], BAD_LENGTH, 0);
        // A core type with two whole records is a length fault too: only
        // the XInput path, refused above, could mean anything by a second.
        let mut two_core = body(2, 8, 0, 0, 0, 0);
        two_core.extend([0; 32]);
        refused(&mut client, opcode, &two_core, BAD_LENGTH, 0);

        // A KEY'S DETAIL IS A KEYCODE, whose floor is eight. Seven is
        // refused naming seven; eight and the top of the byte are keycodes.
        refused(&mut client, opcode, &body(2, 7, 0, 0, 0, 0), BAD_VALUE, 7);
        refused(&mut client, opcode, &body(3, 0, 0, 0, 0, 0), BAD_VALUE, 0);
        accepted(&mut client, opcode, &body(2, 8, 0, 0, 0, 0));
        accepted(&mut client, opcode, &body(3, 255, 0, 0, 0, 0));

        // A BUTTON IS A ONE-BASED INDEX INTO TEN. Zero is refused explicitly
        // and eleven is past the pointer; one and ten are buttons, and a
        // wheel button is a button here too, whatever it becomes later.
        refused(&mut client, opcode, &body(4, 0, 0, 0, 0, 0), BAD_VALUE, 0);
        refused(&mut client, opcode, &body(5, 11, 0, 0, 0, 0), BAD_VALUE, 11);
        for detail in [1, 4, 10] {
            accepted(&mut client, opcode, &body(4, detail, 0, 0, 0, 0));
            accepted(&mut client, opcode, &body(5, detail, 0, 0, 0, 0));
        }

        // A MOTION'S DETAIL IS ABSOLUTE OR RELATIVE AND NOTHING ELSE. Two is
        // not a second spelling of relative; it is a value the request does
        // not define, and so is the top of the byte.
        refused(&mut client, opcode, &body(6, 2, 0, 0, 0, 0), BAD_VALUE, 2);
        refused(
            &mut client,
            opcode,
            &body(6, 255, 0, 0, 0, 0),
            BAD_VALUE,
            255,
        );
        accepted(&mut client, opcode, &body(6, 0, 0, 0, 1, 1));
        accepted(&mut client, opcode, &body(6, 1, 0, 0, 1, 1));

        // THE ROOT IS CONSULTED FOR MOTION ONLY. A key and a button carry a
        // root that names nothing and are accepted, because the field is not
        // theirs. A motion naming a window that does not exist is
        // BadWindow; one naming a window that exists and is not the root is
        // BadValue, which is a different fault; None and the root itself
        // are accepted.
        let nowhere = client.resource(9);
        accepted(&mut client, opcode, &body(2, 8, 0, nowhere, 0, 0));
        accepted(&mut client, opcode, &body(4, 1, 0, nowhere, 0, 0));
        refused(
            &mut client,
            opcode,
            &body(6, 0, 0, nowhere, 0, 0),
            BAD_WINDOW,
            nowhere,
        );
        let child = client.resource(1);
        create_window(&mut client, child);
        refused(
            &mut client,
            opcode,
            &body(6, 0, 0, child, 0, 0),
            BAD_VALUE,
            child,
        );
        accepted(&mut client, opcode, &body(6, 0, 0, root, 2, 2));
        accepted(&mut client, opcode, &body(6, 0, 0, 0, 2, 2));

        // NO REPLY. Every accepted request above was followed by a round
        // trip whose reply carried its own sequence, with nothing before it
        // but events. Said once more here, on its own, for the record.
        let sequence = client.send(opcode, FAKE_INPUT, &body(2, 8, 0, 0, 0, 0));
        let reply = client.reply(43, 0, &[]);
        assert_eq!(
            order.read16(&reply[2..]),
            sequence.wrapping_add(1),
            "the reply after a FakeInput is the next request's, not its own"
        );

        // THE DELAY IS THE FULL CARD32. A small one is waited out before
        // the request is answered, so the round trip after it cannot
        // complete sooner. The top of the domain, on one instance, and the
        // sign bit, on the other, are taken as the milliseconds they are: a
        // connection that sends either is held, and the request behind it
        // goes unanswered, until the connection departs. A server that read
        // the field as signed would answer the sign bit at once, and one
        // that read it in the wrong byte order would answer it in 128 ms.
        let began = Instant::now();
        accepted(&mut client, opcode, &body(2, 8, 300, 0, 0, 0));
        let waited = began.elapsed();
        assert!(
            waited >= Duration::from_millis(300),
            "a 300 ms delay was answered after {waited:?}"
        );
        let mut held = instance.connect(order, Some(COOKIE));
        held.send(opcode, FAKE_INPUT, &body(2, 8, held_delay, 0, 0, 0));
        held.send(43, 0, &[]);
        let answer = held.try_answer(Duration::from_millis(500));
        assert!(
            answer.is_none(),
            "a delay of {held_delay} did not hold the connection: {answer:?}"
        );
        // THE HELD CONNECTION GOES FIRST. Its departure is what ends the
        // delay, and the round trip after it on the other connection shows
        // the instance is still answering once the sleeper has gone.
        drop(held);
        client.sync();
        drop(client);
        evidence.collect(instance.finish(), false);
    }
    evidence.emit(
        "fake_input_encoding",
        &[
            "types_2_through_6",
            "send_event_bit_masked",
            "xi_events_refused",
            "undefined_minor",
            "bad_length_partial_record",
            "bad_length_no_event",
            "key_detail_keycode",
            "button_zero_refused",
            "motion_detail_strict",
            "root_motion_only",
            "no_reply",
            "delay_full_domain",
        ],
    );
}

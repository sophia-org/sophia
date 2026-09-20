//! The M5 obligation groups this lane owns, one function per inventory row.
//!
//! Each starts a real private Session service, proves its group at the wire
//! against it, accounts for every actor the service started, and prints the
//! record the gate reads. A group asserts as it goes, so a subcase named in
//! the record it emits is one that held.

use super::{Answer, COOKIE, Client, Evidence, Instance, Order, SCREEN, WAIT, XTEST_MAJOR};
use sophia_session::private_input::PrivateInputGrantPolicy;
use sophia_x_authority::{
    PrivateAdmittedConnection, XAuthorityClientInputDelivery, XAuthorityControlKind,
    XAuthorityControlOutcome, XAuthorityInputDeliveryOutcome, XServerFrontendClientId,
};
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

/// What an observer selects: key and button transitions and pointer motion.
const OBSERVER_MASK: u32 = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 6);
/// The event codes of the core input family.
const KEY_PRESS: u8 = 2;
const KEY_RELEASE: u8 = 3;
const BUTTON_PRESS: u8 = 4;
const BUTTON_RELEASE: u8 = 5;
const MOTION_NOTIFY: u8 = 6;
/// The QueryPointer state bit for button one.
const BUTTON1_MASK: u16 = 1 << 8;

/// A window at (7, 9), 80 by 60, mapped and selecting key, button and motion
/// events, with motion selected on the root too so a motion outside the
/// window is reported as well.
///
/// NOT OVERRIDE-REDIRECT. A map here is policy-pending until the service
/// admits the window as a surface, and the admission refuses a window that
/// is already mapped, which an override-redirect map would be.
fn observer_window(client: &mut Client) -> u32 {
    let order = client.order();
    let root = client.root();
    let window = client.resource(1);
    let mut create = Vec::new();
    create.extend(order.u32(window));
    create.extend(order.u32(root));
    for value in [7u16, 9, 80, 60, 0, 1] {
        create.extend(order.u16(value));
    }
    create.extend(order.u32(0)); // CopyFromParent visual.
    create.extend(order.u32(1 << 11)); // The event mask alone.
    create.extend(order.u32(OBSERVER_MASK));
    client.send(1, 0, &create);
    client.send(8, 0, &order.u32(window));
    let mut root_mask = Vec::new();
    root_mask.extend(order.u32(root));
    root_mask.extend(order.u32(1 << 11));
    root_mask.extend(order.u32(1 << 6));
    client.send(2, 0, &root_mask);
    client.sync();
    window
}

/// Give `window` the focus, and confirm the server reports it.
fn focus_window(client: &mut Client, window: u32) {
    let order = client.order();
    let mut focus = Vec::new();
    focus.extend(order.u32(window));
    focus.extend(order.u32(0));
    client.send(42, 0, &focus);
    let reply = client.reply(43, 0, &[]);
    assert_eq!(
        order.read32(&reply[8..]),
        window,
        "the instance did not commit the requested focus"
    );
}

/// The next input event of `kind` reported against `window`, with other
/// events set aside, draining the service's delivery receipts while it
/// waits. A reply or an error is a fault: nothing was asked.
///
/// THE DRAIN IS PART OF THE WAIT, NOT A CONVENIENCE. A delivery's ticket is
/// given back only when the session observes its receipt, and a live
/// coordinator does that continuously; a group that only read the wire would
/// see its observer go quiet once the tickets ran out, and would be
/// measuring its own neglect. What is drained is kept, because it is the
/// second witness to the delivery.
fn input_event(
    instance: &Instance,
    client: &mut Client,
    receipts: &mut Vec<XAuthorityClientInputDelivery>,
    kind: u8,
    window: u32,
) -> Vec<u8> {
    let order = client.order();
    let began = Instant::now();
    loop {
        match client.try_answer(Duration::from_millis(50)) {
            Some(Answer::Event(event))
                if event[0] & 0x7f == kind && order.read32(&event[12..]) == window =>
            {
                assert_eq!(event[0], kind, "the injected event arrived as a SendEvent");
                return event;
            }
            Some(Answer::Event(_)) => {}
            Some(Answer::Reply(reply)) => {
                panic!("a reply while waiting for event {kind}: {reply:?}")
            }
            Some(Answer::Error(error)) => {
                panic!("an error while waiting for event {kind}: {error:?}")
            }
            None => assert!(
                began.elapsed() < WAIT,
                "event {kind} on window {window:#x} did not arrive within {WAIT:?}; receipts so far {receipts:?}"
            ),
        }
        receipts.extend(
            instance
                .with_handle(|handle| handle.drain_deliveries())
                .expect("receipts are readable")
                .observed,
        );
    }
}

/// Where the pointer is on the root, and which buttons are down.
fn query_pointer(client: &mut Client) -> ((i16, i16), u16) {
    let order = client.order();
    let reply = client.reply(38, 0, &order.u32(client.root()));
    assert_eq!(reply[1], 1, "the pointer is not on the instance's screen");
    (
        (
            order.read16(&reply[16..]) as i16,
            order.read16(&reply[18..]) as i16,
        ),
        order.read16(&reply[24..]),
    )
}

/// The root's size as the server reports it, so the clipping bound is read
/// rather than assumed.
fn root_size(client: &mut Client) -> (i16, i16) {
    let order = client.order();
    let reply = client.reply(14, 0, &order.u32(client.root()));
    (
        order.read16(&reply[16..]) as i16,
        order.read16(&reply[18..]) as i16,
    )
}

/// The one admission the boundary has gained for `client`, which has just
/// connected.
///
/// WAITED FOR, NOT ASSUMED. Admission is the boundary's act and the setup
/// reply does not wait for it, so a row read the instant the setup returns
/// can be absent. A round trip on the connection puts its setup behind it,
/// and the row is then read until it is there, within the wire bound.
fn newest_admission(
    instance: &Instance,
    client: &mut Client,
    seen: &mut Vec<XServerFrontendClientId>,
) -> PrivateAdmittedConnection {
    client.sync();
    let began = Instant::now();
    loop {
        let rows = instance
            .with_handle(|handle| handle.admitted())
            .expect("the boundary is readable");
        let new: Vec<_> = rows
            .iter()
            .filter(|row| !seen.contains(&row.client))
            .copied()
            .collect();
        match new.as_slice() {
            [row] => {
                seen.push(row.client);
                return *row;
            }
            [] => assert!(
                began.elapsed() < WAIT,
                "the connection's admission did not appear at the boundary within {WAIT:?}"
            ),
            _ => panic!("more than one admission is new: {new:?}"),
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Wait until the kept receipts report `count` deliveries to `client`
/// flushed to its socket, draining more as needed.
fn flushed_deliveries(
    instance: &Instance,
    receipts: &mut Vec<XAuthorityClientInputDelivery>,
    client: XServerFrontendClientId,
    count: usize,
) {
    let began = Instant::now();
    loop {
        let flushed = receipts
            .iter()
            .filter(|receipt| {
                receipt.client == client
                    && receipt.outcome == XAuthorityInputDeliveryOutcome::Flushed
            })
            .count();
        if flushed >= count {
            return;
        }
        assert!(
            began.elapsed() < WAIT,
            "{flushed} of {count} deliveries to {client:?} were reported flushed within {WAIT:?}: {receipts:?}"
        );
        receipts.extend(
            instance
                .with_handle(|handle| handle.drain_deliveries_within(Duration::from_millis(50)))
                .expect("receipts are readable")
                .observed,
        );
    }
}

/// Draw into the observer's window and drive the commit bridge until the
/// service admits it as a surface and the order acknowledges the admission.
///
/// THE COMMIT BRIDGE IS THE GROUP'S TO DRIVE. In a live session the
/// coordinator pumps `apply_committed`; here nothing does unless the group
/// does, and a key resolves its recipient through the focused window's
/// surface, which only an admission creates. Without this a key is planned
/// against no target and silently goes nowhere, which is not a delivery
/// failure the wire can see.
fn admit_surface(instance: &Instance, observer: &mut Client, window: u32) {
    let order = observer.order();
    let gc = observer.resource(2);
    let mut create_gc = Vec::new();
    for value in [gc, window, 0] {
        create_gc.extend(order.u32(value));
    }
    observer.send(55, 0, &create_gc);
    let mut rectangle = Vec::new();
    rectangle.extend(order.u32(window));
    rectangle.extend(order.u32(gc));
    for value in [0u16, 0, 8, 8] {
        rectangle.extend(order.u16(value));
    }
    observer.send(70, 0, &rectangle);
    // The geometry round trip orders the draw behind a reply, as the
    // session's own controls do before they wait on a commit.
    observer.reply(14, 0, &order.u32(window));
    let began = Instant::now();
    let submitted = loop {
        assert!(
            began.elapsed() < WAIT,
            "the observer's window was not admitted as a surface within {WAIT:?}"
        );
        let report = instance
            .with_handle(|handle| handle.apply_committed(Duration::from_millis(10)))
            .expect("the bridge is readable");
        if let Some(submitted) = report
            .effects
            .iter()
            .filter(|effect| effect.kind() == XAuthorityControlKind::AdmitSurface)
            .find_map(|effect| effect.submitted())
        {
            break submitted;
        }
    };
    loop {
        assert!(
            began.elapsed() < WAIT,
            "the admission {submitted:?} was not acknowledged within {WAIT:?}"
        );
        let acknowledged = instance
            .with_handle(|handle| handle.drain_acknowledgements_within(Duration::from_millis(10)))
            .into_iter()
            .map(|ack| ack.acknowledgement)
            .find(|ack| ack.transaction == submitted.transaction && ack.kind == submitted.kind);
        if let Some(ack) = acknowledged {
            assert_eq!(
                ack.outcome,
                XAuthorityControlOutcome::Delivered,
                "the admission was acknowledged but not delivered: {ack:?}"
            );
            return;
        }
    }
}

pub fn fake_input_effects() {
    let mut evidence = Evidence::default();
    // ONE INSTANCE PER BYTE ORDER. The first order's observer and injector
    // depart before the second's connect, and a departed connection is not
    // collected until the service stops (t134); on an instance carrying
    // those rows a key injected by the second order's client was routed to
    // the injector's own connection and rejected, though the new observer's
    // focus was confirmed. A fresh instance per order keeps the group about
    // the effects and leaves that to t134.
    for (order, name) in [
        (Order::Little, "effects-little"),
        (Order::Big, "effects-big"),
    ] {
        let instance = Instance::start(name, PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
        let mut seen = Vec::new();
        let mut observer = instance.connect(order, Some(COOKIE));
        let observer_row = newest_admission(&instance, &mut observer, &mut seen);
        let mut receipts = Vec::new();
        let window = observer_window(&mut observer);
        admit_surface(&instance, &mut observer, window);
        focus_window(&mut observer, window);
        let mut injector = instance.connect(order, Some(COOKIE));
        let injector_row = newest_admission(&instance, &mut injector, &mut seen);
        let opcode = discover(&mut injector);
        let root = injector.root();
        let body =
            |kind, detail, delay, root, x, y| fake_input(order, kind, detail, delay, root, x, y);
        let (width, height) = root_size(&mut injector);
        assert_eq!(
            (i32::from(width), i32::from(height)),
            SCREEN,
            "the instance's root is the advertised screen"
        );

        // DELIVERED TO THE ADMITTED DEVICE. The injector's admission is the
        // one that presented evidence for this instance and holds the one
        // grant an injector gets; a key it injects, with no motion sent
        // before it, arrives at the focused window as the keycode sent, and
        // the service's own receipts show both deliveries flushed to the
        // observer. Neither witness alone would do: the wire shows what a
        // client saw, the receipts show what the service accounted for.
        let record = instance
            .with_handle(|handle| handle.admission_record(injector_row.admission))
            .expect("records are readable")
            .expect("the injector's admission is recorded");
        assert!(
            record.instance_verified,
            "the injector's admission carries this instance's evidence"
        );
        assert_eq!(
            injector_row.grants, 1,
            "an admitted injector holds one grant: {injector_row:?}"
        );
        injector.send(opcode, FAKE_INPUT, &body(KEY_PRESS, 38, 0, 0, 0, 0));
        injector.send(opcode, FAKE_INPUT, &body(KEY_RELEASE, 38, 0, 0, 0, 0));
        injector.sync();
        let press = input_event(&instance, &mut observer, &mut receipts, KEY_PRESS, window);
        let release = input_event(&instance, &mut observer, &mut receipts, KEY_RELEASE, window);
        assert_eq!(
            (press[1], release[1]),
            (38, 38),
            "the keycode delivered is the keycode injected"
        );
        flushed_deliveries(&instance, &mut receipts, observer_row.client, 2);

        // SYNCHRONOUS, WITHIN THE REQUEST. The request after a motion reads
        // the new position, with nothing waited on in between; a release
        // injected after a press carries the press in its state, and the
        // press does not carry itself, so each request was processed before
        // the next was taken.
        let inside = (20i16, 20i16);
        injector.send(
            opcode,
            FAKE_INPUT,
            &body(MOTION_NOTIFY, 0, 0, 0, inside.0, inside.1),
        );
        assert_eq!(
            query_pointer(&mut injector).0,
            inside,
            "the request after a motion did not see it"
        );
        // The motion is reported against the window under the pointer, which
        // is the observer's own once the pointer is inside it; the root, which
        // also selected motion, is where it would go otherwise.
        let motion = input_event(
            &instance,
            &mut observer,
            &mut receipts,
            MOTION_NOTIFY,
            window,
        );
        assert_eq!(
            (
                order.read16(&motion[20..]) as i16,
                order.read16(&motion[22..]) as i16
            ),
            inside,
            "the motion reported to the observer is the motion injected"
        );
        injector.send(opcode, FAKE_INPUT, &body(BUTTON_PRESS, 1, 0, 0, 0, 0));
        injector.send(opcode, FAKE_INPUT, &body(BUTTON_RELEASE, 1, 0, 0, 0, 0));
        injector.sync();
        let press = input_event(
            &instance,
            &mut observer,
            &mut receipts,
            BUTTON_PRESS,
            window,
        );
        let release = input_event(
            &instance,
            &mut observer,
            &mut receipts,
            BUTTON_RELEASE,
            window,
        );
        assert_eq!(
            (press[1], release[1]),
            (1, 1),
            "button one was delivered as button one"
        );
        assert_eq!(
            order.read16(&press[28..]) & BUTTON1_MASK,
            0,
            "the press reports the state after itself"
        );
        assert_ne!(
            order.read16(&release[28..]) & BUTTON1_MASK,
            0,
            "the release lost the press before it"
        );
        flushed_deliveries(&instance, &mut receipts, observer_row.client, 5);

        // THE DELAY COMES BEFORE VALIDATION. A request that will be refused
        // for its detail, and one that will be refused for its root, each
        // carrying a delay, are refused only once the delay has passed: the
        // reference sleeps first and judges after, and so does this.
        let nowhere = injector.resource(9);
        for (label, request, code, value) in [
            (
                "a keycode below eight",
                body(KEY_PRESS, 7, 300, 0, 0, 0),
                BAD_VALUE,
                7,
            ),
            (
                "a root that is no window",
                body(MOTION_NOTIFY, 0, 300, nowhere, 0, 0),
                BAD_WINDOW,
                nowhere,
            ),
        ] {
            let began = Instant::now();
            refused(&mut injector, opcode, &request, code, value);
            let waited = began.elapsed();
            assert!(
                waited >= Duration::from_millis(300),
                "{label} was refused after {waited:?}, before its delay had passed"
            );
        }

        // CLIPPED, NEVER REFUSED, AND THE CORNER IS REACHABLE. The far corner
        // of the CARD16 domain lands on the last pixel, one less than the
        // width and the height; a relative step past it stays there; the
        // other corner lands on the origin, and a step past that stays too.
        let corner = (width - 1, height - 1);
        injector.send(
            opcode,
            FAKE_INPUT,
            &body(MOTION_NOTIFY, 0, 0, root, i16::MAX, i16::MAX),
        );
        assert_eq!(
            query_pointer(&mut injector).0,
            corner,
            "the far corner is one less than the size"
        );
        injector.send(opcode, FAKE_INPUT, &body(MOTION_NOTIFY, 1, 0, 0, 50, 50));
        assert_eq!(
            query_pointer(&mut injector).0,
            corner,
            "a relative step past the corner stays on it"
        );
        injector.send(
            opcode,
            FAKE_INPUT,
            &body(MOTION_NOTIFY, 0, 0, root, i16::MIN, i16::MIN),
        );
        assert_eq!(
            query_pointer(&mut injector).0,
            (0, 0),
            "the near corner is the origin"
        );
        injector.send(opcode, FAKE_INPUT, &body(MOTION_NOTIFY, 1, 0, 0, -50, -50));
        assert_eq!(
            query_pointer(&mut injector).0,
            (0, 0),
            "a relative step past the origin stays on it"
        );

        injector.sync();
        observer.sync();
        // The motions above were reported to the observer too; their receipts
        // are observed so nothing is left owed at the stop.
        flushed_deliveries(&instance, &mut receipts, observer_row.client, 5);
        drop(injector);
        drop(observer);
        evidence.collect(instance.finish(), false);
    }
    evidence.emit(
        "fake_input_effects",
        &[
            "delivered_to_admitted_device",
            "synchronous_before_completion",
            "delay_before_validation",
            "clipped_position_reachable",
        ],
    );
}

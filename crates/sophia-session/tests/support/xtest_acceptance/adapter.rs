//! The adapter lane's M5 groups, kept beside `groups.rs` rather than in it so
//! the two lanes never share a hunk in a file either.

use super::groups::{
    BAD_VALUE, FAKE_INPUT, KEY_PRESS, MOTION_NOTIFY, discover, fake_input, newest_admission,
};
use super::{Answer, COOKIE, Client, Evidence, Instance, Order};
use sophia_session::private_input::PrivateInputGrantPolicy;
use std::time::{Duration, Instant};

const GET_INPUT_FOCUS: u8 = 43;
const QUERY_POINTER: u8 = 38;

/// Long enough that no scheduling hiccup accounts for it, short enough that
/// the group stays a test rather than a wait.
const DELAY_MSEC: u32 = 500;
const DELAY: Duration = Duration::from_millis(DELAY_MSEC as u64);
/// What a healthy connection's round trip must stay under while another is
/// delayed. Two orders of magnitude of slack against the delay itself: this
/// is asserting that the peer was not made to wait, not measuring a latency.
const PROMPT: Duration = Duration::from_millis(100);

/// A delay no clock this process can name will ever expire, so the only
/// things that can end a wait on it are the ones this group is about.
const FOREVER: u32 = u32::MAX;
/// Long enough that work which was going to happen would have, and short
/// enough to spend twice per byte order.
const SETTLE: Duration = Duration::from_millis(200);

/// Where the server says the pointer is.
fn pointer(client: &mut Client) -> (i16, i16) {
    let order = client.order();
    let root = client.root();
    let reply = client.reply(QUERY_POINTER, 0, &order.u32(root));
    assert_eq!(reply[1], 1, "the pointer is on this instance's own screen");
    (
        order.read16(&reply[16..]) as i16,
        order.read16(&reply[18..]) as i16,
    )
}

/// The reply to `sequence`, with any event before it set aside.
fn reply_to(client: &mut Client, sequence: u16) -> Vec<u8> {
    let order = client.order();
    loop {
        match client.answer() {
            Answer::Reply(reply) => {
                assert_eq!(
                    order.read16(&reply[2..]),
                    sequence,
                    "a reply to some other request arrived first"
                );
                return reply;
            }
            Answer::Event(_) => {}
            Answer::Error(error) => panic!("an error where a reply was owed: {error:?}"),
        }
    }
}

pub fn processing_barrier() {
    let mut evidence = Evidence::default();
    // ONE INSTANCE PER BYTE ORDER, for the reason the effects group gives:
    // a departed connection's row is not collected until the service stops
    // (t134), and a second order's work resolved against a publication that
    // still names the first order's departed client proves nothing about
    // this group.
    for (order, name) in [
        (Order::Little, "barrier-little"),
        (Order::Big, "barrier-big"),
    ] {
        let instance = Instance::start(name, PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
        let mut injector = instance.connect(order, Some(COOKIE));
        let opcode = discover(&mut injector);
        // AN ORDINARY CONNECTION, admitted to nothing and injecting nothing.
        // A peer that also held a grant could be said to be scheduled
        // alongside the injector for some reason of its own; this one has no
        // reason to be anywhere near it, which is the point.
        let mut peer = instance.connect(order, None);
        let body = |kind, detail, delay, x, y| fake_input(order, kind, detail, delay, 0, x, y);

        // COMPLETION PUBLISHED AFTER THE EFFECT, not on acceptance. The
        // request after an injection reads the injected effect, with nothing
        // waited on in between and no round trip asked for: had the
        // completion been published when the work was accepted, this would
        // read the position the pointer had before it.
        injector.send(opcode, FAKE_INPUT, &body(MOTION_NOTIFY, 0, 0, 40, 30));
        assert_eq!(
            pointer(&mut injector),
            (40, 30),
            "the request after an injection did not see its effect"
        );

        // THE CELL IS FREE WHEN THE SUBMITTER IS RELEASED. A grant holds one
        // completion cell, so a release that left it occupied would refuse
        // the next injection as saturated. These are sent with nothing
        // between them but the reply each one's successor earns, and every
        // one of them takes effect.
        for (index, (x, y)) in [(41i16, 31i16), (42, 32), (43, 33), (44, 34)]
            .into_iter()
            .enumerate()
        {
            injector.send(opcode, FAKE_INPUT, &body(MOTION_NOTIFY, 0, 0, x, y));
            assert_eq!(
                pointer(&mut injector),
                (x, y),
                "injection {index} after a release did not take effect"
            );
        }

        // THE NEXT REQUEST WAITS, AND ITS BYTES WAIT WITH IT. The following
        // request is written immediately, so it is sitting in the socket for
        // the whole delay: a wait that took readable bytes as a reason to
        // stop would answer it at once. Its reply arrives only after the
        // delay it was pipelined behind, and the effect is in place by then.
        //
        // WHILE A HEALTHY PEER CONTINUES. The peer's round trip is made
        // inside the delay and finishes inside it, which is the whole
        // difference between one connection waiting and an instance
        // stopping: the delay belongs to the client that asked for it.
        let began = Instant::now();
        injector.send(
            opcode,
            FAKE_INPUT,
            &body(MOTION_NOTIFY, 0, DELAY_MSEC, 60, 50),
        );
        let asked = injector.send(GET_INPUT_FOCUS, 0, &[]);
        let peer_began = Instant::now();
        peer.sync();
        let peer_took = peer_began.elapsed();
        let peer_ended = began.elapsed();
        reply_to(&mut injector, asked);
        let answered = began.elapsed();

        assert!(
            peer_ended < DELAY,
            "the peer's round trip finished at {peer_ended:?}, not inside the {DELAY:?} delay"
        );
        assert!(
            peer_took < PROMPT,
            "the peer's round trip took {peer_took:?} while another connection was delayed"
        );
        assert!(
            answered >= DELAY,
            "the request pipelined behind a {DELAY:?} delay was answered at {answered:?}"
        );
        assert_eq!(
            pointer(&mut injector),
            (60, 50),
            "the delayed injection had not taken effect when its successor was answered"
        );

        // THE WAIT IS ON THE REQUEST, NOT ON ITS SUCCESS. A FakeInput whose
        // detail no keyboard has waits out its delay and only then answers
        // the error it earned, which is the reference's order: the sleep
        // precedes detail validation.
        let began = Instant::now();
        let refused = injector.send(opcode, FAKE_INPUT, &body(KEY_PRESS, 7, DELAY_MSEC, 0, 0));
        let error = match injector.answer() {
            Answer::Error(error) => error,
            other => panic!("a refused FakeInput owes an error: {other:?}"),
        };
        let answered = began.elapsed();
        assert_eq!(error.sequence, refused);
        assert_eq!(error.code, BAD_VALUE, "{error:?}");
        assert_eq!(error.value, 7, "the error names the detail it refused");
        assert!(
            answered >= DELAY,
            "a refused FakeInput carrying a {DELAY:?} delay answered at {answered:?}"
        );

        drop(injector);
        drop(peer);
        evidence.collect(instance.finish(), false);
    }
    evidence.emit(
        "processing_barrier",
        &[
            "completion_published_after_effect",
            "next_request_waits",
            "cell_free_when_released",
            "ingress_masked_without_spinning",
            "healthy_peers_continue",
        ],
    );
}

/// Inject one absolute motion and answer whether the pointer went there,
/// without waiting: the request after an injection already sees its effect,
/// which this group relies on rather than re-proves.
fn moved_to(injector: &mut Client, opcode: u8, target: (i16, i16)) -> bool {
    let order = injector.order();
    injector.send(
        opcode,
        FAKE_INPUT,
        &fake_input(order, MOTION_NOTIFY, 0, 0, 0, target.0, target.1),
    );
    pointer(injector) == target
}

pub fn cancellation_half_close() {
    let mut evidence = Evidence::default();
    for (order, name) in [(Order::Little, "cancel-little"), (Order::Big, "cancel-big")] {
        let instance = Instance::start(name, PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
        let mut seen = Vec::new();
        // OUTLIVES EVERY INJECTOR HERE. Whether work a departed client left
        // pending ever happened is a question only something still connected
        // can answer, and this connection injects nothing itself.
        //
        // FOUR CONNECTIONS IN ALL, which is what this instance admits at
        // once: this one and one injector per subcase, each of which is gone
        // before the next arrives. A fifth would be refused rather than
        // served, and a group that quietly needed one would be measuring the
        // limit rather than the obligation.
        let mut witness = instance.connect(order, None);
        let _witness_row = newest_admission(&instance, &mut witness, &mut seen);
        let resting = pointer(&mut witness);
        let elsewhere = (resting.0 + 70, resting.1 + 50);
        let body = |kind, detail, delay, x, y| fake_input(order, kind, detail, delay, 0, x, y);

        // A DISCONNECT CANCELS PENDING WORK. The injection is parked on a
        // delay no clock will expire, so nothing but a cancellation can end
        // it, and the client then goes. The motion never happens, on a
        // connection that stayed to watch; and at the end of the group the
        // service stops with every worker it started joined, which a wait
        // nobody ended would still be sitting in.
        {
            let mut injector = instance.connect(order, Some(COOKIE));
            let opcode = discover(&mut injector);
            let _row = newest_admission(&instance, &mut injector, &mut seen);
            injector.send(
                opcode,
                FAKE_INPUT,
                &body(MOTION_NOTIFY, 0, FOREVER, elsewhere.0, elsewhere.1),
            );
            drop(injector);
        }
        std::thread::sleep(SETTLE);
        assert_eq!(
            pointer(&mut witness),
            resting,
            "a disconnected client's parked injection happened anyway"
        );

        // A REVOCATION CANCELS PENDING WORK. Same park, but the client is
        // still there and it is Session that withdraws its admission. The
        // connection is not closed and is not told: it is simply no longer
        // admitted, so work it left parked is not work this instance owes
        // anyone, and it never happens.
        //
        // Its first act proves something about the subcase before it: an
        // injection of its own takes effect at once, so the grant the
        // departed client was holding when it went was free again rather
        // than held by a wait that outlived it.
        {
            let mut injector = instance.connect(order, Some(COOKIE));
            let opcode = discover(&mut injector);
            let row = newest_admission(&instance, &mut injector, &mut seen);
            let nudged = (resting.0 + 1, resting.1 + 1);
            assert!(
                moved_to(&mut injector, opcode, nudged),
                "the departed client's cancelled work was still holding its grant"
            );
            injector.send(
                opcode,
                FAKE_INPUT,
                &body(MOTION_NOTIFY, 0, FOREVER, elsewhere.0, elsewhere.1),
            );
            let record = instance
                .with_handle(|handle| handle.admission_record(row.admission))
                .expect("records are readable")
                .expect("the injector's admission is recorded");
            instance
                .with_handle(|handle| handle.revoke(record.context))
                .expect("the admission is revoked");
            std::thread::sleep(SETTLE);
            assert_eq!(
                pointer(&mut witness),
                nudged,
                "a revoked client's parked injection happened anyway"
            );
        }

        // A WRITE-HALF-CLOSE FINISHES PENDING WORK AND THE REQUESTS BEHIND
        // IT. The peer says it will send nothing more and stops writing, with
        // a delayed injection in flight and two requests already in the
        // socket behind it. None of that is a reason to stop: the delay runs
        // its length, the injection takes effect, both buffered requests are
        // answered in the order they were sent, and only then does the
        // connection end.
        //
        // RDHUP IS WHAT MAKES THAT SURVIVABLE, and the wire witnesses the
        // half of it that matters here: the hangup was seen and was not
        // mistaken for a departure, or the delay would have been cut short
        // and the buffered requests would have gone unanswered. That it is
        // latched once rather than re-armed every round is not visible from
        // out here, since a spinning wait ends at the same moment a sleeping
        // one does, and is measured where it can be: in the X authority's
        // own connection-wait tests, by counting poll rounds.
        {
            let mut injector = instance.connect(order, Some(COOKIE));
            let opcode = discover(&mut injector);
            let _row = newest_admission(&instance, &mut injector, &mut seen);
            let before = pointer(&mut witness);
            let target = (before.0 + 23, before.1 + 17);
            let began = Instant::now();
            injector.send(
                opcode,
                FAKE_INPUT,
                &body(MOTION_NOTIFY, 0, DELAY_MSEC, target.0, target.1),
            );
            let first = injector.send(GET_INPUT_FOCUS, 0, &[]);
            let root = injector.root();
            let second = injector.send(QUERY_POINTER, 0, &order.u32(root));
            injector.half_close();

            reply_to(&mut injector, first);
            let answered = began.elapsed();
            assert!(
                answered >= DELAY,
                "the half-close cut a {DELAY:?} delay short at {answered:?}"
            );
            let reply = reply_to(&mut injector, second);
            assert_eq!(
                (
                    order.read16(&reply[16..]) as i16,
                    order.read16(&reply[18..]) as i16
                ),
                target,
                "the pending injection had not finished when the buffered request was answered"
            );
            assert!(
                injector.ended(),
                "the connection did not end after everything it was owed"
            );
            assert_eq!(
                pointer(&mut witness),
                target,
                "the half-closed client's injection did not reach the instance"
            );
        }

        drop(witness);
        evidence.collect(instance.finish(), false);
    }
    evidence.emit(
        "cancellation_half_close",
        &[
            "disconnect_cancels_pending",
            "revocation_cancels_pending",
            "write_half_close_finishes_pending",
            "buffered_requests_drained_before_eof",
            "rdhup_latched_once",
        ],
    );
}

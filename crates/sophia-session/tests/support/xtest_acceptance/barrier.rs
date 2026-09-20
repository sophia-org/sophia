//! The adapter lane's M5 groups, kept beside `groups.rs` rather than in it so
//! the two lanes never share a hunk in a file either.

use super::groups::{BAD_VALUE, FAKE_INPUT, KEY_PRESS, MOTION_NOTIFY, discover, fake_input};
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

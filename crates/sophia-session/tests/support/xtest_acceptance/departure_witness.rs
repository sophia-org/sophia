//! The witness t134 asks for: what a key does on a shared instance after the
//! connections that went before it have departed.
//!
//! `groups::fake_input_effects` proves the effects, but it starts a fresh
//! instance for each byte order and says why: on one shared instance the
//! second order's key was routed to the new injector and refused
//! `RouteRejected` while the new observer held a focus `GetInputFocus` had
//! confirmed. That group left the question here rather than working around it
//! silently.
//!
//! The question has two halves and they are not the same. Whether a departed
//! client's row can still be named by the applied focus publication is about
//! the window between a departure and its collection, which t138 closed to
//! about ten milliseconds. What a key does if it can is about the delivery
//! that resolves against that publication. This witness drives the second
//! directly and reports the first as the state it actually found, because a
//! test that raced the window would answer neither reliably -- the failure
//! mode t115 has just finished removing from this tree.

use super::groups::{
    FAKE_INPUT, KEY_PRESS, KEY_RELEASE, admit_surface, discover, flushed_deliveries, focus_window,
    input_event, newest_admission, observer_window,
};
use super::{COOKIE, Client, Instance, Order};
use sophia_session::private_input::PrivateInputGrantPolicy;
use sophia_x_authority::{PrivateAdmittedConnection, XServerFrontendClientId};
use std::time::Instant;

/// One round's live pair and the window the observer owns.
struct Round {
    observer: Client,
    injector: Client,
    window: u32,
    opcode: u8,
    observer_row: PrivateAdmittedConnection,
}

/// Connect an observer and an injector, admit the observer's window as a
/// surface and give it the focus. This is `fake_input_effects`'s own opening,
/// kept identical so a difference here is a difference in the departure and
/// not in how the round was set up.
fn round(instance: &Instance, order: Order, seen: &mut Vec<XServerFrontendClientId>) -> Round {
    let mut observer = instance.connect(order, Some(COOKIE));
    let observer_row = newest_admission(instance, &mut observer, seen);
    let window = observer_window(&mut observer);
    admit_surface(instance, &mut observer, window);
    focus_window(&mut observer, window);
    let mut injector = instance.connect(order, Some(COOKIE));
    let _injector_row = newest_admission(instance, &mut injector, seen);
    let opcode = discover(&mut injector);
    Round {
        observer,
        injector,
        window,
        opcode,
        observer_row,
    }
}

/// Inject one press and release and require both at the observer, by the
/// wire and by the service's own receipts. Returns the keycodes delivered.
fn key_reaches_observer(instance: &Instance, round: &mut Round, keycode: u8) -> (u8, u8) {
    let order = round.injector.order();
    let body = |kind, detail| super::groups::fake_input(order, kind, detail, 0, 0, 0, 0);
    let mut receipts = Vec::new();
    round
        .injector
        .send(round.opcode, FAKE_INPUT, &body(KEY_PRESS, keycode));
    round
        .injector
        .send(round.opcode, FAKE_INPUT, &body(KEY_RELEASE, keycode));
    round.injector.sync();
    let press = input_event(
        instance,
        &mut round.observer,
        &mut receipts,
        KEY_PRESS,
        round.window,
    );
    let release = input_event(
        instance,
        &mut round.observer,
        &mut receipts,
        KEY_RELEASE,
        round.window,
    );
    flushed_deliveries(instance, &mut receipts, round.observer_row.client, 2);
    (press[1], release[1])
}

/// The rows the boundary holds, as `(client, closed, lifecycle_open)`.
fn rows(instance: &Instance) -> Vec<(u64, bool, bool)> {
    instance
        .with_handle(|handle| handle.admitted())
        .expect("the boundary is readable")
        .iter()
        .map(|row| (row.client.raw(), row.closed, row.lifecycle_open))
        .collect()
}

/// A key injected after an earlier pair departed reaches the live observer,
/// on the instance that served the departed pair.
///
/// The first round establishes that this instance delivers at all, so a
/// second-round failure is about the departure and not about the fixture.
/// Both rounds use the same byte order: the order was never the mechanism,
/// it was only how `fake_input_effects` happened to reach a second round.
pub fn a_key_after_a_departure_reaches_the_live_observer() {
    let instance = Instance::start(
        "departure-witness",
        PrivateInputGrantPolicy::EnabledWithVerifiedEvidence,
    );
    let mut seen = Vec::new();

    let mut first = round(&instance, Order::Little, &mut seen);
    assert_eq!(
        key_reaches_observer(&instance, &mut first, 38),
        (38, 38),
        "the instance delivers to its first observer, before any departure"
    );
    let departed = first.observer_row.client.raw();

    drop(first);
    let dropped = Instant::now();

    // THE STATE AT THE MOMENT THE NEXT ROUND IS BUILT, NOT A RACE AGAINST IT.
    // Whether the departed rows are still open here depends on collection,
    // which is timing this cannot pin; it is recorded for the report and
    // deliberately not asserted. The assertion that follows holds either way,
    // which is the point: a key must not answer for a connection that has
    // gone, whether or not its row has been collected yet.
    let at_second_round = rows(&instance);
    let still_open = at_second_round.iter().filter(|row| !row.1).count();

    let mut second = round(&instance, Order::Big, &mut seen);
    assert_ne!(
        second.observer_row.client.raw(),
        departed,
        "the second observer must be a new connection, not the departed one"
    );
    assert_eq!(
        key_reaches_observer(&instance, &mut second, 39),
        (39, 39),
        "a key injected after the first pair departed reaches the live \
         observer; rows {at_second_round:?} ({still_open} still open) \
         {:?} after the departure",
        dropped.elapsed()
    );

    drop(second);
    let outcome = instance.finish();
    assert!(
        outcome.failure.is_none(),
        "the invocation must not fail: {:?}",
        outcome.failure
    );
}

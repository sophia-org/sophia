//! M5 acceptance: the XTEST adapter's eight obligation groups, run through
//! `cargo xtask check m5-acceptance`. Every group test carries `#[ignore]` so
//! an ordinary `cargo test` cannot claim acceptance; the gate includes them.
//!
//! The groups land with the mechanism they prove. Until a group's test exists
//! here, its inventory row reports NOT_RUN as a bound test absent from the
//! built binary, which is the honest state and not a pass.
#![cfg(unix)]

#[path = "support/xtest_acceptance/mod.rs"]
mod support;

/// The record shape the gate parses, pinned where it is produced. Not a
/// group test: it makes no claim about XTEST behaviour.
#[test]
fn evidence_record_names_the_group_and_every_subcase() {
    let line = support::record(
        "grab_control",
        &["strict_boolean", "impervious_during_server_grab"],
        3,
        3,
        2,
    );
    let body = line
        .strip_prefix("sophia_m5_acceptance ")
        .expect("the gate keys the record by this prefix");
    assert!(body.starts_with("{\"schema\":1,\"case\":\"M5.grab_control\","));
    assert!(body.contains(
        "\"subcases\":{\"strict_boolean\":\"PASS\",\"impervious_during_server_grab\":\"PASS\"}"
    ));
    assert!(body.contains(
        "\"cleanup\":{\"actors_started\":3,\"actors_collected\":3,\"pending_actors\":0,\"complete\":true}"
    ));
    assert!(body.contains("\"real_session_invocations\":2"));
    assert!(body.ends_with('}'));
    let mut evidence = support::Evidence::default();
    // Nothing was started, so nothing may be claimed: emitting is refused.
    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        std::mem::take(&mut evidence).emit("grab_control", &["strict_boolean"])
    }));
    assert!(refused.is_err());
}

#[test]
#[ignore = "run through cargo xtask check m5-acceptance"]
fn processing_barrier() {
    support::adapter::processing_barrier();
}

#[test]
#[ignore = "run through cargo xtask check m5-acceptance"]
fn cancellation_half_close() {
    support::adapter::cancellation_half_close();
}

// This lane's groups are appended here and the top of the file is left for
// the adapter lane's, so the two never share a hunk.

#[test]
#[ignore = "run through cargo xtask check m5-acceptance"]
fn registration_admission() {
    support::groups::registration_admission();
}

#[test]
#[ignore = "run through cargo xtask check m5-acceptance"]
fn version_negotiation() {
    support::groups::version_negotiation();
}

#[test]
#[ignore = "run through cargo xtask check m5-acceptance"]
fn fake_input_encoding() {
    support::groups::fake_input_encoding();
}

#[test]
#[ignore = "run through cargo xtask check m5-acceptance"]
fn fake_input_effects() {
    support::groups::fake_input_effects();
}

#[test]
#[ignore = "run through cargo xtask check m5-acceptance"]
fn cursor_comparison() {
    support::groups::cursor_comparison();
}

#[test]
#[ignore = "run through cargo xtask check m5-acceptance"]
fn grab_control() {
    support::groups::grab_control();
}

/// t138. A private instance that has seen `max_concurrent_clients`
/// departures keeps admitting. Not a group: it makes no claim about XTEST
/// behaviour, so it is not ignored and runs with ordinary `cargo test`.
///
/// The shape is the probe that measured the defect, kept as the witness
/// that it is gone. Ten rounds on one instance admitting four: each round
/// connects an admitted client, parks a FakeInput behind an infinite delay
/// with a round trip queued behind it, drops the connection, and waits for
/// its rows to close. The fifth round used to fail at "no retained place",
/// then at "no evidence custody"; every round is answered now because a
/// departed connection's place and custody come back during the run, in the
/// service frame's idle window, rather than at shutdown.
#[test]
fn departures_are_reclaimed_during_the_run_and_the_instance_keeps_admitting() {
    use std::time::{Duration, Instant};
    let instance = support::Instance::start(
        "reclaim",
        sophia_session::private_input::PrivateInputGrantPolicy::EnabledWithVerifiedEvidence,
    );
    let order = support::Order::Little;
    let rows = |instance: &support::Instance| {
        instance
            .with_handle(|h| h.admitted())
            .unwrap()
            .iter()
            .map(|r| (r.client.raw(), r.closed, r.lifecycle_open))
            .collect::<Vec<_>>()
    };
    // MORE THAN TWICE THE CAPACITY, so the instance is driven through two
    // full turnovers of every place it has.
    for round in 0..10 {
        let mut held = support::Client::connect(instance.socket(), order, Some(support::COOKIE))
            .unwrap_or_else(|error| {
                panic!(
                    "round {round}: connect refused: {error:?}; rows {:?}",
                    rows(&instance)
                )
            });
        let mut query = Vec::new();
        query.extend(order.u16(5));
        query.extend([0, 0]);
        query.extend(b"XTEST");
        query.extend([0, 0, 0]);
        let sequence = held.send(98, 0, &query);
        let opcode = match held.try_answer(Duration::from_secs(3)) {
            Some(support::Answer::Reply(reply)) => {
                assert_eq!(order.read16(&reply[2..]), sequence);
                reply[9]
            }
            other => panic!(
                "round {round}: connection {} was not answered: {other:?}; rows {:?}",
                round + 1,
                rows(&instance)
            ),
        };
        // Parked: an infinite delay with a round trip queued behind it, which
        // is the departure that used to leave the most behind.
        let mut body = vec![2u8, 8];
        body.extend(order.u16(0));
        body.extend(order.u32(u32::MAX));
        body.extend([0; 24]);
        held.send(opcode, 2, &body);
        held.send(43, 0, &[]);
        assert!(
            held.try_answer(Duration::from_millis(300)).is_none(),
            "the park held"
        );
        drop(held);
        let dropped = Instant::now();
        loop {
            let now = rows(&instance);
            let open = now.iter().filter(|r| !r.1).count();
            if open == 0 {
                break;
            }
            assert!(
                dropped.elapsed() < Duration::from_secs(5),
                "round {round}: departure not collected: {now:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let outcome = instance.finish();
    assert!(
        outcome.failure.is_none(),
        "the invocation must not fail: {:?}",
        outcome.failure
    );
}

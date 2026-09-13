use sophia_x_authority::{ReadyClass, ReadyRefusal, ReadyStream};

fn stream() -> ReadyStream<&'static str> {
    ReadyStream::new(8, 2)
}

#[test]
fn every_class_shares_one_order_rather_than_one_queue_each() {
    let mut ready = stream();
    // Admitted from five different producers, interleaved.
    let order = [
        (ReadyClass::RoutedInput, "press"),
        (ReadyClass::ConnectionMutation, "grab"),
        (ReadyClass::Control, "focus"),
        (ReadyClass::Thawed, "unfrozen"),
        (ReadyClass::Cleanup, "teardown"),
    ];
    for (class, payload) in order {
        ready.admit(class, payload).expect("capacity");
    }

    // They leave in the order they were admitted, not grouped by producer.
    // Grouping is what five queues visited in a loop would produce.
    let drained: Vec<_> = std::iter::from_fn(|| ready.take_next())
        .map(|(_, class, payload)| (class, payload))
        .collect();
    assert_eq!(drained, order);
}

#[test]
fn a_sequence_a_caller_is_told_is_one_the_consumer_can_already_see() {
    let mut ready = stream();
    let first = ready.admit(ReadyClass::RoutedInput, "a").expect("capacity");

    // No reserve-then-publish: the entry is there the moment its position
    // exists, so there is no hole for a consumer to reach.
    let (sequence, _, payload) = ready.take_next().expect("the admitted entry");
    assert_eq!(sequence, first);
    assert_eq!(payload, "a");
}

#[test]
fn positions_rise_and_are_never_reused() {
    let mut ready = stream();
    let mut seen = Vec::new();
    for _ in 0..6 {
        seen.push(
            ready
                .admit(ReadyClass::RoutedInput, "x")
                .expect("capacity")
                .raw(),
        );
        ready.take_next().expect("drained immediately");
    }
    let mut sorted = seen.clone();
    sorted.dedup();
    assert_eq!(seen, sorted, "a position must not repeat");
    assert!(seen.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn cleanup_keeps_capacity_that_ordinary_work_cannot_take() {
    let mut ready = ReadyStream::new(4, 2);
    // Ordinary work fills everything except the reserve.
    for _ in 0..2 {
        ready
            .admit(ReadyClass::RoutedInput, "press")
            .expect("capacity");
    }
    assert_eq!(
        ready.admit(ReadyClass::RoutedInput, "press"),
        Err(ReadyRefusal::AtCapacity)
    );
    assert_eq!(
        ready.admit(ReadyClass::Control, "focus"),
        Err(ReadyRefusal::AtCapacity)
    );

    // Cleanup still fits. Refused input is a caller told no; refused cleanup
    // is state nobody comes back to release.
    ready
        .admit(ReadyClass::Cleanup, "teardown")
        .expect("the reserve");
    ready
        .admit(ReadyClass::Cleanup, "teardown")
        .expect("the reserve");
    assert_eq!(
        ready.admit(ReadyClass::Cleanup, "teardown"),
        Err(ReadyRefusal::AtCapacity),
        "the reserve is finite too"
    );
}

#[test]
fn delayed_work_takes_its_position_when_it_becomes_runnable() {
    let mut ready = stream();
    // A press is frozen, so it is not admitted. Other work runs meanwhile.
    ready
        .admit(ReadyClass::RoutedInput, "later")
        .expect("capacity");
    ready
        .admit(ReadyClass::Control, "meanwhile")
        .expect("capacity");
    let drained: Vec<_> = std::iter::from_fn(|| ready.take_next())
        .map(|(_, _, payload)| payload)
        .collect();
    assert_eq!(drained, ["later", "meanwhile"]);

    // The thawed work is admitted now, so it sits after what ran while it
    // waited. A position held from request time would put it in front.
    let thawed = ready
        .admit(ReadyClass::Thawed, "unfrozen")
        .expect("capacity");
    let after = ready
        .admit(ReadyClass::RoutedInput, "newest")
        .expect("capacity");
    assert!(thawed < after);
    assert_eq!(
        ready.take_next().expect("the thawed entry").2,
        "unfrozen",
        "thawed work runs before work admitted after it, and after work admitted before it"
    );
}

#[test]
fn one_producers_order_survives_other_producers_interleaving() {
    let mut ready = ReadyStream::new(16, 2);
    for step in 0..4 {
        ready
            .admit(ReadyClass::RoutedInput, ["a1", "a2", "a3", "a4"][step])
            .expect("capacity");
        ready
            .admit(ReadyClass::Control, ["b1", "b2", "b3", "b4"][step])
            .expect("capacity");
    }

    let drained: Vec<_> = std::iter::from_fn(|| ready.take_next())
        .map(|(_, _, payload)| payload)
        .collect();
    let from_a: Vec<_> = drained.iter().filter(|p| p.starts_with('a')).collect();
    assert_eq!(from_a, [&"a1", &"a2", &"a3", &"a4"]);
    let from_b: Vec<_> = drained.iter().filter(|p| p.starts_with('b')).collect();
    assert_eq!(from_b, [&"b1", &"b2", &"b3", &"b4"]);
}

#[test]
fn positions_are_refused_rather_than_reused_when_exhausted() {
    // Wrapping would hand out a position an earlier entry still answers to.
    assert_eq!(
        sophia_x_authority::next_ready_sequence(u64::MAX),
        Err(ReadyRefusal::SequencesExhausted)
    );
    assert_eq!(sophia_x_authority::next_ready_sequence(41), Ok(42));
}

// Real private service, real producer port and ordered writer. Native/rendering
// authority is supplied by the existing device-hidden fixture.
#[test]
fn overlapping_releases_join_the_source_receipt_then_flush_in_order_and_allow_repress() {
    let (launched, socket_path) = launch_producing("producer-shared-receipt", 9611, 4);
    launched.access.await_ready(Duration::from_secs(15)).expect("readiness");
    let (mut client, surface, sequence, custody, window) =
        admitted_connection(&launched, &socket_path, 0x0e81);
    let client_id = custody.cleanup_record().client;
    let owner = Arc::clone(&launched.owner);
    let lease = owner.lease();
    let control = launched.access.control_producer(&lease).expect("control producer");
    let focus = apply_focus(&launched, &control, &mut client, client_id, surface, 97101);
    let ingress = launched.access.ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .expect("ingress");
    let mut submitted = Vec::new();
    let mut events = Vec::new();
    for (delivery, button) in [(97110, 272), (97111, 274)] {
        submitted.push(ingress.submit(&lease, button_to(surface,
            XAuthorityInputDeliveryId::from_raw(delivery), button, true)).is_ok());
        events.push(read_event(&mut client, 3));
    }
    submitted.push(ingress.submit(&lease, button_to(surface,
        XAuthorityInputDeliveryId::from_raw(97112), 272, false)).is_ok());
    // An earlier release has no native proof while the other button holds the
    // shared activation. Its own recipient half must remain unanswered too.
    let early_wire = read_event(&mut client, 1);
    let early_answer = delivery_cell(&launched.registry, 97112)
        .and_then(|cell| cell.answer());
    submitted.push(ingress.submit(&lease, button_to(surface,
        XAuthorityInputDeliveryId::from_raw(97113), 274, false)).is_ok());
    events.push(read_event(&mut client, 3));
    events.push(read_event(&mut client, 3));
    let answers: Vec<_> = [97110, 97111, 97112, 97113].into_iter().map(|delivery| {
        delivery_cell(&launched.registry, delivery).and_then(|cell| {
            waited_for(|| cell.answer().is_some());
            cell.answer().map(|answer| answer.outcome)
        })
    }).collect();

    // Same ingress/grant: the ledger must actually settle the old debt before
    // this new hold can be admitted. A wire enqueue alone cannot authorize it.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let repressed = loop {
        if ingress.submit(&lease, button_to(surface,
            XAuthorityInputDeliveryId::from_raw(97114), 272, true)).is_ok() {
            break true;
        }
        if std::time::Instant::now() >= deadline { break false; }
        std::thread::yield_now();
    };
    let repress = read_event(&mut client, 3);
    let released = ingress.submit(&lease, button_to(surface,
        XAuthorityInputDeliveryId::from_raw(97115), 272, false)).is_ok();
    let rerelease = read_event(&mut client, 3);
    launched.commands.send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("service command");
    let registry = launched.registry.clone();
    let outcome = produced_outcome(launched, "shared activation receipt");
    let seen = observe_worker(&custody, &registry);
    // All behavioral comparisons follow collection, including failure cases.
    assert_eq!(focus, (Some(XAuthorityControlOutcome::Delivered),
        Some(expected_focus_in(sequence, window))));
    assert!(submitted.iter().all(|accepted| *accepted), "{submitted:?}");
    assert_eq!(early_wire, None);
    assert_eq!(early_answer, None);
    assert_eq!(events, vec![
        Some(expected_chord_event(true, sequence, window, 1, 0)),
        Some(expected_chord_event(true, sequence, window, 2, 1 << 8)),
        Some(expected_chord_event(false, sequence, window, 1, (1 << 8) | (1 << 9))),
        Some(expected_chord_event(false, sequence, window, 2, 1 << 9)),
    ]);
    assert_eq!(answers, vec![Some(XAuthorityInputDeliveryOutcome::Flushed); 4]);
    assert!(repressed && released, "same grant becomes usable after both proofs");
    assert_eq!(repress, Some(expected_button_event(true, sequence, window, 1)));
    assert_eq!(rerelease, Some(expected_button_event(false, sequence, window, 1)));
    assert_collected_running(&seen, "shared activation receipt");
    assert!(outcome.retained_holds.is_empty(), "all physical releases recorded");
    assert_eq!(outcome.order.unwrap().activations_joined, 1);
    let _ = std::fs::remove_file(socket_path);
}

// Fixture-level budget discriminator: source-created records, no ordered
// worker and no wire receipt supplied. A scan must resume across visits and
// transfer with the inventory, then become idle until source work changes.
#[test]
fn shared_activation_scan_is_bounded_retained_and_idle_without_new_source_work() {
    let mut fixture = prepared_ordered_fixture(XServerFrontendClientId(9731));
    for (id, button) in [(97310, 272), (97320, 274), (97330, 273)] {
        attempt_release(&mut fixture, id, button);
    }
    let private = fixture.runner.frontend.as_mut().unwrap();
    let terminal = &mut private.terminal;
    assert_eq!(terminal.settling.len(), 3);
    assert_eq!(terminal.shared_activation.visit(&mut terminal.settling), Some((8, 0)));
    let mut retained = terminal.hand_over();
    assert_eq!(retained.shared_activation.visit(&mut retained.settling), Some((1, 0)));
    assert_eq!(retained.shared_activation.visit(&mut retained.settling), None);
    assert!(!retained.shared_activation.pending());
    retained.shared_activation.invalidate();
    assert_eq!(retained.shared_activation.visit(&mut retained.settling), Some((8, 0)));
    // Restore the whole inventory to its actual owner, including the remaining
    // scan and all source obligations, before ordinary fixture destruction.
    *terminal = retained;
}

// The reserved emergency chord cannot be supplied by a synthetic source.
// Recovery recognises it from physical devices in the guard's own process,
// which synthetic input never reaches; what a synthetic source could still
// do is hand Ctrl-Alt-Backspace to a client as ordinary key events, which
// the engine never lets a policy client bind. The executor refuses the press
// that would complete the chord, and nothing else about it: either modifier
// is held, an ordinary key under both is delivered, and Backspace under one
// of them is delivered.

/// Submit one route through a grant whose one cell may still be held by the
/// previous request's unobserved outcome; busy is retried within the bound.
#[cfg(unix)]
fn submit_key(
    ingress: &PrivateIngress,
    lease: &PrivateServiceLease<'_>,
    surface: SurfaceId,
    id: u64,
    evdev: u32,
    pressed: bool,
) {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut route = key_service_route(surface, id, evdev, pressed);
    loop {
        match ingress.submit(lease, route) {
            Ok(_) => return,
            Err(crate::PrivateSendError::Saturated(returned)) => {
                assert!(Instant::now() < deadline, "the grant's cell was not released before delivery {id}");
                std::thread::yield_now();
                route = returned;
            }
            Err(other) => panic!("a live producer's submission was refused: {other:?}"),
        }
    }
}

/// The exact core event for a key at the pointer's starting position, the
/// root's centre, with the pre-transition modifier state given.
#[cfg(unix)]
fn key_at_centre(sequence: u16, window: u32, detail: u8, pressed: bool, state: u16) -> [u8; 32] {
    let mut event = expected_key_service_event(sequence, window, detail, pressed, state);
    event[20..28].copy_from_slice(&[128, 2, 104, 1, 128, 2, 104, 1]);
    event
}

#[cfg(unix)]
#[test]
fn a_synthetic_press_that_would_complete_the_reserved_chord_is_refused_and_nothing_else_is() {
    let (launched, socket_path) = launch_producing("reserved-chord", 9631, 4);
    launched.access.await_ready(Duration::from_secs(15)).unwrap();
    let mut client = connect_private_client(&socket_path);
    let window = handshake_ids(&mut client) | 0x0a31;
    let (surface, sequence) = selecting_window(
        &mut client,
        &launched.transactions,
        window,
        (1 << 0) | (1 << 1) | (1 << 21),
    );
    let client_id = wait_attached(&launched.registry).cleanup_record().client;
    let lease = launched.owner.lease();
    // KEYS FOLLOW FOCUS, so the window is focused through the service's own
    // control producer and told so.
    launched
        .access
        .control_producer(&lease)
        .unwrap()
        .submit(&lease, XAuthorityClientControlCommand {
            client: client_id,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(96310),
                surface,
            },
        })
        .unwrap();
    assert_eq!(
        ack_for(&launched.acks, 96310).unwrap().acknowledgement.outcome,
        XAuthorityControlOutcome::Delivered
    );
    assert_eq!(read_event(&mut client, 3), Some(expected_focus_in(sequence, window)));
    let ingress = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .unwrap();
    const CONTROL: u16 = 1 << 2;
    const MOD1: u16 = 1 << 3;

    // EITHER MODIFIER MAY BE HELD SYNTHETICALLY: Ctrl_L then Alt_L, each
    // delivered, the second with Control already in the state.
    submit_key(&ingress, &lease, surface, 96311, 29, true);
    assert_eq!(read_event(&mut client, 3), Some(key_at_centre(sequence, window, 37, true, 0)));
    submit_key(&ingress, &lease, surface, 96312, 56, true);
    assert_eq!(read_event(&mut client, 3), Some(key_at_centre(sequence, window, 64, true, CONTROL)));

    // THE PRESS THAT WOULD COMPLETE THE CHORD IS REFUSED: nothing reaches the
    // wire for it.
    submit_key(&ingress, &lease, surface, 96313, 14, true);
    assert_eq!(read_event(&mut client, 1), None, "Ctrl-Alt-Backspace is not handed to a client");

    // AND ONLY THAT PRESS. An ordinary key under both modifiers is delivered
    // with the state the refusal left untouched, so the executor is neither
    // stopped nor confused by what it refused.
    submit_key(&ingress, &lease, surface, 96314, 30, true);
    assert_eq!(read_event(&mut client, 3), Some(key_at_centre(sequence, window, 38, true, CONTROL | MOD1)));
    submit_key(&ingress, &lease, surface, 96315, 30, false);
    assert_eq!(read_event(&mut client, 3), Some(key_at_centre(sequence, window, 38, false, CONTROL | MOD1)));

    // BACKSPACE UNDER ONE MODIFIER IS AN ORDINARY KEY: Alt released, the same
    // press is delivered as Ctrl-Backspace.
    submit_key(&ingress, &lease, surface, 96316, 56, false);
    assert_eq!(read_event(&mut client, 3), Some(key_at_centre(sequence, window, 64, false, CONTROL | MOD1)));
    submit_key(&ingress, &lease, surface, 96317, 14, true);
    assert_eq!(read_event(&mut client, 3), Some(key_at_centre(sequence, window, 22, true, CONTROL)));
    submit_key(&ingress, &lease, surface, 96318, 14, false);
    assert_eq!(read_event(&mut client, 3), Some(key_at_centre(sequence, window, 22, false, CONTROL)));
    submit_key(&ingress, &lease, surface, 96319, 29, false);
    assert_eq!(read_event(&mut client, 3), Some(key_at_centre(sequence, window, 37, false, CONTROL)));

    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    let outcome = produced_outcome(launched, "reserved chord");
    let order = outcome.order.unwrap();
    assert_eq!(
        (order.taken, order.refused, order.dispatched),
        (10, 1, 8),
        "the focus control and nine keys taken, one key refused, eight keys delivered"
    );
    assert_eq!(order.last_refusal, Some(PrivateExecutionRefusal::ReservedChord));
    assert!(
        outcome.retained_holds.is_empty() && outcome.store_holds.is_empty(),
        "every key was released as itself; the refused press held nothing"
    );
    let _ = std::fs::remove_file(socket_path);
}

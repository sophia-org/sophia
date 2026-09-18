use super::*;

#[test]
fn negotiated_payload_limit_bounds_text_before_receipt_or_revision_transfer() {
    let mut r = empty();
    let mut small = limits();
    small.max_width_px = 64;
    small.max_chunk_bytes = 256;
    small.max_frame_payload = 304;
    let mut p = Peer::connected_with_limits(&mut r, small);
    let a = p.allocation(&mut r);
    p.upload(&mut r);
    let c = catalog();
    let focus = initial_focus(&mut r, &mut p, &a, &c);
    assert_eq!(
        p.transport.issue_native_launcher_input(
            &mut r,
            focus,
            tx(79),
            NativeLauncherInputKind::Text,
            &"a".repeat(173),
            1
        ),
        Err(ShellTransportError::WrongContentRecord)
    );
    assert_eq!(p.transport.native_launcher_state().unwrap().1, 1);
    no_frame(&mut p, &mut r);
    let event = input(
        &mut p,
        &mut r,
        NativeLauncherInputKind::Text,
        &"a".repeat(172),
        1,
    );
    assert_eq!(event.event.state_revision, 2);
    assert_eq!(event.text.len(), 172);
}

#[test]
fn negotiated_receipt_limit_also_bounds_pending_enter() {
    let mut r = empty();
    let mut small = limits();
    small.max_pending_actions = 1;
    let mut p = Peer::connected_with_limits(&mut r, small);
    let a = p.allocation(&mut r);
    p.upload(&mut r);
    let c = catalog();
    let focus = initial_focus(&mut r, &mut p, &a, &c);
    let first = input(&mut p, &mut r, NativeLauncherInputKind::Text, "x", 10);
    assert_eq!(
        p.transport.issue_native_launcher_input(
            &mut r,
            focus,
            tx(80),
            NativeLauncherInputKind::Next,
            "",
            11
        ),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    assert!(
        p.transport
            .issue_native_launcher_input(
                &mut r,
                focus,
                tx(81),
                NativeLauncherInputKind::Accept,
                "",
                11
            )
            .is_err()
    );
    assert_eq!(p.transport.native_launcher_state().unwrap().1, 2);
    no_frame(&mut p, &mut r);
    assert!(ack(&mut p, &mut r, first.event, 1));
    assert_eq!(
        p.transport
            .issue_native_launcher_input(
                &mut r,
                focus,
                tx(82),
                NativeLauncherInputKind::Accept,
                "",
                12
            )
            .unwrap(),
        None
    );
    // Pending Enter reserves a future receipt rather than increasing the cap.
    // A later edit may cancel that unsent intent and reuse its one slot.
    let next = input(&mut p, &mut r, NativeLauncherInputKind::Next, "", 13);
    assert_eq!(next.event.state_revision, 3);
}

#[test]
fn zero_negotiated_actions_do_not_inherit_the_array_capacity() {
    let mut r = empty();
    let mut small = limits();
    small.max_pending_actions = 0;
    let mut p = Peer::connected_with_limits(&mut r, small);
    let a = p.allocation(&mut r);
    p.upload(&mut r);
    let c = catalog();
    let focus = initial_focus(&mut r, &mut p, &a, &c);
    assert_eq!(
        p.transport.issue_native_launcher_input(
            &mut r,
            focus,
            tx(83),
            NativeLauncherInputKind::Accept,
            "",
            1
        ),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    assert_eq!(p.transport.native_launcher_state().unwrap().1, 1);
    no_frame(&mut p, &mut r);
}

#[test]
fn native_receipts_and_pointer_cancellation_share_the_pending_limit() {
    let mut r = empty();
    let mut small = limits();
    small.max_pending_actions = 1;
    let mut p = Peer::connected_with_limits(&mut r, small);
    let a = p.allocation(&mut r);
    p.upload(&mut r);
    let c = catalog();
    let focus = initial_focus(&mut r, &mut p, &a, &c);
    let edit = input(&mut p, &mut r, NativeLauncherInputKind::Text, "x", 10);
    // This supplies the Session pointer decision, testing only the real shared
    // transport inventory. It does not establish pointer target authority.
    let mut action = ContentAction {
        grant: GRANT,
        output: OUTPUT,
        candidate_generation: focus.candidate_generation,
        presentation_epoch: focus.presentation_epoch,
        interaction_generation: focus.interaction_generation,
        allocation: focus.allocation,
        target_id: 1,
        target_generation: 1,
        action_id: 1,
        event_id: 900,
        kind: 1,
        reason: 0,
    };
    assert_eq!(
        p.transport.send_content_action(&mut r, tx(85), &action),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    assert!(ack(&mut p, &mut r, edit.event, 1));
    p.transport
        .send_content_action(&mut r, tx(85), &action)
        .unwrap();
    assert_eq!(
        p.transport.issue_native_launcher_input(
            &mut r,
            focus,
            tx(86),
            NativeLauncherInputKind::Next,
            "",
            11
        ),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    action.kind = 3;
    action.reason = ContentReason::Cancelled as u16;
    p.transport
        .send_content_action(&mut r, tx(87), &action)
        .unwrap();
    p.transport.poll_io(&mut r).unwrap();
    for kind in [1, 3] {
        assert!(matches!(decode_shell_content_frame(&p.read()).unwrap().1,
            ShellContentRecord::Action(v) if v.kind == kind));
    }
    let next = input(&mut p, &mut r, NativeLauncherInputKind::Next, "", 12);
    assert_eq!(next.event.state_revision, 3);
    assert!(ack(&mut p, &mut r, next.event, 1));
    assert_eq!(
        p.transport
            .issue_native_launcher_input(
                &mut r,
                focus,
                tx(88),
                NativeLauncherInputKind::Accept,
                "",
                13
            )
            .unwrap(),
        None
    );
    action.kind = 1;
    action.reason = 0;
    action.event_id += 1;
    assert_eq!(
        p.transport.send_content_action(&mut r, tx(89), &action),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    no_frame(&mut p, &mut r);
}

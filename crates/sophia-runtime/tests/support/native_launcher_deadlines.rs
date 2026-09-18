use super::*;

fn timed_out(p: &mut Peer, r: &mut ContentEpochRegistry, focus: NativeLauncherBinding) {
    assert!(p.transport.native_launcher_state().is_none());
    assert!(p.transport.native_launcher_focus().is_none());
    p.transport.poll_io(r).unwrap();
    assert!(
        matches!(decode_shell_native_launcher_frame(&p.read()).unwrap().1,
        ShellNativeLauncherRecord::FocusRevoked(v)
        if v.binding == focus && v.reason == ContentReason::Timeout as u16)
    );
    assert!(
        matches!(decode_shell_native_launcher_frame(&p.read()).unwrap().1,
        ShellNativeLauncherRecord::Closed(v)
        if v.opening == opening().opening && v.reason == ContentReason::Timeout as u16)
    );
    no_frame(p, r);
}

#[test]
fn unacknowledged_input_expires_exactly_at_its_own_deadline() {
    let (mut r, mut p, a, c) = setup();
    let focus = initial_focus(&mut r, &mut p, &a, &c);
    input(&mut p, &mut r, NativeLauncherInputKind::Text, "x", 10);
    let timeout = u64::from(limits().action_ack_timeout_ms) * 1000;
    assert!(
        !p.transport
            .service_native_launcher_deadlines(&r, opening(), tx(70), timeout + 9)
            .unwrap()
    );
    assert!(
        p.transport
            .service_native_launcher_deadlines(&r, opening(), tx(70), timeout + 10)
            .unwrap()
    );
    timed_out(&mut p, &mut r, focus);
}

#[test]
fn acknowledged_edit_does_not_hide_a_waiting_enter_timeout() {
    let (mut r, mut p, a, c) = setup();
    let focus = initial_focus(&mut r, &mut p, &a, &c);
    let edit = input(&mut p, &mut r, NativeLauncherInputKind::Text, "x", 10);
    assert!(ack(&mut p, &mut r, edit.event, 1));
    assert_eq!(
        p.transport
            .issue_native_launcher_input(&r, focus, tx(71), NativeLauncherInputKind::Accept, "", 11)
            .unwrap(),
        None
    );
    let timeout = u64::from(limits().presentation_timeout_ms) * 1000;
    assert!(
        !p.transport
            .service_native_launcher_deadlines(&r, opening(), tx(72), timeout + 10)
            .unwrap()
    );
    assert!(
        p.transport
            .service_native_launcher_deadlines(&r, opening(), tx(72), timeout + 11)
            .unwrap()
    );
    timed_out(&mut p, &mut r, focus);
}

#[test]
fn clock_or_opening_mismatch_cannot_expire_successor_and_new_input_checks_deadline() {
    let (mut r, mut p, a, c) = setup();
    let focus = initial_focus(&mut r, &mut p, &a, &c);
    input(&mut p, &mut r, NativeLauncherInputKind::Text, "x", 10);
    assert!(
        !p.transport
            .service_native_launcher_deadlines(&r, opening(), tx(73), 20)
            .unwrap()
    );
    assert!(
        p.transport
            .service_native_launcher_deadlines(&r, opening(), tx(74), 19)
            .is_err()
    );
    let mut wrong = opening();
    wrong.opening += 1;
    assert!(
        p.transport
            .service_native_launcher_deadlines(&r, wrong, tx(74), u64::MAX)
            .is_err()
    );
    assert_eq!(p.transport.native_launcher_focus(), Some(focus));
    let timeout = u64::from(limits().action_ack_timeout_ms) * 1000;
    assert!(
        p.transport
            .issue_native_launcher_input(
                &r,
                focus,
                tx(75),
                NativeLauncherInputKind::Next,
                "",
                timeout + 10
            )
            .is_err()
    );
    timed_out(&mut p, &mut r, focus);
}

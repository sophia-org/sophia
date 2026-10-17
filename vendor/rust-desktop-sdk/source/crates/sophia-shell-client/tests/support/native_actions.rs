use super::*;

fn action(kind: u16, row: u64) -> ContentAction {
    let b = b();
    ContentAction {
        grant: b.grant,
        output: b.output,
        candidate_generation: b.candidate_generation,
        presentation_epoch: b.presentation_epoch,
        interaction_generation: b.interaction_generation,
        allocation: b.allocation,
        target_id: 2,
        target_generation: 1,
        action_id: row,
        event_id: 123,
        kind,
        reason: if kind == 3 { 1 } else { 0 },
    }
}
fn apply(l: &mut NativeLauncherLifecycle, action: ContentAction) {
    l.observe(&NativeLauncherObservation::Content(
        tx(70),
        ShellContentRecord::Action(action),
    ))
    .unwrap();
}

#[test]
fn pointer_activates_nonselected_displayed_row_and_echoes_exact_ack() {
    let (mut c, mut p, mut l) = setup();
    let mut candidate = candidate();
    candidate.candidate.rows.push(2);
    let mut target = candidate.candidate.candidate.targets[0].clone();
    target.action_id = 2;
    target.target_id = 2;
    candidate.candidate.candidate.targets.push(target);
    let a = c
        .enqueue_native_candidate_with_lifecycle(&mut l, &candidate)
        .unwrap();
    commit(&mut c, &mut p, a.first);
    l.observe(&outcome(2)).unwrap();
    observe(&mut l, ShellNativeLauncherRecord::Focus(b()));
    let event = action(1, 2);
    apply(&mut l, event.clone());
    assert!(l.action().unwrap().1.can_activate);
    assert!(l.observe(&publication(ShellFileKind::Outputs, 2)).is_err());
    drop(
        c.prepare_native_action(&mut l, tx(71), 1, Some(tx(70)))
            .unwrap(),
    );
    assert_eq!(l.action().unwrap().0, &event);
    assert!(
        c.prepare_native_action(&mut l, tx(70), 1, Some(tx(70)))
            .is_err()
    );
    let a = c
        .prepare_native_action(&mut l, tx(71), 1, Some(tx(70)))
        .unwrap()
        .commit();
    assert_eq!(a.count, 2);
    assert!(l.action().is_none());
    let bytes = commit(&mut c, &mut p, a.first);
    let activation =
        decode_shell_file_native_launcher_transaction(&bytes, ShellFileKind::NativeActivate)
            .unwrap();
    let expected = NativeLauncherActivation {
        event: NativeLauncherEvent {
            binding: b(),
            event_id: event.event_id,
            state_revision: 1,
        },
        cause: 2,
        slot: 2,
    };
    assert_eq!(activation.transaction, tx(70));
    assert_eq!(
        activation.record,
        ShellNativeLauncherRecord::Activate(expected)
    );
    let bytes = commit(&mut c, &mut p, Ticket(a.first.0 + 1));
    let ack = decode_shell_file_transaction(&bytes, ShellFileKind::ActionAck).unwrap();
    assert_eq!(ack.transaction, tx(71));
    assert_eq!(
        ack.record,
        ShellContentRecord::ActionAck(ContentActionAck {
            grant: event.grant,
            output: event.output,
            candidate_generation: event.candidate_generation,
            presentation_epoch: event.presentation_epoch,
            interaction_generation: event.interaction_generation,
            allocation: event.allocation,
            target_id: event.target_id,
            target_generation: event.target_generation,
            action_id: event.action_id,
            event_id: event.event_id,
            disposition: 1,
        })
    );
    l.observe(&NativeLauncherObservation::Record(
        ShellFileNativeLauncherRecord {
            transaction: tx(70),
            record: ShellNativeLauncherRecord::ActivationOutcome(NativeLauncherActivationOutcome {
                activation: expected,
                status: 1,
                reason: 0,
            }),
        },
    ))
    .unwrap();
    assert!(l.focus().is_none());
}

#[test]
fn stale_dismissed_and_cancelled_actions_never_activate() {
    for case in 0..5 {
        let (mut c, _p, mut l) = shown();
        let mut event = action(1, 1);
        match case {
            0 => event.presentation_epoch += 1,
            1 => event.action_id = 99,
            2 => event.kind = 2,
            3 => {
                event.kind = 3;
                event.reason = 1;
            }
            _ => observe(
                &mut l,
                ShellNativeLauncherRecord::FocusRevoked(NativeLauncherFocusRevoked {
                    binding: b(),
                    reason: 1,
                }),
            ),
        }
        apply(&mut l, event);
        let status = l.action().unwrap().1;
        assert!(!status.can_activate);
        assert!(
            c.prepare_native_action(&mut l, tx(71), 1, Some(tx(70)))
                .is_err()
        );
        if case == 3 {
            assert!(!status.ack_owed);
            assert!(c.prepare_native_action(&mut l, tx(71), 2, None).is_err());
            l.observe(&publication(ShellFileKind::Outputs, 2)).unwrap();
            assert!(l.action().is_none());
        } else {
            assert!(status.ack_owed);
            assert!(c.prepare_native_action(&mut l, tx(71), 0, None).is_err());
            c.prepare_native_action(&mut l, tx(71), 2, None)
                .unwrap()
                .commit();
        }
    }
}

#[test]
fn saturated_action_response_keeps_event_without_partial_activation() {
    let (mut c, _p, mut l) = shown();
    for n in 0..62 {
        c.enqueue_native_input_response(tx(100 + n), ack(), None)
            .unwrap();
    }
    apply(&mut l, action(1, 1));
    for _ in 0..3 {
        assert!(matches!(
            c.prepare_native_action(&mut l, tx(71), 1, Some(tx(70))),
            Err(ShellClientError::QueueSaturated)
        ));
        assert!(l.action().unwrap().1.can_activate);
    }
    // One credit remains: ACK alone fits, proving the failed pair consumed none.
    assert_eq!(
        c.prepare_native_action(&mut l, tx(71), 2, None)
            .unwrap()
            .commit()
            .count,
        1
    );
}

#[test]
fn activation_and_ack_have_independent_refusal_outcomes() {
    for refuse_activation in [true, false] {
        let (mut c, mut p, mut l) = shown();
        commit(&mut c, &mut p, Ticket(1));
        apply(&mut l, action(1, 1));
        let a = c
            .prepare_native_action(&mut l, tx(71), 1, Some(tx(70)))
            .unwrap()
            .commit();
        for index in 0..2 {
            let ticket = Ticket(a.first.0 + index);
            if (index == 0) == refuse_activation {
                drive(&mut c, &mut p, |_, p| !p.submits.is_empty()).unwrap();
                let held = p.submits.pop_front().unwrap();
                p.answer_error(held.tag, 22);
                drive(&mut c, &mut p, |c, _| {
                    c.custody(ticket) == Some(Custody::Refused(22))
                })
                .unwrap();
            } else {
                commit(&mut c, &mut p, ticket);
            }
        }
        l.service(&c).unwrap();
        l.service(&c).unwrap();
        assert_eq!(l.lost_acks(), u8::from(!refuse_activation));
        apply(&mut l, action(1, 1));
        // A submitted activation still awaits its semantic outcome. Only a
        // definitive activation refusal permits a fresh one; ACK is not replayed.
        assert_eq!(l.action().unwrap().1.can_activate, refuse_activation);
    }
}

use super::*;
#[path = "native_actions.rs"]
mod actions;
#[path = "native_resources.rs"]
mod resources;

#[test]
fn late_presented_after_close_does_not_restore_focus() {
    let (mut c, _p, mut l) = setup();
    c.enqueue_native_candidate_with_lifecycle(&mut l, &candidate())
        .unwrap();
    observe(
        &mut l,
        ShellNativeLauncherRecord::Closed(NativeLauncherClosed {
            grant: b().grant,
            opening: 1,
            reason: 11,
        }),
    );
    l.observe(&outcome(2)).unwrap();
    observe(&mut l, ShellNativeLauncherRecord::Focus(b()));
    assert!(l.opening().is_none() && l.focus().is_none());
    assert!(l.presented().is_none());
    assert_eq!(l.state(), NativeLifecycleState::Live);
}

#[test]
fn refused_ack_is_counted_once_and_not_replayed() {
    let (mut c, mut p, mut l) = shown();
    commit(&mut c, &mut p, Ticket(1));
    input(&mut l, NativeLauncherInputKind::Text, 2);
    let ticket = c
        .prepare_native_input(&mut l, tx(51), 1, None)
        .unwrap()
        .commit()
        .first;
    drive(&mut c, &mut p, |_, p| !p.submits.is_empty()).unwrap();
    let held = p.submits.pop_front().unwrap();
    p.answer_error(held.tag, 22);
    drive(&mut c, &mut p, |c, _| {
        c.custody(ticket) == Some(Custody::Refused(22))
    })
    .unwrap();
    l.service(&c).unwrap();
    l.service(&c).unwrap();
    assert_eq!(l.lost_acks(), 1);
    assert!(l.input().is_none());
}

#[test]
fn full_queue_keeps_input_and_revision_unchanged() {
    let (mut c, _p, mut l) = shown();
    for n in 0..63 {
        c.enqueue_native_input_response(tx(100 + n), ack(), None)
            .unwrap();
    }
    input(&mut l, NativeLauncherInputKind::Text, 2);
    for _ in 0..3 {
        assert!(matches!(
            c.prepare_native_input(&mut l, tx(51), 1, None),
            Err(ShellClientError::QueueSaturated)
        ));
        assert!(l.input().unwrap().1.editable);
        assert_eq!(l.revision(), 2);
    }
}

#[test]
fn old_opening_and_wrong_grant_fail_closed() {
    for wrong_grant in [false, true] {
        let (_c, _p, mut l) = setup();
        observe(
            &mut l,
            ShellNativeLauncherRecord::Closed(NativeLauncherClosed {
                grant: b().grant,
                opening: 1,
                reason: 11,
            }),
        );
        let mut opening = NativeLauncherOpening {
            grant: b().grant,
            opening: if wrong_grant { 2 } else { 1 },
            output: b().output,
            catalog_generation: 6,
            state_revision: 1,
        };
        if wrong_grant {
            opening.grant.content_grant_epoch += 1;
        }
        assert!(
            l.observe(&NativeLauncherObservation::Record(record(
                ShellNativeLauncherRecord::Opening(opening)
            )))
            .is_err()
        );
        assert_eq!(l.state(), NativeLifecycleState::Failed);
    }
}

fn b() -> NativeLauncherBinding {
    NativeLauncherBinding {
        interaction_generation: 1,
        ..binding()
    }
}
fn observe(l: &mut NativeLauncherLifecycle, r: ShellNativeLauncherRecord) {
    l.observe(&NativeLauncherObservation::Record(record(r)))
        .unwrap();
}
fn publication(kind: ShellFileKind, generation: u64) -> NativeLauncherObservation {
    NativeLauncherObservation::Published(ShellFileObjectPublished {
        object: kind,
        generation,
        qid: 88,
    })
}
fn candidate() -> ShellFileNativeCandidate {
    let b = b();
    ShellFileNativeCandidate {
        transaction: tx(40),
        candidate: NativeContentCandidate {
            opening: b.opening,
            catalog_generation: b.catalog_generation,
            state_revision: 1,
            selected: 1,
            rows: vec![1],
            candidate: ContentCandidate {
                grant: b.grant,
                candidate_generation: b.candidate_generation,
                output: b.output,
                facts_generation: 1,
                pacing_permit: 1,
                interaction_generation: 1,
                surfaces: vec![ContentSurface {
                    allocation: b.allocation,
                    scale_generation: 1,
                    role: 3,
                    edge: 2,
                    margins: ContentMargins::default(),
                    reservation_extent: 0,
                    parent_surface_index: u16::MAX,
                    anchor_parent_rect: ContentPixelRect::default(),
                }],
                placements: vec![ContentPlacement {
                    resource: ContentResourceId {
                        id: 1,
                        generation: 1,
                    },
                    surface_index: 0,
                    destination_x_px: 0,
                    destination_y_px: 0,
                }],
                targets: vec![ContentTarget {
                    surface_index: 0,
                    action_kind: 2,
                    action_id: 1,
                    target_id: 1,
                    target_generation: 1,
                    bounds_px: ContentPixelRect {
                        x: 0,
                        y: 0,
                        width: 10,
                        height: 10,
                    },
                }],
            },
        },
    }
}
fn outcome(kind: u16) -> NativeLauncherObservation {
    NativeLauncherObservation::Content(
        tx(40),
        ShellContentRecord::CandidateOutcome(ContentCandidateOutcome {
            grant: b().grant,
            candidate_generation: b().candidate_generation,
            output: b().output,
            kind,
            reason: if kind > 2 { 1 } else { 0 },
            presentation_epoch: if kind == 2 { b().presentation_epoch } else { 0 },
            work_area_generation: 1,
            wm_commit_generation: 1,
        }),
    )
}
fn setup() -> (ShellConnection, Peer, NativeLauncherLifecycle) {
    setup_with(ready())
}
fn setup_with(
    (mut c, p): (ShellConnection, Peer),
) -> (ShellConnection, Peer, NativeLauncherLifecycle) {
    let Some(NativeLauncherObservation::Content(_, ShellContentRecord::Limits(limits))) =
        c.take_native_launcher_observation().unwrap()
    else {
        panic!("limits")
    };
    let mut l = NativeLauncherLifecycle::new(limits).unwrap();
    l.observe(&publication(ShellFileKind::Catalog, 6)).unwrap();
    l.observe(&NativeLauncherObservation::Catalog(
        tx(1),
        ShellApplicationCatalog {
            connection_epoch: EPOCH,
            generation: 6,
            entries: vec![],
        },
    ))
    .unwrap();
    l.observe(&publication(ShellFileKind::Outputs, 1)).unwrap();
    l.observe(&NativeLauncherObservation::Content(
        tx(2),
        ShellContentRecord::OutputFacts(ContentOutputFacts {
            grant: b().grant,
            facts_generation: 1,
            outputs: vec![],
        }),
    ))
    .unwrap();
    observe(
        &mut l,
        ShellNativeLauncherRecord::Opening(NativeLauncherOpening {
            grant: b().grant,
            opening: b().opening,
            output: b().output,
            catalog_generation: 6,
            state_revision: 1,
        }),
    );
    (c, p, l)
}
fn shown() -> (ShellConnection, Peer, NativeLauncherLifecycle) {
    let (mut c, p, mut l) = setup();
    c.enqueue_native_candidate_with_lifecycle(&mut l, &candidate())
        .unwrap();
    l.observe(&outcome(2)).unwrap();
    observe(&mut l, ShellNativeLauncherRecord::Focus(b()));
    assert_eq!(l.focus(), Some(&b()));
    (c, p, l)
}
fn input(l: &mut NativeLauncherLifecycle, kind: NativeLauncherInputKind, revision: u64) {
    observe(
        l,
        ShellNativeLauncherRecord::Input(NativeLauncherInput {
            event: NativeLauncherEvent {
                binding: b(),
                event_id: 11,
                state_revision: revision,
            },
            issued_mono_usec: 5_000_999,
            kind,
            text: if kind == NativeLauncherInputKind::Text {
                "ab".into()
            } else {
                String::new()
            },
        }),
    );
}

#[test]
fn prepared_is_not_presented_and_focus_requires_exact_binding() {
    let (mut c, _p, mut l) = setup();
    let mut invalid = candidate();
    invalid.candidate.selected = 2;
    assert!(
        c.enqueue_native_candidate_with_lifecycle(&mut l, &invalid)
            .is_err()
    );
    c.enqueue_native_candidate_with_lifecycle(&mut l, &candidate())
        .unwrap();
    l.observe(&outcome(1)).unwrap();
    assert!(l.presented().is_none());
    observe(&mut l, ShellNativeLauncherRecord::Focus(b()));
    assert!(l.focus().is_none());
    l.observe(&outcome(2)).unwrap();
    let presented = l.presented().unwrap();
    assert_eq!(presented.rows, &[1]);
    assert_eq!(presented.selected, 1);
    assert_eq!(presented.presentation_epoch, b().presentation_epoch);
    assert_eq!(presented.transaction, tx(40));
    observe(&mut l, ShellNativeLauncherRecord::Focus(b()));
    assert_eq!(l.focus(), Some(&b()));
    let other = NativeLauncherBinding {
        presentation_epoch: 99,
        ..b()
    };
    observe(&mut l, ShellNativeLauncherRecord::Focus(other));
    assert!(l.focus().is_none());
    // A smaller nonzero lease can supersede: the contract has no lease-order rule.
    let lower = NativeLauncherBinding {
        focus_lease: 1,
        ..b()
    };
    observe(&mut l, ShellNativeLauncherRecord::Focus(lower));
    assert_eq!(l.focus(), Some(&lower));
    observe(
        &mut l,
        ShellNativeLauncherRecord::FocusRevoked(NativeLauncherFocusRevoked {
            binding: b(),
            reason: 1,
        }),
    );
    assert_eq!(l.focus(), Some(&lower));
    observe(
        &mut l,
        ShellNativeLauncherRecord::FocusRevoked(NativeLauncherFocusRevoked {
            binding: lower,
            reason: 1,
        }),
    );
    assert!(l.focus().is_none());
}

#[test]
fn input_is_retained_until_reserved_ack_commits_once() {
    let (mut c, _p, mut l) = shown();
    input(&mut l, NativeLauncherInputKind::Text, 2);
    let status = l.input().unwrap().1;
    assert!(status.editable);
    assert!(!status.can_activate);
    assert_eq!(l.revision(), 2);
    assert_eq!(
        status.ack_due_ms,
        5000 + u64::from(ContentLimits::prototype(b().grant).action_ack_timeout_ms)
    );
    assert!(l.observe(&publication(ShellFileKind::Outputs, 2)).is_err());
    assert_eq!(l.input().unwrap().1, status);
    assert!(
        c.prepare_native_input(&mut l, tx(51), 1, Some(tx(50)))
            .is_err()
    );
    drop(c.prepare_native_input(&mut l, tx(51), 1, None).unwrap());
    assert_eq!(l.input().unwrap().1, status);
    assert!(c.prepare_native_input(&mut l, tx(51), 0, None).is_err());
    let response = c.prepare_native_input(&mut l, tx(51), 1, None).unwrap();
    assert!(response.status().editable);
    let admission = response.commit();
    assert_eq!(admission.count, 1);
    assert!(l.input().is_none());
    assert!(c.prepare_native_input(&mut l, tx(52), 1, None).is_err());
    // Repeated older revision cannot be applied as a fresh edit.
    input(&mut l, NativeLauncherInputKind::Text, 2);
    assert!(!l.input().unwrap().1.editable);
}

#[test]
fn admitted_activation_disarms_the_opening_and_close_requires_larger_reopen() {
    let (mut c, _p, mut l) = shown();
    input(&mut l, NativeLauncherInputKind::Accept, 1);
    assert!(l.input().unwrap().1.can_activate);
    c.prepare_native_input(&mut l, tx(51), 1, Some(tx(50)))
        .unwrap()
        .commit();
    input(&mut l, NativeLauncherInputKind::Accept, 1);
    assert!(!l.input().unwrap().1.can_activate);
    c.prepare_native_input(&mut l, tx(52), 2, None)
        .unwrap()
        .commit();
    l.observe(&NativeLauncherObservation::Record(
        ShellFileNativeLauncherRecord {
            transaction: tx(50),
            record: ShellNativeLauncherRecord::ActivationOutcome(NativeLauncherActivationOutcome {
                activation: NativeLauncherActivation {
                    event: NativeLauncherEvent {
                        binding: b(),
                        event_id: 11,
                        state_revision: 1,
                    },
                    cause: 1,
                    slot: 1,
                },
                status: 1,
                reason: 0,
            }),
        },
    ))
    .unwrap();
    assert!(l.focus().is_none());
    observe(&mut l, ShellNativeLauncherRecord::Focus(b()));
    assert!(l.focus().is_none());
    input(&mut l, NativeLauncherInputKind::Text, 2);
    assert!(!l.input().unwrap().1.editable);
    c.prepare_native_input(&mut l, tx(53), 2, None)
        .unwrap()
        .commit();
    observe(
        &mut l,
        ShellNativeLauncherRecord::Closed(NativeLauncherClosed {
            grant: b().grant,
            opening: 1,
            reason: 11,
        }),
    );
    assert!(l.opening().is_none());
    assert_eq!(l.revision(), 0);
    observe(
        &mut l,
        ShellNativeLauncherRecord::Opening(NativeLauncherOpening {
            grant: b().grant,
            opening: 2,
            output: b().output,
            catalog_generation: 6,
            state_revision: 1,
        }),
    );
    input(&mut l, NativeLauncherInputKind::Accept, 1);
    assert!(!l.input().unwrap().1.current);
}

#[test]
fn announcements_disarm_before_fetch_and_bad_correlation_fails_closed() {
    let (mut c, _p, mut l) = shown();
    l.observe(&publication(ShellFileKind::Catalog, 7)).unwrap();
    input(&mut l, NativeLauncherInputKind::Accept, 1);
    assert!(!l.input().unwrap().1.can_activate);
    c.prepare_native_input(&mut l, tx(51), 2, None)
        .unwrap()
        .commit();
    let mut next = candidate();
    next.candidate.candidate.candidate_generation += 1;
    assert!(
        c.enqueue_native_candidate_with_lifecycle(&mut l, &next)
            .is_err()
    );
    assert!(l.observe(&outcome(2)).is_err()); // no pending candidate
    assert_eq!(l.state(), NativeLifecycleState::Failed);
    assert!(l.focus().is_none());
    let (mut c, _p, mut l) = setup();
    l.observe(&publication(ShellFileKind::Outputs, 2)).unwrap();
    assert!(
        c.enqueue_native_candidate_with_lifecycle(&mut l, &candidate())
            .is_err()
    );
}

#[test]
fn disconnect_disarms_all_input_authority() {
    let (mut c, mut p, mut l) = shown();
    input(&mut l, NativeLauncherInputKind::Text, 2);
    p.close();
    let _ = c.poll_io();
    assert!(l.service(&c).is_err());
    assert_eq!(l.state(), NativeLifecycleState::Ended);
    assert!(l.focus().is_none() && l.opening().is_none() && l.input().is_none());
}

#[test]
fn edit_result_selects_ack_after_capacity_is_reserved() {
    for consumed in [false, true] {
        let (mut c, mut p, mut l) = shown();
        commit(&mut c, &mut p, Ticket(1));
        input(&mut l, NativeLauncherInputKind::Text, 2);
        let mut calls = 0;
        let a = c
            .apply_native_input(&mut l, tx(51), None, |input| {
                assert_eq!(input.text, "ab");
                calls += 1;
                consumed
            })
            .unwrap();
        assert_eq!(calls, 1);
        assert!(l.input().is_none());
        let bytes = commit(&mut c, &mut p, a.first);
        let record =
            decode_shell_file_native_launcher_transaction(&bytes, ShellFileKind::NativeInputAck)
                .unwrap();
        let ShellNativeLauncherRecord::InputAck(ack) = record.record else {
            panic!("ACK")
        };
        assert_eq!(ack.disposition, if consumed { 1 } else { 2 });
        assert_eq!(ack.event.state_revision, 2);
        assert_eq!(record.transaction, tx(51));
    }
}

#[test]
fn edit_is_not_called_under_pressure_or_for_uneditable_input() {
    let (mut c, _p, mut l) = shown();
    input(&mut l, NativeLauncherInputKind::Text, 1);
    c.apply_native_input(&mut l, tx(51), None, |_| panic!("uneditable"))
        .unwrap();
    for n in 0..62 {
        c.enqueue_native_input_response(tx(100 + n), ack(), None)
            .unwrap();
    }
    input(&mut l, NativeLauncherInputKind::Text, 2);
    for _ in 0..2 {
        assert!(matches!(
            c.apply_native_input(&mut l, tx(52), None, |_| panic!("full queue")),
            Err(ShellClientError::QueueSaturated)
        ));
    }
    assert!(l.input().unwrap().1.editable);
}

#[test]
fn panicking_edit_fails_closed_and_cannot_be_replayed() {
    let (mut c, _p, mut l) = shown();
    input(&mut l, NativeLauncherInputKind::Text, 2);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = c.apply_native_input(&mut l, tx(51), None, |_| panic!("application edit"));
    }));
    assert!(result.is_err());
    assert_eq!(l.state(), NativeLifecycleState::Failed);
    assert!(l.presented().is_none() && l.input().is_none());
    assert!(
        c.apply_native_input(&mut l, tx(51), None, |_| panic!("replay"))
            .is_err()
    );
}

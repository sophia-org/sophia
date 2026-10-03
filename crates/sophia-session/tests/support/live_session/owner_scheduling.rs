#[test]
fn held_owner_work_selects_the_low_latency_owner_wait_budget() {
    assert_eq!(
        authority_wait_timeout(true, false, false),
        Duration::from_millis(1)
    );
    assert_eq!(
        authority_wait_timeout(false, true, false),
        Duration::from_millis(1)
    );
    assert_eq!(
        authority_wait_timeout(false, false, true),
        Duration::from_millis(1)
    );
    assert_eq!(
        authority_wait_timeout(false, false, false),
        Duration::from_millis(25)
    );
}

#[test]
fn physical_input_alone_lets_an_idle_owner_sleep_until_rung() {
    let budget = |physical, proof, held| {
        authority_wait_timeout(
            owner_input_work_pending(physical, proof, held),
            false,
            false,
        )
    };
    let idle = OwnerHeldWork::default();
    // The input worker rings the owner, so an attached seat is not a reason
    // to poll. This was a 1 ms budget whenever physical input existed.
    assert_eq!(budget(true, false, idle), Duration::from_millis(25));
    // Proof sessions keep their established timing.
    assert_eq!(budget(true, true, idle), Duration::from_millis(1));
    // Each class of owner-held work keeps the short service wait, because
    // nothing rings when it comes due.
    let held = [
        OwnerHeldWork {
            input: true,
            ..idle
        },
        OwnerHeldWork {
            input_receipts: true,
            ..idle
        },
        OwnerHeldWork {
            frames: true,
            ..idle
        },
        OwnerHeldWork {
            output_topology: true,
            ..idle
        },
        OwnerHeldWork { seat: true, ..idle },
        OwnerHeldWork {
            shell_interaction: true,
            ..idle
        },
        OwnerHeldWork {
            lifecycle: true,
            ..idle
        },
    ];
    for work in held {
        assert_eq!(
            budget(true, false, work),
            Duration::from_millis(1),
            "{work:?}"
        );
        // Without physical input the owner keeps its prior budget.
        assert_eq!(
            budget(false, false, work),
            Duration::from_millis(25),
            "{work:?}"
        );
    }
    // Cursor and control work keep the short wait whatever else holds.
    assert_eq!(
        authority_wait_timeout(owner_input_work_pending(true, false, idle), true, false),
        Duration::from_millis(1)
    );
    assert_eq!(
        authority_wait_timeout(owner_input_work_pending(true, false, idle), false, true),
        Duration::from_millis(1)
    );
}

#[test]
fn native_frame_progress_preempts_metadata_only_authority_batches() {
    let request = OutputFrameServiceRequest {
        outputs: vec![OutputFrameServiceObservation {
            output: OutputId::from_raw(1),
            primary: true,
            native_phase: OutputNativeFramePhase::Idle,
            pending_frame: true,
        }],
        presentation_queued: false,
        software_frame_waiting: false,
        preparation_pending: false,
    };

    assert!(native_frame_service_requires_owner_progress(&request));

    let mut in_flight = request;
    in_flight.outputs[0].pending_frame = false;
    in_flight.outputs[0].native_phase = OutputNativeFramePhase::InFlight;
    assert!(native_frame_service_requires_owner_progress(&in_flight));

    let idle = OutputFrameServiceRequest {
        outputs: vec![OutputFrameServiceObservation {
            output: OutputId::from_raw(1),
            primary: true,
            native_phase: OutputNativeFramePhase::Idle,
            pending_frame: false,
        }],
        presentation_queued: false,
        software_frame_waiting: false,
        preparation_pending: false,
    };
    assert!(!native_frame_service_requires_owner_progress(&idle));

    // A waiting software present is owed work even though no output reports a
    // native frame of its own, so the owner must keep its fast pacing.
    let mut software_waiting = idle;
    software_waiting.software_frame_waiting = true;
    assert!(native_frame_service_requires_owner_progress(
        &software_waiting
    ));
}

fn shortcut_timer_router(plan: sophia_engine::WmShortcutPlan) -> sophia_engine::WmShortcutRouter {
    sophia_engine::WmShortcutRouter::new(
        sophia_engine::WmShortcutRegistry::from_plan(
            &plan,
            sophia_protocol::WmCapabilities::all_supported(),
            1,
            sophia_protocol::WmChromePolicy::default(),
        )
        .unwrap(),
    )
}

#[test]
fn an_open_chord_without_a_timer_keeps_the_idle_wait() {
    use sophia_engine::WmShortcutPlan;
    use sophia_protocol::{
        DeviceId, PolicyActionLifecycleInterest, WmActionId, WmBindingRegistration,
    };
    let idle = authority_wait_timeout(false, false, false);
    assert_eq!(shortcut_wait_cap(None, 0, idle), idle);
    let action = WmActionId::from_raw(5);
    let mut router = shortcut_timer_router(WmShortcutPlan {
        immediate: vec![WmBindingRegistration {
            action,
            keycode: 67,
            modifiers: sophia_protocol::WmModifierMask { bits: 0 },
        }],
        ..WmShortcutPlan::default()
    });
    router.set_action_lifecycles(&[PolicyActionLifecycleInterest { action, held_ms: 0 }]);
    assert_eq!(shortcut_wait_cap(Some(&router), 0, idle), idle);
    let outputs = router
        .key_event(SeatId::from_raw(1), DeviceId::from_raw(1), 67, true, 0)
        .accept();
    assert!(
        matches!(outputs[..], [sophia_engine::WmShortcutOutput::Activation(a)] if a.chord.is_some())
    );
    assert_eq!(router.next_deadline(), None);
    assert_eq!(shortcut_wait_cap(Some(&router), 500, idle), idle);
}

#[test]
fn servicing_hold_and_held_deadlines_restores_the_idle_wait() {
    use sophia_engine::{WmChordEvent, WmHoldBinding, WmKeyStep, WmShortcutOutput, WmShortcutPlan};
    use sophia_protocol::{DeviceId, PolicyActionLifecycleInterest, WmActionId};
    let action = WmActionId::from_raw(5);
    let mut router = shortcut_timer_router(WmShortcutPlan {
        holds: vec![WmHoldBinding {
            step: WmKeyStep {
                keycode: 67,
                modifiers: 0,
            },
            hold_ms: 500,
            action,
        }],
        ..WmShortcutPlan::default()
    });
    router.set_action_lifecycles(&[PolicyActionLifecycleInterest {
        action,
        held_ms: 150,
    }]);
    assert!(
        router
            .key_event(SeatId::from_raw(1), DeviceId::from_raw(1), 67, true, 100)
            .accept()
            .is_empty()
    );
    let idle = authority_wait_timeout(false, false, false);
    assert_eq!(router.next_deadline(), Some(600));
    assert_eq!(shortcut_wait_cap(Some(&router), 100, idle), idle);
    assert_eq!(
        shortcut_wait_cap(Some(&router), 590, idle),
        Duration::from_millis(10)
    );
    // A frame or shell deadline that is nearer stays authoritative.
    assert_eq!(
        shortcut_wait_cap(Some(&router), 590, Duration::from_millis(1)),
        Duration::from_millis(1)
    );
    assert_eq!(shortcut_wait_cap(Some(&router), 600, idle), Duration::ZERO);
    router.poll_shortcuts(600);
    assert!(
        matches!(router.take_outputs()[..], [WmShortcutOutput::Activation(a)] if a.action == action)
    );
    assert_eq!(router.next_deadline(), Some(750));
    assert_eq!(shortcut_wait_cap(Some(&router), 600, idle), idle);
    router.poll_shortcuts(600);
    assert!(router.take_outputs().is_empty());
    assert_eq!(
        shortcut_wait_cap(Some(&router), 745, idle),
        Duration::from_millis(5)
    );
    assert_eq!(shortcut_wait_cap(Some(&router), 755, idle), Duration::ZERO);
    router.poll_shortcuts(755);
    assert!(matches!(
        router.take_outputs()[..],
        [WmShortcutOutput::Chord(WmChordEvent::Held { .. })]
    ));
    // The chord is still open, but its one Held was delivered to the outbox.
    assert_eq!(router.next_deadline(), None);
    assert_eq!(shortcut_wait_cap(Some(&router), 755, idle), idle);
    router.poll_shortcuts(900);
    assert!(router.take_outputs().is_empty());
}

#[test]
fn a_sequence_timeout_does_not_leave_a_zero_length_owner_wait() {
    use sophia_engine::{
        WmChordEvent, WmKeyStep, WmSequenceBinding, WmSequenceLeader, WmShortcutOutput,
        WmShortcutPlan,
    };
    use sophia_protocol::{DeviceId, PolicyActionLifecycleInterest, PolicyChordEnd, WmActionId};
    let prefix = WmKeyStep {
        keycode: 67,
        modifiers: 0,
    };
    let hint = WmActionId::from_raw(5);
    let mut router = shortcut_timer_router(WmShortcutPlan {
        sequences: vec![WmSequenceBinding {
            steps: vec![
                prefix,
                WmKeyStep {
                    keycode: 68,
                    modifiers: 0,
                },
            ],
            action: WmActionId::from_raw(6),
        }],
        leaders: vec![WmSequenceLeader {
            steps: vec![prefix],
            action: hint,
        }],
        ..WmShortcutPlan::default()
    });
    router.set_action_lifecycles(&[PolicyActionLifecycleInterest {
        action: hint,
        held_ms: 0,
    }]);
    assert!(matches!(
        router
            .key_event(SeatId::from_raw(1), DeviceId::from_raw(1), 67, true, 1000)
            .accept()[..],
        [WmShortcutOutput::Activation(_)]
    ));
    let idle = authority_wait_timeout(false, false, false);
    assert_eq!(router.next_deadline(), Some(2000));
    assert_eq!(
        shortcut_wait_cap(Some(&router), 1999, idle),
        Duration::from_millis(1)
    );
    assert_eq!(shortcut_wait_cap(Some(&router), 2000, idle), Duration::ZERO);
    router.poll_shortcuts(2000);
    assert!(matches!(
        router.take_outputs()[..],
        [WmShortcutOutput::Chord(WmChordEvent::Ended {
            end: PolicyChordEnd::TimedOut,
            ..
        })]
    ));
    assert_eq!(shortcut_wait_cap(Some(&router), 2000, idle), idle);
    router.poll_shortcuts(2000);
    assert!(router.take_outputs().is_empty());
}

#[test]
fn a_notification_preempts_a_future_shortcut_deadline() {
    use sophia_engine::{WmHoldBinding, WmKeyStep, WmShortcutPlan};
    use sophia_protocol::{DeviceId, WmActionId};
    let mut router = shortcut_timer_router(WmShortcutPlan {
        holds: vec![WmHoldBinding {
            step: WmKeyStep {
                keycode: 67,
                modifiers: 0,
            },
            hold_ms: 5000,
            action: WmActionId::from_raw(5),
        }],
        ..WmShortcutPlan::default()
    });
    assert!(
        router
            .key_event(SeatId::from_raw(1), DeviceId::from_raw(1), 67, true, 0)
            .accept()
            .is_empty()
    );
    let owner = crate::live_session::OwnerWake::new().unwrap();
    let (_authority, receiver) = std::sync::mpsc::sync_channel::<()>(1);
    owner.begin_pass().unwrap();
    let budget = shortcut_wait_cap(Some(&router), 0, Duration::from_secs(30));
    assert_eq!(budget, Duration::from_secs(5));
    // Work published after inspection rings; its notification is not replaced
    // by the shortcut deadline. The next turn must inspect that work first.
    owner.notifier().notify();
    let started = Instant::now();
    assert_eq!(
        owner.receive(&receiver, budget),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "shortcut wait ignored the notification"
    );
    assert_eq!(router.next_deadline(), Some(5000));
}

#[test]
fn deferred_cpu_composition_retains_the_native_visual_owner() {
    assert_eq!(
        production_cycle_native_owner_policy(true, true),
        ProductionCycleNativeOwnerPolicy::Available
    );
    assert_eq!(
        production_cycle_native_owner_policy(true, false),
        ProductionCycleNativeOwnerPolicy::Available
    );
    assert_eq!(
        production_cycle_native_owner_policy(false, true),
        ProductionCycleNativeOwnerPolicy::Unavailable
    );
}

#[test]
fn a_repaint_that_cannot_run_does_not_take_the_turn_from_authority() {
    // The repaint yields to a pending layout epoch and to a topology
    // preparation, and neither refusal moves the pacer's deadline, so the
    // repaint stays due. Letting it preempt anyway is a livelock: the epoch
    // ends on the authority batch carrying the client's frame, and that batch
    // is exactly what the preemption keeps refusing to take. Each kitty launch
    // paid the epoch's full four-second budget this way, compositing nothing
    // and routing no input, with the right frame queued after 76ms.
    assert!(paced_repaint_runnable(true, true));
    assert!(
        !paced_repaint_runnable(false, true),
        "a pending layout epoch"
    );
    assert!(
        !paced_repaint_runnable(true, false),
        "a topology being prepared"
    );
    assert!(!paced_repaint_runnable(false, false));
}

#[test]
fn native_frame_progress_cannot_consecutively_preempt_authority() {
    let pending = OutputFrameServiceRequest {
        outputs: vec![OutputFrameServiceObservation {
            output: OutputId::from_raw(1),
            primary: true,
            native_phase: OutputNativeFramePhase::InFlight,
            pending_frame: true,
        }],
        presentation_queued: false,
        software_frame_waiting: false,
        preparation_pending: false,
    };

    assert!(native_frame_service_should_preempt_authority(
        &pending, false, false, 0, false, false
    ));
    assert!(!native_frame_service_should_preempt_authority(
        &pending, true, false, 0, false, false
    ));
    assert!(!native_frame_service_should_preempt_authority(
        &pending, false, true, 0, false, false
    ));
    assert!(!native_frame_service_should_preempt_authority(
        &pending, false, true, 3, false, false
    ));
    assert!(native_frame_service_should_preempt_authority(
        &pending, false, true, 4, false, false
    ));

    let idle = OutputFrameServiceRequest {
        outputs: vec![OutputFrameServiceObservation {
            output: OutputId::from_raw(1),
            primary: true,
            native_phase: OutputNativeFramePhase::Idle,
            pending_frame: false,
        }],
        presentation_queued: false,
        software_frame_waiting: false,
        preparation_pending: false,
    };
    assert!(!native_frame_service_should_preempt_authority(
        &idle, false, false, 0, false, false
    ));
    assert!(native_frame_service_should_preempt_authority(
        &idle, false, false, 0, true, false
    ));
    assert!(!native_frame_service_should_preempt_authority(
        &idle, false, true, 3, true, false
    ));
    assert!(native_frame_service_should_preempt_authority(
        &idle, false, true, 4, true, false
    ));
}

#[test]
fn a_pending_pointer_grab_counts_as_a_control_for_both_the_decision_and_the_counter() {
    // The bug: the preemption decision counted explicit pointer grabs and the
    // counter that earns priority counted only session controls. A held grab
    // with no session control pinned the counter at zero while the decision
    // kept reporting a pending control, so the four-cycle priority it gates
    // was unreachable and the request path could not preempt at all for as
    // long as the grab lasted.
    assert!(
        control_is_pending(0, 1),
        "a pointer grab is a pending control"
    );
    assert!(
        control_is_pending(1, 0),
        "a session control is a pending control"
    );
    assert!(
        !control_is_pending(0, 0),
        "nothing waiting is not a pending control"
    );

    // The counter must hold whenever the decision sees a control, so priority
    // can actually accumulate to the threshold.
    assert!(
        !control_priority_should_reset(0, 1, false),
        "a pending grab must let the counter accumulate, not reset it"
    );
    assert!(!control_priority_should_reset(1, 0, false));
    assert!(
        control_priority_should_reset(0, 0, false),
        "nothing waiting resets"
    );

    // A preemption always resets: the control path just yielded, so it has no
    // backlog left to earn priority for.
    assert!(control_priority_should_reset(1, 1, true));
    assert!(control_priority_should_reset(0, 1, true));
}

#[test]
fn cold_preparation_requests_owner_progress_without_a_fake_pending_frame() {
    let mut request = OutputFrameServiceRequest {
        preparation_pending: true,
        ..Default::default()
    };
    assert!(native_frame_service_requires_owner_progress(&request));
    request.preparation_pending = false;
    assert!(!native_frame_service_requires_owner_progress(&request));
}

#[test]
fn completion_only_work_preempts_saturated_ingress_at_its_service_deadline() {
    let pending = OutputFrameServiceRequest {
        outputs: vec![OutputFrameServiceObservation {
            output: OutputId::from_raw(1),
            primary: true,
            native_phase: OutputNativeFramePhase::InFlight,
            pending_frame: true,
        }],
        presentation_queued: false,
        software_frame_waiting: false,
        preparation_pending: false,
    };
    let mut previous = false;
    let mut control_cycles = 0;
    let mut services = Vec::new();
    // Continuous queued authority bypasses poll. A held control earns at most
    // four priority turns; every service still yields the next turn to ingress.
    for turn in 0..20 {
        let due = turn >= 3;
        let service = native_frame_service_should_preempt_authority(
            &pending,
            previous,
            true,
            control_cycles,
            due,
            true,
        );
        if service {
            services.push(turn);
        }
        control_cycles = if service { 0 } else { control_cycles + 1 };
        previous = service;
    }
    assert_eq!(services, [4, 9, 14, 19]);
    assert!(
        !native_frame_service_should_preempt_authority(&pending, false, false, 0, false, true,),
        "a subscribed flip alone must not force a polling turn"
    );
    assert!(
        native_frame_service_should_preempt_authority(&pending, false, false, 0, false, false,),
        "renderer-only work retains bounded service"
    );
}

#[test]
fn native_event_waits_and_drained_work_have_no_short_polling_tail() {
    let mut request = OutputFrameServiceRequest {
        outputs: vec![OutputFrameServiceObservation {
            output: OutputId::from_raw(1),
            primary: true,
            native_phase: OutputNativeFramePhase::InFlight,
            pending_frame: true,
        }],
        presentation_queued: false,
        software_frame_waiting: false,
        preparation_pending: false,
    };
    let budget = |request: &OutputFrameServiceRequest, event, cursor, retiring| {
        authority_wait_timeout(
            owner_input_work_pending(
                true,
                false,
                OwnerHeldWork {
                    frames: native_frame_short_service(Some(request), event, cursor, retiring),
                    ..OwnerHeldWork::default()
                },
            ),
            false,
            false,
        )
    };
    assert_eq!(
        budget(&request, true, false, false),
        Duration::from_millis(25)
    );
    assert_eq!(
        budget(&request, false, false, false),
        Duration::from_millis(1)
    );
    request.outputs[0].native_phase = OutputNativeFramePhase::Idle;
    request.outputs[0].pending_frame = false;
    // The very first idle visit is back at maintenance cadence, with no tail.
    assert_eq!(
        budget(&request, false, false, false),
        Duration::from_millis(25)
    );
    assert_eq!(
        budget(&request, false, true, false),
        Duration::from_millis(1)
    );
    assert_eq!(
        budget(&request, false, false, true),
        Duration::from_millis(1)
    );
    request.preparation_pending = true;
    assert_eq!(
        budget(&request, false, false, false),
        Duration::from_millis(1)
    );
}

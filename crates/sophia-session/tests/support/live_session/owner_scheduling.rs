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
        OwnerHeldWork { input: true, ..idle },
        OwnerHeldWork { input_receipts: true, ..idle },
        OwnerHeldWork { frames: true, ..idle },
        OwnerHeldWork { output_topology: true, ..idle },
        OwnerHeldWork { seat: true, ..idle },
        OwnerHeldWork { shell_interaction: true, ..idle },
        OwnerHeldWork { lifecycle: true, ..idle },
    ];
    for work in held {
        assert_eq!(budget(true, false, work), Duration::from_millis(1), "{work:?}");
        // Without physical input the owner keeps its prior budget.
        assert_eq!(budget(false, false, work), Duration::from_millis(25), "{work:?}");
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
    };

    assert!(native_frame_service_should_preempt_authority(
        &pending, false, false, 0, false
    ));
    assert!(!native_frame_service_should_preempt_authority(
        &pending, true, false, 0, false
    ));
    assert!(!native_frame_service_should_preempt_authority(
        &pending, false, true, 0, false
    ));
    assert!(!native_frame_service_should_preempt_authority(
        &pending, false, true, 3, false
    ));
    assert!(native_frame_service_should_preempt_authority(
        &pending, false, true, 4, false
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
    };
    assert!(!native_frame_service_should_preempt_authority(
        &idle, false, false, 0, false
    ));
    assert!(native_frame_service_should_preempt_authority(
        &idle, false, false, 0, true
    ));
    assert!(!native_frame_service_should_preempt_authority(
        &idle, false, true, 3, true
    ));
    assert!(native_frame_service_should_preempt_authority(
        &idle, false, true, 4, true
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

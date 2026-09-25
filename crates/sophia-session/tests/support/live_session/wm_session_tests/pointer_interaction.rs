#[test]
fn completed_pointer_geometry_reduces_raw_motion_to_one_bounded_target() {
    let initial = Rect {
        x: 100,
        y: 80,
        width: 300,
        height: 200,
    };
    let resize = sophia_protocol::WmPointerGestureCompleted {
        surface: SurfaceId::new(91, 1),
        output: OutputId::INVALID,
        workspace: sophia_protocol::WorkspaceId::INVALID,
        mode: sophia_protocol::WmPointerGestureMode::Resize,
        start: sophia_protocol::WmPointerPosition { x: 120, y: 100 },
        end: sophia_protocol::WmPointerPosition { x: 220, y: 50 },
    };
    assert_eq!(
        completed_pointer_gesture_geometry(resize, initial),
        Rect {
            width: 400,
            height: 150,
            ..initial
        }
    );
}

#[test]
fn public_pointer_updates_replace_only_the_latest_matching_update() {
    let surface = SurfaceId::new(91, 1);
    let source = LiveWmProposalSource::PointerGesture {
        surface,
        mode: sophia_protocol::WmPointerGestureMode::Move,
    };
    let cause = |phase, x| LivePublicPolicyCause {
        source,
        cause: sophia_protocol::PolicyRequestCause::Interaction {
            phase,
            kind: sophia_protocol::PolicyInteractionKind::Move,
            axis: sophia_protocol::PolicyInteractionAxis::None,
            target: surface,
            geometry: Rect {
                x,
                y: 20,
                width: 300,
                height: 200,
            },
        },
        affected_outputs: vec![OutputId::from_raw(1)],
    };
    let mut queue = VecDeque::new();

    assert_eq!(
        enqueue_public_policy_cause(
            &mut queue,
            None,
            false,
            cause(sophia_protocol::PolicyInteractionPhase::Begin, 10),
        ),
        LiveWmRequestAdmission::Admitted
    );
    assert_eq!(
        enqueue_public_policy_cause(
            &mut queue,
            Some(source),
            true,
            cause(sophia_protocol::PolicyInteractionPhase::Update, 20),
        ),
        LiveWmRequestAdmission::Admitted
    );
    assert_eq!(
        enqueue_public_policy_cause(
            &mut queue,
            Some(source),
            true,
            cause(sophia_protocol::PolicyInteractionPhase::Update, 30),
        ),
        LiveWmRequestAdmission::Duplicate
    );
    assert_eq!(queue.len(), 2);
    assert!(matches!(
        queue.back().map(|pending| &pending.cause),
        Some(sophia_protocol::PolicyRequestCause::Interaction {
            phase: sophia_protocol::PolicyInteractionPhase::Update,
            geometry: Rect { x: 30, .. },
            ..
        })
    ));
    assert_eq!(
        enqueue_public_policy_cause(
            &mut queue,
            Some(source),
            true,
            cause(sophia_protocol::PolicyInteractionPhase::End, 40),
        ),
        LiveWmRequestAdmission::Admitted
    );
    assert_eq!(queue.len(), 3);
}

#[test]
fn public_security_cancel_purges_stale_values_and_preempts_unrelated_work() {
    let surface = SurfaceId::new(92, 1);
    let source = LiveWmProposalSource::PointerGesture {
        surface,
        mode: sophia_protocol::WmPointerGestureMode::Resize,
    };
    let interaction = |phase, width| LivePublicPolicyCause {
        source,
        cause: sophia_protocol::PolicyRequestCause::Interaction {
            phase,
            kind: sophia_protocol::PolicyInteractionKind::Resize,
            axis: sophia_protocol::PolicyInteractionAxis::None,
            target: surface,
            geometry: Rect {
                x: 10,
                y: 20,
                width,
                height: 200,
            },
        },
        affected_outputs: vec![OutputId::from_raw(1)],
    };
    let mut queue = VecDeque::from([
        interaction(sophia_protocol::PolicyInteractionPhase::Begin, 300),
        LivePublicPolicyCause {
            source: LiveWmProposalSource::Action(WmActionId::from_raw(7)),
            cause: sophia_protocol::PolicyRequestCause::Action {
                activation_serial: 9,
                action: WmActionId::from_raw(7),
            },
            affected_outputs: vec![OutputId::from_raw(1)],
        },
        interaction(sophia_protocol::PolicyInteractionPhase::Update, 350),
        interaction(sophia_protocol::PolicyInteractionPhase::End, 400),
    ]);

    assert_eq!(
        enqueue_public_policy_security_cancel(
            &mut queue,
            true,
            interaction(sophia_protocol::PolicyInteractionPhase::Cancel, 350),
        ),
        LiveWmRequestAdmission::Admitted
    );
    assert_eq!(queue.len(), 2);
    assert!(matches!(
        queue.front().map(|pending| &pending.cause),
        Some(sophia_protocol::PolicyRequestCause::Interaction {
            phase: sophia_protocol::PolicyInteractionPhase::Cancel,
            ..
        })
    ));
    assert!(matches!(
        queue.back().map(|pending| pending.source),
        Some(LiveWmProposalSource::Action(_))
    ));
}

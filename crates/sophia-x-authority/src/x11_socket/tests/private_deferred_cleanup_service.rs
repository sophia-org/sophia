// Controls for the deferred connection cleanup at the real private service
// exit: what the connection frame's completion, not its registration's
// destruction, authorizes; and what a service exit answers when a custody
// cannot discharge its duty. Harness in `private_worker_attachment.rs`; the
// pause hook is STAGE-ONLY, compiled into test builds of the dispatch alone.

/// The error path, with the connection frame paused right after its
/// registration has gone: the destruction is decided and deferred, and the
/// cleanup is nevertheless not run until the frame has completed and the
/// service's collection says so. A visit without that word refuses, with no
/// effect, and the real exit re-evaluates that refusal into a discharge.
#[test]
fn a_registrations_destruction_is_not_the_frames_completion_and_the_cleanup_waits_for_it() {
    let (launched, socket_path) = quiet_launch("cleanup-frame-pause", 9420);
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let registry = &launched.handles.registry;
    let custody = wait_attached(registry);
    let client_id = custody.cleanup_record().client;
    let release = pause_after_registration_drop(registry, client_id);
    let (acknowledgement, acknowledged) = sync_channel(1);
    drop(acknowledged);
    launched
        .commands
        .send(XServerFrontendServiceCommand::UpdateOutputTopology {
            snapshot: sophia_protocol::OutputTopologySnapshot {
                generation: 1,
                primary: sophia_protocol::OutputId::from_raw(1),
                outputs: Vec::new(),
            },
            acknowledgement,
        })
        .expect("the service is listening for commands");
    // THE FRAME REACHES ITS PAUSE: its registration has gone and its
    // destruction has been decided, and nothing after that has run.
    let decided = waited_for(|| {
        matches!(
            custody.cleanup_record().destruction_standing(),
            PrivateDestructionStanding::Decided(PrivateDestructionDecision::Deferred(_))
        )
    });
    let paused = observe_worker(&custody, registry);
    let pause_reached = !pause_pending(registry, client_id);
    // The service is still inside its wait for that frame.
    let finished_early = launched.finished.recv_timeout(Duration::from_millis(300)).is_ok();
    let without_word = custody.visit_deferred_cleanup(None);
    let after_refusal = observe_worker(&custody, registry);
    let progressed_while_paused = launched.finished.recv_timeout(Duration::from_millis(100)).is_ok();
    release.send(()).expect("the frame is waiting at its pause");
    let client_ended = eof_within(&mut client, 3);
    let outcome = launch_outcome(
        launched.handle,
        &launched.finished,
        finished_early || progressed_while_paused,
        "frame pause",
    );
    let seen = observe_worker(&custody, registry);
    assert!(decided, "the registration's destruction was decided: {paused:?}");
    assert!(pause_reached, "the frame reached its pause: {paused:?}");
    assert!(!finished_early, "the service did not return over a frame still running: {paused:?}");
    assert!(
        matches!(
            paused.standing,
            PrivateDestructionStanding::Decided(PrivateDestructionDecision::Deferred(
                PrivateDestructionDeferral::WorkerRunning
                    | PrivateDestructionDeferral::WorkerHandedOn
            ))
        ),
        "deferred, and to this custody: {:?}",
        paused.standing
    );
    assert_eq!(
        paused.deferred,
        PrivateDeferredCleanupStanding::NotVisited,
        "no cleanup ran at the registration's destruction"
    );
    assert_eq!(paused.join_phase, PrivateReapingPhase::NotBegun, "and no collection had joined");
    assert_eq!(paused.number, Some(PrivateNumberStanding::Held));
    assert!(paused.row && !paused.gate_fenced && !paused.committed, "{paused:?}");
    assert_eq!(
        without_word.result,
        Err(PrivateDeferredCleanupRefusal::ConnectionsUncollected),
        "without the collection's word the visit refuses first"
    );
    assert_eq!(
        after_refusal.deferred,
        PrivateDeferredCleanupStanding::Refused {
            refusal: PrivateDeferredCleanupRefusal::ConnectionsUncollected,
            progress: PrivateDeferredCleanupProgress::default(),
        }
    );
    assert_eq!(after_refusal.number, Some(PrivateNumberStanding::Held));
    assert!(after_refusal.row && !after_refusal.gate_fenced && !after_refusal.committed);
    assert!(!progressed_while_paused, "the refusal released nothing the service was waiting on");
    assert!(client_ended);
    assert_eq!(outcome.ok, Some(false));
    let error = outcome.error.clone().expect("the loop's own error is reported");
    assert!(error.contains("acknowledgement"), "the original error is kept: {error}");
    one_collected(&outcome, "frame pause");
    assert_collected_running(&seen, "frame pause: the real exit re-evaluated the refusal");
    let _ = std::fs::remove_file(&socket_path);
}

/// The refusal a service exit reports for a worker that was handed on
/// without a custody join: owned, effect-free, and not lifted by a joiner
/// elsewhere collecting the handle it took.
fn assert_owed_for_want_of_a_join(
    outcome: &AttachedOutcome,
    seen: &WorkerSeen,
    place: usize,
    what: &str,
) {
    assert_eq!(
        outcome.maintenance,
        vec![PrivateDeferredCleanupOutcome {
            place,
            result: Err(PrivateDeferredCleanupRefusal::JoinUnpublished),
        }],
        "{what}: the exit reports the duty, refused for want of this custody's join"
    );
    assert_eq!(
        seen.deferred,
        PrivateDeferredCleanupStanding::Refused {
            refusal: PrivateDeferredCleanupRefusal::JoinUnpublished,
            progress: PrivateDeferredCleanupProgress::default(),
        },
        "{what}"
    );
    assert_eq!(
        seen.standing,
        PrivateDestructionStanding::Decided(PrivateDestructionDecision::Deferred(
            PrivateDestructionDeferral::WorkerHandedOn
        )),
        "{what}: the original decision stands"
    );
    assert_collected_but_owed(seen, what);
    assert!(!seen.gate_fenced, "{what}: no fence was recorded");
}

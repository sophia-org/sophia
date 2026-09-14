// Actual source calls after the real participant revocation/lifecycle APIs.
fn revoke_focus_fixture(fixture: &Fixture) {
    fixture
        .private
        .participant
        .revoke_admission(client(), namespaced(client(), namespace()).client_id)
        .unwrap();
}

#[test]
fn revoked_admission_cannot_apply_surface_or_core_focus_while_registration_remains() {
    for change in [
        X11FocusChange::Surface { window: window() },
        X11FocusChange::Core {
            window: window(),
            revert_to: 2,
        },
    ] {
        let fixture = fixture();
        let claim = fixture.reserve(window());
        revoke_focus_fixture(&fixture);
        let error = fixture.apply(&claim, change).unwrap_err();
        assert!(matches!(
            error,
            X11FocusApplyError::State(
                PrivateAppliedRegistryRefusal::MissingAdmission
                    | PrivateAppliedRegistryRefusal::AdmissionClosed
            )
        ));
        assert_eq!(
            fixture
                .state
                .runtime
                .lock()
                .unwrap()
                .input_focus(namespace()),
            (root(), 1)
        );
        assert_eq!(
            fixture.projection.load(Ordering::Acquire),
            root().local.raw()
        );
        assert!(!fixture.published());
    }
}

#[test]
fn a_closed_query_lifecycle_refuses_focus_before_cleanup_removes_the_binding() {
    let fixture = fixture();
    let claim = fixture.reserve(window());
    let lifecycle = fixture
        .private
        .broker
        .registry
        .input_recovery
        .lifecycle
        .get()
        .unwrap()
        .register(client(), namespaced(client(), namespace()))
        .unwrap();
    lifecycle.close();
    assert!(matches!(
        fixture.apply(&claim, X11FocusChange::Surface { window: window() }),
        Err(X11FocusApplyError::State(
            PrivateAppliedRegistryRefusal::AdmissionClosed
        ))
    ));
    assert_eq!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .input_focus(namespace()),
        (root(), 1)
    );
    assert_eq!(
        fixture.projection.load(Ordering::Acquire),
        root().local.raw()
    );
    assert!(!fixture.published());
}

#[test]
fn a_replacement_admission_with_the_same_generation_cannot_revive_an_old_focus_claim() {
    let fixture = fixture();
    let claim = fixture.reserve(window());
    revoke_focus_fixture(&fixture);
    fixture
        .private
        .broker
        .registry
        .input_recovery
        .lifecycle
        .get()
        .unwrap()
        .drive(NonZeroUsize::new(1).unwrap())
        .unwrap();
    let old = namespaced(client(), namespace());
    let replacement = sophia_protocol::ClientAdmissionContext::new(
        sophia_protocol::ClientAdmissionId::from_raw(5252),
        old.namespace,
        old.auth_provenance,
    )
    .unwrap();
    fixture
        .private
        .participant
        .admit(client(), replacement)
        .unwrap();
    assert!(matches!(
        fixture.apply(&claim, X11FocusChange::Surface { window: window() }),
        Err(X11FocusApplyError::State(
            PrivateAppliedRegistryRefusal::ForeignOrigin
        ))
    ));
    assert_eq!(
        fixture.projection.load(Ordering::Acquire),
        root().local.raw()
    );
}

#[test]
fn the_same_authority_cannot_replace_its_applied_participant_boundary() {
    let fixture = fixture();
    let unrelated = PrivateAdmissionParticipant::new(fixture.private.controller.clone());
    assert!(matches!(
        fixture
            .private
            .broker
            .registry
            .install_private_applied(&unrelated, namespace()),
        Err(PrivateAppliedRegistryRefusal::ForeignOrigin)
    ));
}

#[test]
fn an_unreadable_admission_boundary_is_not_permission_to_apply_focus() {
    let fixture = fixture();
    let claim = fixture.reserve(window());
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _bindings = fixture.private.participant.bindings.lock().unwrap();
        panic!("unreadable authoritative boundary");
    }));
    assert!(matches!(
        fixture.apply(&claim, X11FocusChange::Surface { window: window() }),
        Err(X11FocusApplyError::State(
            PrivateAppliedRegistryRefusal::AdmissionUnavailable
        ))
    ));
    assert_eq!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .input_focus(namespace()),
        (root(), 1)
    );
    assert_eq!(
        fixture.projection.load(Ordering::Acquire),
        root().local.raw()
    );
}

#[test]
fn revocation_before_core_output_prevents_old_focusin_and_publication() {
    let fixture = fixture();
    let (stream, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_millis(60)))
        .unwrap();
    let stream = Arc::new(Mutex::new(stream));
    let (output, mut pending) =
        core_change(&fixture, window(), &stream, &Arc::new(AtomicUsize::new(0)));
    revoke_focus_fixture(&fixture);
    pending.records = Some(output.encoded_outputs(XByteOrder::LittleEndian));
    pending.write_output(&AtomicU16::new(1), 3).unwrap();
    assert_eq!(pending.emission, X11CoreFocusEmission::Superseded);
    let mut byte = [0];
    assert!(std::io::Read::read(&mut peer, &mut byte).is_err());
    assert!(!fixture.published());
}

#[test]
fn revocation_after_real_core_flush_cannot_republish_the_applied_focus() {
    let fixture = fixture();
    let (stream, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let stream = Arc::new(Mutex::new(stream));
    let (output, mut pending) =
        core_change(&fixture, window(), &stream, &Arc::new(AtomicUsize::new(0)));
    let frames = output.encoded_outputs(XByteOrder::LittleEndian);
    let count = frames.len();
    assert!(count > 0);
    pending.records = Some(frames);
    // The actual source output drops its socket before reacquiring Runtime.
    // Retain only Runtime here: bytes can flush, publication cannot yet finish.
    let runtime = fixture.state.runtime.lock().unwrap();
    let writer = std::thread::spawn(move || {
        pending.write_output(&AtomicU16::new(1), 3).unwrap();
        pending.emission
    });
    for _ in 0..count {
        let mut record = [0_u8; 32];
        std::io::Read::read_exact(&mut peer, &mut record).unwrap();
    }
    revoke_focus_fixture(&fixture);
    assert_eq!(runtime.input_focus(namespace()), (window(), 1));
    assert_eq!(
        fixture.projection.load(Ordering::Acquire),
        window().local.raw()
    );
    assert!(!fixture.published());
    drop(runtime);
    assert_eq!(writer.join().unwrap(), X11CoreFocusEmission::Superseded);
    assert!(
        !fixture.published(),
        "a real flush cannot restore revoked focus authority"
    );
}

#[test]
fn revoked_focus_still_allows_exact_dependent_and_destruction_cleanup() {
    let fixture = destruction_fixture();
    let claim = fixture.reserve(window());
    fixture
        .apply(&claim, X11FocusChange::Surface { window: window() })
        .unwrap();
    revoke_focus_fixture(&fixture);
    assert_eq!(
        fixture.focus_out(&claim),
        X11DependentFocusEffect::ProjectionCleared
    );
    assert!(!fixture.published());
    fixture
        .state
        .runtime
        .lock()
        .unwrap()
        .destroy_window(namespace(), window())
        .unwrap();
    assert_destroyed_focus(&fixture, &claim);
}

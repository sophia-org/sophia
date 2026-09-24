// These controls enter the real runtime destruction methods and the existing
// connection teardown function; setup uses the same bind_runtime producer.
fn destruction_fixture() -> Fixture {
    let fixture = fixture();
    fixture
        .private
        .broker
        .registry
        .bind_runtime(&fixture.state.runtime)
        .unwrap();
    fixture
        .state
        .runtime
        .lock()
        .unwrap()
        .prepare_input_focus_namespace(namespace());
    fixture
}

fn create_focus_test_window(fixture: &Fixture, target: XResourceId, parent: XResourceId) {
    let mut runtime = fixture.state.runtime.lock().unwrap();
    let response = runtime.apply(crate::XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(811),
        namespace: namespace(),
        kind: crate::XAuthorityRequestKind::CreateWindow {
            window: target,
            surface: SurfaceId::new(u32::try_from(target.local.raw()).unwrap(), 1),
            geometry: Rect {
                x: 0,
                y: 0,
                width: 20,
                height: 20,
            },
            constraints: sophia_protocol::SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    });
    assert_eq!(response.outcome, crate::XAuthorityResponseOutcome::Accepted);
    runtime
        .set_window_parent(namespace(), target, parent)
        .unwrap();
    drop(runtime);
    // Parented first, then mapped: a window whose parent is not yet viewable
    // maps to Unviewable, which is a state no focus fixture wants to be in
    // by accident.
    map_focus_test_window(&fixture.state.runtime, target);
}

fn assert_destroyed_focus(fixture: &Fixture, old: &PrivateFocusClaim) {
    // Destroying the focus window reverts the focus, and these fixtures took
    // it through an Engine surface focus, whose revert_to is PointerRoot. The
    // protocol says a PointerRoot revert_to reverts to PointerRoot and keeps
    // itself, so the focus is the PointerRoot sentinel rather than the root
    // window. It read the root while destroy reset unconditionally and
    // ignored revert_to.
    let pointer_root = XResourceId::new(u64::from(crate::X_FOCUS_POINTER_ROOT), 1);
    assert_eq!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .input_focus(namespace()),
        (pointer_root, crate::X_REVERT_TO_POINTER_ROOT)
    );
    assert_eq!(
        fixture.projection.load(Ordering::Acquire),
        pointer_root.local.raw()
    );
    let publication = fixture
        .private
        .broker
        .registry
        .private_applied
        .get()
        .unwrap()
        .publication
        .lock()
        .unwrap();
    assert!(publication.published);
    assert!(
        publication.focus.is_none(),
        "destroying focus publishes no key recipient"
    );
    assert!(publication.focus_generation > old.issued.generation);
    drop(publication);
    assert_eq!(fixture.focus_out(old), X11DependentFocusEffect::Superseded);
}

#[test]
fn real_destroyed_focused_window_retires_exact_publication_and_projection() {
    let fixture = destruction_fixture();
    let old = fixture.reserve(window());
    fixture
        .apply(&old, X11FocusChange::Surface { window: window() })
        .unwrap();
    fixture
        .state
        .runtime
        .lock()
        .unwrap()
        .destroy_window(namespace(), window())
        .unwrap();
    assert_destroyed_focus(&fixture, &old);
    // A later private focus succeeds without a second preparation. Its
    // prepared-only setter cannot allocate a missing namespace map entry.
    let next = XResourceId::new(0x200253, 1);
    create_focus_test_window(&fixture, next, root());
    fixture
        .apply(
            &fixture.reserve(next),
            X11FocusChange::Surface { window: next },
        )
        .unwrap();
}

#[test]
fn real_destroyed_focus_suppresses_pending_core_focusin_and_republication() {
    let fixture = destruction_fixture();
    let (stream, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_millis(60)))
        .unwrap();
    let stream = X11ClientOutput::shared(stream, 0);
    let (output, mut pending) =
        core_change(&fixture, window(), &stream, &Arc::new(AtomicUsize::new(0)));
    assert!(!fixture.published());
    fixture
        .state
        .runtime
        .lock()
        .unwrap()
        .destroy_window(namespace(), window())
        .unwrap();
    pending.records = Some(output.encoded_outputs(XByteOrder::LittleEndian));
    pending.write_output(&AtomicU16::new(1), 3).unwrap();
    assert_eq!(pending.emission, X11CoreFocusEmission::Superseded);
    let mut byte = [0];
    assert!(std::io::Read::read(&mut peer, &mut byte).is_err());
    assert!(fixture.published());
    assert!(
        fixture
            .private
            .broker
            .registry
            .private_applied
            .get()
            .unwrap()
            .publication
            .lock()
            .unwrap()
            .focus
            .is_none()
    );
}

#[test]
fn unrelated_window_destruction_preserves_applied_focus_generation() {
    let fixture = destruction_fixture();
    let other = XResourceId::new(0x200253, 1);
    create_focus_test_window(&fixture, other, root());
    let old = fixture.reserve(window());
    fixture
        .apply(&old, X11FocusChange::Surface { window: window() })
        .unwrap();
    let revision = fixture
        .private
        .broker
        .registry
        .private_applied
        .get()
        .unwrap()
        .publication
        .lock()
        .unwrap()
        .revision;
    fixture
        .state
        .runtime
        .lock()
        .unwrap()
        .destroy_window(namespace(), other)
        .unwrap();
    let publication = fixture
        .private
        .broker
        .registry
        .private_applied
        .get()
        .unwrap()
        .publication
        .lock()
        .unwrap();
    assert!(publication.published);
    assert_eq!(publication.focus_generation, old.issued.generation);
    assert_eq!(publication.revision, revision);
    assert_eq!(publication.focus.unwrap().window, window());
    assert_eq!(
        fixture.projection.load(Ordering::Acquire),
        window().local.raw()
    );
}

#[test]
fn focused_descendant_destruction_enters_the_same_runtime_source() {
    let fixture = destruction_fixture();
    let child = XResourceId::new(0x200253, 1);
    create_focus_test_window(&fixture, child, window());
    let old = fixture.reserve(child);
    fixture
        .apply(&old, X11FocusChange::Surface { window: child })
        .unwrap();
    let destroyed = fixture
        .state
        .runtime
        .lock()
        .unwrap()
        .destroy_window_subtree(namespace(), window())
        .unwrap();
    assert_eq!(destroyed.len(), 2);
    assert_destroyed_focus(&fixture, &old);
}

#[test]
fn real_client_resource_teardown_retires_focused_window_publication() {
    let fixture = destruction_fixture();
    let old = fixture.reserve(window());
    fixture
        .apply(&old, X11FocusChange::Surface { window: window() })
        .unwrap();
    let release = release_x11_client_lease(
        &fixture.state,
        namespace(),
        XServerFrontendClientLease {
            client: client(),
            resource_id_range: crate::XWireClientResourceRange {
                base: 0x200000,
                mask: 0xffff,
            },
        },
    )
    .unwrap();
    assert_eq!(release.destroyed_windows, [window()]);
    assert_destroyed_focus(&fixture, &old);
}

#[test]
fn interrupted_real_destruction_keeps_the_applied_publication_unavailable() {
    let fixture = destruction_fixture();
    let old = fixture.reserve(window());
    fixture
        .apply(&old, X11FocusChange::Surface { window: window() })
        .unwrap();
    let mut runtime = fixture.state.runtime.lock().unwrap();
    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _authority = runtime.input_authority_mut();
        panic!("make the real destruction's query cleanup unavailable");
    }));
    assert!(poisoned.is_err());
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.destroy_window(namespace(), window()).unwrap();
    }));
    assert!(interrupted.is_err());
    assert!(
        !fixture.published(),
        "native unwind must not leave the old focus readable"
    );
}

#[test]
fn unreadable_focus_authority_refuses_real_destruction_with_typed_error() {
    let fixture = destruction_fixture();
    let old = fixture.reserve(window());
    fixture
        .apply(&old, X11FocusChange::Surface { window: window() })
        .unwrap();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _common = fixture.private.controller.common.lock().unwrap();
        panic!("authority unavailable before destruction");
    }));
    let error = fixture
        .state
        .runtime
        .lock()
        .unwrap()
        .destroy_window(namespace(), window())
        .unwrap_err();
    assert_eq!(
        error,
        crate::XAuthorityRuntimeError::FocusAuthorityUnavailable
    );
    assert!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .window_geometry(namespace(), window())
            .is_ok()
    );
    assert_eq!(
        crate::x_error_from_runtime(error, 1, 4, 0, window().local.raw() as u32).code,
        crate::XErrorCode::BadImplementation
    );
    let response = crate::XAuthorityResponsePacket::rejected(TransactionId::from_raw(915), error);
    let frame = crate::encode_x_authority_response_frame(&response).unwrap();
    assert_eq!(
        crate::decode_x_authority_response_frame(&frame)
            .unwrap()
            .outcome,
        response.outcome
    );
}

#[test]
fn unprepared_private_focus_refuses_before_any_runtime_or_projection_mutation() {
    let fixture = fixture_with_focus_preparation(false);
    let error = fixture
        .apply(
            &fixture.reserve(window()),
            X11FocusChange::Surface { window: window() },
        )
        .unwrap_err();
    assert!(matches!(
        error,
        X11FocusApplyError::Runtime(crate::XAuthorityRuntimeError::FocusAuthorityUnavailable)
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

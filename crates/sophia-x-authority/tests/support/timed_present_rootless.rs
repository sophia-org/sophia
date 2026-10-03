use super::*;

#[test]
fn unmapped_pixmap_without_a_renderer_route_settles_skip_and_idle_once() {
    let f = fixture();
    let pixmap = f.prepare_next(PIXMAP, false);
    f.state
        .runtime
        .lock()
        .unwrap()
        .request_prepared_present_clock(
            pixmap,
            crate::XPresentMscTiming::new(11, 0, 0, false).unwrap(),
        )
        .unwrap();
    assert!(bind(
        &f,
        pixmap,
        crate::XPresentClockSample::background(10_100_000)
    ));
    assert!(f.broker.registry.remove_surface(SURFACE).unwrap());
    let mut generated = XGeneratedEgress::default();
    assert!(!turn(&f, &mut generated, 10_999_999));
    assert!(turn(&f, &mut generated, 11_000_000));
    assert!(!generated.pending());
    assert!(turn(&f, &mut generated, 11_000_000));
    assert!(matches!(
        f._channels.protocol.try_recv().unwrap(),
        XClientEvent::PresentIdleNotify { .. }
    ));
    assert!(matches!(
        f._channels.protocol.try_recv().unwrap(),
        XClientEvent::PresentCompleteNotify {
            kind: 0,
            mode: 2,
            msc: 11,
            ..
        }
    ));
    assert!(f._channels.protocol.try_recv().is_err());
    assert!(!turn(&f, &mut generated, 12_000_000));
    assert!(
        f.broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .is_empty()
    );
    assert!(
        f.state
            .runtime
            .lock()
            .unwrap()
            .take_cpu_buffer_updates()
            .is_empty()
    );
}

#[test]
fn losing_a_sampling_target_cannot_hide_or_block_earlier_admissions() {
    let f = fixture();
    {
        let mut runtime = f.state.runtime.lock().unwrap();
        let response = runtime.apply(XAuthorityRequestPacket {
            transaction: f.state.allocate_transaction().unwrap(),
            namespace: NS,
            kind: XAuthorityRequestKind::MapWindow {
                window: WINDOW,
                generation: 1,
            },
        });
        assert_eq!(response.outcome, crate::XAuthorityResponseOutcome::Accepted);
    }
    let first = prepare_notify(&f, 101, 11, 0, 0);
    let clocks = f.broker.present_clock_router();
    assert_eq!(
        clocks.admissions().unwrap()[0].target.map(|t| t.0),
        Some(SURFACE)
    );
    f.state
        .runtime
        .lock()
        .unwrap()
        .unmap_window(NS, WINDOW)
        .unwrap();
    let later = prepare_notify(&f, 102, 11, 0, 0);
    assert_eq!(
        clocks.admissions().unwrap(),
        vec![
            crate::XPresentClockAdmission {
                request: first,
                target: None
            },
            crate::XPresentClockAdmission {
                request: later,
                target: None
            },
        ]
    );
    let sample = crate::XPresentClockSample::background(10_100_000);
    assert!(!bind(&f, later, sample));
    for admission in clocks.admissions().unwrap() {
        assert!(bind(&f, admission.request, sample));
    }
    assert!(clocks.admissions().unwrap().is_empty());
    let mut generated = XGeneratedEgress::default();
    assert!(turn(&f, &mut generated, 11_000_000));
    assert_eq!(notification(&f), (101, 11_000_000, 11));
    assert_eq!(notification(&f), (102, 11_000_000, 11));
    assert!(clocks.bound_sources().unwrap().is_empty());
    assert!(!generated.pending());
}

#[test]
fn valid_rootless_windows_accept_both_kinds_but_invalid_windows_do_not() {
    let f = fixture();
    let mut runtime = f.state.runtime.lock().unwrap();
    for window in [
        XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1),
        XResourceId::new(u64::from(crate::X_SETUP_WM_CHECK_WINDOW), 1),
    ] {
        assert!(
            runtime
                .window_presentation_root_and_offset(NS, window)
                .is_err()
        );
        let notify = f.state.allocate_transaction().unwrap();
        runtime
            .prepare_present_msc_notify(
                CLIENT.raw(),
                notify,
                NS,
                window,
                103,
                crate::XPresentMscTiming::notify(11, 0, 0).unwrap(),
            )
            .unwrap();
        let pixmap = f.state.allocate_transaction().unwrap();
        runtime
            .prepare_standard_pixmap(
                CLIENT.raw(),
                pixmap,
                NS,
                window,
                PIXMAP,
                (0, 0),
                None,
                None,
                crate::XPresentFenceResources::default(),
            )
            .unwrap();
        runtime
            .request_prepared_present_clock(
                pixmap,
                crate::XPresentMscTiming::new(11, 0, 0, false).unwrap(),
            )
            .unwrap();
        for request in [notify, pixmap] {
            assert_eq!(
                runtime
                    .present_clock_admissions()
                    .into_iter()
                    .find(|a| a.request == request)
                    .unwrap()
                    .target,
                None
            );
            assert!(
                runtime
                    .bind_present_clock_admission(
                        request,
                        crate::XPresentClockSample::background(10_100_000),
                        None
                    )
                    .unwrap()
                    .is_some()
            );
        }
        runtime
            .advance_prepared_present_fake_clocks(11_000_000)
            .unwrap();
        let execution = f.state.allocate_transaction().unwrap();
        let result = runtime
            .execute_prepared_standard_pixmap(pixmap, execution)
            .unwrap()
            .unwrap();
        assert!(result.response.transactions.is_empty());
        assert!(
            runtime
                .ready_prepared_skips()
                .iter()
                .any(|(id, _, msc)| *id == pixmap && *msc == 11)
        );
        assert!(
            runtime
                .ready_prepared_msc_notifies()
                .iter()
                .any(|n| n.request == notify && n.msc == 11)
        );
        runtime.cancel_prepared_standard_pixmap(pixmap);
        runtime.cancel_prepared_msc_notify(notify);
    }
    for (namespace, window) in [
        (NS, XResourceId::new(0x499999, 1)),
        (NamespaceId::from_raw(999), WINDOW),
    ] {
        let id = f.state.allocate_transaction().unwrap();
        assert!(matches!(
            runtime.prepare_present_msc_notify(
                CLIENT.raw(),
                id,
                namespace,
                window,
                104,
                crate::XPresentMscTiming::notify(0, 0, 0).unwrap()
            ),
            Err(crate::XPresentPreparationError::Invalid(_))
        ));
        assert!(matches!(
            runtime.prepare_standard_pixmap(
                CLIENT.raw(),
                id,
                namespace,
                window,
                PIXMAP,
                (0, 0),
                None,
                None,
                crate::XPresentFenceResources::default()
            ),
            Err(crate::XPresentPreparationError::Invalid(_))
        ));
    }
}

#[test]
fn binding_uses_newer_accepted_progress_if_fake_service_races_selection() {
    let f = fixture();
    let first = prepare_notify(&f, 105, 12, 0, 0);
    assert!(bind(
        &f,
        first,
        crate::XPresentClockSample::background(10_000_000)
    ));
    let later = prepare_notify(&f, 106, 12, 0, 0);
    let selected = crate::XPresentClockSample::background(10_100_000);
    assert!(!turn(&f, &mut XGeneratedEgress::default(), 11_000_000));
    assert!(bind(&f, later, selected));
    assert!(turn(&f, &mut XGeneratedEgress::default(), 12_000_000));
    assert_eq!(notification(&f), (105, 12_000_000, 12));
    assert_eq!(notification(&f), (106, 12_000_000, 12));
    assert_eq!(
        f.state
            .runtime
            .lock()
            .unwrap()
            .present_timing_statistics()
            .clock_stale_samples,
        0
    );
}

#![cfg(all(test, unix))]

use super::*;

fn fixture() -> Fixture {
    let fixture = Fixture::new();
    fixture.broker.registry.cancel_present(TRANSACTION).unwrap();
    fixture
}

fn queue(fixture: &Fixture, serial: u32, suboptimal: bool) -> TransactionId {
    let transaction = TransactionId::from_raw(u64::from(serial));
    fixture
        .broker
        .registry
        .queue_present(
            transaction,
            PRESENTER,
            WINDOW,
            PIXMAP,
            serial,
            None,
            suboptimal,
        )
        .unwrap();
    let mut runtime = fixture.state.runtime.lock().unwrap();
    let response =
        runtime.present_standard_pixmap(transaction, NS, WINDOW, PIXMAP, 0, 0, None, None);
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    assert_eq!(response.transactions.len(), 1);
    let published = &response.transactions[0];
    let sophia_protocol::BufferSource::DmaBuf { handle } = published.target_buffer() else {
        panic!("completion advice requires an admitted DMA Present");
    };
    let subject = runtime
        .present_allocation_subject(
            NS,
            PRESENTER.raw(),
            WINDOW,
            PIXMAP,
            &XAuthorityPresentSubmission {
                transaction: response.transaction,
                surface: published.surface,
                buffer: BufferHandle::from_raw(handle),
                x_offset: 0,
                y_offset: 0,
                acquire_fence: None,
                idle_fence: None,
            },
        )
        .unwrap();
    fixture
        .broker
        .registry
        .record_present_allocation_subject(subject);
    transaction
}

fn complete(
    fixture: &Fixture,
    transaction: TransactionId,
    mode: XPresentCompletionMode,
    comparison: Option<XPresentLayoutComparison>,
) -> Result<crate::XPresentCompleteRouteOutcome, XServerFrontendRouteError> {
    fixture
        .broker
        .protocol_router()
        .route_present_complete_with_layout(transaction, 11, 22, mode, comparison)
}

fn expect_complete(fixture: &Fixture, serial: u32, expected: XPresentCompletionMode) {
    let event = fixture
        .channels
        .protocol
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    assert!(
        matches!(event, XClientEvent::PresentCompleteNotify { serial: actual, mode, ust: 11, msc: 22, kind: 0, .. }
            if actual == serial && mode == expected as u8),
        "{event:?}"
    );
}

fn idle(fixture: &Fixture, transaction: TransactionId) {
    assert!(fixture.broker.route_present_idle(transaction).unwrap());
    assert!(matches!(
        fixture
            .channels
            .protocol
            .recv_timeout(Duration::from_secs(1))
            .unwrap(),
        XClientEvent::PresentIdleNotify { pixmap: PIXMAP, .. }
    ));
}

fn next_generation(fixture: &mut Fixture) {
    fixture.comparison.preference_generation += 1;
    assert_eq!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .update_window_allocation_preferences(Fixture::snapshot(fixture.comparison)),
        XWindowAllocationUpdate::Applied
    );
}

#[test]
fn suboptimal_completion_is_once_per_accepted_generation() {
    assert_eq!(XPresentCompletionMode::SuboptimalCopy as u8, 3);
    let mut fixture = fixture();
    for (serial, expected) in [
        (201, XPresentCompletionMode::SuboptimalCopy),
        (202, XPresentCompletionMode::Copy),
        (203, XPresentCompletionMode::SuboptimalCopy),
    ] {
        if serial == 203 {
            next_generation(&mut fixture);
        }
        let transaction = queue(&fixture, serial, true);
        assert_eq!(
            complete(
                &fixture,
                transaction,
                XPresentCompletionMode::Copy,
                Some(fixture.comparison)
            )
            .unwrap(),
            crate::XPresentCompleteRouteOutcome {
                routed: true,
                mode: expected,
                layout_comparison: Some(XPresentLayoutComparisonResult::Matched),
            }
        );
        expect_complete(&fixture, serial, expected);
        idle(&fixture, transaction);
    }
    assert!(
        fixture
            .broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn suboptimal_completion_nonqualifying_inputs_do_not_consume_the_claim() {
    for case in 0..8 {
        let fixture = fixture();
        let mut comparison = Some(fixture.comparison);
        let incoming = match case {
            3 => XPresentCompletionMode::Flip,
            4 => XPresentCompletionMode::Skip,
            5 | 6 => XPresentCompletionMode::SuboptimalCopy,
            _ => XPresentCompletionMode::Copy,
        };
        match case {
            1 | 5 => comparison = None,
            2 | 6 => comparison.as_mut().unwrap().buffer = BufferHandle::from_raw(999),
            7 => comparison.as_mut().unwrap().preference_generation += 1,
            _ => {}
        }
        let expected = match incoming {
            XPresentCompletionMode::SuboptimalCopy => XPresentCompletionMode::Copy,
            mode => mode,
        };
        let transaction = queue(&fixture, 204, case != 0);
        let outcome = complete(&fixture, transaction, incoming, comparison).unwrap();
        assert!(outcome.routed);
        assert_eq!(outcome.mode, expected, "case={case}");
        assert_eq!(
            outcome.layout_comparison,
            comparison.map(|_| if case == 0 {
                XPresentLayoutComparisonResult::Matched
            } else {
                XPresentLayoutComparisonResult::Rejected
            }),
            "case={case}"
        );
        expect_complete(&fixture, 204, expected);
        idle(&fixture, transaction);

        let next = queue(&fixture, 205, true);
        let outcome = complete(
            &fixture,
            next,
            XPresentCompletionMode::Copy,
            Some(fixture.comparison),
        )
        .unwrap();
        assert_eq!(
            outcome.mode,
            XPresentCompletionMode::SuboptimalCopy,
            "case={case}"
        );
        expect_complete(&fixture, 205, XPresentCompletionMode::SuboptimalCopy);
        idle(&fixture, next);
    }
}

#[test]
fn suboptimal_completion_idle_first_and_duplicates_preserve_exactly_once_claims() {
    for idle_first in [false, true] {
        let mut fixture = fixture();
        let first = queue(&fixture, 206, true);
        if idle_first {
            idle(&fixture, first);
            assert!(!fixture.broker.route_present_idle(first).unwrap());
        }
        let outcome = complete(
            &fixture,
            first,
            XPresentCompletionMode::Copy,
            Some(fixture.comparison),
        )
        .unwrap();
        assert_eq!(outcome.mode, XPresentCompletionMode::SuboptimalCopy);
        expect_complete(&fixture, 206, XPresentCompletionMode::SuboptimalCopy);
        next_generation(&mut fixture);
        let duplicate = fixture
            .broker
            .protocol_router()
            .route_present_complete_with_layout(
                first,
                33,
                44,
                XPresentCompletionMode::Copy,
                Some(fixture.comparison),
            )
            .unwrap();
        assert!(!duplicate.routed);
        assert_eq!(duplicate.mode, XPresentCompletionMode::Copy);
        assert_eq!(duplicate.layout_comparison, None);
        assert_eq!(
            *fixture.broker.registry.present_clock.lock().unwrap(),
            Some((11, 22))
        );
        assert!(fixture.channels.protocol.try_recv().is_err());

        if !idle_first {
            idle(&fixture, first);
        }
        let next = queue(&fixture, 207, true);
        assert_eq!(
            complete(
                &fixture,
                next,
                XPresentCompletionMode::Copy,
                Some(fixture.comparison)
            )
            .unwrap()
            .mode,
            XPresentCompletionMode::SuboptimalCopy
        );
        expect_complete(&fixture, 207, XPresentCompletionMode::SuboptimalCopy);
        idle(&fixture, next);
    }
}

#[test]
fn suboptimal_completion_busy_runtime_routes_copy_without_waiting_or_spending() {
    let fixture = fixture();
    let first = queue(&fixture, 208, true);
    let guard = fixture.state.runtime.lock().unwrap();
    let router = fixture.broker.protocol_router();
    let comparison = fixture.comparison;
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = router.route_present_complete_with_layout(
            first,
            11,
            22,
            XPresentCompletionMode::Copy,
            Some(comparison),
        );
        let _ = sender.send(result);
    });
    let before_unlock = receiver.recv_timeout(Duration::from_millis(500));
    drop(guard);
    worker.join().unwrap();
    let outcome = before_unlock
        .expect("completion waited for optional runtime access")
        .unwrap();
    assert!(outcome.routed);
    assert_eq!(outcome.mode, XPresentCompletionMode::Copy);
    assert_eq!(
        outcome.layout_comparison,
        Some(XPresentLayoutComparisonResult::Rejected)
    );
    expect_complete(&fixture, 208, XPresentCompletionMode::Copy);
    idle(&fixture, first);

    let next = queue(&fixture, 209, true);
    assert_eq!(
        complete(
            &fixture,
            next,
            XPresentCompletionMode::Copy,
            Some(fixture.comparison)
        )
        .unwrap()
        .mode,
        XPresentCompletionMode::SuboptimalCopy
    );
    expect_complete(&fixture, 209, XPresentCompletionMode::SuboptimalCopy);
    idle(&fixture, next);
}

/// A recipient whose queue is full is ended and skipped (t090); the
/// completion is still routed, with the advice it carried, and the claim it
/// took stays spent, so the next completion is plain Copy.
#[test]
fn suboptimal_completion_with_a_blocked_recipient_keeps_the_claim_spent() {
    for partial in [false, true] {
        let fixture = fixture();
        let other = XServerFrontendClientId::from_raw(3);
        let (_registration, channels) = fixture.broker.registry.register_client(other).unwrap();
        let event_id = XResourceId::new(0x600001, 1);
        if partial {
            fixture
                .broker
                .registry
                .select_present_input(other, event_id, WINDOW, 2)
                .unwrap();
        }
        let blocked = if partial { other } else { PRESENTER };
        let sender = fixture
            .broker
            .registry
            .client_senders(blocked)
            .unwrap()
            .protocol;
        let capacity = fixture.broker.registry.per_client_protocol_capacity.get();
        for _ in 0..capacity {
            sender
                .try_send(XClientEvent::PresentIdleNotify {
                    sequence: 0,
                    event_id,
                    window: WINDOW,
                    serial: 999,
                    pixmap: PIXMAP,
                    idle_fence: None,
                })
                .unwrap();
        }
        let first = queue(&fixture, 210, true);
        // Blocked or not, the recipient's failure is its own: the route
        // completes with the advice, and the blocked one is told nothing more.
        let outcome = complete(
            &fixture,
            first,
            XPresentCompletionMode::Copy,
            Some(fixture.comparison),
        )
        .unwrap_or_else(|error| {
            panic!("a blocked recipient {blocked:?} failed the presenter: {error:?}")
        });
        assert!(outcome.routed);
        assert_eq!(outcome.mode, XPresentCompletionMode::SuboptimalCopy);
        if partial {
            expect_complete(&fixture, 210, XPresentCompletionMode::SuboptimalCopy);
        }
        let blocked_receiver = if partial {
            &channels.protocol
        } else {
            &fixture.channels.protocol
        };
        for _ in 0..capacity {
            assert!(matches!(
                blocked_receiver.try_recv().unwrap(),
                XClientEvent::PresentIdleNotify { serial: 999, .. }
            ));
        }
        assert!(blocked_receiver.try_recv().is_err());
        fixture
            .broker
            .registry
            .select_present_input(other, event_id, WINDOW, 0)
            .unwrap();
        assert!(
            !complete(
                &fixture,
                first,
                XPresentCompletionMode::Copy,
                Some(fixture.comparison)
            )
            .unwrap()
            .routed
        );
        assert!(fixture.channels.protocol.try_recv().is_err());
        idle(&fixture, first);

        let next = queue(&fixture, 211, true);
        let outcome = complete(
            &fixture,
            next,
            XPresentCompletionMode::Copy,
            Some(fixture.comparison),
        )
        .unwrap();
        assert!(outcome.routed);
        assert_eq!(outcome.mode, XPresentCompletionMode::Copy);
        assert_eq!(
            outcome.layout_comparison,
            Some(XPresentLayoutComparisonResult::Matched)
        );
        expect_complete(&fixture, 211, XPresentCompletionMode::Copy);
        idle(&fixture, next);
    }
}

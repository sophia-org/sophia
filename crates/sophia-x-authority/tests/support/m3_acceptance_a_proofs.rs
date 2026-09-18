// These cases start the actual private service and use its leased producer,
// source, original ordered worker and completion cells. Observation hooks only
// read the live owner. The recipient fault pauses that exact worker at entry;
// native withholding is the real shared-activation dependency between buttons.

#[derive(Debug)]
struct AReleaseReading {
    delivery: Option<XAuthorityInputDeliveryId>,
    incarnation: sophia_input_authority::HoldIncarnation,
    native_proof: bool,
    native_recorded: bool,
    dispatch: PrivateDispatchPhase,
    attempt: Option<sophia_input_authority::AttemptToken>,
    completion: Option<usize>,
    answer: Option<XAuthorityInputDeliveryOutcome>,
    endpoint: String,
    grant: String,
}

#[derive(Debug)]
struct AProofReading {
    held: usize,
    pending_native: bool,
    pending_detail: Option<String>,
    unsettled_requests: Vec<String>,
    releases: Vec<AReleaseReading>,
    debts: Vec<(
        sophia_input_authority::HoldIncarnation,
        sophia_input_authority::SettlementBit,
    )>,
}

impl AProofReading {
    fn release(&self, delivery: u64) -> &AReleaseReading {
        self.releases
            .iter()
            .find(|row| row.delivery == Some(XAuthorityInputDeliveryId::from_raw(delivery)))
            .expect("the original source still owns this release")
    }

    fn debt(&self, release: &AReleaseReading) -> sophia_input_authority::SettlementBit {
        self.debts
            .iter()
            .find(|(incarnation, _)| *incarnation == release.incarnation)
            .expect("the exact common incarnation remains owed")
            .1
    }
}

fn a_read_proofs(service: &LifecycleService) -> AProofReading {
    let (send, receive) = sync_channel(1);
    arm_runner(
        &service.registry,
        Box::new(move |runner, _| {
            let frontend = runner.frontend();
            let terminal = &frontend.terminal;
            let releases = terminal
                .settling
                .iter()
                .map(|release| {
                    let native = release.native.as_ref().expect("source custody");
                    AReleaseReading {
                        delivery: release.delivery(),
                        incarnation: release.incarnation,
                        native_proof: native.proof().is_some(),
                        native_recorded: release.native_recorded,
                        dispatch: release.custody.dispatch,
                        attempt: release.custody.attempt,
                        completion: release.completion().map(|cell| Arc::as_ptr(cell) as usize),
                        answer: release.completion_answer().map(|answer| answer.outcome),
                        endpoint: format!("{:?}", native.endpoint()),
                        grant: format!("{:?}", native.grant()),
                    }
                })
                .collect();
            let debts = frontend
                .controller
                .under_common(|authority| {
                    let mut debts = Vec::new();
                    let mut cursor = 0;
                    for _ in 0..=PRIVATE_HOLD_RECORDS {
                        let Some(debt) = authority.next_debt(&mut cursor) else {
                            break;
                        };
                        if debts.iter().any(|(incarnation, _)| *incarnation == debt.0) {
                            break;
                        }
                        debts.push(debt);
                    }
                    debts
                })
                .unwrap();
            send.send(AProofReading {
                held: terminal.holds.len(),
                pending_native: terminal.native_pending.is_some(),
                pending_detail: terminal.native_pending.pointer().map(|hold| {
                    format!(
                        "status={:?}, incarnation={:?}",
                        hold.status(),
                        hold.incarnation()
                    )
                }),
                unsettled_requests: terminal
                    .turn
                    .iter()
                    .chain(terminal.delivering.iter())
                    .chain(terminal.undelivered.iter().map(|entry| &entry.item))
                    .filter_map(|item| match item {
                        PrivateOrderedItem::Refused {
                            sequence,
                            refusal,
                            custody,
                            ..
                        } => Some(format!(
                            "sequence={sequence:?}, refusal={refusal:?}, phase={:?}, observed={:?}",
                            custody.phase.get(),
                            custody.observed_outcome.get()
                        )),
                        _ => None,
                    })
                    .collect(),
                releases,
                debts,
            })
            .unwrap();
        }),
    );
    receive.recv_timeout(Duration::from_secs(3)).unwrap()
}

fn a_wait_proofs(
    service: &LifecycleService,
    ready: impl Fn(&AProofReading) -> bool,
) -> AProofReading {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let reading = a_read_proofs(service);
        if ready(&reading) {
            return reading;
        }
        assert!(
            Instant::now() < deadline,
            "source did not reach expected state: {reading:?}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn a_submit_button(
    service: &LifecycleService,
    ingress: &PrivateIngress,
    surface: SurfaceId,
    delivery: u64,
    button: u32,
    pressed: bool,
) -> (crate::ReadySequence, Arc<PrivateDeliveryCompletion>) {
    let position = ingress
        .submit(
            &service.owner.lease(),
            button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                button,
                pressed,
            ),
        )
        .unwrap();
    let cell =
        delivery_cell(&service.registry, delivery).expect("acceptance minted the exact cell");
    (position, cell)
}

fn a_assert_flushed(cell: &Arc<PrivateDeliveryCompletion>) {
    assert!(waited_for(|| cell.answer().is_some()));
    assert_eq!(
        cell.answer().unwrap().outcome,
        XAuthorityInputDeliveryOutcome::Flushed
    );
}

fn a_assert_barrier(
    service: &LifecycleService,
    ingress: &PrivateIngress,
    surface: SurfaceId,
    delivery: u64,
) -> (crate::ReadySequence, Arc<PrivateDeliveryCompletion>) {
    let submitted = a_submit_button(service, ingress, surface, delivery, 272, true);
    let deadline = Instant::now() + Duration::from_secs(1);
    while submitted.1.answer().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    if submitted.1.answer().is_none() {
        let diagnostic = a_read_proofs(service);
        panic!(
            "actual barrier refusal remains unanswered: {diagnostic:?}; pending={:?}; requests={:?}",
            diagnostic.pending_detail, diagnostic.unsettled_requests
        );
    }
    assert_eq!(
        submitted.1.answer().unwrap().outcome,
        XAuthorityInputDeliveryOutcome::RouteRejected
    );
    submitted
}

fn a_assert_barrier_turn(turns: &[PrivateRunnerProgress]) {
    assert!(
        turns.iter().any(|turn| {
            turn.refused > 0
                && turn.last_refusal
                    == Some(PrivateExecutionRefusal::Native(
                        private_native::Refusal::Authority(
                            sophia_input_authority::RegistrationError::ReleaseBarrier,
                        ),
                    ))
        }),
        "the original executor must report the exact common ReleaseBarrier: {turns:?}"
    );
}

fn a_repress_after_settlement(
    service: &LifecycleService,
    ingress: &PrivateIngress,
    peer: &mut UnixStream,
    surface: SurfaceId,
    sequence: u16,
    window: u32,
    first: u64,
) -> Value {
    assert!(waited_for(|| service
        .controller
        .under_common(|authority| authority.next_debt(&mut 0).is_none())
        .unwrap()));
    let mut observed = Vec::new();
    for (offset, pressed) in [(0, true), (1, false)] {
        let (position, cell) =
            a_submit_button(service, ingress, surface, first + offset, 272, pressed);
        let bytes = read_event(peer, 3).expect("the original writer emits the new event");
        assert_eq!(bytes, expected_button_event(pressed, sequence, window, 1));
        a_assert_flushed(&cell);
        observed.push(json!({"position":format!("{position:?}"),"completion":format!("{:p}",Arc::as_ptr(&cell)),"outcome":format!("{:?}",cell.answer()),"bytes":bytes.to_vec()}));
    }
    await_empty_cleanup(service);
    let clean = a_read_proofs(service);
    assert!(clean.debts.is_empty() && clean.releases.is_empty());
    assert_eq!(clean.held, 0);
    assert!(!clean.pending_native);
    json!({"events":observed,"final_original_inventory":format!("{clean:?}")})
}

fn a_recipient_withheld() -> (Value, Vec<String>) {
    let (pause, release_worker) = Pause::pair();
    let mut service = LifecycleService::launch(
        "a-recipient-proof",
        11300,
        Some(AttachFault::Body { pause, panic: None }),
        false,
    );
    service.start();
    let (mut peer, custody) = service.connect();
    let worker = release_worker.entered();
    let window = 0x313001;
    let (surface, sequence, ingress) = focus_window(&service, &mut peer, window, 11300);
    observe_turns(&service.registry);
    let (press_position, press) = a_submit_button(&service, &ingress, surface, 11301, 272, true);
    a_wait_proofs(&service, |reading| reading.held == 1);
    let (release_position, release) =
        a_submit_button(&service, &ingress, surface, 11302, 272, false);
    let before = a_wait_proofs(&service, |reading| {
        reading.releases.iter().any(|row| {
            row.native_recorded
                && row.attempt.is_some()
                && row.dispatch == PrivateDispatchPhase::Enqueued
        })
    });
    let row = before.release(11302);
    assert!(row.native_proof && row.native_recorded);
    assert_eq!(row.completion, Some(Arc::as_ptr(&release) as usize));
    assert_eq!(row.answer, None);
    assert!(!row.endpoint.is_empty() && !row.grant.is_empty());
    assert_eq!(
        before.debt(row),
        sophia_input_authority::SettlementBit {
            native_reconciled: true,
            recipient_settled: false
        }
    );
    assert_eq!(press.answer(), None);
    assert_eq!(release.answer(), None);
    let (blocked_position, blocked) = a_assert_barrier(&service, &ingress, surface, 11303);
    let after_refusal = a_read_proofs(&service);
    assert_eq!(after_refusal.release(11302).incarnation, row.incarnation);
    assert_eq!(
        after_refusal.debt(after_refusal.release(11302)),
        before.debt(row)
    );
    release_worker.release();
    let press_bytes = read_event(&mut peer, 3).unwrap();
    let release_bytes = read_event(&mut peer, 3).unwrap();
    assert_eq!(
        press_bytes,
        expected_button_event(true, sequence, window, 1)
    );
    assert_eq!(
        release_bytes,
        expected_button_event(false, sequence, window, 1)
    );
    a_assert_flushed(&press);
    a_assert_flushed(&release);
    assert_eq!(
        blocked.answer().unwrap().outcome,
        XAuthorityInputDeliveryOutcome::RouteRejected
    );
    let success = a_repress_after_settlement(
        &service, &ingress, &mut peer, surface, sequence, window, 11304,
    );
    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed = service.closed();
    assert!(closed.succeeded, "{closed:?}");
    let turns = take_turns(&service.registry);
    a_assert_barrier_turn(&turns);
    let observed = json!({"fault_seam":"actual original ordered worker paused at body entry; release-on-drop and five-second bound","worker":format!("{worker:?}"),"press_position":format!("{press_position:?}"),"release_position":format!("{release_position:?}"),"blocked_position":format!("{blocked_position:?}"),"before_recipient_proof":format!("{before:?}"),"after_repress_refusal":format!("{after_refusal:?}"),"press_bytes":press_bytes.to_vec(),"release_bytes":release_bytes.to_vec(),"original_press_completion":format!("{:p}",Arc::as_ptr(&press)),"original_release_completion":format!("{:p}",Arc::as_ptr(&release)),"same_ingress_repress":success,"turns":format!("{turns:?}"),"service_order":format!("{:?}",closed.order)});
    (observed, service.finish(&[custody]))
}

fn a_native_withheld_by_overlap() -> (Value, Vec<String>) {
    let mut service = LifecycleService::launch("a-native-proof", 11310, None, false);
    service.start();
    let (mut peer, custody) = service.connect();
    let window = 0x313101;
    let (surface, sequence, ingress) = focus_window(&service, &mut peer, window, 11310);
    observe_turns(&service.registry);
    let mut presses = Vec::new();
    let mut bytes = Vec::new();
    for (id, button, core, state) in [(11311, 272, 1, 0), (11312, 274, 2, 1 << 8)] {
        let (position, cell) = a_submit_button(&service, &ingress, surface, id, button, true);
        let event = read_event(&mut peer, 3).unwrap();
        assert_eq!(
            event,
            expected_chord_event(true, sequence, window, core, state)
        );
        a_assert_flushed(&cell);
        presses.push((position, cell));
        bytes.push(event.to_vec());
    }
    let (left_position, left) = a_submit_button(&service, &ingress, surface, 11313, 272, false);
    let withheld = a_wait_proofs(&service, |reading| !reading.releases.is_empty());
    let row = withheld.release(11313);
    assert_eq!(withheld.held, 1);
    assert!(!row.native_proof && !row.native_recorded);
    assert_eq!(row.dispatch, PrivateDispatchPhase::Untaken);
    assert_eq!(row.attempt, None);
    assert_eq!(row.completion, Some(Arc::as_ptr(&left) as usize));
    assert_eq!(
        withheld.debt(row),
        sophia_input_authority::SettlementBit::default()
    );
    assert_eq!(left.answer(), None);
    let (blocked_position, blocked) = a_assert_barrier(&service, &ingress, surface, 11314);
    let still_withheld = a_read_proofs(&service);
    assert_eq!(still_withheld.release(11313).incarnation, row.incarnation);
    assert!(!still_withheld.release(11313).native_proof);
    assert_eq!(
        still_withheld.release(11313).dispatch,
        PrivateDispatchPhase::Untaken
    );
    assert_eq!(
        still_withheld.debt(still_withheld.release(11313)),
        sophia_input_authority::SettlementBit::default()
    );
    let (middle_position, middle) = a_submit_button(&service, &ingress, surface, 11315, 274, false);
    for (core, state) in [(1, (1 << 8) | (1 << 9)), (2, 1 << 9)] {
        let event = read_event(&mut peer, 3).unwrap();
        assert_eq!(
            event,
            expected_chord_event(false, sequence, window, core, state)
        );
        bytes.push(event.to_vec());
    }
    a_assert_flushed(&left);
    a_assert_flushed(&middle);
    for (_, cell) in &presses {
        a_assert_flushed(cell);
    }
    assert_eq!(
        blocked.answer().unwrap().outcome,
        XAuthorityInputDeliveryOutcome::RouteRejected
    );
    let success = a_repress_after_settlement(
        &service, &ingress, &mut peer, surface, sequence, window, 11316,
    );
    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed = service.closed();
    assert!(closed.succeeded, "{closed:?}");
    assert_eq!(closed.order.unwrap().activations_joined, 1);
    let turns = take_turns(&service.registry);
    a_assert_barrier_turn(&turns);
    let observed = json!({"withholding_source":"left release retains its exact shared activation while middle button remains held; no fabricated native or recipient outcome","completion_scope":"both original press cells are Flushed; left release has not been dispatched and remains unanswered","before_native_proof":format!("{withheld:?}"),"after_repress_refusal":format!("{still_withheld:?}"),"left_release_position":format!("{left_position:?}"),"middle_release_position":format!("{middle_position:?}"),"blocked_position":format!("{blocked_position:?}"),"exact_chord_bytes":bytes,"original_release_completions":[format!("{:p}",Arc::as_ptr(&left)),format!("{:p}",Arc::as_ptr(&middle))],"same_ingress_repress":success,"turns":format!("{turns:?}"),"source_activation_joins":closed.order.unwrap().activations_joined});
    (observed, service.finish(&[custody]))
}

#[test]
fn a_press_release_repress() {
    let (recipient, mut actors) = a_recipient_withheld();
    let (overlap, more) = a_native_withheld_by_overlap();
    actors.extend(more);
    emit_case(
        "A.press_release_repress",
        &[
            ("exact_press_release_bytes", recipient.clone()),
            ("native_and_recipient_proofs", recipient.clone()),
            ("repress_barrier_then_success", recipient),
            ("overlapping_button_releases", overlap),
        ],
        &actors,
    );
}

#[test]
fn a_partial_proof() {
    let (recipient, mut actors) = a_recipient_withheld();
    let (native, more) = a_native_withheld_by_overlap();
    actors.extend(more);
    emit_case(
        "A.partial_proof",
        &[
            ("native_proof_withheld", native.clone()),
            ("recipient_proof_withheld", recipient),
            ("completion_alone_keeps_barrier", native),
        ],
        &actors,
    );
}

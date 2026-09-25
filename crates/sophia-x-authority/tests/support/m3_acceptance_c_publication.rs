// Enqueued observation, the focus window without motion and refused
// publication. Included from m3_acceptance.rs beside m3_acceptance_c.rs
// (t026).

/// One capsule handed to its recipient's queue and held there, by holding the
/// connection's own ordered home.
///
/// A REAL HOLD ON THE THING THAT SERVES. The connection's worker takes its
/// ordered home to perform each serving step, so while this case holds that
/// lock the capsule can be handed to the queue and cannot be written. Nothing
/// is saturated and no recipient is starved; the interval is made by holding
/// the one lock the writer must have.
fn enqueued_observation(namespace: u64, window: u32) -> (Value, Vec<String>) {
    let _observer = own_the_observer();
    let store = PrivateSettlementOwner::with_capacity(C_DEEP_BOUND);
    let mut service = LifecycleService::launch_over_store(
        "c-enqueued-observation",
        namespace,
        None,
        false,
        1,
        store.clone(),
    );
    service.start();
    let (mut peer, custody) = service.connect();
    let (surface, sequence, ingress) = focus_window(&service, &mut peer, window, namespace);
    observe_frames(&service.registry);

    // ORDERED ATTACHMENT FIRST. The focus notification is a legacy writer's
    // work and says nothing about this connection's ordered worker being
    // live. Taking the home before attachment finished would hold it through
    // promotion rather than through the serving step this case is about.
    let attachment =
        waited_for_value(|| custody.attachment()).expect("the ordered attachment settled");
    assert_eq!(
        attachment,
        PrivateAttachment::Started,
        "the connection's own ordered worker is live before its home is held"
    );

    let delivery = 14_000 + namespace;
    let wanted = XAuthorityInputDeliveryId::from_raw(delivery);
    let home = Arc::clone(&custody.cleanup_record().ordered_home);
    let held_home = home
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    ingress
        .submit(&service.owner.lease(), axis_to(surface, wanted, 30))
        .expect("the order accepts one real axis capsule");
    let cell = waited_for_value(|| delivery_cell(&service.registry, delivery))
        .expect("the capsule's own completion, minted by its own admission");
    watch_completion(&cell);

    assert!(
        waited_for(|| queue_handovers_snapshot()
            .iter()
            .any(|handover| handover.delivery == wanted && handover.accepted)),
        "the capsule reached its recipient's queue"
    );
    assert!(
        cell.answer().is_none(),
        "and nothing has answered it while it sits there"
    );
    let frames_while_held = send_entries_snapshot()
        .iter()
        .filter(|entry| entry.delivery == Some(wanted))
        .count();
    assert_eq!(
        frames_while_held, 0,
        "no frame of it reached the wire while the home was held"
    );

    // THE SERVICE'S OWN CHARGED VISITS OVER IT. A turn may perform many
    // observation steps and only then report that its starts ran out, so what
    // is counted is steps, not turns, and the final allowance is kept apart
    // from them rather than deciding whether they happened.
    let (report, reported) = sync_channel(1);
    let inspected = Arc::clone(&cell);
    arm_runner(
        &service.registry,
        Box::new(move |runner, lease| {
            let state_of = |runner: &PrivatePreparedRunner| {
                runner
                    .frontend()
                    .terminal
                    .transients
                    .records
                    .iter()
                    .find(|record| {
                        record
                            .custody
                            .completion
                            .as_ref()
                            .is_some_and(|held| Arc::ptr_eq(held, &inspected))
                    })
                    .map(|record| {
                        (
                            record.custody.dispatch,
                            record.custody.pending.is_some(),
                            record.custody.outcome_seen,
                        )
                    })
            };
            let mut visits = Vec::new();
            let mut states = Vec::new();
            let mut charged_steps = 0usize;
            let mut final_allowance = None;
            let deadline = std::time::Instant::now() + Duration::from_secs(8);
            while charged_steps < 2 && std::time::Instant::now() < deadline {
                let before = state_of(runner);
                match runner.service_turn(lease) {
                    Ok(progress) => {
                        let after = state_of(runner);
                        let steps = progress.transient_observed;
                        assert!(
                            !progress.watch_failed,
                            "a visit over the pending capsule kept its supervisor"
                        );
                        if steps > 0 {
                            assert!(
                                progress.starts >= steps,
                                "each observation step was charged a start: starts={} steps={steps}",
                                progress.starts
                            );
                            charged_steps += steps;
                        }
                        visits.push(format!(
                            "starts={} observation_steps={steps} taken={} observed={} allowance={:?}",
                            progress.starts, progress.taken, progress.observed, progress.allowance
                        ));
                        states.push((before, after));
                        final_allowance = progress.allowance.map(|refusal| format!("{refusal:?}"));
                        match progress.allowance {
                            Some(
                                sophia_input_authority::ServiceStartRefusal::StartsExhausted {
                                    retry_after,
                                },
                            ) => std::thread::sleep(retry_after.min(Duration::from_millis(50))),
                            Some(other) => panic!(
                                "a visit refused for something waiting cannot fix: {other:?} in {visits:?}"
                            ),
                            None => {}
                        }
                    }
                    Err(error) => {
                        visits.push(format!("{error:?}"));
                        break;
                    }
                }
            }
            report
                .send((visits, charged_steps, states, final_allowance))
                .expect("the case is waiting");
        }),
    );
    let (visits, charged_steps, states, final_allowance) = reported
        .recv_timeout(Duration::from_secs(12))
        .expect("the actual runner reported its visits over the pending capsule");
    assert!(
        charged_steps >= 2,
        "the pending transient was observed by at least two charged steps: {visits:?}"
    );
    // EVERY LOOKUP HAS TO FIND IT. A record that could not be found says
    // nothing about its state, and a run of misses would otherwise pass.
    assert!(
        !states.is_empty()
            && states.iter().all(|(before, after)| {
                matches!(
                    (before, after),
                    (
                        Some((PrivateDispatchPhase::Enqueued, false, None)),
                        Some((PrivateDispatchPhase::Enqueued, false, None))
                    )
                )
            }),
        "the record stayed enqueued, with no capsule copy and no outcome, either side of every visit: {states:?}"
    );
    let visits_of_this_cell = transient_visits_snapshot();
    assert!(
        visits_of_this_cell.len() >= 2,
        "this exact completion was read by at least two charged observations: {visits_of_this_cell:?}"
    );
    assert!(
        visits_of_this_cell
            .iter()
            .all(|visit| visit.dispatch == PrivateDispatchPhase::Enqueued && !visit.answer_seen),
        "each found it enqueued and unanswered: {visits_of_this_cell:?}"
    );
    let handovers_during = queue_handovers_snapshot()
        .iter()
        .filter(|handover| handover.delivery == wanted && handover.accepted)
        .count();
    assert_eq!(
        handovers_during, 1,
        "and none of those visits handed it over again: {visits:?}"
    );
    assert!(
        cell.answer().is_none(),
        "nor answered it: observation is not settlement"
    );

    // RELEASED. The worker takes its home back and writes what it already
    // had. The recipient's reads and the writer's receipt are scheduled
    // independently, so both are waited for rather than either standing in
    // for the other.
    drop(held_home);
    let mut completion = None;
    let mut released_frames = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline
        && !(released_frames.len() >= 2 && completion.is_some())
    {
        if let Some(event) = read_event(&mut peer, 1) {
            released_frames.push(event);
        }
        while let Ok(receipt) = service.deliveries.try_recv() {
            assert_eq!(
                receipt.delivery, wanted,
                "only this capsule was owed a receipt"
            );
            assert!(
                completion.replace(receipt.outcome).is_none(),
                "and it is answered once"
            );
        }
    }
    let completion = completion.expect("the released worker published its own completion");
    assert_eq!(
        completion,
        XAuthorityInputDeliveryOutcome::Flushed,
        "which is its own flush"
    );
    // THE TWO EXACT WHEEL FRAMES, hand-encoded from the X protocol and the
    // request this case submitted: an emulated wheel button down with nothing
    // held, then up with that button in the prior state.
    assert_eq!(
        released_frames,
        vec![
            expected_transient_event(4, 5, sequence, window, 0, 30),
            expected_transient_event(5, 5, sequence, window, 1 << 12, 30),
        ],
        "the released capsule put out its own two frames and no others"
    );
    assert_eq!(read_event(&mut peer, 1), None, "and nothing followed them");
    let second = service
        .deliveries
        .recv_timeout(Duration::from_millis(300))
        .ok();
    assert!(second.is_none(), "with no second receipt: {second:?}");

    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed = service.closed();
    stop_watching_completion();
    let (_, send_entries, queue_handovers) = take_observations();
    assert!(
        !observation_overflowed(),
        "no recorder dropped a record; a missing tail would read like a retry that never happened"
    );
    let frame_sequence: Vec<usize> = send_entries
        .iter()
        .filter(|entry| entry.delivery == Some(wanted))
        .map(|entry| entry.frame_index)
        .collect();
    assert_eq!(
        frame_sequence,
        vec![0, 1],
        "the released capsule put its own two frames out, in order and once each"
    );
    let handovers_total = queue_handovers
        .iter()
        .filter(|handover| handover.delivery == wanted && handover.accepted)
        .count();
    assert_eq!(
        handovers_total, 1,
        "and was handed to the queue exactly once in the whole interval"
    );
    let seen = json!({
        "delivery": delivery,
        "hold": "the connection's own ordered home, taken only after its ordered attachment reported Started",
        "attachment_before_hold": format!("{attachment:?}"),
        "frames_while_held": frames_while_held,
        "charged_visits": visits,
        "charged_observation_steps": charged_steps,
        "final_allowance": final_allowance,
        "record_state_before_and_after_each_visit": format!("{states:?}"),
        "observations_of_this_exact_cell": visits_of_this_cell.len(),
        "queue_handovers_accepted": handovers_total,
        "frame_sequence_after_release": frame_sequence,
        "released_frames": released_frames.iter().map(|frame| frame.to_vec()).collect::<Vec<_>>(),
        "its_completion": format!("{completion:?}"),
        "closed_error": closed.error.clone(),
        "what_this_establishes": "a capsule held in its recipient's queue is observed by at least two of the service's own charged steps, each of which read its exact completion and found it enqueued and unanswered, is handed over exactly once, and on release puts its own two frames out once each for a single flush. This is a first write after a hold, not the resumption of a blocked frame; the partial case establishes prefix preservation.",
    });
    let collected = finish_labelled("enqueued-observation invocation", service, &[custody]);
    (seen, collected)
}

/// A decided request whose outcome cannot be published, through two
/// deterministic holds of the original runner.
///
/// The recipient's own selection refuses this request, so the executor decides
/// it and then owes its refusal to the completion that request was admitted
/// with. Taking that admission away while the runner is held means the
/// publication is attempted and refused rather than never tried; giving it
/// back while the runner is held again means the no-publication check cannot
/// race the service's own legitimate retry.
/// A real focused recipient that selects buttons and focus changes and no
/// pointer motion at all.
///
/// THE REFUSAL HAS TO BE THE RECIPIENT'S OWN. The shared arrangement selects
/// pointer motion, so a motion submitted to it is work the consumer takes;
/// what this row needs is a request the consumer actually declines, which is a
/// motion to a window that never asked for one.
fn focus_window_without_motion(
    service: &LifecycleService,
    peer: &mut UnixStream,
    window: u32,
    transaction: u64,
) -> (SurfaceId, u16, PrivateIngress) {
    let (surface, sequence) = selecting_window(
        peer,
        &service.transactions,
        window,
        (1 << 2) | (1 << 3) | (1 << 21),
    );
    let custody = wait_attached(&service.registry);
    let client = custody.cleanup_record().client;
    let lease = service.owner.lease();
    let control = service
        .access
        .control_producer(&lease)
        .expect("the service's own control producer");
    control
        .submit(
            &lease,
            XAuthorityClientControlCommand {
                client,
                command: XAuthorityControlCommand::FocusSurface {
                    transaction: TransactionId::from_raw(transaction),
                    surface,
                },
            },
        )
        .expect("the order accepts the focus control");
    assert_eq!(
        ack_for(&service.acks, transaction)
            .expect("the writer published its own outcome")
            .acknowledgement
            .outcome,
        XAuthorityControlOutcome::Delivered,
        "the focus control was actually applied"
    );
    assert_eq!(
        read_event(peer, 3),
        Some(expected_focus_in(sequence, window)),
        "and this recipient read its own focus notification"
    );
    let ingress = service
        .access
        .ingress_for(&lease, client, DeviceId::from_raw(1))
        .expect("an actual leased producer");
    (surface, sequence, ingress)
}

/// What one charged turn found of the refused request, read on the runner's
/// own thread while it held the frontend.
///
/// TAKEN WHERE IT IS TRUE. The item, its carried completion, its route and its
/// phase live inside the runner, and reading them afterwards would read the
/// aftermath of whatever the service did next.
struct RefusedReading {
    turns: BoundedTrace,
    undelivered: usize,
    /// The completion the retained item is still carrying, as the original
    /// handle, to be compared with the one this request's admission minted.
    carried: Option<Arc<PrivateDeliveryCompletion>>,
    route_delivery: Option<XAuthorityInputDeliveryId>,
    completion: Option<sophia_input_authority::RequestCompletion>,
    phase: Option<PrivateRequestPhase>,
    /// Whether the item still held its accepted-item credit, and the store it
    /// was drawn on.
    credit_against: Option<std::sync::Weak<Mutex<AbandonedSettlements>>>,
    /// Starts charged on the turn that reached the adjudication, and whether
    /// EVERY turn taken here kept its supervisor.
    starts: usize,
    turns_taken: usize,
    supervised: bool,
    /// An allowance refusal that is not an exhausted start budget. Waiting
    /// cannot fix one, so the loop stops and the case fails on it rather than
    /// continuing quietly.
    unexpected_allowance: Option<String>,
}

/// A bounded record of repeated readings.
///
/// AN UNBOUNDED TRACE IS ITS OWN FAILURE. A loop that formats one line per
/// turn can grow a log by hundreds of megabytes while establishing nothing the
/// first few lines did not, and the formatting is most of the cost. This keeps
/// a bounded head, counts everything, and says how much it left out.
#[derive(Default, Clone)]
struct BoundedTrace {
    kept: Vec<String>,
    total: usize,
}

const TRACE_BOUND: usize = 32;

impl BoundedTrace {
    /// Record one reading. The line is not built at all once the bound is
    /// reached, so a long idle loop costs a counter and nothing else.
    fn push(&mut self, line: impl FnOnce() -> String) {
        self.total += 1;
        if self.kept.len() < TRACE_BOUND {
            self.kept.push(line());
        }
    }

    fn elided(&self) -> usize {
        self.total - self.kept.len()
    }

    fn evidence(&self) -> Value {
        json!({
            "kept": self.kept,
            "total": self.total,
            "elided": self.elided(),
        })
    }
}

impl std::fmt::Debug for BoundedTrace {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            out,
            "{:?} (+{} more, {} in all)",
            self.kept,
            self.elided(),
            self.total
        )
    }
}

/// Whether that accepted-item credit is drawn on this store.
///
/// The credit holds a weak handle to the store it was taken from, so this is
/// the credit naming its own store rather than two totals agreeing.
fn credit_is_against(
    store: &PrivateSettlementOwner,
    credit: &std::sync::Weak<Mutex<AbandonedSettlements>>,
) -> bool {
    credit
        .upgrade()
        .is_some_and(|held| Arc::ptr_eq(&held, &store.inner))
}

/// The retained refused request, as the runner's own frontend holds it.
struct RefusedItem {
    /// The completion it is carrying, as the original handle.
    carried: Option<Arc<PrivateDeliveryCompletion>>,
    route_delivery: Option<XAuthorityInputDeliveryId>,
    completion: Option<sophia_input_authority::RequestCompletion>,
    phase: PrivateRequestPhase,
    /// Whether the item still holds its accepted-item credit, and which store
    /// that credit is against.
    ///
    /// THE CREDIT ITSELF, NOT A COUNT. A store's reserved total going from one
    /// to one says a credit is outstanding somewhere; this says the credit is
    /// still on this item and is drawn on this store.
    credit_against: Option<std::sync::Weak<Mutex<AbandonedSettlements>>>,
}

/// What the runner's frontend is holding for the refused request right now.
fn read_refused_item(runner: &PrivatePreparedRunner) -> Option<RefusedItem> {
    runner
        .frontend()
        .terminal
        .undelivered
        .iter()
        .find_map(|undelivered| match &undelivered.item {
            PrivateOrderedItem::Refused { custody, route, .. } => Some(RefusedItem {
                carried: custody
                    .input_completion()
                    .map(|held| Arc::clone(&held.cell)),
                route_delivery: route.delivery,
                completion: custody.observed_outcome.get(),
                phase: custody.phase.get(),
                credit_against: custody
                    .accepted_store_credit
                    .as_ref()
                    .map(|credit| credit.store.clone()),
            }),
            PrivateOrderedItem::Ran { .. } | PrivateOrderedItem::Parked { .. } => None,
        })
}

/// A decided request whose outcome cannot be published, through two
/// deterministic holds of the original runner.
///
/// The recipient's own selection refuses this request, so the executor decides
/// it and then owes its refusal to the completion that request was admitted
/// with. Taking that admission away while the runner is held means the
/// publication is attempted and refused rather than never tried; giving it
/// back while the runner is held again means the no-publication check cannot
/// race the service's own legitimate retry.
fn refused_publication(namespace: u64, window: u32) -> (Value, Vec<String>) {
    let _observer = own_the_observer();
    let store = PrivateSettlementOwner::with_capacity(C_DEEP_BOUND);
    let mut service = LifecycleService::launch_over_store(
        "c-refused-publication",
        namespace,
        None,
        false,
        1,
        store.clone(),
    );
    service.start();
    let (mut peer, custody) = service.connect();
    let (surface, _sequence, _focus) =
        focus_window_without_motion(&service, &mut peer, window, namespace);
    let client = custody.cleanup_record().client;
    let producers = leased_producers(&service, client, C_PRODUCERS);
    let delivery = 12_070 + namespace;
    let wanted = XAuthorityInputDeliveryId::from_raw(delivery);

    // THE ARRANGEMENT'S OWN CREDIT IS SPENT FIRST. The focus work above took
    // credits and gave them back; waiting for the store to be empty is what
    // makes the one credit below exactly this request's rather than a
    // leftover that happens to add up.
    assert!(
        waited_for(|| store.reserved() == Some(0)),
        "the focus arrangement released every credit it took: {:?}",
        store.reserved()
    );

    // HOLD ONE. The request is submitted while nothing can run, and the fault
    // that takes its admission away is armed on its own completion.
    //
    // THE ADMISSION CANNOT BE TAKEN ANY EARLIER. Execution claims that same
    // admission before it runs, so a request whose ticket is already gone is
    // never executed and never decided -- there would be no source refusal to
    // owe an answer for, and nothing to refuse the publication of. The fault
    // therefore fires at the one moment that produces this state: the refusal
    // has been decided and is being offered, and the admission goes at that
    // instant. It removes the real ticket and keeps it whole; nothing is
    // fabricated and the adjudication below decides for itself.
    let witness: Arc<Mutex<Option<Arc<PrivateDeliveryCompletion>>>> = Arc::default();
    let hook_cell = Arc::clone(&witness);
    let (pause, held) = Pause::pair();
    let (report, reported) = sync_channel(1);
    arm_runner(
        &service.registry,
        Box::new(move |runner, lease| {
            pause.wait();
            // THE ORIGINAL CELL, HANDED IN. This is the completion the case
            // took from the request's own admission before anything could run;
            // comparing the item against it is comparing it against that
            // admission and not against a second reading of itself.
            let original =
                hook_cell.lock().unwrap().clone().expect(
                    "the case shared the request's own completion before releasing the runner",
                );
            let mut turns = BoundedTrace::default();
            let mut reading = RefusedReading {
                turns: BoundedTrace::default(),
                undelivered: 0,
                carried: None,
                route_delivery: None,
                completion: None,
                phase: None,
                credit_against: None,
                starts: 0,
                turns_taken: 0,
                supervised: true,
                unexpected_allowance: None,
            };
            let deadline = std::time::Instant::now() + Duration::from_secs(8);
            while std::time::Instant::now() < deadline {
                let progress = match runner.service_turn(lease) {
                    Ok(progress) => progress,
                    Err(error) => {
                        turns.push(|| format!("{error:?}"));
                        break;
                    }
                };
                turns.push(|| {
                    format!(
                        "starts={} taken={} refused={} settled={} terminal_steps={} allowance={:?} watch_failed={}",
                        progress.starts,
                        progress.taken,
                        progress.refused,
                        progress.settled,
                        progress.terminal_steps,
                        progress.allowance,
                        progress.watch_failed
                    )
                });
                reading.turns_taken += 1;
                reading.supervised = reading.supervised && !progress.watch_failed;
                // STOP ON THE THING ITSELF. A retained item appears in the
                // vector before anything has observed its common outcome, so
                // occupancy says nothing. What this waits for is an actual
                // refused adjudication of THIS cell and the item's own typed
                // reading of what common gave it.
                let refused_on_this_cell = adjudications_snapshot()
                    .iter()
                    .any(|seen| seen.answer == PrivateAdjudication::Refused);
                if let Some(item) = read_refused_item(runner)
                    && refused_on_this_cell
                    && item.completion.is_some()
                {
                    reading.carried = item.carried;
                    reading.route_delivery = item.route_delivery;
                    reading.completion = item.completion;
                    reading.phase = Some(item.phase);
                    reading.starts = progress.starts;
                    reading.credit_against = item.credit_against;
                    reading.undelivered = runner.frontend().terminal.undelivered.len();
                    break;
                }
                // ONLY AN EXHAUSTED ALLOWANCE IS WAITED OUT, for the delay it
                // reports. Every other refusal is answered by stopping and
                // reporting it, never by going round again.
                match progress.allowance {
                    None => {}
                    // EVERY REFUSAL THAT NAMES A DELAY IS WAITED OUT for the
                    // delay it names. These are the budget saying "not now",
                    // and which of the four it is does not change the answer.
                    Some(
                        sophia_input_authority::ServiceStartRefusal::StartsExhausted {
                            retry_after,
                        }
                        | sophia_input_authority::ServiceStartRefusal::TimeExhausted { retry_after }
                        | sophia_input_authority::ServiceStartRefusal::CleanupStartsReserved {
                            retry_after,
                        }
                        | sophia_input_authority::ServiceStartRefusal::CleanupTimeReserved {
                            retry_after,
                        },
                    ) => std::thread::sleep(retry_after.min(Duration::from_millis(50))),
                    // AND THE TWO THAT NAME NONE STOP THIS. Waiting cannot fix
                    // either, so the loop ends and the case fails on it.
                    Some(
                        other @ (sophia_input_authority::ServiceStartRefusal::ClockRegressed
                        | sophia_input_authority::ServiceStartRefusal::Interrupted),
                    ) => {
                        reading.unexpected_allowance = Some(format!("{other:?}"));
                        break;
                    }
                }
            }
            if reading.undelivered == 0 {
                reading.undelivered = runner.frontend().terminal.undelivered.len();
            }
            reading.turns = turns;
            report
                .send((reading, original))
                .expect("the case is waiting");
        }),
    );
    let first_hold = held.entered();
    producers[0]
        .submit(&service.owner.lease(), motion_to(surface, wanted))
        .expect("the order accepts a request this recipient's selection refuses");
    let cell = waited_for_value(|| delivery_cell(&service.registry, delivery))
        .expect("the request's own completion, minted by its own admission");
    *witness.lock().unwrap() = Some(Arc::clone(&cell));
    watch_completion(&cell);
    let removed: Arc<Mutex<Option<TrackedInputDelivery>>> = Arc::default();
    let fault_slot = Arc::clone(&removed);
    let fault_recovery = service.registry.input_recovery.clone();
    arm_adjudication_fault(
        &cell,
        wanted,
        XAuthorityInputDeliveryOutcome::RouteRejected,
        Box::new(move || {
            // Removes the real admission and keeps it, and returns. No wait
            // and no assertion inside a charged visit: an empty slot is what
            // the case reads if this could not do its one job.
            if let Ok(mut state) = fault_recovery.state.lock() {
                let taken = state.tickets.remove(&wanted);
                drop(state);
                *fault_slot.lock().unwrap() = taken;
            }
        }),
    );
    held.release();

    let (reading, handed_in) = reported
        .recv_timeout(Duration::from_secs(14))
        .expect("the actual runner reported what it did with the decided request");
    assert!(
        Arc::ptr_eq(&handed_in, &cell),
        "the runner was given this request's own completion and no other"
    );
    assert_eq!(
        reading.undelivered, 1,
        "the decided request is retained as undelivered: {:?}",
        reading.turns
    );
    // TYPED, AND THIS REQUEST'S. The retained item carries the very completion
    // this request's admission minted, answers for this delivery, and reports
    // the refusal common decided rather than a formatted resemblance of one.
    assert!(
        reading
            .carried
            .as_ref()
            .is_some_and(|carried| Arc::ptr_eq(carried, &cell)),
        "the retained item carries the completion this request's own admission minted"
    );
    assert_eq!(
        reading.route_delivery,
        Some(wanted),
        "and its route names this delivery"
    );
    let observed_outcome = reading
        .completion
        .expect("the retained item reported the outcome common gave it");
    assert!(
        matches!(
            observed_outcome,
            sophia_input_authority::RequestCompletion::Refused(_)
        ),
        "which is the refusal the source decided: {observed_outcome:?}"
    );
    assert_eq!(
        reading.phase,
        Some(PrivateRequestPhase::Settled),
        "on a request that returned rather than one still inside the source: {:?}",
        reading.turns
    );
    assert_eq!(
        reading.unexpected_allowance, None,
        "no turn refused for something waiting cannot fix: {:?}",
        reading.turns
    );
    assert!(
        reading.starts > 0,
        "the turn that reached the adjudication was charged: {:?}",
        reading.turns
    );
    assert!(
        reading.turns_taken > 0 && reading.supervised,
        "and every turn taken here kept its supervisor, so the refusal is a decision and not a lost guard: {:?}",
        reading.turns
    );
    // THE CREDIT IS STILL ON THE ITEM, AND IT IS THIS STORE'S. A reserved
    // total of one says a credit is outstanding somewhere; this says the item
    // is the one holding it.
    assert!(
        reading
            .credit_against
            .as_ref()
            .is_some_and(|credit| credit_is_against(&store, credit)),
        "the retained item still holds its accepted-item credit, drawn on this store"
    );
    // THE PUBLICATION WAS ATTEMPTED AND REFUSED, on this exact completion,
    // because its admission was gone. A run in which nothing was attempted
    // would leave this empty.
    let adjudications = adjudications_snapshot();
    assert!(
        adjudications
            .iter()
            .any(|seen| seen.delivery == wanted && seen.answer == PrivateAdjudication::Refused),
        "an adjudication of this exact completion was refused for its missing admission: {adjudications:?}"
    );
    assert_eq!(
        cell.answer(),
        None,
        "nothing was published for it: {adjudications:?}"
    );
    assert_eq!(
        store.reserved(),
        Some(1),
        "and it still holds the one credit it took"
    );
    // THE ADMISSION THAT WENT WAS THIS REQUEST'S, AND IT HAD NOT APPLIED.
    // Kept whole rather than described, so hold two gives back the same entry.
    let taken = removed.lock().unwrap().take().expect(
        "the fault took this request's own admission at the moment its refusal was offered",
    );
    assert!(
        Arc::ptr_eq(&taken.completion, &cell),
        "the entry that went is the one holding this request's own completion"
    );
    assert!(
        !taken.claimed,
        "its claim had already resolved when its refusal was offered"
    );
    assert!(
        !taken.may_have_applied,
        "and the source established no effect for it"
    );

    // HOLD TWO. The admission goes back while the runner is held, so the
    // no-publication check cannot race a legitimate retry.
    let (second_pause, second_held) = Pause::pair();
    let (second_report, second_reported) = sync_channel(1);
    let (before_report, before_reported) = sync_channel(1);
    let before_cell = Arc::clone(&cell);
    arm_runner(
        &service.registry,
        Box::new(move |runner, lease| {
            // READ WHERE IT IS TRUE, BEFORE THE ADMISSION GOES BACK. The
            // runner is at this seam and running nothing, so what this reports
            // is the state the first hold left, taken from the frontend itself
            // rather than inferred from outside it.
            let before = read_refused_item(runner).map(|item| {
                (
                    item.carried
                        .is_some_and(|carried| Arc::ptr_eq(&carried, &before_cell)),
                    item.route_delivery,
                    item.completion,
                    item.phase,
                    item.credit_against,
                )
            });
            before_report
                .send((runner.frontend().terminal.undelivered.len(), before))
                .expect("the case is waiting");
            second_pause.wait();
            let mut turns = BoundedTrace::default();
            let mut starts = 0;
            let mut turns_taken = 0usize;
            let mut supervised = true;
            let mut unexpected: Option<String> = None;
            let deadline = std::time::Instant::now() + Duration::from_secs(8);
            while std::time::Instant::now() < deadline
                && !runner.frontend().terminal.undelivered.is_empty()
            {
                let progress = match runner.service_turn(lease) {
                    Ok(progress) => progress,
                    Err(error) => {
                        turns.push(|| format!("{error:?}"));
                        break;
                    }
                };
                turns.push(|| {
                    format!(
                        "starts={} settled={} terminal_steps={} allowance={:?} watch_failed={}",
                        progress.starts,
                        progress.settled,
                        progress.terminal_steps,
                        progress.allowance,
                        progress.watch_failed
                    )
                });
                starts += progress.starts;
                turns_taken += 1;
                supervised = supervised && !progress.watch_failed;
                match progress.allowance {
                    None => {}
                    Some(
                        sophia_input_authority::ServiceStartRefusal::StartsExhausted {
                            retry_after,
                        }
                        | sophia_input_authority::ServiceStartRefusal::TimeExhausted { retry_after }
                        | sophia_input_authority::ServiceStartRefusal::CleanupStartsReserved {
                            retry_after,
                        }
                        | sophia_input_authority::ServiceStartRefusal::CleanupTimeReserved {
                            retry_after,
                        },
                    ) => std::thread::sleep(retry_after.min(Duration::from_millis(50))),
                    Some(
                        other @ (sophia_input_authority::ServiceStartRefusal::ClockRegressed
                        | sophia_input_authority::ServiceStartRefusal::Interrupted),
                    ) => {
                        unexpected = Some(format!("{other:?}"));
                        break;
                    }
                }
            }
            second_report
                .send((
                    turns,
                    runner.frontend().terminal.undelivered.len(),
                    starts,
                    turns_taken,
                    supervised,
                    unexpected,
                ))
                .expect("the case is waiting");
        }),
    );
    let second_hold = second_held.entered();
    // THE SAME ITEM AND THE SAME CREDIT ARE STILL THERE, checked while nothing
    // can run, so what the restoration is measured against is the state the
    // first hold established and not a state the service moved on from.
    let (retained_before_restore, before_restore) = before_reported
        .recv_timeout(Duration::from_secs(5))
        .expect("the held runner reported what it was holding before the admission went back");
    assert_eq!(
        retained_before_restore, 1,
        "the refused request is still the one thing undelivered when its admission goes back"
    );
    let (carries_original, route_before, completion_before, phase_before, credit_before) =
        before_restore.expect("and it is still the refused item this case is about");
    assert!(
        carries_original,
        "still carrying the completion this request's own admission minted"
    );
    assert_eq!(
        route_before,
        Some(wanted),
        "still naming this delivery: {route_before:?}"
    );
    assert!(
        matches!(
            completion_before,
            Some(sophia_input_authority::RequestCompletion::Refused(_))
        ),
        "with the same typed refusal it reported before: {completion_before:?}"
    );
    assert_eq!(
        phase_before,
        PrivateRequestPhase::Settled,
        "in the same phase: {phase_before:?}"
    );
    assert!(
        credit_before
            .as_ref()
            .is_some_and(|credit| credit_is_against(&store, credit)),
        "the item is still the one holding its accepted-item credit against this store"
    );
    assert_eq!(store.reserved(), Some(1), "still holding its single credit");
    assert_eq!(cell.answer(), None, "and still unanswered");
    // THE SAME ENTRY GOES BACK, AND IT REPLACES NOTHING. A delivery id that
    // had been admitted again in the meantime would be overwritten by this,
    // which is the one way giving an admission back could do harm.
    let displaced = service
        .registry
        .input_recovery
        .state
        .lock()
        .expect("a readable ledger")
        .tickets
        .insert(wanted, taken);
    assert!(
        displaced.is_none(),
        "restoring this request's admission replaced no other admission for the same delivery"
    );
    let published_by_restoring = cell.answer();
    assert_eq!(
        published_by_restoring, None,
        "giving the admission back publishes nothing by itself"
    );
    assert_eq!(
        store.reserved(),
        Some(1),
        "and releases nothing by itself either"
    );
    second_held.release();

    let (
        second_turns,
        undelivered_after,
        second_starts,
        second_turns_taken,
        second_supervised,
        second_unexpected,
    ) = second_reported
        .recv_timeout(Duration::from_secs(14))
        .expect("the actual runner reported its retry");
    assert_eq!(
        second_unexpected, None,
        "no retry turn refused for something waiting cannot fix: {second_turns:?}"
    );
    assert!(
        waited_for(|| cell.answer().is_some()),
        "the charged retry published through the completion this request was admitted with: {second_turns:?}"
    );
    assert!(
        second_starts > 0 && second_turns_taken > 0 && second_supervised,
        "on charged turns, every one of which kept its supervisor: {second_turns:?}"
    );
    let published = cell.answer().expect("its answer");
    assert_eq!(
        published,
        XAuthorityClientInputDelivery {
            client,
            delivery: wanted,
            outcome: XAuthorityInputDeliveryOutcome::RouteRejected,
        },
        "and what it published is the refusal, to that exact delivery"
    );
    assert!(
        waited_for(|| store.reserved() == Some(0)),
        "and exactly the one credit it was holding came back"
    );
    assert_eq!(
        undelivered_after, 0,
        "with nothing left undelivered: {second_turns:?}"
    );
    let answered_once = adjudications_snapshot()
        .iter()
        .filter(|seen| seen.delivery == wanted && seen.answer == PrivateAdjudication::Answered)
        .count();
    assert_eq!(
        answered_once,
        1,
        "answered once, not twice: {:?}",
        adjudications_snapshot()
    );
    // EXACTLY ONE RECEIPT, AND NOTHING BEHIND IT. The refusal reaches the
    // client once; any further receipt is unaccounted for and is not filtered
    // away.
    let receipt = service
        .deliveries
        .recv_timeout(Duration::from_secs(5))
        .expect("the refusal reached this client");
    assert_eq!(
        receipt,
        XAuthorityClientInputDelivery {
            client,
            delivery: wanted,
            outcome: XAuthorityInputDeliveryOutcome::RouteRejected,
        },
        "as the one receipt for this delivery"
    );
    let trailing = service
        .deliveries
        .recv_timeout(Duration::from_millis(300))
        .ok();
    assert!(
        trailing.is_none(),
        "and nothing was published behind it: {trailing:?}"
    );

    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed = service.closed();
    let adjudications = adjudications_snapshot();
    stop_watching_completion();
    assert!(
        !observation_overflowed(),
        "no recorder dropped a record while this was measured"
    );
    let seen = json!({
        "delivery": delivery,
        "first_hold_on": format!("{first_hold:?}"),
        "when_the_admission_was_taken": "at the entry to the adjudication of this exact completion offering RouteRejected, one shot; the source refusal had already been decided by then",
        "removed_entry_holds_this_completion": true,
        "removed_entry_claimed": false,
        "removed_entry_may_have_applied": false,
        "restoring_replaced_another_admission": false,
        "turns_while_the_admission_was_gone": reading.turns.evidence(),
        "retained_undelivered": reading.undelivered,
        "retained_item_carries_this_requests_completion": true,
        "retained_item_route_delivery": reading.route_delivery.map(|id| id.raw()),
        "its_own_common_outcome": format!("{observed_outcome:?}"),
        "its_own_phase": format!("{:?}", reading.phase),
        "starts_on_the_adjudicating_turn": reading.starts,
        "turns_taken_while_the_admission_was_gone": reading.turns_taken,
        "every_turn_kept_its_supervisor": reading.supervised,
        "item_still_holds_its_credit_against_this_store": true,
        "adjudications_of_this_completion": adjudications
            .iter()
            .map(|seen| format!("{:?}", seen.answer))
            .collect::<Vec<_>>(),
        "published_while_admission_gone": Option::<String>::None,
        "credit_while_admission_gone": 1,
        "second_hold_on": format!("{second_hold:?}"),
        "retained_undelivered_before_restore": retained_before_restore,
        "same_item_before_restore": json!({
            "carries_the_original_completion": carries_original,
            "route_delivery": route_before.map(|id| id.raw()),
            "common_outcome": format!("{completion_before:?}"),
            "phase": format!("{phase_before:?}"),
            "still_holds_its_credit_against_this_store": true,
        }),
        "published_by_restoring_admission": published_by_restoring.map(|answer| format!("{answer:?}")),
        "turns_after_restoring": second_turns.evidence(),
        "starts_after_restoring": second_starts,
        "published_by_the_charged_retry": format!("{published:?}"),
        "receipt_for_this_delivery": format!("{receipt:?}"),
        "receipts_behind_it": Option::<String>::None,
        "credit_after_retry": store.reserved(),
        "closed_error": closed.error.clone(),
        "what_this_establishes": "a decided request whose completion cannot be reached keeps its item, the completion its own admission minted, its typed Refused outcome and its single credit; the publication is attempted and refused on that exact cell because its admission is gone; restoring the admission publishes nothing by itself; and the service's own charged, supervised retry then publishes the refusal to that exact delivery once, delivers one receipt with nothing behind it, and returns exactly the one credit.",
    });
    let collected = finish_labelled("refused-publication invocation", service, &[custody]);
    (seen, collected)
}

// Actual private-service input helpers. Faults pause existing source owners;
// observations never replace a source completion, history, event or receipt.

static FOCUS_PAUSES: Mutex<Vec<(usize, Pause)>> = Mutex::new(Vec::new());

pub(crate) fn before_focus_apply(registry: &XServerFrontendRouteRegistry) {
    let key = Arc::as_ptr(&registry.clients) as usize;
    let pause = {
        let mut pauses = FOCUS_PAUSES.lock().unwrap();
        pauses
            .iter()
            .position(|(candidate, _)| *candidate == key)
            .map(|index| pauses.remove(index).1)
    };
    if let Some(pause) = pause {
        pause.wait();
    }
}

struct BConnection {
    peer: UnixStream,
    custody: Arc<PrivateEvidenceCustody>,
    surface: SurfaceId,
    window: u32,
    sequence: u16,
}

impl BConnection {
    fn open(service: &LifecycleService, suffix: u32) -> Self {
        let mut peer = connect_private_client(&service.path);
        let window = handshake_ids(&mut peer) | suffix;
        let (surface, sequence) = selecting_window(
            &mut peer,
            &service.transactions,
            window,
            15 | (1 << 6) | (1 << 21),
        );
        let custody = waited_for_value(|| {
            kept_custodies(&service.registry)
                .into_iter()
                .find(|custody| {
                    custody.attachment() == Some(PrivateAttachment::Started)
                        && registry_window_of(&service.registry, custody.cleanup_record().client)
                            == Some(window)
                })
        })
        .expect("exact newly serving connection");
        Self {
            peer,
            custody,
            surface,
            window,
            sequence,
        }
    }

    fn client(&self) -> XServerFrontendClientId {
        self.custody.cleanup_record().client
    }

    fn ingress(&self, service: &LifecycleService, device: u64) -> PrivateIngress {
        service
            .access
            .ingress_for(
                &service.owner.lease(),
                self.client(),
                DeviceId::from_raw(device),
            )
            .unwrap()
    }

    fn focus(&mut self, service: &LifecycleService, transaction: u64) {
        service
            .access
            .control_producer(&service.owner.lease())
            .unwrap()
            .submit(
                &service.owner.lease(),
                XAuthorityClientControlCommand {
                    client: self.client(),
                    command: XAuthorityControlCommand::FocusSurface {
                        transaction: TransactionId::from_raw(transaction),
                        surface: self.surface,
                    },
                },
            )
            .unwrap();
        assert_eq!(
            ack_for(&service.acks, transaction)
                .unwrap()
                .acknowledgement
                .outcome,
            XAuthorityControlOutcome::Delivered
        );
        assert_eq!(
            read_event(&mut self.peer, 3),
            Some(expected_focus_in(self.sequence, self.window))
        );
    }

    fn pointer_pair(&mut self, service: &LifecycleService, ingress: &PrivateIngress, id: u64) {
        for (offset, pressed) in [(0, true), (1, false)] {
            ingress
                .submit(
                    &service.owner.lease(),
                    button_to(
                        self.surface,
                        XAuthorityInputDeliveryId::from_raw(id + offset),
                        272,
                        pressed,
                    ),
                )
                .unwrap();
            assert_eq!(
                read_event(&mut self.peer, 3),
                Some(expected_button_event(
                    pressed,
                    self.sequence,
                    self.window,
                    1
                ))
            );
            b_flushed(service, id + offset, self.client());
        }
    }

    fn select_xkb(&mut self) {
        use std::io::Write;
        let mut request = [0u8; 16];
        request[0] = crate::X_KEYBOARD_MAJOR_OPCODE;
        request[1] = crate::X_KEYBOARD_SELECT_EVENTS_MINOR_OPCODE;
        request[2..4].copy_from_slice(&4u16.to_le_bytes());
        request[4..6].copy_from_slice(&3u16.to_le_bytes());
        request[10..12].copy_from_slice(&4u16.to_le_bytes());
        self.peer.write_all(&request).unwrap();
        self.peer.write_all(&[43, 0, 1, 0]).unwrap();
        let barrier = read_event(&mut self.peer, 3).unwrap();
        self.sequence += 2;
        assert_eq!(
            (barrier[0], u16::from_le_bytes([barrier[2], barrier[3]])),
            (1, self.sequence)
        );
    }
}

fn b_flushed(service: &LifecycleService, id: u64, client: XServerFrontendClientId) -> Value {
    let cell = delivery_cell(&service.registry, id).expect("original producer completion");
    assert!(waited_for(|| cell.answer().is_some()));
    let answer = cell.answer().unwrap();
    assert_eq!(
        (answer.client, answer.delivery, answer.outcome),
        (
            client,
            XAuthorityInputDeliveryId::from_raw(id),
            XAuthorityInputDeliveryOutcome::Flushed
        )
    );
    json!({"cell":Arc::as_ptr(&cell) as usize,"delivery":id,"client":client.raw(),"outcome":"Flushed"})
}

fn b_empty_debt(service: &LifecycleService) {
    assert!(waited_for(|| service
        .controller
        .under_common(|authority| authority.next_debt(&mut 0).is_none())
        .unwrap_or(false)));
}

fn b_state_notify(sequence: u16, key: u8, pressed: bool, modifiers: u8) -> [u8; 32] {
    // Independent XKB StateNotify layout for the deterministic Shift edge:
    // effective/depressed and five derived masks change, groups stay zero.
    let mut bytes = [0u8; 32];
    bytes[0] = crate::X_KEYBOARD_FIRST_EVENT;
    bytes[1] = 2;
    bytes[2..4].copy_from_slice(&sequence.to_le_bytes());
    bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
    bytes[8] = 3;
    bytes[9] = modifiers;
    bytes[10] = modifiers;
    bytes[19..24].fill(modifiers);
    bytes[26..28].copy_from_slice(&0x1f03u16.to_le_bytes());
    bytes[28] = key;
    bytes[29] = if pressed { 2 } else { 3 };
    bytes
}

fn b_history(service: &LifecycleService) -> (usize, u16) {
    let (send, observed) = sync_channel(1);
    arm_runner(
        &service.registry,
        Box::new(move |runner, _| {
            let state = &runner.keyboards.seats[&runner.seat];
            send.send((
                state as *const crate::XkbKeyboardState as usize,
                state.modifier_mask(),
            ))
            .unwrap();
        }),
    );
    observed.recv_timeout(Duration::from_secs(5)).unwrap()
}

fn b_observed_input(
    service: &LifecycleService,
    ingress: &PrivateIngress,
    route: XAuthorityRoutedInput,
) -> Value {
    let (pause, release) = Pause::pair();
    let (send, observed) = sync_channel(1);
    arm_runner(
        &service.registry,
        Box::new(move |runner, lease| {
            pause.wait();
            let step = runner.execute_accounted_step().unwrap();
            assert!(matches!(
                step,
                PrivateAccountedStep::Step {
                    step: PrivateOrderedStep::Decided(_),
                    ..
                }
            ));
            let Some(PrivateOrderedItem::Ran {
                sequence,
                run,
                custody,
                ..
            }) = runner.frontend().terminal.turn.last()
            else {
                panic!("actual ordered input completed")
            };
            let evidence = json!({"sequence":format!("{sequence:?}"),"token":format!("{:?}",custody.token()),"completion":format!("{:?}",run.completion),"first_press":run.first_press,"keyboard_applied":run.keyboard_applied,"owes_event":run.owes_event,"release":format!("{:?}",run.release),"modifiers":runner.keyboards.modifiers(runner.seat)});
            // Let the real charged terminal visits observe/dispose this original
            // common cell before the next request from the same grant is offered.
            for _ in 0..8 {
                runner.service_turn(lease).unwrap();
            }
            send.send(evidence).unwrap();
        }),
    );
    release.entered();
    ingress.submit(&service.owner.lease(), route).unwrap();
    release.release();
    observed.recv_timeout(Duration::from_secs(5)).unwrap()
}

fn b_history_refusals(
    service: &LifecycleService,
    ingress: &PrivateIngress,
    surface: SurfaceId,
    id: u64,
) -> Value {
    let (pause, release) = Pause::pair();
    let (send, observed) = sync_channel(1);
    arm_runner(
        &service.registry,
        Box::new(move |runner, _| {
            assert!(matches!(
                runner.frontend().keyboards(),
                Err(PrivateKeyboardsRefusal::AlreadyIssued)
            ));
            let original =
                &runner.keyboards.seats[&runner.seat] as *const crate::XkbKeyboardState as usize;
            let modifiers = runner.keyboards.modifiers(runner.seat);
            let keeper = service_owner(&PrivateSettlementOwner::default(), 16);
            let foreign = private_for_roles(&keeper);
            let mut other = foreign.keyboards().unwrap();
            assert!(other.prepare(runner.seat));
            assert!(!other.answers_for(runner.frontend().controller.identity().unwrap()));
            pause.wait();
            std::mem::swap(&mut runner.keyboards, &mut other);
            let step = runner.execute_accounted_step();
            std::mem::swap(&mut runner.keyboards, &mut other);
            assert!(matches!(
                step.unwrap(),
                PrivateAccountedStep::Step {
                    step: PrivateOrderedStep::Decided(_),
                    ..
                }
            ));
            let Some(PrivateOrderedItem::Refused {
                refusal: PrivateExecutionRefusal::ForeignKeyboards,
                custody,
                sequence,
                ..
            }) = runner.frontend().terminal.turn.last()
            else {
                panic!("actual accepted input must refuse foreign history")
            };
            assert_eq!(custody.phase.get(), PrivateRequestPhase::Unused);
            assert!(custody.observe().unwrap().is_none());
            assert_eq!(runner.keyboards.modifiers(runner.seat), modifiers);
            assert_eq!(
                &runner.keyboards.seats[&runner.seat] as *const crate::XkbKeyboardState as usize,
                original
            );
            send.send(json!({"refusal":"ForeignKeyboards","replacement":"AlreadyIssued","sequence":format!("{sequence:?}"),"token":format!("{:?}",custody.token()),"original_history":original,"modifiers":modifiers})).unwrap();
        }),
    );
    release.entered();
    ingress
        .submit(
            &service.owner.lease(),
            key_service_route(surface, id, 30, true),
        )
        .unwrap();
    let original = delivery_cell(&service.registry, id).unwrap();
    release.release();
    let evidence = observed.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        original.answer().is_none(),
        "foreign history produced no input or writer outcome"
    );
    evidence
}

fn b_finish(service: LifecycleService, custodies: &[Arc<PrivateEvidenceCustody>]) -> Vec<String> {
    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed = service.closed();
    assert!(closed.succeeded && !closed.unwound, "{closed:?}");
    assert_eq!(closed.modifiers, Some(0));
    service.finish(custodies)
}

// Keyboard history and its debt: disposal recorded through a poisoned list, and
// the history of one instance that cannot drive another.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_deferred_disposal_records_and_pays_through_a_poisoned_debt_list() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let _admitted = admit_role_client(&private, XServerFrontendClientId(521));
    let _other_admitted = admit_role_client(&private, XServerFrontendClientId(522));
    let running = private
        .reservation_role(XServerFrontendClientId(521), DeviceId::from_raw(1))
        .expect("a capability");
    let other = private
        .reservation_role(XServerFrontendClientId(522), DeviceId::from_raw(2))
        .expect("a second capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let request = running
        .reserve(stamp, 1)
        .expect("a reservation to execute")
        .accepted();
    let stranded = other.reserve(stamp, 1).expect("an unpublished reservation");

    // Poisoned before the debt is recorded. Declining to record here is the
    // difference between deferred and dropped, and dropped means a cell
    // nothing can publish, consume or reissue.
    let debts = std::sync::Arc::clone(&private.authority().owed_disposal);
    assert!(
        std::thread::spawn(move || {
            let _guard = debts.lock().unwrap();
            panic!("poisoning the debt list");
        })
        .join()
        .is_err()
    );

    let completion = private.execute_ordered(&request, XServerFrontendClientId(521), move |permit, _bindings| {
            drop(stranded);
            permit.begin_external_effect()
        });
    assert!(completion.is_ok(), "the execution returned");

    // Recorded through the poison, and paid by the next caller holding common.
    assert!(
        other.reserve(stamp, 2).is_ok(),
        "the stranded cell was released despite the poisoned debt list"
    );
}

#[test]
fn a_debt_already_recorded_is_paid_through_a_poisoned_list() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let _admitted = admit_role_client(&private, XServerFrontendClientId(523));
    let _other_admitted = admit_role_client(&private, XServerFrontendClientId(524));
    let running = private
        .reservation_role(XServerFrontendClientId(523), DeviceId::from_raw(1))
        .expect("a capability");
    let other = private
        .reservation_role(XServerFrontendClientId(524), DeviceId::from_raw(2))
        .expect("a second capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let request = running
        .reserve(stamp, 1)
        .expect("a reservation to execute")
        .accepted();
    let stranded = other.reserve(stamp, 1).expect("an unpublished reservation");

    // Recorded first, with the list healthy.
    let completion = private.execute_ordered(&request, XServerFrontendClientId(523), move |permit, _bindings| {
            drop(stranded);
            permit.begin_external_effect()
        });
    assert!(completion.is_ok());

    // Poisoned only now, before anything has paid it. A payment that skips a
    // poisoned list leaves a debt recorded and never settled, which reads as
    // deferred and behaves as dropped.
    let debts = std::sync::Arc::clone(&private.authority().owed_disposal);
    assert!(
        std::thread::spawn(move || {
            let _guard = debts.lock().unwrap();
            panic!("poisoning the debt list");
        })
        .join()
        .is_err()
    );

    assert!(
        other.reserve(stamp, 2).is_ok(),
        "the recorded debt was paid despite the poisoned list"
    );
}

#[test]
fn recording_a_disposal_debt_does_not_allocate() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let _admitted = admit_role_client(&private, XServerFrontendClientId(525));
    let _other_admitted = admit_role_client(&private, XServerFrontendClientId(526));
    let reserved = private
        .authority()
        .owed_disposal
        .lock()
        .expect("a fresh debt list")
        .capacity();
    assert!(
        reserved >= 1,
        "storage for a debt is taken at construction, not when one is owed"
    );

    let running = private
        .reservation_role(XServerFrontendClientId(525), DeviceId::from_raw(1))
        .expect("a capability");
    let other = private
        .reservation_role(XServerFrontendClientId(526), DeviceId::from_raw(2))
        .expect("a second capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let request = running
        .reserve(stamp, 1)
        .expect("a reservation")
        .accepted();
    let stranded = other.reserve(stamp, 1).expect("an unpublished reservation");
    let _ = private.execute_ordered(&request, XServerFrontendClientId(525), move |permit, _bindings| {
            drop(stranded);
            permit.begin_external_effect()
        });

    // A debt is recorded while common is held, on a path where something has
    // already failed. Growing the list there is an allocation at the worst
    // available moment.
    assert_eq!(
        private
            .authority()
            .owed_disposal
            .lock()
            .expect("the debt list")
            .capacity(),
        reserved,
        "recording a debt used storage that was already reserved"
    );
}

#[test]
fn nothing_can_be_issued_for_a_client_the_boundary_never_admitted() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    // Registered with the frontend, deliberately not admitted to the boundary.
    // The old check would have found this client and called it current.
    let (_registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(
            XServerFrontendClientId(531),
            Some(admitted(XServerFrontendClientId(531))),
        )
        .expect("a fresh client to register");

    let refused = private.reservation_role(XServerFrontendClientId(531), DeviceId::from_raw(1));
    assert!(
        matches!(refused, Err(crate::PrivateAdmissionRefusal::NotAdmitted)),
        "a capability issued here would be a grant revocation could never find"
    );

    // Admitted through the producer hook, and now it issues.
    private
        .admission_participant()
        .admit(
            XServerFrontendClientId(531),
            admitted(XServerFrontendClientId(531)),
        )
        .expect("the boundary to admit");
    assert!(
        private
            .reservation_role(XServerFrontendClientId(531), DeviceId::from_raw(1))
            .is_ok(),
        "and once admitted the role issues"
    );
}

#[test]
fn a_revoked_admission_stops_a_later_execution() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let _registration = admit_role_client(&private, XServerFrontendClientId(561));
    let role = private
        .reservation_role(XServerFrontendClientId(561), DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    // Execute-wins: reserved and executed before anything revokes.
    let first = role.reserve(stamp, 1).expect("a reservation").accepted();
    assert!(
        private
            .execute_ordered(&first, XServerFrontendClientId(561), |permit, _bindings| permit
                .begin_external_effect())
            .is_ok(),
        "work that reaches execution before revocation applies"
    );
    assert!(matches!(
        first.observe(),
        Ok(Some(sophia_input_authority::RequestCompletion::Processed))
    ));

    // A second request, reserved while still admitted.
    let second = role.reserve(stamp, 2).expect("a second reservation").accepted();

    // Revoke-wins: once the producer has revoked, a later attempt cannot
    // apply, even though the request was reserved while the client was live.
    let retired = private
        .admission_participant()
        .revoke_admission(
            XServerFrontendClientId(561),
            sophia_protocol::ClientAdmissionId::from_raw(561),
        )
        .expect("the boundary to revoke");
    assert_eq!(
        retired,
        crate::PrivateRevocation {
            closed: 1,
            retired: 1
        },
        "the binding closed and the grant it authorised was retired"
    );

    let refused = private.execute_ordered(&second, XServerFrontendClientId(561), |_permit, _bindings| {
        panic!("a revoked admission must not reach the permit");
    });
    assert!(
        matches!(
            refused,
            Err(crate::PrivateAuthorityRefusal::NoCurrentAdmission)
        ),
        "a revoked client cannot execute, got {refused:?}"
    );

    // Readmitting does not revive it. A replacement admission is a different
    // admission, whatever the generation says.
    lifecycle_drain(&private.terminal.lifecycle);
    private
        .admission_participant()
        .admit(
            XServerFrontendClientId(561),
            sophia_protocol::ClientAdmissionContext::new(
                sophia_protocol::ClientAdmissionId::from_raw(9561),
                sophia_protocol::NamespaceContext::new(
                    NamespaceId::from_raw(561),
                    sophia_protocol::NamespaceProfile::Confined,
                    sophia_protocol::NamespaceCapabilities::NONE,
                )
                .unwrap(),
                sophia_protocol::ClientAuthProvenance::new(
                    sophia_protocol::ClientAuthenticationMethod::PeerCredentials,
                    ROLE_SESSION_GENERATION,
                )
                .unwrap(),
            )
            .unwrap(),
        )
        .expect("a replacement admission");
    let still_refused =
        private.execute_ordered(&second, XServerFrontendClientId(561), |_permit, _bindings| {
            panic!("an old grant must not become current under a new admission");
        });
    assert!(
        matches!(
            still_refused,
            Err(crate::PrivateAuthorityRefusal::NoCurrentAdmission)
        ),
        "a replacement admission does not make an old grant current, got {still_refused:?}"
    );
}

#[test]
fn a_namespace_closes_every_binding_in_it_whatever_it_holds() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let namespace = NamespaceId::from_raw(571);

    // Three shapes in one namespace: one that never issued a grant, one whose
    // grant is already retired, and one still holding a live grant. Closing is
    // what denies further work, so having nothing left to clean up is not a
    // reason to leave a namespace admitted.
    let bare = XServerFrontendClientId(571);
    let spent = XServerFrontendClientId(572);
    let live = XServerFrontendClientId(573);
    for client in [bare, spent, live] {
        private
            .admission_participant()
            .admit(client, namespaced(client, namespace))
            .expect("the boundary to admit");
    }
    let _spent_role = private
        .reservation_role(spent, DeviceId::from_raw(1))
        .expect("a capability");
    let _live_role = private
        .reservation_role(live, DeviceId::from_raw(2))
        .expect("a capability");

    // Retire one admission on its own first, so its binding is gone and the
    // namespace sweep meets a client with nothing left.
    let first = private
        .admission_participant()
        .revoke_admission(spent, sophia_protocol::ClientAdmissionId::from_raw(spent.raw()))
        .expect("the boundary to revoke");
    assert_eq!(
        first,
        crate::PrivateRevocation {
            closed: 1,
            retired: 1
        }
    );

    let swept = private
        .admission_participant()
        .revoke_namespace(namespace)
        .expect("the boundary to revoke the namespace");
    assert_eq!(
        swept,
        crate::PrivateRevocation {
            closed: 2,
            retired: 1
        },
        "both remaining bindings closed; only the live grant had anything to retire"
    );

    // A zero retired count is not evidence that nothing closed.
    for client in [bare, live] {
        assert!(
            matches!(
                private.reservation_role(client, DeviceId::from_raw(9)),
                Err(crate::PrivateAdmissionRefusal::NotAdmitted)
            ),
            "every binding in the namespace is closed"
        );
    }
}

#[test]
fn revoking_a_namespace_with_nothing_to_retire_still_closes_it() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let namespace = NamespaceId::from_raw(581);
    let client = XServerFrontendClientId(581);
    private
        .admission_participant()
        .admit(client, namespaced(client, namespace))
        .expect("the boundary to admit");

    let swept = private
        .admission_participant()
        .revoke_namespace(namespace)
        .expect("the boundary to revoke the namespace");
    assert_eq!(
        swept,
        crate::PrivateRevocation {
            closed: 1,
            retired: 0
        },
        "closed with nothing to retire, which is not the same as nothing closed"
    );
    assert!(matches!(
        private.reservation_role(client, DeviceId::from_raw(1)),
        Err(crate::PrivateAdmissionRefusal::NotAdmitted)
    ));
}

#[test]
fn a_boundary_nobody_can_read_is_not_a_client_nobody_admitted() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let _registration = admit_role_client(&private, XServerFrontendClientId(591));
    let role = private
        .reservation_role(XServerFrontendClientId(591), DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let request = role.reserve(stamp, 1).expect("a reservation").accepted();

    let bindings = std::sync::Arc::clone(&private.admission_participant().bindings);
    assert!(
        std::thread::spawn(move || {
            let _guard = bindings.lock().unwrap();
            panic!("poisoning the boundary");
        })
        .join()
        .is_err()
    );

    let outcome = private.execute_ordered(&request, XServerFrontendClientId(591), |_permit, _bindings| {
        panic!("an unreadable boundary must not reach the permit");
    });
    assert!(
        matches!(outcome, Err(crate::PrivateAuthorityRefusal::Unreachable)),
        "unreadable is not absent: nothing was established about who is admitted, got {outcome:?}"
    );
}

#[test]
fn a_binding_refuses_a_grant_it_could_not_account_for() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let client = XServerFrontendClientId(601);
    private
        .admission_participant()
        .admit(client, admitted(client))
        .expect("the boundary to admit");

    // Issued until the binding is full. The bound is the authority's own
    // supported grant count, not a number chosen here, so this is the point
    // where the authority itself would stop being able to answer for them.
    let mut issued = 0usize;
    loop {
        match private.reservation_role(client, DeviceId::from_raw(1)) {
            Ok(role) => {
                std::mem::forget(role);
                issued += 1;
            }
            Err(refusal) => {
                assert!(
                    matches!(refusal, crate::PrivateAdmissionRefusal::GrantRecordsExhausted),
                    "the binding refuses before issuing, got {refusal:?}"
                );
                break;
            }
        }
        assert!(issued <= 64, "the binding must refuse rather than grow");
    }
    assert_eq!(
        issued,
        sophia_input_authority::Capacity::PLANNED.grants,
        "the bound is the authority's supported grant count"
    );

    // Refused before the grant existed, so the binding still accounts for
    // exactly what it authorised and revocation can retire all of it.
    let revoked = private
        .admission_participant()
        .revoke_admission(client, sophia_protocol::ClientAdmissionId::from_raw(client.raw()))
        .expect("the boundary to revoke");
    assert_eq!(
        revoked.closed, 1,
        "the binding closed"
    );
    assert_eq!(
        revoked.retired, issued,
        "every grant it recorded was retired, and it recorded every grant it authorised"
    );
}

#[test]
fn cleanup_left_unresolved_is_resumed_rather_than_revisited_by_revocation() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let namespace = NamespaceId::from_raw(611);
    let client = XServerFrontendClientId(611);
    private
        .admission_participant()
        .admit(client, namespaced(client, namespace))
        .expect("the boundary to admit");
    let _role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");

    // Staged as an interrupted revocation leaves it: closed, so further work
    // is already denied, with its grant still recorded because retirement did
    // not finish.
    {
        let mut bindings = private
            .admission_participant()
            .bindings
            .lock()
            .expect("the boundary");
        let bound = bindings.bound.get_mut(&client).expect("the binding");
        bound.closed = true;
        assert_eq!(bound.grants.len(), 1, "its grant is still owed retirement");
    }

    // Revoking the namespace again does not finish it. Closing is not what
    // this binding is waiting for, and it is already closed.
    let swept = private
        .admission_participant()
        .revoke_namespace(namespace)
        .expect("the boundary to revoke the namespace");
    assert_eq!(
        swept,
        crate::PrivateRevocation {
            closed: 0,
            retired: 0
        },
        "a second revocation passes over work that is already closed"
    );
    assert_eq!(
        private
            .admission_participant()
            .bindings
            .lock()
            .expect("the boundary")
            .bound
            .get(&client)
            .map_or(0, |bound| bound.grants.len()),
        1,
        "so the outstanding retirement is still outstanding"
    );

    // The origin's continuation is what finishes it.
    let resumed = private
        .admission_participant()
        .resume_unresolved()
        .expect("the boundary");
    assert_eq!(resumed.retired, 1, "the owed retirement was completed");
    assert!(
        !private
            .admission_participant()
            .bindings
            .lock()
            .expect("the boundary")
            .bound
            .contains_key(&client),
        "and with nothing left owed against it the record goes"
    );

    // Nothing left to resume, and it says so rather than looping.
    assert_eq!(
        private
            .admission_participant()
            .resume_unresolved()
            .expect("the boundary"),
        crate::PrivateRevocation::default()
    );
}

#[test]
fn keyboard_state_is_applied_on_this_thread_with_both_modifier_facts() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let mut keyboards = private.keyboards().expect("a keymap that compiles");
    let seat = SeatId::from_raw(1);

    // Applying before preparing does not build anything. Building compiles a
    // keymap, which is neither free nor infallible, and doing it inside a
    // transaction would put that failure where refusing is no longer free.
    assert!(
        keyboards.apply(seat, 42, true).is_none(),
        "an unprepared seat applies nothing"
    );
    assert!(keyboards.prepare(seat));
    assert!(keyboards.prepare(seat), "preparing twice is not an error");

    // Left shift down, then a key while it is held. The event carries the
    // modifiers from *before* it, while what follows has to see the state it
    // produced -- two different facts, which is why both are returned.
    let (_shift_code, before_shift, after_shift) =
        keyboards.apply(seat, 42, true).expect("left shift to map");
    assert_eq!(before_shift, 0, "nothing was held before the first key");
    assert_ne!(
        after_shift, 0,
        "and the state the key produced is not the state it was reported with"
    );
    assert_eq!(
        keyboards.modifiers(seat),
        Some(after_shift),
        "the seat keeps what the key left"
    );

    let (_code, before_key, _after_key) =
        keyboards.apply(seat, 30, true).expect("a letter to map");
    assert_eq!(
        before_key, after_shift,
        "the next event reports the modifiers that were held when it happened"
    );

    // Released, and the seat follows. No worker, no channel, no deadline: this
    // is state this thread owns, so nothing here waits.
    keyboards.apply(seat, 42, false).expect("left shift release");
    assert_eq!(
        keyboards.modifiers(seat),
        Some(0),
        "releasing the modifier clears it"
    );

    // A second seat is independent, and is built the same way as the first.
    let other = SeatId::from_raw(2);
    assert!(keyboards.prepare(other));
    assert_eq!(keyboards.modifiers(other), Some(0));
    assert_eq!(
        keyboards.modifiers(seat),
        Some(0),
        "seats do not share state"
    );
}

#[test]
fn keyboard_state_answers_for_one_instance_only() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let first = private_for_roles(&service_keeper);
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let second = private_for_roles(&service_keeper);
    let mut keyboards = first.keyboards().expect("a keymap that compiles");

    let first_identity = first.authority().identity().expect("an identity");
    let second_identity = second.authority().identity().expect("an identity");
    assert_ne!(
        first_identity, second_identity,
        "two instances are two identities"
    );

    // Being owned by this thread says nothing about whose state it is. Without
    // the binding, one instance's turn could be driven with the other's
    // keyboard history and every modifier would be read from the wrong past.
    assert!(keyboards.answers_for(first_identity));
    assert!(
        !keyboards.answers_for(second_identity),
        "another instance's state cannot be substituted for this one's"
    );

    // And the state it holds is this instance's, not a fresh one: a seat
    // already prepared keeps what it is holding rather than starting again.
    let seat = SeatId::from_raw(1);
    assert!(keyboards.prepare(seat));
    keyboards.apply(seat, 42, true).expect("left shift to map");
    let held = keyboards.modifiers(seat).expect("the seat");
    assert_ne!(held, 0, "a modifier is held");
    assert!(keyboards.prepare(seat), "preparing again is not rebuilding");
    assert_eq!(
        keyboards.modifiers(seat),
        Some(held),
        "so a key held across it is still held by the same state"
    );
}

#[test]
fn an_instance_hands_out_its_keyboard_history_once() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let mut keyboards = private.keyboards().expect("the instance's state");
    let seat = SeatId::from_raw(1);
    assert!(keyboards.prepare(seat));
    keyboards.apply(seat, 42, true).expect("left shift to map");
    let held = keyboards.modifiers(seat).expect("the seat");
    assert_ne!(held, 0, "a modifier is held in the state that exists");

    // A second object would carry this instance's identity and pass every
    // check that identity answers, while holding none of what the first is
    // holding. The shift down above would be a key nobody released as far as
    // it could tell, so there is no second one to be given.
    let second = private.keyboards();
    assert!(
        matches!(second, Err(crate::PrivateKeyboardsRefusal::AlreadyIssued)),
        "one history per instance, got {second:?}"
    );

    // The one that exists still holds it.
    assert_eq!(keyboards.modifiers(seat), Some(held));
}

#[test]
fn an_unreadable_authority_is_not_reported_as_a_broken_keymap() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let poisoner = private.authority().clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.common.lock().unwrap();
            panic!("poisoning the authority");
        })
        .join()
        .is_err()
    );

    let refused = private.keyboards();
    assert!(
        matches!(
            refused,
            Err(crate::PrivateKeyboardsRefusal::AuthorityUnreadable)
        ),
        "an authority nobody can read is not a keymap that will not compile, got {refused:?}"
    );
}

#[test]
fn losing_the_handle_for_executed_work_does_not_erase_its_outcome() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let _admitted = admit_role_client(&private, XServerFrontendClientId(541));
    let role = private
        .reservation_role(XServerFrontendClientId(541), DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let request = role.reserve(stamp, 1).expect("a reservation").accepted();
    let token = request.token();

    assert!(
        private
            .execute_ordered(&request, XServerFrontendClientId(541), |permit, _bindings| permit
                .begin_external_effect())
            .is_ok()
    );

    // The handle goes without the outcome being taken. Reclaiming the cell
    // here would erase what happened: a later observation would report a stale
    // request rather than the outcome that really occurred, and capacity would
    // have been bought by destroying evidence.
    drop(request);

    let surviving = private
        .authority()
        .under_common_as_origin(|authority, _issuer| {
            // Reached through the private field rather than a production
            // accessor: taking an outcome without holding its custody is
            // exactly what production must not offer, so it does not get a
            // method for the sake of a test.
            authority.take_completion(&private.submit, token, role_connection(541))
        })
        .expect("a readable authority");
    assert!(
        matches!(
            surviving,
            Ok(Some(sophia_input_authority::RequestCompletion::Processed))
        ),
        "the terminal outcome survived the handle, got {surviving:?}"
    );
}

#[test]
fn an_interrupted_execution_is_not_mistaken_for_one_that_never_ran() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let _admitted = admit_role_client(&private, XServerFrontendClientId(551));
    let role = private
        .reservation_role(XServerFrontendClientId(551), DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let request = role.reserve(stamp, 1).expect("a reservation").accepted();
    let token = request.token();

    // Marks an effect, then does not return. Whether that effect happened is
    // exactly what the interruption destroyed.
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        private.execute_ordered(&request, XServerFrontendClientId(551), |permit, _bindings| {
            permit.begin_external_effect()?;
            panic!("interrupting execution after its effect was marked");
        })
    }));
    assert!(interrupted.is_err(), "the execution unwound");

    // The handle goes without an outcome ever being recorded. Treating that as
    // a request that never ran would discard the cell, and discarding it turns
    // "nobody can say whether this ran" into "this never happened".
    drop(request);

    // Read through the poison the interruption caused, because that poison and
    // the interruption are one event.
    let mut authority = private
        .authority()
        .common
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let surviving = authority.take_completion(&private.submit, token, role_connection(551));
    assert!(
        matches!(surviving, Ok(None)),
        "the record survives with no outcome -- unknown, not absent -- got {surviving:?}"
    );
}

#[test]
fn a_sweep_leaves_its_inventory_the_buffer_it_reserved() {
    let client = XServerFrontendClientId(377);
    let surface = SurfaceId::new(377, 1);
    // Full, so shutdown carries the obligation here instead of answering it.
    let (acknowledgements, acks) = sync_channel(1);
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    let durable = crate::PrivateSettlementOwner::with_capacity(8);
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 74001))
        .expect("the shared admission to accept control");
    drop(private.shutdown());
    assert_eq!(durable.owed(), Some(1));

    // Freed, so the sweep can finish and the lists are the ones a completed
    // sweep left behind rather than ones it never emptied.
    assert!(acks.try_recv().is_ok());
    let progress = durable.drive();
    assert_eq!(progress.answered, 1, "swept and answered");
    assert_eq!(
        acks.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(74001)
    );

    // The buffers were taken at construction so that settling work a failing
    // instance handed over never has to allocate. A sweep that moves its
    // inventory out through a local swaps in a fresh vector and drops the
    // buffer with it, so the next sweep allocates during exactly the teardown
    // the reservation was for. Moving between two owned lists keeps it.
    let held = durable.records_even_if_poisoned();
    assert!(
        held.held.capacity() >= 8,
        "the list swept from kept its reserved buffer, had {}",
        held.held.capacity()
    );
    assert!(
        held.outstanding.capacity() >= 8,
        "and so did the list of routed work, had {}",
        held.outstanding.capacity()
    );
    assert!(held.in_flight.is_empty(), "a finished sweep carries nothing");
    assert!(
        held.in_flight.capacity() >= 8,
        "and the list it carries in keeps its own buffer too, had {}",
        held.in_flight.capacity()
    );
}

/// The completion cell an admission minted, taken at the admission boundary.
///
/// RETAINED THERE, NOT LOOKED UP LATER. A delivery id is a reusable number and
/// its ticket is pruned once answered, so asking the recovery for the cell at
/// assertion time can hand back a different admission's cell, or none -- and
/// none is not evidence that nothing was handed over. Holding the cell from
/// the start is what fixes which admission a control is talking about.
fn admitted_cell(
    private: &crate::PrivateXServerFrontend,
    delivery: u64,
) -> Arc<PrivateDeliveryCompletion> {
    private
        .broker
        .registry
        .input_recovery
        .completion_for(XAuthorityInputDeliveryId::from_raw(delivery))
        .expect("a readable recovery")
        .expect("this admission minted a cell")
}

/// Capsules an instrument took off a recipient's ordered queue.
///
/// FIXTURE-OWNED, because reading a queue consumes it. Anything taken while
/// looking for one admission's capsule is kept here rather than dropped: a
/// control asking about one event must not destroy another event's evidence,
/// and the capsules it did not ask for are still owed to whoever does.
#[derive(Default)]
struct OrderedInbox {
    taken: Vec<XAuthorityOrderedDelivery>,
}

impl OrderedInbox {
    fn collect(&mut self, queue: &Receiver<XAuthorityOrderedDelivery>) {
        self.taken.extend(queue.try_iter());
    }

    fn take_carrying(
        &mut self,
        expected: &Arc<PrivateDeliveryCompletion>,
    ) -> Option<XAuthorityOrderedDelivery> {
        let found = self.taken.iter().position(|capsule| {
            capsule
                .finalizer()
                .is_some_and(|finalizer| Arc::ptr_eq(&finalizer.completion, expected))
        })?;
        Some(self.taken.remove(found))
    }

    /// The capsule this exact admission's recipient accepted, if it has one.
    ///
    /// WHAT IS ALREADY QUEUED COUNTS, and is looked at before any visit is
    /// driven: a handover from an earlier call is still a handover, and asking
    /// only for new ones reported nothing for an event the recipient had.
    ///
    /// Identity is the retained cell the capsule carries -- the one thing that
    /// names this admission and nothing else. A driver failure is propagated
    /// rather than answered as "no handover", because it establishes neither.
    fn accepted(
        &mut self,
        private: &mut crate::PrivateXServerFrontend,
        queue: &Receiver<XAuthorityOrderedDelivery>,
        expected: &Arc<PrivateDeliveryCompletion>,
        steps: usize,
    ) -> Result<Option<XAuthorityOrderedDelivery>, XServerFrontendRouteError> {
        for _ in 0..steps {
            self.collect(queue);
            if let Some(found) = self.take_carrying(expected) {
                return Ok(Some(found));
            }
            if matches!(
                private.deliver_one(None, &mut |_, _| Ok(()))?,
                PrivateDeliveryStep::Idle
            ) {
                break;
            }
        }
        self.collect(queue);
        Ok(self.take_carrying(expected))
    }
}

/// The handover phase of the custody that owns exactly this admission's cell.
///
/// AN INTERNAL PHASE, for controls whose claim is about the phase itself. A
/// claim about what a recipient accepted goes through the queue instead.
fn handover_phase(
    private: &crate::PrivateXServerFrontend,
    cell: &Arc<PrivateDeliveryCompletion>,
) -> Option<PrivateDispatchPhase> {
    let owns = |custody: &PrivateDeliveryCustody| {
        custody
            .completion
            .as_ref()
            .is_some_and(|held| Arc::ptr_eq(held, cell))
    };
    private
        .terminal
        .holds
        .iter()
        .find(|record| owns(&record.custody))
        .map(|record| record.custody.dispatch)
        .or_else(|| {
            private.terminal.settling.iter().find_map(|release| {
                release
                    .press_custody
                    .as_ref()
                    .filter(|custody| owns(custody))
                    .map(|custody| custody.dispatch)
                    .or_else(|| owns(&release.custody).then_some(release.custody.dispatch))
            })
        })
}

/// Admit a delivery the way the ingress would, for controls that drive
/// run_ordered_input directly.
///
/// A real delivery is always admitted before it is executed -- that is where
/// its completion is minted -- so a control that presses one which was never
/// admitted is describing a route that cannot occur. Added rather than
/// loosening the executor, which now refuses a release whose answer it could
/// never recognise.
fn admit_for_direct_run(private: &crate::PrivateXServerFrontend, route: &XAuthorityRoutedInput) {
    private
        .broker
        .registry
        .input_recovery
        .admit(route, 0, std::time::Instant::now());
}

#[test]
fn an_admitted_button_runs_the_ordered_path_and_releases_to_its_recorded_hold() {
    let client = XServerFrontendClientId(701);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    // Press, through the whole ordered path: custody accepted, common then the
    // boundary then the X guards, target resolved there rather than earlier,
    // ledger transition, and an immutable record of where it went.
    let pressed = role.reserve(stamp, 1).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(701), 272, true);
                admit_for_direct_run(private, &route);
                route
            },
            &pressed,

                watch,
            )
        .expect("the press to run");
    let reached = run.reached.expect("a press decides where it went");
    assert_eq!(reached.client(), client, "it reached the route's client");
    assert_eq!(reached.window(), window);
    // An assertion that no grab chose the recipient stood here. It is REMOVED,
    // not relocated: the source resolves the recipient itself now and does not
    // report whether a grab was involved, so this executor cannot establish
    // that fact and no longer records it.
    //
    // The two assertions above do NOT stand in for it. An active grab with
    // owner_events over the surface's own window reaches exactly this client
    // and this window, so they hold whether or not a grab chose the recipient,
    // and reading them as evidence of its absence would be inferring the fact
    // from a pair that cannot distinguish it.
    //
    // Nothing read the removed flag in production. The selected-event
    // authority it was watching is still decided at the source boundary, while
    // the immutable plan is built, and the control over it belongs there --
    // not to a replacement boolean reported back out to this executor.
    assert!(run.first_press, "this press began the hold");
    assert!(!run.keyboard_applied, "a button moves no keyboard state");
    assert!(matches!(
        run.completion,
        sophia_input_authority::RequestCompletion::Processed
    ));

    // Observed exactly once, which is what frees the grant's cell.
    assert!(matches!(
        pressed.observe(),
        Ok(Some(sophia_input_authority::RequestCompletion::Processed))
    ));

    // A second press of the same input joins the hold rather than beginning
    // one. The ledger says so, not the route: nothing about where this event
    // would go has changed, and treating a join as a new press would deliver
    // the same button down twice.
    let joined = role.reserve(stamp, 2).expect("a second reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(703), 272, true);
                admit_for_direct_run(private, &route);
                route
            },
            &joined,

                watch,
            )
        .expect("the joining press to run");
    assert!(
        !run.first_press,
        "a press onto a held input joins rather than begins"
    );
    assert!(
        !run.keyboard_applied,
        "and a join moves no state, which is the rule keys will need"
    );
    assert!(matches!(
        joined.observe(),
        Ok(Some(sophia_input_authority::RequestCompletion::Processed))
    ));

    // Release. The recipient comes from the hold the press recorded, not from
    // resolving the route again -- so it still answers even though nothing
    // about the route is consulted for it.
    let released = role.reserve(stamp, 3).expect("a third reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(702), 272, false);
                admit_for_direct_run(private, &route);
                route
            },
            &released,

                watch,
            )
        .expect("the release to run");
    let released_to = run.reached.expect("a delivering release names its hold");
    assert_eq!(
        released_to.client(),
        client,
        "the release answers to the recipient the press reached"
    );
    assert_eq!(
        released_to.window(),
        window,
        "and to the window that press recorded, not whatever the route says now"
    );
    assert!(matches!(
        run.release,
        Some(sophia_input_authority::ReleaseOutcome::DeliverTo(_))
    ));
    assert!(!run.first_press, "a release begins nothing");
    assert!(matches!(
        released.observe(),
        Ok(Some(sophia_input_authority::RequestCompletion::Processed))
    ));
}

#[test]
fn a_key_press_refuses_rather_than_delivering_on_queued_focus() {
    let client = XServerFrontendClientId(711);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let custody = role.reserve(stamp, 1).expect("a reservation").accepted();

    let mut key = motion_to(surface, XAuthorityInputDeliveryId::from_raw(711));
    key.request.kind = InputEventKind::Key {
        keycode: 30,
        pressed: true,
    };
    private.broker.registry.input_recovery.admit_typed(&key, 1, Instant::now()).unwrap();
    let refused = private.run_ordered_input(keyboards, &key, &custody, watch);
    assert!(
        matches!(refused, Err(crate::PrivateExecutionRefusal::Native(private_native::Refusal::Resolution(PrivateAppliedRefusal::FocusNotApplied)))),
        "the reason is the missing applied focus, not an authority error standing in for it, got {refused:?}"
    );

    // Refused before any keyboard effect: the seat is prepared but nothing
    // moved it, so no modifier describes a key no admitted request applied.
    assert_eq!(
        keyboards.modifiers(SeatId::from_raw(1)),
        Some(0),
        "the refusal came before any keyboard transition"
    );
}

#[test]
fn another_instances_keyboard_history_cannot_drive_this_one() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let other = private_for_roles(&service_keeper);
    let client = XServerFrontendClientId(721);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards: _, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let mut foreign = other.keyboards().expect("the other instance's state");
    let custody = role.reserve(stamp, 1).expect("a reservation").accepted();

    let refused = private.run_ordered_input(
        &mut foreign,
        &button_to(surface, XAuthorityInputDeliveryId::from_raw(721), 272, true),
        &custody,

                watch,
            );
    assert!(
        matches!(
            refused,
            Err(crate::PrivateExecutionRefusal::ForeignKeyboards)
        ),
        "one instance is not driven with another's keyboard history, got {refused:?}"
    );
}

/// A frontend with one admitted, registered client and a surface, ready to run
/// ordered input for it.
fn ordered_fixture(
    client: XServerFrontendClientId,
    surface: SurfaceId,
    window: XResourceId,
) -> (
    crate::PrivateXServerFrontend,
    XServerFrontendClientRouteRegistration,
    crate::PrivateReservationRole,
    crate::PrivateKeyboards,
    // The owner goes back to the caller: a fixture that kept it in its own
    // frame would hand out a service whose keeper died as it returned.
    crate::PrivateServiceOwner,
) {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let registration = admit_role_client(&private, client);
    private
        .broker
        .registry
        .register_surface(client, NamespaceId::from_raw(client.raw()), surface, window)
        .expect("the surface to register");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let keyboards = private.keyboards().expect("this instance's state");
    (private, registration, role, keyboards, service_keeper)
}

// The completion barrier, driven through the real runner rather than in
// isolation: a submitter learns its own request finished, and learns it only
// once the work has actually been processed.
//
// Kept beside the runner's own tests rather than inside them because that file
// is at the layout limit, and a debt row buys nothing a second file does not.

#[test]
fn private_runner_reports_internal_completion_to_a_waiting_submitter() {
    let (mut runner, durable, ..) = prepared_runner_fixture();
    let client = XServerFrontendClientId::from_raw(9000);
    let ingress = runner
        .ingress_for(&durable.lease(), client, DeviceId::from_raw(1))
        .unwrap();

    let notifier = crate::ConnectionNotifier::new().unwrap();
    let barrier = crate::PrivateRequestBarrier::over(&notifier);
    assert!(
        ingress.report_completions_to(barrier.clone()),
        "a producer takes a slot once"
    );
    assert!(
        !ingress.report_completions_to(barrier.clone()),
        "and only once, because a second slot would arm the wrong one"
    );

    ingress
        .submit(
            &durable.lease(),
            button_to(
                SurfaceId::new(9100, 1),
                XAuthorityInputDeliveryId::from_raw(9100),
                272,
                true,
            ),
        )
        .unwrap();

    // Accepted into the shared order and nothing more. This is the event a
    // submitter would wrongly wait on: the work is queued, not processed.
    assert!(barrier.armed(), "the request is outstanding once accepted");
    assert_eq!(
        barrier.take(),
        None,
        "acceptance is not completion, and waiting on it would release a \
         client before its input had happened"
    );

    runner.service_turn(&durable.lease()).unwrap();

    // Now it has been processed, which is what the next request is entitled
    // to wait for. This fixture routes nowhere, so processing it ends in a
    // refusal -- and that is the more interesting case, because a refusal is
    // an outcome and must release the waiter exactly as a success does. A
    // barrier that only reported success would park a client forever on every
    // request the authority declined.
    assert_eq!(
        barrier.take(),
        Some(sophia_input_authority::RequestCompletion::Refused(
            sophia_input_authority::RegistrationError::RoutingUnavailable
        ))
    );
    assert!(!barrier.armed(), "a taken outcome ends the request");
}

#[test]
fn private_runner_leaves_an_uninterested_producer_untouched() {
    let (mut runner, durable, ..) = prepared_runner_fixture();
    let client = XServerFrontendClientId::from_raw(9000);
    let ingress = runner
        .ingress_for(&durable.lease(), client, DeviceId::from_raw(1))
        .unwrap();

    // The ordinary shape: no slot installed, so work is reserved, published
    // and answered to nobody. Every producer except an injection adapter
    // wants exactly this, and it must cost nothing.
    ingress
        .submit(
            &durable.lease(),
            button_to(
                SurfaceId::new(9101, 1),
                XAuthorityInputDeliveryId::from_raw(9101),
                272,
                true,
            ),
        )
        .unwrap();

    let turn = runner.service_turn(&durable.lease()).unwrap();
    assert_eq!(turn.taken, 1, "the work still runs");
}

#[test]
fn private_runner_wakes_a_submitter_parked_on_its_own_request() {
    let (mut runner, durable, ..) = prepared_runner_fixture();
    let client = XServerFrontendClientId::from_raw(9000);
    let ingress = runner
        .ingress_for(&durable.lease(), client, DeviceId::from_raw(1))
        .unwrap();
    let notifier = crate::ConnectionNotifier::new().unwrap();
    let barrier = crate::PrivateRequestBarrier::over(&notifier);
    assert!(ingress.report_completions_to(barrier.clone()));

    ingress
        .submit(
            &durable.lease(),
            button_to(
                SurfaceId::new(9000, 1),
                XAuthorityInputDeliveryId::from_raw(9102),
                272,
                true,
            ),
        )
        .unwrap();
    runner.service_turn(&durable.lease()).unwrap();

    // The runner stored the outcome and then raised the wake, in that order,
    // so a connection roused by this always finds an answer rather than
    // having to park again. Without the wake the submitter would sit in poll
    // until its peer happened to move, because the barrier has no backstop
    // timer by design.
    let (ours, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let mut wait = crate::ConnectionWait::new(std::os::fd::AsFd::as_fd(&ours), &notifier);
    assert_eq!(
        wait.wait_until(Some(
            std::time::Instant::now() + std::time::Duration::from_secs(5)
        ))
        .unwrap(),
        crate::ConnectionWake::Notified,
    );
    assert!(barrier.take().is_some(), "the wake must not arrive empty");
}

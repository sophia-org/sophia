// Included into `x11_socket::tests`, so the private classification flags on
// `X11SetupSocketError` are readable here: those flags are exactly what the
// worker reaper consults, and this pins what they say for a dispatch that
// started and did not complete.

/// A client that departs mid-dispatch is a disconnect, and the reaper contains
/// it. Under t154 a single zero-delay FakeInput followed by the client's exit
/// -- what every xdotool invocation does -- reached the tail with an
/// unclassified error, and every serving loop propagated it: one click on the
/// root ended the frontend. The departure is the client's.
#[cfg(unix)]
#[test]
fn a_departure_mid_dispatch_is_the_clients_disconnect() {
    let error = crate::x11_socket::partial_dispatch_error(true);
    assert!(error.client_disconnect, "a departed peer is a client disconnect");
    assert!(!error.client_failure);
    assert!(!error.service_shutdown);
    assert!(
        error.to_string().contains("departed"),
        "the reason names the departure, not a service fault: {error}"
    );
}

/// The other way to end mid-dispatch is the service's own failure, and that
/// stays fatal: the rule that a partial dispatch is never certified as an
/// empty success is what keeps the conformance gate honest, and separating
/// the client's departure from it must not weaken it.
#[cfg(unix)]
#[test]
fn a_service_failure_mid_dispatch_stays_fatal() {
    let error = crate::x11_socket::partial_dispatch_error(false);
    assert!(!error.client_disconnect, "a service fault must not read as the client leaving");
    assert!(!error.client_failure);
    assert!(!error.service_shutdown);
    assert_eq!(
        error.to_string(),
        "X11 dispatch ended before its effects were published",
        "the pre-existing fatal message is unchanged"
    );
}

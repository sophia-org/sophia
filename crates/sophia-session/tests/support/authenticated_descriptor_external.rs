//! Opt-in protocol conformance for an independently built descriptor client.
//! The caller supplies its executable; no client is built or selected here.
use super::*;

#[test]
#[ignore = "requires SOPHIA_DESCRIPTOR_AUTH_PEER, an absolute prebuilt descriptor client"]
fn supplied_descriptor_client_authenticates_and_recovers() {
    let path = std::env::var("SOPHIA_DESCRIPTOR_AUTH_PEER").unwrap();
    let executable = std::path::Path::new(&path);
    assert!(executable.is_absolute() && executable.is_file());
    let mut auth = factotum::Fixture::new();
    let mut shell = LiveMetadataShell::prepare(
        &path,
        None,
        false,
        false,
        sophia_config::ShellGpuMode::Denied,
        None,
        None,
        std::path::Path::new(sophia_runtime::DEFAULT_BUBBLEWRAP_PATH),
    )
    .unwrap();
    shell
        .authenticate(sophia_protocol::NamespaceId::from_raw(2), &auth.coordinator)
        .unwrap();
    shell.set_presentation_available(true, "fixture").unwrap();
    let wake = sophia_wake::Wake::new().unwrap();
    let mut previous_registration = None;
    for epoch in 1..=2 {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            auth.tick();
            let event = shell
                .poll_authenticated(&mut auth.coordinator, wake.notifier())
                .unwrap();
            if matches!(
                event,
                LiveMetadataShellPoll::Connected { connection_epoch }
                    | LiveMetadataShellPoll::Reconnected { connection_epoch }
                    if connection_epoch == epoch
            ) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "supplied descriptor authentication deadline"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(shell.connected);
        assert_eq!(shell.transport.connection_epoch(), epoch);
        let registration = shell.authentication.as_ref().unwrap().registration.unwrap();
        assert_eq!(auth.coordinator.phase(registration), Some(Phase::Active));
        if let Some(old) = previous_registration {
            assert_ne!(
                registration, old,
                "replacement must have fresh launch custody"
            );
        }
        // This is an idle liveness observation, not a semantic round trip.
        let alive_until = Instant::now() + Duration::from_millis(250);
        while Instant::now() < alive_until {
            auth.tick();
            shell
                .poll_authenticated(&mut auth.coordinator, wake.notifier())
                .unwrap();
            assert!(
                shell.connected,
                "authenticated descriptor must remain connected"
            );
            assert!(
                shell.supervisor.poll().unwrap().is_none(),
                "descriptor peer must remain alive"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        auth.coordinator
            .revoke(registration, Instant::now())
            .unwrap();
        shell
            .poll_authenticated(&mut auth.coordinator, wake.notifier())
            .unwrap();
        assert!(
            !shell.connected,
            "revocation must remove descriptor authority immediately"
        );
        while shell.authentication.as_ref().unwrap().retained() {
            auth.tick();
            shell
                .poll_authenticated(&mut auth.coordinator, wake.notifier())
                .unwrap();
            assert!(
                Instant::now() < deadline,
                "supplied descriptor cleanup deadline"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(shell.supervisor.child_id().is_none());
        assert_eq!(shell.next_connection_epoch, epoch + 1);
        previous_registration = Some(registration);
    }
    shell
        .set_presentation_available(false, "fixture finished")
        .unwrap();
    shell
        .finish_authentication_shutdown(&mut auth.coordinator, wake.notifier())
        .unwrap();
}

//! Descriptor owner lifetimes through real protected spawn. The idle peer does
//! not authenticate or negotiate a role; client interoperability is separate.
#![cfg(test)]
use super::*;
use crate::live_session::tests::lock_provider_factotum as factotum;

#[path = "authenticated_descriptor_sdk.rs"]
mod sdk;

#[path = "authenticated_descriptor_external.rs"]
mod external;

fn shell(launches: &LaunchCoordinator) -> LiveMetadataShell {
    let mut shell = LiveMetadataShell::prepare(
        "/usr/bin/sleep",
        None,
        false,
        false,
        sophia_config::ShellGpuMode::Denied,
        None,
        None,
        std::path::Path::new(sophia_runtime::DEFAULT_BUBBLEWRAP_PATH),
    )
    .unwrap();
    shell.base_launch_spec.args = vec!["20".into()];
    shell
        .authenticate(sophia_protocol::NamespaceId::from_raw(2), launches)
        .unwrap();
    shell.set_presentation_available(true, "fixture").unwrap();
    shell
}

#[test]
fn descriptor_authentication_pause_reclaims_pending_startup_and_burns_epoch() {
    let mut launches = LaunchCoordinator::new(Default::default(), factotum::namespaces());
    let mut shell = shell(&launches);
    let wake = sophia_wake::Wake::new().unwrap();
    assert_eq!(
        shell
            .poll_authenticated(&mut launches, wake.notifier())
            .unwrap(),
        LiveMetadataShellPoll::Unavailable
    );
    assert!(
        shell.supervisor.child_id().is_none(),
        "descriptor must not spawn without Factotum"
    );
    assert!(shell.authentication.as_ref().unwrap().pending.is_some());
    shell.set_presentation_available(false, "pause").unwrap();
    assert!(
        shell.authentication.as_ref().unwrap().retained(),
        "pause must retain pending spawn cleanup"
    );
    shell
        .poll_authenticated(&mut launches, wake.notifier())
        .unwrap();
    assert!(!shell.authentication.as_ref().unwrap().retained());
    assert_eq!(
        shell.next_connection_epoch, 2,
        "cancelled descriptor epoch must not be reused"
    );
    shell.set_presentation_available(true, "resume").unwrap();
    shell
        .poll_authenticated(&mut launches, wake.notifier())
        .unwrap();
    assert_eq!(shell.authentication.as_ref().unwrap().epoch, Some(2));
    shell
        .finish_authentication_shutdown(&mut launches, wake.notifier())
        .unwrap();
    assert!(!shell.authentication.as_ref().unwrap().retained());
    assert_eq!(shell.next_connection_epoch, 3);
}

#[test]
fn descriptor_launch_revocation_closes_negotiation_before_reaping() {
    let mut auth = factotum::Fixture::new();
    let mut shell = shell(&auth.coordinator);
    let wake = sophia_wake::Wake::new().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !shell.authentication.as_ref().unwrap().negotiating {
        auth.tick();
        shell
            .poll_authenticated(&mut auth.coordinator, wake.notifier())
            .unwrap();
        assert!(
            Instant::now() < deadline,
            "authenticated descriptor launch deadline"
        );
    }
    let registration = shell.authentication.as_ref().unwrap().registration.unwrap();
    assert_eq!(auth.coordinator.phase(registration), Some(Phase::Active));
    assert!(shell.supervisor.child_id().is_some());
    auth.coordinator
        .revoke(registration, Instant::now())
        .unwrap();
    assert_eq!(
        shell
            .poll_authenticated(&mut auth.coordinator, wake.notifier())
            .unwrap(),
        LiveMetadataShellPoll::Unavailable
    );
    assert!(
        shell.transport.poll_file_negotiation(65536).is_err(),
        "revoked descriptor launch must close pending negotiation"
    );
    shell
        .finish_authentication_shutdown(&mut auth.coordinator, wake.notifier())
        .unwrap();
    assert!(shell.supervisor.child_id().is_none());
    assert!(!shell.authentication.as_ref().unwrap().retained());
    assert_eq!(shell.next_connection_epoch, 2);
}

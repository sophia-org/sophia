//! Successful descriptor authentication through the normal owner and C SDK.
use super::*;
use crate::live_session::authenticated_shell_peer as peer;
use std::os::unix::fs::DirBuilderExt;

#[test]
fn descriptor_sdk_authenticates_and_recovers_through_the_normal_owner() {
    let peer = peer::Peer::build("descriptor");
    let reports = std::env::temp_dir().join(format!("descriptor-sdk-{}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&reports)
        .unwrap();
    let report = reports.join("ready");
    let mut auth = factotum::Fixture::new();
    let mut shell = LiveMetadataShell::prepare(
        peer.0.to_str().unwrap(),
        None,
        false,
        false,
        sophia_config::ShellGpuMode::Denied,
        None,
        None,
        std::path::Path::new(sophia_runtime::DEFAULT_BUBBLEWRAP_PATH),
    )
    .unwrap();
    shell.base_launch_spec.args = vec!["descriptor".into(), report.clone().into_os_string()];
    shell.base_launch_spec.protection_domain = Some(
        shell
            .base_launch_spec
            .protection_domain
            .take()
            .unwrap()
            .path(sophia_runtime::ProtectionPath::read_write(&reports))
            .unwrap(),
    );
    shell
        .authenticate(sophia_protocol::NamespaceId::from_raw(2), &auth.coordinator)
        .unwrap();
    shell.set_presentation_available(true, "fixture").unwrap();
    let wake = sophia_wake::Wake::new().unwrap();
    let mut previous_registration = None;
    for epoch in 1..=2 {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut connection_observed = false;
        while !report.exists() {
            auth.tick();
            let event = shell
                .poll_authenticated(&mut auth.coordinator, wake.notifier())
                .unwrap();
            if matches!(
                event,
                LiveMetadataShellPoll::Connected { .. } | LiveMetadataShellPoll::Reconnected { .. }
            ) {
                connection_observed = true;
            }
            assert!(
                Instant::now() < deadline,
                "descriptor SDK readiness deadline: {}",
                std::fs::read_to_string(report.with_extension("stderr")).unwrap_or_default()
            );
        }
        assert!(
            connection_observed && shell.connected,
            "descriptor owner must observe authenticated negotiation"
        );
        assert_eq!(
            std::fs::read_to_string(&report).unwrap(),
            format!("descriptor ready {epoch}\n"),
            "descriptor SDK must negotiate this exact launch epoch"
        );
        assert_eq!(shell.transport.connection_epoch(), epoch);
        let registration = shell.authentication.as_ref().unwrap().registration.unwrap();
        assert_eq!(auth.coordinator.phase(registration), Some(Phase::Active));
        if let Some(old) = previous_registration {
            assert_ne!(registration, old);
        }
        auth.coordinator
            .revoke(registration, Instant::now())
            .unwrap();
        shell
            .poll_authenticated(&mut auth.coordinator, wake.notifier())
            .unwrap();
        assert!(
            !shell.connected,
            "revoked descriptor authentication closes local authority immediately"
        );
        while shell.authentication.as_ref().unwrap().retained() {
            auth.tick();
            shell
                .poll_authenticated(&mut auth.coordinator, wake.notifier())
                .unwrap();
            assert!(Instant::now() < deadline, "descriptor cleanup deadline");
        }
        assert!(shell.supervisor.child_id().is_none());
        assert_eq!(shell.next_connection_epoch, epoch + 1);
        std::fs::remove_file(&report).unwrap();
        previous_registration = Some(registration);
    }
    shell
        .set_presentation_available(false, "fixture finished")
        .unwrap();
    shell
        .finish_authentication_shutdown(&mut auth.coordinator, wake.notifier())
        .unwrap();
    drop(shell);
    std::fs::remove_dir_all(reports).unwrap();
}

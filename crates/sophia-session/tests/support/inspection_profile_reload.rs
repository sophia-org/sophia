use super::*;

#[test]
fn desktop_reload_retains_startup_inspection_permission_and_endpoint() {
    use sophia_config::{DesktopControlAccess, DesktopInspectionAccess};
    for (startup, requested, expected) in [
        ("disabled", "host-admin", DesktopInspectionAccess::Disabled),
        ("host-admin", "disabled", DesktopInspectionAccess::HostAdmin),
    ] {
        let source = ConfigFixture::from_documents(
            CORE,
            &format!(
                "schema 1\nshell {{ enabled #false; }}\nsession {{ inspection \"{startup}\"; }}\n"
            ),
            &[],
        );
        let mut fixture = ReloadFixture::from_config_fixture(source);
        let socket = (expected == DesktopInspectionAccess::HostAdmin)
            .then(|| fixture.source.directory.join("inspection.sock"));
        fixture.source.config.inspection_socket = socket.clone();
        let original_slot = fixture.source.config.session_profile.slot().clone();
        std::fs::write(
            fixture
                .source
                .config
                .desktop_profile_source
                .as_ref()
                .unwrap(),
            format!(
                "schema 1\nshell {{ enabled #false; }}\nsession {{ inspection \"{requested}\"; }}\n"
            ),
        )
        .unwrap();
        assert_eq!(fixture.reload(), DesktopProfileReloadOutcome::Applied);
        let config = &fixture.source.config;
        assert_eq!(config.inspection_access, expected);
        assert_eq!(config.inspection_socket, socket);
        assert_eq!(config.control_access, DesktopControlAccess::Disabled);
        assert_eq!(config.session_profile.slot(), &original_slot);
        // The requested launch document is retained for its mutable application
        // fields. Effective permissions still come from the startup participant.
        assert_eq!(
            config
                .effective_launch_profile(original_slot.active().unwrap())
                .inspection,
            expected
        );
    }
}

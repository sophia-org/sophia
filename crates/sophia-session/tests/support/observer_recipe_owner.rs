//! Real Session inspection publication, with supplied WM scene/configuration.
use super::*;
use sophia_config::namespace_policy::*;
use crate::live_session::tests::lock_provider_factotum as factotum;

#[test]
fn observer_recipe_uses_private_inspection_and_refreshes_after_fencing() {
    let mut fixture = ReloadFixture::new();
    let config = &mut fixture.source.config;
    config.inspection_access = sophia_config::DesktopInspectionAccess::Disabled;
    let mut snapshot = config.core_config_state.active().clone();
    snapshot.session.namespace_policy = NamespacePolicyConfig {
        recipes: BTreeMap::from([("observer".into(), NamespaceRecipeConfig {
            name: "observer".into(), profile: NamespaceRecipeProfile::Observer,
            source: None, request: vec![RecipePortalCapability::ScreenCapture], publish: vec![], developer_launch: false,
        })]), applications: BTreeMap::from([("registered:probe".into(), "observer".into())]),
    };
    config.core_config_state = sophia_config::CoreConfigState::from_snapshot(snapshot);
    fixture.wm.start_inspection(config, 31);
    assert!(config.inspection_socket.is_none(), "observer recipe must not create a host inspection endpoint");
    let inspection = fixture.wm.public.as_ref().unwrap().inspection.as_ref().unwrap();
    assert!(inspection.service.socket_path().is_none());
    let previous = inspection.service.authenticated_acceptor(inspection.service.generation().unwrap()).unwrap();
    let registry = factotum::namespaces();
    let mut applications = LiveApplicationLaunches::prepare(config, registry.clone(), &BTreeMap::new(), Path::new("/unused-observer-cookie"), 31, Some(&fixture.wm)).unwrap();
    fixture.wm.public.as_mut().unwrap().fence_inspection(2, None);
    assert!(!previous.is_live(), "old observer service generation must stay fenced");
    applications.refresh_inspection(Some(&fixture.wm));
    let mut auth = factotum::Fixture::with_namespaces(registry);
    let wake = sophia_wake::Wake::new().unwrap();
    let mut child = ApplicationLaunchTurn {applications: &mut applications, authentication: &mut auth.coordinator, wake: wake.notifier()}.spawn_registered(
        config, Path::new("/unused-observer-cookie"), &SessionApplicationSpec {
            id: "probe".into(), executable: "/usr/bin/sleep".into(), arguments: vec!["30".into()], placement_classification: None,
        }, crate::diagnostics::application::LaunchContext {source: crate::diagnostics::application::LaunchSource::Startup, transaction: None}).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "observer process activation deadline");
        auth.tick();
        match child.poll(&mut auth.coordinator, wake.notifier(), Instant::now()) {
            Some(crate::namespace_launch::ApplicationProcessEvent::Started {..}) => break,
            Some(_) => panic!("fresh observer launch must select the current inspection generation"),
            None => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    let pid = child.process_id().unwrap();
    assert!(child.cancel_namespace());
    while crate::launch_origin::read_process(pid).is_some() {
        assert!(Instant::now() < deadline, "observer process reap deadline");
        auth.tick();
    }
}

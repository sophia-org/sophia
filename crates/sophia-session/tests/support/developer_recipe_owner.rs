//! Real 9P request, HostDomain claim and Session recipe process owner.
//! WM inspection state is supplied; no native display or capture is claimed.
use super::*;
use sophia_config::namespace_policy::*;
use sophia_protocol::developer_launch::*;
use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
use crate::live_session::tests::lock_provider_factotum as factotum;

fn request(path: std::path::PathBuf, recipe: &str) -> DeveloperLaunchResult {
    let mut peer=Pipeline::connect(&path,PipelineLimits::default(),Duration::from_secs(3)).unwrap();
    let deadline=Instant::now()+Duration::from_secs(10);
    let (tag,root)=peer.attach(b"",b"").unwrap(); assert!(matches!(peer.wait(tag,deadline).unwrap(),Reply::Attach(_)));
    let mut files=Vec::new();
    for name in [b"request".as_slice(),b"submit",b"result"] {
        let (tag,fid)=peer.walk(root,&[name]).unwrap(); assert!(matches!(peer.wait(tag,deadline).unwrap(),Reply::Walk(_))); files.push(fid);
    }
    for fid in &files[..2] { let tag=peer.lopen(*fid,1).unwrap(); assert!(matches!(peer.wait(tag,deadline).unwrap(),Reply::Lopen {..})); }
    let bytes=encode_developer_launch_request(&DeveloperLaunchRequest {version:1, request:71, recipe:recipe.into(),executable:"/usr/bin/sleep".into(),arguments:vec!["30".into()]}).unwrap();
    let tag=peer.write(files[0],0,&bytes).unwrap(); assert_eq!(peer.wait(tag,deadline).unwrap(),Reply::Write(bytes.len() as u32));
    let tag=peer.write(files[1],0,&71u32.to_le_bytes()).unwrap(); assert_eq!(peer.wait(tag,deadline).unwrap(),Reply::Write(4));
    loop {
        let tag=peer.lopen(files[2],0).unwrap();
        match peer.wait(tag,deadline).unwrap() { Reply::Lopen {..} => break, Reply::Error(e) if e.0==11 => { assert!(Instant::now()<deadline); std::thread::sleep(Duration::from_millis(1)); }, other => panic!("{other:?}") }
    }
    let tag=peer.read(files[2],0,256).unwrap();
    let Reply::Read(bytes)=peer.wait(tag,deadline).unwrap() else { panic!("result read"); };
    decode_developer_launch_result(&bytes).unwrap()
}

#[test]
fn developer_launch_requires_both_host_administration_and_explicit_recipe_permission() {
    let mut fixture=ReloadFixture::new();
    let config=&mut fixture.source.config;
    let wake=sophia_wake::Wake::new().unwrap();
    config.control_access=sophia_config::DesktopControlAccess::HostAdmin;
    assert!(LiveDeveloperLaunches::start(config,wake.notifier()).unwrap().socket_path().is_none(),"default recipes cannot publish developer launch");
    let mut snapshot=config.core_config_state.active().clone();
    snapshot.session.namespace_policy=NamespacePolicyConfig {recipes:[("tools",true),("closed",false)].into_iter().map(|(name,enabled)| (name.into(),NamespaceRecipeConfig {
        name:name.into(), profile:NamespaceRecipeProfile::Observer,source:None,request:vec![RecipePortalCapability::ScreenCapture],publish:vec![],developer_launch:enabled, capture_outputs: vec![],
    })).collect(),applications:BTreeMap::new()};
    config.core_config_state=sophia_config::CoreConfigState::from_snapshot(snapshot);
    config.control_access=sophia_config::DesktopControlAccess::Disabled;
    assert!(LiveDeveloperLaunches::start(config,wake.notifier()).unwrap().socket_path().is_none(),"recipe permission cannot imply host administration");
    config.control_access=sophia_config::DesktopControlAccess::HostAdmin;
    config.inspection_access=sophia_config::DesktopInspectionAccess::Disabled;
    fixture.wm.start_inspection(config,31);
    let registry=factotum::namespaces();
    let mut applications=LiveApplicationLaunches::prepare(config,registry.clone(),&BTreeMap::new(),Path::new("/unused"),31,Some(&fixture.wm)).unwrap();
    let mut auth=factotum::Fixture::with_namespaces(registry);
    let mut owner=LiveDeveloperLaunches::start(config,wake.notifier()).unwrap();
    let mut children=Vec::new();
    for (recipe,status) in [("tools",DeveloperLaunchStatus::Started),("closed",DeveloperLaunchStatus::Denied),("admin",DeveloperLaunchStatus::Denied)] {
        let socket=owner.socket_path().unwrap().to_path_buf();
        let client=std::thread::spawn(move || request(socket,recipe));
        let deadline=Instant::now()+Duration::from_secs(10);
        while !client.is_finished() {
            assert!(Instant::now()<deadline,"developer Session join deadline");
            auth.tick();
            owner.service(config,&mut ApplicationLaunchTurn {applications:&mut applications,authentication:&mut auth.coordinator,wake:wake.notifier()},&mut children,false);
            for child in &mut children { child.poll(&mut auth.coordinator,wake.notifier(),Instant::now()); }
            std::thread::sleep(Duration::from_millis(1));
        }
        let result=client.join().unwrap();
        let actual=status==DeveloperLaunchStatus::Started;
        let pids:Vec<_>=children.iter().filter_map(|c| c.child.process_id()).collect();
        // Cleanup precedes the oracle so a policy-bypass control cannot leave
        // a successfully mislaunched process behind when the assertion fails.
        for child in &mut children { child.terminate(true).unwrap(); }
        children.clear();
        while pids.iter().any(|pid|crate::launch_origin::read_process(*pid).is_some()) { assert!(Instant::now()<deadline); auth.tick(); }
        assert_eq!(result.status,status,"developer launch must enforce the exact recipe permission");
        assert_eq!(result.process.is_some(),actual);
        assert_eq!(pids.len(),usize::from(actual),"denied developer launch cannot create process custody");
    }
}

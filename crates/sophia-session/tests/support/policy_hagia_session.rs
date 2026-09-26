//! Opt-in normal Hagia through the protected production WM file factory.
//! Common protected startup custody for opt-in normal Hagia controls.
//! Output bootstrap, native targets and presentation receipts are not supplied.
use super::*;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::time::{Duration, Instant};

const FROZEN_BINARY: &str =
    "/home/niltempus/dev/hagia-overview-fix/.artifacts/h006-endpoint/hagia-7455c3e";
const FROZEN_SHA256: &str = "0419e09e224676c4d925438f80b22df9532c1653ec339507637edbe01ea52f5f";

#[path = "policy_hagia_layout.rs"]
mod layout_settlement;

fn binary_hash(path: &Path) -> String {
    assert!(std::fs::metadata(path).unwrap().len() <= 64 * 1024 * 1024);
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

#[test]
#[ignore = "requires exact frozen normal Hagia and explicit fresh evidence inputs"]
fn protected_normal_hagia_admits_profile_configuration_and_catalog_over_files() {
    with_normal_hagia("protected-startup", |_, _, _, _, checkpoint, _| {
        assert!(!checkpoint.exists(), "startup must not write a layout checkpoint");
    });
}

// Each case gets a fresh child, profile and checkpoint. The callback runs only
// after real configuration publication and ReadyForCycle; cleanup and binary
// custody remain common to startup and layout-settlement controls.
fn with_normal_hagia(
    case: &str,
    exercise: impl FnOnce(
        &mut LiveWmSession,
        &mut PersistentLiveLayout,
        &mut ConfigFixture,
        sophia_engine::HeadlessOutput,
        &Path,
        &mut std::fs::File,
    ),
) {
    let binary = std::fs::canonicalize(
        std::env::var_os("SOPHIA_HAGIA_FILE_BIN").expect("required frozen normal Hagia missing"),
    )
    .unwrap();
    let expected = std::env::var("SOPHIA_HAGIA_FILE_SHA256").expect("required pinned hash missing");
    assert_eq!(expected, FROZEN_SHA256);
    assert_eq!(binary, std::fs::canonicalize(FROZEN_BINARY).unwrap());
    assert_eq!(binary_hash(&binary), expected);
    let evidence = PathBuf::from(
        std::env::var_os("SOPHIA_HAGIA_FILE_EVIDENCE")
            .expect("required fresh evidence parent missing"),
    )
    .join(case);
    std::fs::create_dir(&evidence).expect("case evidence must be fresh; parent must exist");
    let mut identity = std::fs::File::create(evidence.join("identity.txt")).unwrap();
    writeln!(identity, "binary={}\npath_sha256_before={expected}\nsource=7455c3edd713770ed43630d0989073d2f14ba623\nidentity_qualification=path hashed before/after; not descriptor-pinned exec", binary.display()).unwrap();

    let mut source = ConfigFixture::new(&[]);
    source.config.wm_socket_path = source.directory.join("hagia-file.sock");
    source.config.wm_process = Some(binary.to_str().unwrap().to_owned());
    source.config.wm_process_args.clear();
    source.config.wm_transport = WmTransportSelection::NineP2000L;
    source.config.native_scanout = false;
    let expected_profile =
        sophia_config::DesktopProfileActivationKey::from(&source.config.desktop_profile);
    writeln!(
        identity,
        "expected_epoch=1\nprofile_generation={}\nprofile_digest={}",
        expected_profile.generation().raw(),
        expected_profile.digest()
    )
    .unwrap();
    let prepared = LiveWmSession::prepare_public_launch(&mut source.config).unwrap();
    let checkpoint = prepared.as_ref().unwrap().directory.checkpoint_path();
    assert!(!checkpoint.exists(), "fresh normal Hagia checkpoint");
    let started = LiveWmSession::activate_public_launch(&mut source.config, prepared)
        .unwrap()
        .unwrap();
    let output = sophia_engine::HeadlessOutput::deterministic();
    let mut wm =
        LiveWmSession::from_started_public_config(&source.config, &[output], started, None)
            .unwrap();
    let protection = wm
        .supervisor
        .protection_evidence()
        .expect("production protected launch evidence")
        .clone();
    assert_eq!(wm.supervisor.peer_id(), Some(protection.peer_pid));
    assert!(
        protection
            .roles
            .contains(&sophia_runtime::ProtectionDomainRole::SpatialPolicy)
    );
    writeln!(identity, "supervisor_pid={}\nprotected_peer_pid={}\nprotection_evidence={protection:?}\nprotection_qualification=supervisor launch evidence; no independent namespace inventory", protection.supervisor_pid, protection.peer_pid).unwrap();
    let spec = wm.supervisor.launch_spec();
    assert_eq!(spec.program, binary);
    assert!(
        spec.args.is_empty(),
        "no scripted peer or inherited fault arguments"
    );
    let keys = spec
        .environment
        .iter()
        .map(|(key, _)| key.to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        keys,
        [
            "SOPHIA_WM_9P_SOCKET",
            "HAGIA_POLICY_CHECKPOINT",
            "HAGIA_POLICY_CANDIDATE",
            "HAGIA_POLICY_PROFILE_ACTIVATION"
        ]
        .map(str::to_owned)
        .into_iter()
        .collect()
    );
    assert!(
        spec.environment
            .iter()
            .any(|(key, value)| key == "HAGIA_POLICY_PROFILE_ACTIVATION" && value == "required")
    );
    assert!(
        spec.environment
            .iter()
            .any(|(key, _)| key == "SOPHIA_WM_9P_SOCKET")
    );
    assert!(!spec.environment.iter().any(
        |(key, _)| key == "SOPHIA_WM_SOCKET" || key == sophia_runtime::SOPHIA_OUTPUT_SOCKET_ENV
    ));
    let forbidden = sophia_protocol::SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES
        | sophia_protocol::SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS;
    let ceiling = sophia_runtime::select_policy_capabilities(u64::MAX, !forbidden, true);
    let mut layout = PersistentLiveLayout::default();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            wm.poll_public_request(&mut layout, output, false)
                .unwrap()
                .is_none()
        );
        // The production idle-input publication owner promotes the admitted
        // catalog. Polling the transport alone only stages configuration.
        wm.settle_desktop_reload(&mut source.config, true).unwrap();
        let public = wm.public.as_ref().unwrap();
        assert_eq!(public.connection_epoch, 1);
        assert_eq!(public.profile_key, Some(expected_profile));
        assert_eq!(public.selected_capabilities & !ceiling, 0);
        if public.configured && public.transport_ready {
            break;
        }
        assert!(
            !wm.force_transport_restart && !wm.degraded,
            "real Hagia startup rejected"
        );
        assert!(
            Instant::now() < deadline,
            "real configuration and ReadyForCycle deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let public = wm.public.as_ref().unwrap();
    let selected = public.selected_capabilities;
    assert_ne!(
        selected & sophia_protocol::SOPHIA_WM_CAPABILITY_CONFIGURATION,
        0
    );
    assert_ne!(
        selected & sophia_protocol::SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION,
        0
    );
    assert_eq!(selected & forbidden, 0);
    assert!(public.output_service.is_none());
    assert!(public.in_flight_request.is_none());
    let configuration = public.accepted_configuration.as_ref().unwrap();
    assert_eq!(configuration.connection_epoch, 1);
    assert_eq!(
        configuration.generation, 1,
        "fresh real Hagia catalog namespace"
    );
    assert!(!configuration.actions.is_empty());
    assert!(configuration.actions.iter().all(|action| action.name != "toggle-overview" && !action.name.starts_with("overview-")), "no overview catalog without native retirement capabilities");
    assert_eq!(public.actions, configuration.actions);
    assert!(wm.shortcuts.is_some());
    writeln!(identity, "configured=true\ntransport_ready=true\nselected_capabilities={selected}\nceiling={ceiling}\ncatalog_generation={}\ncatalog_actions={}", configuration.generation, configuration.actions.len()).unwrap();
    // Revisit the same owners without issuing a cycle: selected capabilities
    // must stay fixed after admission, rather than track later Session polls.
    for _ in 0..3 {
        assert!(
            wm.poll_public_request(&mut layout, output, false)
                .unwrap()
                .is_none()
        );
        wm.settle_desktop_reload(&mut source.config, true).unwrap();
        assert_eq!(wm.public.as_ref().unwrap().selected_capabilities, selected);
    }
    exercise(&mut wm, &mut layout, &mut source, output, &checkpoint, &mut identity);
    let stopped = Instant::now();
    wm.public.as_mut().unwrap().worker.take(); // Existing worker Drop/Stop owner.
    wm.supervisor.request_termination().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !wm.supervisor.poll_termination().unwrap() {
        assert!(
            Instant::now() < deadline,
            "normal child reap exceeded fixture budget"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(wm.supervisor.peer_id().is_none());
    // Existing supervisor Drop may still wait on a stuck kernel on a failing
    // path; this successful-run measurement is not a hard total cleanup bound.
    writeln!(
        identity,
        "child_reaped=true\ncleanup_msec={}\npath_sha256_after={}",
        stopped.elapsed().as_millis(),
        binary_hash(&binary)
    )
    .unwrap();
    assert_eq!(binary_hash(&binary), expected);
    if case == "protected-startup" {
        assert!(!checkpoint.exists(), "startup alone must not produce a layout checkpoint");
    }
    std::fs::write(
        evidence.join("result.txt"),
        format!("PASS protected normal Hagia case={case}; no native settlement\n"),
    )
    .unwrap();
}

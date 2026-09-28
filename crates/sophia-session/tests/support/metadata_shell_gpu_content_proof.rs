#![cfg(test)]

use super::*;
use sophia_config::ShellComponentEdge;
use std::cell::Cell;

fn proof() -> ShellGpuContentProof {
    ShellGpuContentProof {
        client: "/absent/shell-client".into(),
        client_args: Vec::new(),
        config: None,
        seat: "seat0".into(),
        render_node: "/absent/renderD128".into(),
        expected_device: None,
        output: ShellGpuProofExtent {
            width: 800,
            height: 600,
        },
        surface: ShellGpuProofSurface {
            edge: ShellComponentEdge::Left,
            width: 48,
            height: 600,
        },
        outcomes: vec![ShellGpuProofOutcome::PresentedSynthetic; 3],
        end: ShellGpuProofEnd::StopClient,
        pixels: ShellGpuProofPixels::FullSurfaceRaster,
        discrete_input: false,
        timeout: SHELL_GPU_PROOF_DEFAULT_TIMEOUT,
    }
}

#[test]
fn admission_policy_follows_the_discrete_input_parameter() {
    for discrete_input in [false, true] {
        assert!(matches!(
            proof_content_admission_policy(discrete_input),
            ShellContentAdmissionPolicy::Granted { discrete_input: granted }
                if granted == discrete_input
        ));
    }
}

#[test]
fn pinned_device_expectation_refuses_before_the_protected_launch() {
    let device = super::super::gpu::ShellGpuLaunchEvidence {
        epoch: 1,
        major: 226,
        minor: 128,
        render_node: "/dev/dri/renderD128".into(),
        pci_bus_id: Some("0000:03:00.0".into()),
        pci_vendor_id: Some(0x1002),
        pci_device_id: Some(0x744c),
    };
    require_expected_device(&device, Some("226:128@0000:03:00.0")).unwrap();
    for wrong in [
        "226:129@0000:03:00.0",
        "226:128@0000:04:00.0",
        "invalid",
        "",
    ] {
        assert!(require_expected_device(&device, Some(wrong)).is_err());
    }
}

/// Invalid parameters are refused before the render inventory is read, so no
/// device is enumerated, opened or granted.
#[test]
fn invalid_parameters_never_reach_the_render_inventory() {
    let mut invalid = Vec::new();
    let mut zero = proof();
    zero.surface.width = 0;
    invalid.push(zero);
    let mut outside = proof();
    outside.surface.height = 601;
    invalid.push(outside);
    let mut none = proof();
    none.outcomes.clear();
    invalid.push(none);
    let mut relative = proof();
    relative.render_node = "renderD128".into();
    invalid.push(relative);
    let mut slow = proof();
    slow.timeout = SHELL_GPU_PROOF_MAX_TIMEOUT + Duration::from_millis(1);
    invalid.push(slow);
    for parameters in invalid {
        let consulted = Cell::new(false);
        let error = run_with_inventory(&parameters, |_| {
            consulted.set(true);
            Err("inventory spy reached".into())
        })
        .unwrap_err();
        assert!(!consulted.get(), "inventory consulted for {parameters:?}");
        assert!(
            error.downcast_ref::<ShellGpuProofError>().is_some(),
            "not a parameter refusal: {error}"
        );
    }
}

/// Valid parameters pass validation; this client path is absent, so the file
/// check refuses next -- still before the inventory.
#[test]
fn valid_parameters_still_check_the_client_file_before_the_inventory() {
    let consulted = Cell::new(false);
    let error = run_with_inventory(&proof(), |_| {
        consulted.set(true);
        Err("inventory spy reached".into())
    })
    .unwrap_err();
    assert!(!consulted.get());
    assert_eq!(error.to_string(), "shell client must be an absolute file");
}

#[test]
fn inventory_is_read_only_after_every_local_check_passes() {
    let client = std::env::current_exe().unwrap();
    let mut parameters = proof();
    parameters.client = client;
    let consulted = Cell::new(false);
    let error = run_with_inventory(&parameters, |seat| {
        assert_eq!(seat, "seat0");
        consulted.set(true);
        Ok(Vec::new())
    })
    .unwrap_err();
    assert!(consulted.get());
    assert_eq!(
        error.to_string(),
        "proof requires exactly one admitted render-node identity"
    );
}

/// A fragmented candidate -- Begin, several chunks, End -- that arrives one
/// record per visit must be serviced through its End. A cap on intake across
/// visits (formerly three records per render) strands it after the third
/// record and the proof times out; this control fails under that cap.
#[test]
fn a_fragmented_candidate_across_visits_is_serviced_to_its_end() {
    let records = ["begin", "chunk", "chunk", "chunk", "chunk", "end"];
    let mut arriving = records.iter();
    let mut serviced = Vec::new();
    let mut intake = CandidateIntake::default();
    for _visit in 0..records.len() {
        intake
            .visit(1, || {
                Ok::<_, ()>(match arriving.next() {
                    Some(record) => {
                        serviced.push(*record);
                        1
                    }
                    None => 0,
                })
            })
            .unwrap();
    }
    assert_eq!(serviced, records);
    assert_eq!(intake.serviced, records.len());
}

#[test]
fn intake_waits_for_a_permit_and_propagates_owner_errors() {
    let mut intake = CandidateIntake::default();
    let mut called = false;
    intake
        .visit(0, || {
            called = true;
            Ok::<_, ()>(1)
        })
        .unwrap();
    assert!(!called, "a candidate cannot begin before a frame permit");
    assert_eq!(intake.visit(1, || Err::<usize, _>("owner")), Err("owner"));
    assert_eq!(intake.serviced, 0);
}

#[test]
fn full_surface_raster_requires_exact_size_and_varied_bytes() {
    let surface = ShellGpuProofSurface {
        edge: ShellComponentEdge::Top,
        width: 4,
        height: 2,
    };
    let varied = (0..32_u8).collect::<Vec<_>>();
    check_full_surface_raster(surface, 4, 2, &varied).unwrap();
    for (width, height, bytes) in [
        (3, 2, varied.clone()),
        (4, 1, varied.clone()),
        (4, 2, varied[..28].to_vec()),
        (4, 2, vec![0; 32]),
        (4, 2, [1, 2, 3, 4].repeat(8)),
        (4, 2, Vec::new()),
    ] {
        assert!(
            check_full_surface_raster(surface, width, height, &bytes).is_err(),
            "{width}x{height} with {} bytes",
            bytes.len()
        );
    }
}

#[path = "../../../sophia-conformance/tests/support/c_content_peer.rs"]
mod c_content_peer;

/// Proof records, as the session host would receive them.
static RECORDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

#[path = "../../../sophia-conformance/tests/support/bounded_peer.rs"]
mod bounded_peer;

/// Session output is intentionally installed once per process. Run capture
/// tests in distinct processes so neither they nor the startup tests can steal
/// another test's callback; a mutex cannot reset the production OnceLock.
fn isolated_capture_test(name: &str) -> bool {
    let exact = format!("{}::{name}", module_path!());
    // module_path includes the crate name; the test harness does not.
    let exact = exact.split_once("::").unwrap().1;
    if std::env::var("SOPHIA_TEST_GPU_PROOF_CHILD").as_deref() == Ok(exact) {
        return false;
    }
    let output = bounded_peer::run(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", exact, "--nocapture"])
            .env("SOPHIA_TEST_GPU_PROOF_CHILD", exact),
        Duration::from_secs(60),
    )
    .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}{stderr}");
    assert!(
        stdout.contains("1 passed; 0 failed"),
        "child did not run {exact}: {stdout}{stderr}"
    );
    true
}

fn capture(line: &str) {
    RECORDS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(line.to_owned());
}

#[test]
fn the_retired_socket_variable_is_refused_not_ignored() {
    refuse_retired_socket(false).unwrap();
    let error = refuse_retired_socket(true).unwrap_err().to_string();
    assert!(error.contains("SOPHIA_SHELL_SOCKET"), "{error}");
}

/// Launch the independent C peer in the proof's protected domain with the
/// proof's endpoint variables, then run the proof's own service loop. No render
/// node exists here: the grant evidence is a stand-in and nothing is opened,
/// so this proves the file-wire content lifecycle, not GPU execution.
fn serve_with_c_peer(
    name: &str,
    thickness: u32,
    mutation: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let scratch =
        std::env::temp_dir().join(format!("gpu-proof-serve-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&scratch)?;
    let peer = c_content_peer::build(&scratch, mutation);
    let mut parameters = proof();
    parameters.client = peer.clone();
    parameters.output = ShellGpuProofExtent {
        width: 64,
        height: 64,
    };
    parameters.surface = ShellGpuProofSurface {
        edge: ShellComponentEdge::Top,
        width: 64,
        height: 32,
    };
    parameters.outcomes = vec![
        ShellGpuProofOutcome::PresentedSynthetic,
        ShellGpuProofOutcome::RendererFailed,
    ];
    parameters.end = ShellGpuProofEnd::ClientExits;
    parameters.timeout = Duration::from_secs(10);
    parameters.validate()?;
    crate::output::install(crate::output::SessionOutput::new(capture, capture))?;
    let mut owner = ShellComponentTransport::bind_for_supervised_uid(
        scratch.join("socket"),
        rustix::process::geteuid().as_raw(),
    )?;
    let mut epochs =
        ContentEpochRegistry::new(64 * 1024 * 1024).map_err(ShellTransportError::from)?;
    let socket = owner.socket_path().to_path_buf();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])?.path(
        ProtectionPath::read_only(socket.parent().ok_or("socket parent")?),
    )?;
    let spec = ProcessLaunchSpec::new(&peer)
        .arg("content-serve")
        .env(FILE_SOCKET_ENV, &socket)
        .env("SOPHIA_SHELL_BAR_THICKNESS", thickness.to_string())
        .process_group()
        .protection_domain(domain);
    let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::Shell, spec);
    supervisor.apply(SupervisorCommand::StartProcess {
        process: SupervisedProcessKind::Shell,
        delay: Duration::ZERO,
    })?;
    let protection = supervisor
        .protection_evidence()
        .ok_or("no protection evidence")?
        .clone();
    owner.authorize_protected_peer(&protection)?;
    let stand_in = super::super::gpu::ShellGpuLaunchEvidence {
        epoch: 1,
        major: 0,
        minor: 0,
        render_node: "/absent/renderD128".into(),
        pci_bus_id: None,
        pci_vendor_id: None,
        pci_device_id: None,
    };
    let result = serve(
        &parameters,
        &mut owner,
        &mut epochs,
        &mut supervisor,
        &protection,
        &stand_in,
        "0123456789abcdef0123456789abcdef",
    );
    let _ = supervisor.terminate();
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

/// Prepared then Presented for the first render, RendererFailed for the last;
/// the client retires and exits, and the renderer-held lease must survive the
/// disconnect before settling to zero reserved and backing bytes.
#[test]
fn the_proof_loop_serves_a_protected_independent_file_peer() {
    if isolated_capture_test("the_proof_loop_serves_a_protected_independent_file_peer") {
        return;
    }
    serve_with_c_peer("valid", 32, 0).unwrap();
    let records = RECORDS.lock().unwrap().clone();
    let completion = records
        .iter()
        .find(|line| line.starts_with("sophia_shell_gpu_content_proof "))
        .expect("completion record");
    for field in [
        "status=complete",
        "protected=true",
        "wire=9p2000.L",
        "revision=6",
        "renders=2",
        "pixels=full_surface_raster",
        "end=client_exits",
        "backing_bytes=0",
        "native_presentation=false",
    ] {
        assert!(completion.contains(field), "{field}: {completion}");
    }
    for outcome in ["outcome=presented_synthetic", "outcome=renderer_failed"] {
        assert!(
            records.iter().any(|line| line
                .starts_with("sophia_shell_gpu_content_render schema=1 ")
                && line.contains("bytes=8192")
                && line.contains(outcome)),
            "{outcome}: {records:?}"
        );
    }
}

/// Red control: the peer asks for a thinner surface than the parameters name.
#[test]
fn the_proof_loop_refuses_a_peer_that_changes_the_surface() {
    if isolated_capture_test("the_proof_loop_refuses_a_peer_that_changes_the_surface") {
        return;
    }
    let error = serve_with_c_peer("thin", 16, 0).unwrap_err().to_string();
    assert!(
        error.contains("client allocation request differs"),
        "{error}"
    );
}

/// Red control for the disconnect tolerance after the final render: a peer
/// that leaves after its first outcome still fails the proof.
#[test]
fn the_proof_loop_refuses_a_peer_that_leaves_before_its_last_render() {
    if isolated_capture_test("the_proof_loop_refuses_a_peer_that_leaves_before_its_last_render") {
        return;
    }
    let error = serve_with_c_peer("early", 32, 3).unwrap_err().to_string();
    assert!(
        error.contains("NotConnected") || error.contains("client exited after 1 of 2"),
        "{error}"
    );
}

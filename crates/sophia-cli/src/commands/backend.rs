#[cfg(feature = "atomic-scanout-smoke-live")]
use std::time::{Duration, Instant};

#[cfg(feature = "atomic-scanout-smoke-live")]
use super::prelude::parse_u64;
#[cfg(feature = "native-session")]
use super::prelude::{BufferSource, Size, XAuthorityCpuBufferSnapshot, arg_value};
#[cfg(feature = "atomic-scanout-smoke-live")]
use sophia_session::backend_args::{
    atomic_scanout_smoke_child_args, atomic_scanout_smoke_child_timeout,
};
#[cfg(feature = "atomic-scanout-smoke-live")]
use sophia_session::backend_evidence::runtime_rendered_scanout_evidence_is_clean;

pub(crate) fn try_run(args: &[String]) -> Result<bool, Box<dyn std::error::Error>> {
    #[cfg(feature = "native-session")]
    if args
        .iter()
        .any(|arg| arg == "sophia-shell-gpu-content-hardware-proof")
    {
        if std::env::var_os("SOPHIA_LOM_GPU_PROOF_ARM").as_deref()
            != Some(std::ffi::OsStr::new("1"))
        {
            return Err("set SOPHIA_LOM_GPU_PROOF_ARM=1 to run the Lom GPU proof".into());
        }
        let client = arg_value(args, "--client").ok_or("proof requires --client=/absolute/path")?;
        let config = arg_value(args, "--config").ok_or("proof requires --config=/absolute/path")?;
        let seat = arg_value(args, "--seat").unwrap_or_else(|| "seat0".into());
        let render_node =
            arg_value(args, "--render-node").unwrap_or_else(|| "/dev/dri/renderD128".into());
        sophia_session::run_shell_gpu_content_hardware_proof(
            std::path::Path::new(&client),
            std::path::Path::new(&config),
            &seat,
            std::path::Path::new(&render_node),
        )?;
        return Ok(true);
    }

    #[cfg(feature = "native-session")]
    if args.first().map(String::as_str) == Some("session") {
        match args.get(1).map(String::as_str) {
            Some("run") => run_native_session(args)?,
            Some("input-guard") => sophia_session::run_input_guard(args)?,
            Some(command) => {
                return Err(format!(
                    "unknown session command {command:?}; expected run or input-guard"
                )
                .into());
            }
            None => {
                return Err("session requires run or input-guard".into());
            }
        }
        return Ok(true);
    }

    #[cfg(feature = "native-session")]
    if args.iter().any(|arg| arg == "sophia-session-input-guard") {
        sophia_session::run_input_guard(args)?;
        return Ok(true);
    }

    if args.iter().any(|arg| arg == "native-topology-probe") {
        // Read-only. Every commit this issues carries TEST_ONLY, and the only
        // framebuffer it touches is one the CRTC already scans out, so no output
        // state changes and nothing is allocated. It does need DRM master, so it
        // must run with no other compositor holding the card.
        let report = sophia_backend_live::native_topology_probe_report();
        println!("{}", report.reduced_log_line());

        if !report.answered() {
            return Err(format!(
                "native topology probe reached no conclusion: {:?}",
                report.status
            )
            .into());
        }

        return Ok(true);
    }

    if args.iter().any(|arg| arg == "native-mirror-probe") {
        // Validation-only. Every commit carries TEST_ONLY, so nothing reaches a
        // screen. Unlike the topology probe this does allocate: a shared
        // framebuffer cannot be tested without one. The buffers are dumb buffers
        // created and destroyed inside the probe and are never shown. It needs
        // DRM master, so no other compositor may hold the card.
        let report = sophia_backend_live::native_mirror_probe_report();
        println!("{}", report.reduced_log_line());

        if !report.answered() {
            return Err(format!(
                "native mirror probe reached no conclusion: {:?}",
                report.status
            )
            .into());
        }

        return Ok(true);
    }

    if args.iter().any(|arg| arg == "native-mirror-page-flip") {
        // This one commits for real, so it is gated behind an explicit opt-in.
        // It is still as close to a no-op as the question allows: each CRTC is
        // flipped to the framebuffer it is already scanning out, at its current
        // mode, with no ALLOW_MODESET. Nothing on screen changes. It cannot be
        // answered with TEST_ONLY, because the kernel rejects that together with
        // PAGE_FLIP_EVENT before looking at anything else -- and the event
        // behaviour is exactly what is being asked.
        if std::env::var_os("SOPHIA_NATIVE_MIRROR_PAGE_FLIP").is_none() {
            return Err("set SOPHIA_NATIVE_MIRROR_PAGE_FLIP=1 to commit a real page flip".into());
        }
        let report = sophia_backend_live::native_mirror_page_flip_report();
        println!("{}", report.reduced_log_line());

        if !report.attempted {
            return Err(format!(
                "native mirror page flip was not attempted: {:?}",
                report.skipped
            )
            .into());
        }

        return Ok(true);
    }

    #[cfg(feature = "native-session")]
    if args.iter().any(|arg| arg == "native-topology-validate") {
        run_native_topology_validation(args)?;
        return Ok(true);
    }

    #[cfg(feature = "native-session")]
    if args.iter().any(|arg| arg == "native-topology-apply") {
        if std::env::var_os("SOPHIA_NATIVE_OUTPUT_APPLY").is_none() {
            return Err(
                "set SOPHIA_NATIVE_OUTPUT_APPLY=1 to apply a topology to real outputs".into(),
            );
        }
        run_native_topology_apply(args)?;
        return Ok(true);
    }

    if args.iter().any(|arg| arg == "atomic-scanout-preflight") {
        let report = sophia_backend_live::real_atomic_scanout_preflight_report();
        println!("{}", report.reduced_log_line());

        if report.status
            != sophia_backend_live::LiveAtomicScanoutPreflightStatus::CandidatePrimaryCardsAtomicReady
        {
            return Err(format!(
                "atomic scanout preflight did not find a smoke-ready host: {:?}",
                report.status
            )
            .into());
        }

        return Ok(true);
    }

    if args
        .iter()
        .any(|arg| arg == "live-session-composition-smoke")
    {
        let authority_batches =
            super::x_authority::collect_x_authority_present_pixmap_authority_batches()?;
        let report = sophia_backend_live::run_live_session_composition_smoke(authority_batches);
        println!("{}", report.reduced_log_line());

        if report.status != sophia_backend_live::LiveSessionCompositionSmokeStatus::Passed {
            return Err(format!(
                "live session composition smoke failed with status {:?}",
                report.status
            )
            .into());
        }

        return Ok(true);
    }

    #[cfg(feature = "native-session")]
    if args
        .iter()
        .any(|arg| arg == "native-egl-vkcube-mixed-smoke")
    {
        run_native_egl_vkcube_mixed_smoke_parent(args)?;
        return Ok(true);
    }

    #[cfg(feature = "native-session")]
    if args
        .iter()
        .any(|arg| arg == "native-egl-vkcube-mixed-smoke-child")
    {
        if std::env::var_os("SOPHIA_NATIVE_EGL_MIXED_CHILD").is_none() {
            return Ok(true);
        }
        let result = sophia_session::run_from_args(args);
        match result {
            Err(error) => {
                let Some(report) =
                    error.downcast_ref::<sophia_backend_live::LiveNativeMixedDiagnosticComplete>()
                else {
                    return Err(error);
                };
                println!("{}", report.reduced_log_line("completed"));
                if report.status
                    != sophia_backend_live::LiveRendererScanoutBufferExportStatus::Exported
                    || report.cpu_layers == 0
                    || report.dmabuf_layers == 0
                    || report.live_sources != 0
                    || report.live_fences != 0
                    || report.live_transactions != 0
                {
                    return Err("native EGL mixed composition diagnostic did not pass".into());
                }
            }
            Ok(()) => {
                return Err(
                    "native EGL mixed composition diagnostic observed no vkcube Present".into(),
                );
            }
        }
        return Ok(true);
    }

    #[cfg(feature = "native-session")]
    if args.iter().any(|arg| arg == "sophia-live-session") {
        run_native_session(args)?;
        return Ok(true);
    }

    #[cfg(feature = "atomic-scanout-smoke-live")]
    if args
        .iter()
        .any(|arg| arg == "sophia-live-session-content-hardware-proof")
    {
        run_sophia_live_session_content_hardware_proof(args)?;
        return Ok(true);
    }

    #[cfg(feature = "atomic-scanout-smoke-live")]
    if args.iter().any(|arg| arg == "atomic-vrr-inspect") {
        let config = sophia_backend_live::RealAtomicScanoutSmokeConfig::default_primary_output()
            .ok_or("default VRR inspection config is invalid")?;
        let mut session_result = sophia_backend_live::select_real_atomic_scanout_card()
            .into_page_flip_session(config.slot, config.output, config.head, config.authority);
        let session = session_result.session.take().ok_or_else(|| {
            format!(
                "VRR inspection selection failed: {:?}",
                session_result.status
            )
        })?;
        let selection = session.selection();
        let discovery = session.vrr_properties_for_selection(selection);
        let (connector_properties, crtc_properties) =
            session.property_names_for_selection(selection)?;
        println!(
            "sophia_vrr_inspect schema=1 connector={} crtc={} status={:?} capable={} enable_property={} connector_properties={} crtc_properties={}",
            selection.connector_id(),
            selection.crtc_id(),
            discovery.status,
            discovery.capable,
            discovery.enable_property.is_some(),
            connector_properties.join(","),
            crtc_properties.join(","),
        );
        return Ok(true);
    }

    #[cfg(feature = "atomic-scanout-smoke-live")]
    if args.iter().any(|arg| arg == "atomic-scanout-smoke") {
        if std::env::var_os("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE").is_none() {
            return Err("set SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1 to run destructive atomic scanout smoke".into());
        }

        let _config = atomic_scanout_smoke_cli_config(args)?;
        run_atomic_scanout_smoke_parent(args, "atomic-scanout-smoke-child")?;
        return Ok(true);
    }

    #[cfg(feature = "atomic-scanout-smoke-live")]
    if args.iter().any(|arg| arg == "atomic-vrr-smoke") {
        if std::env::var_os("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE").is_none() {
            return Err(
                "set SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1 to run destructive VRR smoke".into(),
            );
        }

        let _config = atomic_scanout_smoke_cli_config(args)?;
        run_atomic_scanout_smoke_parent(args, "atomic-vrr-smoke-child")?;
        return Ok(true);
    }

    #[cfg(feature = "atomic-scanout-smoke-live")]
    if args
        .iter()
        .any(|arg| arg == "atomic-scanout-runtime-evidence")
    {
        if std::env::var_os("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE").is_none() {
            return Err("set SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1 to run destructive atomic scanout runtime evidence".into());
        }

        let config = atomic_scanout_smoke_cli_config(args)?;
        let lines =
            sophia_backend_live::run_real_atomic_runtime_rendered_scanout_evidence_with(config);
        for line in &lines {
            println!("{line}");
        }
        if !runtime_rendered_scanout_evidence_is_clean(&lines) {
            return Err(
                "atomic scanout runtime evidence did not capture a clean submit-to-retire frame"
                    .into(),
            );
        }
        return Ok(true);
    }

    #[cfg(feature = "atomic-scanout-smoke-live")]
    if args.iter().any(|arg| arg == "atomic-scanout-smoke-child") {
        if std::env::var_os("SOPHIA_REAL_ATOMIC_SCANOUT_CHILD").is_none() {
            return Ok(true);
        }

        let config = atomic_scanout_smoke_cli_config(args)?;
        for evidence in sophia_backend_live::run_real_atomic_scanout_smoke_phases_with(config) {
            println!("{}", evidence.reduced_log_line());
            if evidence.status != sophia_backend_live::LibdrmNativeAtomicScanoutSmokeStatus::Passed
            {
                return Err(format!(
                    "real atomic scanout smoke failed with status {:?}",
                    evidence.status
                )
                .into());
            }
        }
        return Ok(true);
    }

    #[cfg(feature = "atomic-scanout-smoke-live")]
    if args.iter().any(|arg| arg == "atomic-vrr-smoke-child") {
        if std::env::var_os("SOPHIA_REAL_ATOMIC_SCANOUT_CHILD").is_none() {
            return Ok(true);
        }

        let config = atomic_scanout_smoke_cli_config(args)?;
        let evidence = sophia_backend_live::run_real_atomic_vrr_smoke_phases_with(config);
        for phase in &evidence {
            println!("{}", phase.reduced_log_line());
        }
        let passed = evidence.len() == 2
            && evidence.iter().all(|phase| {
                phase.status == sophia_backend_live::LibdrmNativeAtomicScanoutSmokeStatus::Passed
            });
        print_vrr_hardware_evidence(passed);
        if !passed {
            return Err("real atomic VRR activation/fallback smoke failed".into());
        }
        return Ok(true);
    }

    Ok(false)
}

#[cfg(feature = "native-session")]
fn run_native_session(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.iter().any(|arg| arg.starts_with("--client=")) {
        sophia_session::run_from_args(args)?;
    } else if args.iter().any(|arg| arg == "--proof") {
        run_sophia_live_session_bootstrap(args)?;
    } else if arg_value(args, "--client-backend")
        .as_deref()
        .is_some_and(|backend| backend != "sophia-x")
    {
        return Err("unsupported client backend; expected sophia-x".into());
    } else {
        sophia_session::run_from_args(args)?;
    }
    Ok(())
}

/// Runs startup's output-activation path against real hardware and changes nothing.
///
/// This is the same chain a session runs: read capabilities, project a topology,
/// reconcile the configured candidate, prepare an activation plan, resolve it into
/// heads, and drive the activation phase machine. It differs from startup in two
/// ways worth stating, because they bound what a passing run proves: it opens the
/// cards directly rather than through a seat controller, and it stops at the
/// settlement instead of continuing into a session.
///
/// It is read-only. The executor is the validation one, which has no apply, and
/// bringing the scanout up performs no modeset. It does need DRM master, so no
/// other compositor may hold the card.
/// The desktop profile these topology commands should read.
///
/// They pinned this to `None`, which means the compiled default rather than the
/// operator's file, so they could only ever describe the topology Sophia ships
/// with. That made the tty4 gate unable to test any configured topology at all --
/// including a mirror group, which is the whole point of running it.
///
/// Discovery is the same one the live session uses, so a profile that a session
/// would honour is the profile these commands validate. `--desktop-profile=PATH`
/// overrides it, and `--no-config` forces the compiled default for a run that
/// deliberately wants it.
#[cfg(feature = "native-session")]
fn topology_desktop_profile_source(args: &[String]) -> Result<Option<std::path::PathBuf>, String> {
    if args.iter().any(|arg| arg == "--no-config") {
        return Ok(None);
    }
    let explicit = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--desktop-profile="))
        .map(std::path::PathBuf::from);
    if let Some(path) = explicit.as_ref()
        && !path.is_absolute()
    {
        return Err("--desktop-profile requires an absolute path".to_owned());
    }
    Ok(sophia_config::discover_desktop_profile_source(
        explicit.as_deref(),
        sophia_config::default_user_config_root().as_deref(),
    ))
}

#[cfg(feature = "native-session")]
fn run_native_topology_validation(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use sophia_session::desktop_output_activation::{
        NativeOutputActivationSettlement, run_native_output_activation,
    };
    use sophia_session::desktop_output_commit::NativeOutputTopologyValidationExecutor;
    use sophia_session::desktop_output_heads::{
        LiveNativeOutputTopologyHardware, resolve_native_output_topology_heads,
    };
    use sophia_session::desktop_output_topology::{
        prepare_native_output_activation_plan, project_native_output_topology,
    };

    let profile_source = topology_desktop_profile_source(args)?;
    // Evidence has to say which configuration was tested. A run against the
    // compiled default and a run against the operator's file are different
    // claims, and the log line was unable to tell them apart.
    let profile = profile_source.as_deref().map_or_else(
        || "<compiled>".to_owned(),
        |path| path.display().to_string(),
    );
    let prepared = sophia_config::load_prepared_desktop_profile(
        profile_source.as_deref(),
        sophia_config::ConfigGeneration::INITIAL,
    )?;
    // The card set is built with the grouping the profile asks for, or the command
    // would reconcile a configured group against a desktop that has none.
    let grouping =
        sophia_backend_live::NativeMirrorGrouping::new(prepared.candidates.output.mirror_groups())
            .map_err(|error| format!("configured mirror grouping is invalid: {error:?}"))?;
    let native = sophia_backend_live::LiveProductionNativeScanout::new_with_mirroring(&grouping)?;
    let capabilities = native.output_capabilities()?;
    let topology = project_native_output_topology(&capabilities, &native.outputs())?;
    let profile_source = topology_desktop_profile_source(args)?;
    // Evidence has to say which configuration was tested. A run against the
    // compiled default and a run against the operator's file are different
    // claims, and the log line was unable to tell them apart.
    let prepared = sophia_config::load_prepared_desktop_profile(
        profile_source.as_deref(),
        sophia_config::ConfigGeneration::INITIAL,
    )?;
    let reconciled =
        sophia_config::reconcile_desktop_output_candidate(&prepared.candidates.output, &topology)?;
    let plan = prepare_native_output_activation_plan(&capabilities, &topology, &reconciled)?;
    // Logical outputs, not activation targets. A mirror group is several targets
    // behind one output, so counting targets reported a grouped desktop as an
    // ungrouped one -- which is exactly the claim this line exists to make.
    let outputs = plan
        .targets()
        .iter()
        .map(sophia_session::desktop_output_topology::NativeOutputActivationTarget::output)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let connectors = plan.targets().len();
    let generation = plan.generation().raw();

    let hardware = LiveNativeOutputTopologyHardware::new(&native, &capabilities);
    let resolved = resolve_native_output_topology_heads(&plan, &capabilities, &hardware)
        .map_err(|error| format!("native topology could not be resolved into heads: {error}"))?;
    let heads = resolved.len();
    let card = sophia_session::plan_validation_device(&native, &plan)
        .ok_or("native topology spans more than one DRM device and cannot be validated as one")?;

    let mut executor = NativeOutputTopologyValidationExecutor::new(card, resolved.heads());
    let report = run_native_output_activation(plan, &mut executor)?;
    let validation = executor.validation();
    let settlement = match report.settlement {
        // Activation is unreachable: this executor never applies. Naming it keeps
        // the match honest rather than collapsing two outcomes into one label.
        NativeOutputActivationSettlement::Activated { .. } => "activated",
        NativeOutputActivationSettlement::Rejected { .. } => "not_applied",
    };

    println!(
        "sophia_native_topology_validate schema=2 validation={validation} settlement={settlement} \
outputs={outputs} connectors={connectors} heads={heads} generation={generation} \
profile={profile}"
    );

    if validation != "accepted" {
        return Err(format!("native topology validation did not pass: {validation}").into());
    }
    Ok(())
}

/// Applies the configured topology to real outputs, with rollback.
///
/// This is the first path in the tree that can change what a monitor shows, so what
/// it can reach is bounded deliberately. Apply heads reuse the framebuffer each CRTC
/// already scans out, which means a topology whose scanout size matches what is
/// displayed and nothing else; a mode change needs a buffer allocated at the new
/// size, and this declines rather than guessing. The first useful run is therefore
/// re-applying the topology already on screen, where success looks like nothing
/// happening.
///
/// Rollback heads are resolved before apply runs, from the topology still on screen.
/// Sourcing them afterwards would source them from a desktop that is already wrong.
#[cfg(feature = "native-session")]
fn run_native_topology_apply(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use sophia_backend_live::LiveGbmEglFrameTargetRecord;
    use sophia_session::desktop_output_activation::{
        NativeOutputActivationSettlement, NativeOutputRollbackSettlement,
        run_native_output_activation,
    };
    use sophia_session::desktop_output_commit::{NativeOutputCommitExecutor, NativeOutputHeadSet};
    use sophia_session::desktop_output_frames::{
        NativeOutputFrameTarget, native_output_apply_admission,
    };
    use sophia_session::desktop_output_heads::{
        LiveNativeOutputTopologyHardware, resolve_native_output_scanout_heads,
    };
    use sophia_session::desktop_output_topology::{
        prepare_native_output_activation_plan, project_native_output_topology,
    };

    let profile_source = topology_desktop_profile_source(args)?;
    // Evidence has to say which configuration was tested. A run against the
    // compiled default and a run against the operator's file are different
    // claims, and the log line was unable to tell them apart.
    let profile = profile_source.as_deref().map_or_else(
        || "<compiled>".to_owned(),
        |path| path.display().to_string(),
    );
    let prepared = sophia_config::load_prepared_desktop_profile(
        profile_source.as_deref(),
        sophia_config::ConfigGeneration::INITIAL,
    )?;
    // The card set is built with the grouping the profile asks for, or the command
    // would reconcile a configured group against a desktop that has none.
    let grouping =
        sophia_backend_live::NativeMirrorGrouping::new(prepared.candidates.output.mirror_groups())
            .map_err(|error| format!("configured mirror grouping is invalid: {error:?}"))?;
    let native = sophia_backend_live::LiveProductionNativeScanout::new_with_mirroring(&grouping)?;
    let capabilities = native.output_capabilities()?;
    let topology = project_native_output_topology(&capabilities, &native.outputs())?;
    let profile_source = topology_desktop_profile_source(args)?;
    // Evidence has to say which configuration was tested. A run against the
    // compiled default and a run against the operator's file are different
    // claims, and the log line was unable to tell them apart.
    let prepared = sophia_config::load_prepared_desktop_profile(
        profile_source.as_deref(),
        sophia_config::ConfigGeneration::INITIAL,
    )?;
    let reconciled =
        sophia_config::reconcile_desktop_output_candidate(&prepared.candidates.output, &topology)?;
    let plan = prepare_native_output_activation_plan(&capabilities, &topology, &reconciled)?;
    let generation = plan.generation().raw();

    // Refuse before composing anything if the renderer has not produced a frame at
    // the requested mode. Discovering it while resolving heads would report a
    // missing framebuffer as a property of the hardware, when it is really a
    // statement about where this session is in the mode change.
    // The frame each HEAD is actually scanning out, not the size its mode says it
    // should be. Those differ during a mode change, and the difference is the whole
    // question. A standalone command composes nothing itself, so what is on screen
    // is the only committed frame available to it.
    //
    // Per head rather than per output: a mirror group's connectors run their own
    // modes and own buffers, so checking the group's second head against its
    // primary's frame refused a size difference that is now the ordinary case.
    let frame_targets = native
        .outputs()
        .into_iter()
        .flat_map(|output| {
            native
                .head_indices(output.id)
                .into_iter()
                .map(move |index| (output, index))
        })
        .filter_map(|(output, index)| {
            let selection = native.selection(index);
            let (_, size) = sophia_backend_live::read_native_current_framebuffer(
                native.card(index),
                selection,
            )?;
            Some(NativeOutputFrameTarget {
                connector: capabilities
                    .iter()
                    .find(|capability| capability.connector_id() == selection.connector_id())?
                    .connector_name()
                    .to_owned(),
                output: output.id,
                target: LiveGbmEglFrameTargetRecord::new(size),
            })
        })
        .collect::<Vec<_>>();
    let admission = native_output_apply_admission(&plan, &frame_targets);
    if !admission.is_ready() {
        return Err(format!("native topology is not ready to apply: {admission}").into());
    }

    let hardware = LiveNativeOutputTopologyHardware::new(&native, &capabilities);
    let resolved = resolve_native_output_scanout_heads(&plan, &capabilities, &hardware)
        .map_err(|error| format!("native topology could not be resolved into heads: {error}"))?;
    let heads = resolved.len();
    let card = sophia_session::plan_validation_device(&native, &plan)
        .ok_or("native topology spans more than one DRM device and cannot be applied as one")?;

    let head_set = NativeOutputHeadSet {
        apply: resolved.apply().to_vec(),
        rollback: resolved.rollback().to_vec(),
    };
    let mut executor = NativeOutputCommitExecutor::activating(card, &head_set);
    let report = run_native_output_activation(plan, &mut executor)?;

    let (settlement, rollback) = match report.settlement {
        NativeOutputActivationSettlement::Activated { .. } => ("activated", "none"),
        NativeOutputActivationSettlement::Rejected { rollback, .. } => (
            "not_applied",
            match rollback {
                NativeOutputRollbackSettlement::Failed(_) => "failed",
                NativeOutputRollbackSettlement::NotRequired => "not_required",
                NativeOutputRollbackSettlement::Succeeded => "restored",
            },
        ),
    };

    println!(
        "sophia_native_topology_apply schema=2 settlement={settlement} rollback={rollback} \
heads={heads} generation={generation} profile={profile}"
    );

    if settlement != "activated" {
        return Err(format!(
            "native topology was not applied: settlement={settlement} rollback={rollback}"
        )
        .into());
    }
    Ok(())
}

#[cfg(feature = "native-session")]
fn run_native_egl_vkcube_mixed_smoke_parent(
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE").is_none() {
        return Err(
            "set SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1 to run the native EGL mixed hardware smoke"
                .into(),
        );
    }
    let vkcube = super::x_authority::resolve_external_probe_binary("vkcube", "vkcube")?;
    let display = arg_value(args, "--display").unwrap_or_else(|| ":184".to_owned());
    let runtime_msec = arg_value(args, "--max-runtime-ms").unwrap_or_else(|| "6000".to_owned());
    let terminal = arg_value(args, "--terminal").unwrap_or_else(|| "xterm".to_owned());
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .arg("native-egl-vkcube-mixed-smoke-child")
        .arg(format!("--display={display}"))
        .arg(format!("--terminal={terminal}"))
        .arg("--native-scanout")
        .arg("--secondary-terminal")
        .arg(format!("--terminal-exec={}", vkcube.display()))
        .arg(format!("--max-runtime-ms={runtime_msec}"))
        .arg("--m4-diagnose-first-mixed-export")
        .env("SOPHIA_NATIVE_EGL_MIXED_CHILD", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            println!(
                "sophia_native_egl_mixed schema=1 case=mixed status=child_failed stage=process_exit cpu_layers=0 dmabuf_layers=0 child_outcome={status} live_sources=unknown live_fences=unknown live_transactions=unknown"
            );
            return Err(
                format!("native EGL mixed composition child failed with status {status}").into(),
            );
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            println!(
                "sophia_native_egl_mixed schema=1 case=mixed status=child_failed stage=watchdog cpu_layers=0 dmabuf_layers=0 child_outcome=timeout live_sources=unknown live_fences=unknown live_transactions=unknown"
            );
            return Err("native EGL mixed composition child timed out".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

#[cfg(feature = "native-session")]
fn run_sophia_live_session_bootstrap(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(display) = arg_value(args, "--display") {
        return Err(format!(
            "sophia session run proof mode does not support explicit --display={display} yet; omit --display to use the generated proof display"
        )
        .into());
    }
    let terminal = arg_value(args, "--terminal").unwrap_or_else(|| "xterm".to_owned());
    let terminal_name = std::path::Path::new(&terminal)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(terminal.as_str());
    if terminal_name != "xterm" {
        return Err(format!(
            "sophia session run currently supports --terminal=xterm, got {terminal:?}"
        )
        .into());
    }

    let terminal_proof =
        super::x_authority::collect_x_authority_xterm_render_authority_batches(&terminal)?;
    let pixel_composition = compose_terminal_cpu_buffers(
        &terminal_proof,
        Size {
            width: 1280,
            height: 720,
        },
    )?;
    let keyboard_proof = super::x_authority::run_x_authority_xterm_input_smoke()?;
    let proof_display = terminal_proof.display.clone();
    let proof_requests = terminal_proof.requests;
    let proof_transactions = terminal_proof.transactions;
    let proof_runtime_committed = terminal_proof.runtime_committed;
    let proof_runtime_surfaces = terminal_proof.runtime_surfaces;
    let composition =
        sophia_backend_live::run_live_session_composition_smoke(terminal_proof.authority_batches);

    let status =
        if composition.status == sophia_backend_live::LiveSessionCompositionSmokeStatus::Passed {
            "bootstrap_cpu_pixels_x11_keyboard_ready_native_presentation_pending"
        } else {
            "composition_failed"
        };
    println!(
        "sophia_live_session_bootstrap schema=3 status={} proof_display={} terminal={} authority_requests={} authority_transactions={} authority_runtime_committed={} authority_runtime_surfaces={} cpu_buffers={} cpu_layers={} cpu_nonzero_pixel_bytes={} cpu_checksum={} composition_status={:?} composition_batches={} composition_committed={} composition_surfaces={} keyboard=x11_event_pixel_change_passed keyboard_initial_generation={} keyboard_final_generation={} keyboard_initial_checksum={} keyboard_final_checksum={} physical_input=pending native_presentation=pending persistence=single_client_probe explicit_display=pending",
        status,
        proof_display,
        terminal_name,
        proof_requests,
        proof_transactions,
        proof_runtime_committed,
        proof_runtime_surfaces,
        terminal_proof.cpu_buffers.len(),
        pixel_composition.layers_composed,
        pixel_composition.nonzero_pixel_bytes,
        pixel_composition.checksum,
        composition.status,
        composition.authority_batches_input,
        composition.authority_transactions_committed,
        composition.authority_surfaces_applied,
        keyboard_proof.initial_generation,
        keyboard_proof.final_generation,
        keyboard_proof.initial_checksum,
        keyboard_proof.final_checksum,
    );

    if composition.status != sophia_backend_live::LiveSessionCompositionSmokeStatus::Passed {
        return Err(format!(
            "sophia live session bootstrap composition failed with status {:?}",
            composition.status
        )
        .into());
    }
    if pixel_composition.layers_composed == 0 || pixel_composition.nonzero_pixel_bytes == 0 {
        return Err("sophia live session bootstrap did not compose terminal CPU pixels".into());
    }
    Ok(())
}

#[cfg(feature = "native-session")]
fn compose_terminal_cpu_buffers(
    proof: &super::x_authority::XAuthorityTerminalRenderProof,
    output_size: Size,
) -> Result<sophia_backend_live::LiveCpuCompositionReport, Box<dyn std::error::Error>> {
    let mut buffers = std::collections::BTreeMap::new();
    for buffer in &proof.cpu_buffers {
        let replace =
            buffers
                .get(&buffer.handle)
                .is_none_or(|current: &&XAuthorityCpuBufferSnapshot| {
                    buffer.generation >= current.generation
                });
        if replace {
            buffers.insert(buffer.handle, buffer);
        }
    }

    let mut surfaces = std::collections::BTreeMap::new();
    for batch in &proof.authority_batches {
        for transaction in &batch.transactions {
            let BufferSource::CpuBuffer { handle } = transaction.target_buffer() else {
                continue;
            };
            surfaces.insert(transaction.surface, (transaction.target_geometry, handle));
        }
    }
    let layers = surfaces
        .into_values()
        .filter_map(|(geometry, handle)| {
            let buffer = buffers.get(&handle)?;
            Some(sophia_backend_live::LiveCpuCompositionLayer {
                geometry,
                buffer: sophia_backend_live::LiveCpuBufferSource {
                    handle: buffer.handle,
                    size: buffer.size,
                    stride: buffer.stride,
                    format: buffer.format,
                    generation: buffer.generation,
                    bytes: buffer.bytes.clone(),
                },
            })
        })
        .collect::<Vec<_>>();
    sophia_backend_live::compose_live_cpu_frame(output_size, &layers)
        .map_err(|error| format!("failed to compose terminal CPU pixels: {error:?}").into())
}

#[cfg(feature = "atomic-scanout-smoke-live")]
fn run_sophia_live_session_content_hardware_proof(
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE").is_none() {
        return Err("set SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1 to run destructive terminal-content scanout proof".into());
    }
    let terminal = arg_value(args, "--terminal").unwrap_or_else(|| "xterm".to_owned());
    let output_size = {
        let selection = sophia_backend_live::select_real_atomic_scanout_card();
        selection
            .selection
            .map(|selection| selection.size())
            .ok_or("terminal-content scanout proof could not select a KMS output")?
    };
    let terminal_proof =
        super::x_authority::collect_x_authority_xterm_render_authority_batches(&terminal)?;
    let composition = compose_terminal_cpu_buffers(&terminal_proof, output_size)?;
    if composition.layers_composed == 0 || composition.nonzero_pixel_bytes == 0 {
        return Err("terminal-content scanout proof did not compose nonzero xterm pixels".into());
    }
    let config = atomic_scanout_smoke_cli_config(args)?;
    let evidence =
        sophia_backend_live::run_real_atomic_runtime_rendered_scanout_evidence_with_cpu_frame(
            config,
            composition.frame,
        );
    for line in &evidence.lines {
        println!("{line}");
    }
    let scanout_clean = runtime_rendered_scanout_evidence_is_clean(&evidence.lines);
    let frame_exported = evidence.frame_exported();
    let passed = scanout_clean && frame_exported;
    println!(
        "sophia_live_session_content_scanout schema=1 status={} width={} height={} layers={} nonzero_pixel_bytes={} requested_checksum={} exported_checksum={} export_attempts={} export_status={:?} frame_pending={} scanout_clean={}",
        if passed { "Passed" } else { "Failed" },
        output_size.width,
        output_size.height,
        composition.layers_composed,
        evidence.requested_nonzero_pixel_bytes,
        evidence.requested_checksum,
        evidence.exported_checksum.unwrap_or(0),
        evidence.export_attempts,
        evidence.export_status,
        evidence.frame_pending,
        scanout_clean,
    );
    if !passed {
        return Err(
            "terminal-content scanout proof did not export and retire the composed xterm frame"
                .into(),
        );
    }
    Ok(())
}

#[cfg(feature = "atomic-scanout-smoke-live")]
fn run_atomic_scanout_smoke_parent(
    args: &[String],
    child_command: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let child_timeout = atomic_scanout_smoke_child_timeout(args)?;
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .arg(child_command)
        .env("SOPHIA_REAL_ATOMIC_SCANOUT_CHILD", "1");
    for arg in atomic_scanout_smoke_child_args(args) {
        command.arg(arg);
    }
    let mut child = command.spawn()?;
    let deadline = Instant::now() + child_timeout;

    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            return Err(
                format!("real atomic scanout smoke child failed with status {status}").into(),
            );
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            println!(
                "{}",
                sophia_backend_live::LibdrmNativeAtomicScanoutSmokeEvidence::smoke_child_timeout()
                    .reduced_log_line()
            );
            return Err(
                "real atomic scanout smoke child timed out waiting for page-flip evidence".into(),
            );
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(feature = "atomic-scanout-smoke-live")]
fn print_vrr_hardware_evidence(passed: bool) {
    let status = if passed { "Passed" } else { "Failed" };
    let discovery = if passed { "Discovered" } else { "Unavailable" };
    let commit = if passed { "Presented" } else { "Failed" };
    let retire = if passed {
        "RetiredAfterPageFlip"
    } else {
        "Unproven"
    };
    println!(
        "sophia_vrr_hardware_evidence schema=1 phase=Activation status={status} discovery={discovery} capability=true eligibility=Fullscreen decision=Enabled property_request=true atomic_commit={commit} retire={retire}"
    );
    println!(
        "sophia_vrr_hardware_evidence schema=1 phase=FixedFallback status={status} discovery={discovery} capability=true eligibility=OverlayPresent decision=Ineligible property_request=false atomic_commit={commit} retire={retire}"
    );
}

#[cfg(feature = "atomic-scanout-smoke-live")]
fn atomic_scanout_smoke_cli_config(
    args: &[String],
) -> Result<sophia_backend_live::RealAtomicScanoutSmokeConfig, Box<dyn std::error::Error>> {
    let slot = arg_value(args, "--slot")
        .as_deref()
        .map(parse_u64)
        .transpose()?
        .unwrap_or(1);
    let slot = u16::try_from(slot)
        .map_err(|_| format!("atomic scanout slot {slot} does not fit in u16"))?;
    let output = arg_value(args, "--output")
        .as_deref()
        .map(parse_u64)
        .transpose()?
        .unwrap_or(1);
    let authority = arg_value(args, "--authority")
        .as_deref()
        .map(parse_u64)
        .transpose()?
        .unwrap_or(1);
    let mut wait_policy =
        sophia_backend_live::RealAtomicScanoutPageFlipWaitPolicy::hardware_smoke();
    if let Some(timeout_ms) = arg_value(args, "--page-flip-timeout-ms")
        .as_deref()
        .map(parse_u64)
        .transpose()?
    {
        wait_policy.timeout = Duration::from_millis(timeout_ms);
    }

    sophia_backend_live::RealAtomicScanoutSmokeConfig::from_raw(
        slot,
        output,
        authority,
        wait_policy,
    )
    .ok_or_else(|| {
        format!(
            "invalid atomic scanout smoke config: slot={slot} output={output} authority={authority}"
        )
        .into()
    })
}

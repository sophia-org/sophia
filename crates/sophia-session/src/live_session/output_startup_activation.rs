use super::*;

pub(super) struct StartupOutputActivation {
    pub capabilities: Vec<sophia_backend_live::LibdrmNativeOutputCapability>,
    pub plan: Option<NativeOutputActivationPlan>,
    pub refused: bool,
    pub validation: &'static str,
}

/// A head-to-connector mapping for an accepted startup owner or an adopted
/// replacement. Validation alone must not emit this record for refused heads.
pub(super) fn record_ready_heads(
    capabilities: &[sophia_backend_live::LibdrmNativeOutputCapability],
) -> Result<(), Box<dyn std::error::Error>> {
    for capability in capabilities {
        let mode = capability.selected_mode();
        // The one place the opaque head id is printed beside its connector
        // name: later per-head evidence carries only `head=`, and physical
        // verifiers correlate through this mapping line.
        let head = capability.head().map(|head| head.raw()).ok_or_else(|| {
            format!(
                "native readiness found no head for connector {}",
                capability.connector_name()
            )
        })?;
        crate::session_println!(
            "sophia_live_native_head schema=2 status=ready output={} head={} connector={} connector_id={} mode={}x{} refresh_millihz={} mirrored={}",
            capability.output().raw(),
            head,
            capability.connector_name(),
            capability.connector_id(),
            mode.width,
            mode.height,
            mode.refresh_millihz,
            capabilities
                .iter()
                .filter(|other| other.output() == capability.output())
                .count()
                > 1,
        );
    }
    Ok(())
}

pub(super) fn prepare(
    native: &LiveProductionNativeScanout,
    realization: &sophia_config::DesktopOutputReconciliation,
) -> Result<StartupOutputActivation, Box<dyn std::error::Error>> {
    let capabilities = native.output_capabilities()?;
    let topology = project_native_output_topology(&capabilities, &native.outputs())?;
    let mut reconciled = realization.clone();
    // Discovery includes dark sockets. This activation plan describes only
    // the chosen heads, without reconciling against a smaller inventory.
    reconciled.outputs.retain(|output| output.enabled);
    let activation = prepare_native_output_activation_plan(&capabilities, &topology, &reconciled)?;
    let generation = activation.generation().raw();
    let targets = activation.targets().len();
    let focused = activation.focused_output().is_some();
    // The prepared plan drives the real activation phase machine, and the test
    // phase now reaches hardware: the candidate is resolved into topology heads
    // and submitted as one TEST_ONLY request, so the kernel judges the whole
    // desktop. Startup still performs no KMS mutation, because a validation
    // executor has no apply. What it settles as is now evidence about the
    // topology rather than evidence that nothing was attempted.
    let hardware = LiveNativeOutputTopologyHardware::new(native, &capabilities);
    let resolved = resolve_native_output_topology_heads(&activation, &capabilities, &hardware);
    let (report, executor, validation) = match &resolved {
        Ok(heads) => match plan_validation_device(native, &activation) {
            Some(card) => {
                let mut executor = NativeOutputTopologyValidationExecutor::new(card, heads.heads());
                let report = run_native_output_activation(activation.clone(), &mut executor)?;
                (report, "topology_validation", executor.validation())
            }
            // One atomic request cannot span two DRM devices, so a topology
            // that does is not validatable as a unit and must not be reported
            // as refused.
            None => (
                run_native_output_activation(
                    activation.clone(),
                    &mut UnavailableNativeOutputExecutor,
                )?,
                "multi_device_unvalidatable",
                "not_attempted",
            ),
        },
        Err(error) => {
            tracing::warn!(
                schema = 1,
                %error,
                "native desktop output candidate could not be resolved into heads"
            );
            (
                run_native_output_activation(
                    activation.clone(),
                    &mut UnavailableNativeOutputExecutor,
                )?,
                "unresolved",
                "not_attempted",
            )
        }
    };
    let (status, phase, cause) = match report.settlement {
        NativeOutputActivationSettlement::Activated { .. } => ("applied", "activated", "none"),
        NativeOutputActivationSettlement::Rejected {
            cause, rollback, ..
        } => (
            "prepared_not_applied",
            match rollback {
                NativeOutputRollbackSettlement::Failed(_) => "recovery_failed",
                _ => "rejected",
            },
            match cause {
                NativeOutputActivationFailure::Invalidated => "invalidated",
                NativeOutputActivationFailure::Rejected => "rejected",
                NativeOutputActivationFailure::WouldBlock => "would_block",
                NativeOutputActivationFailure::TimedOut => "timed_out",
                NativeOutputActivationFailure::Disconnected => "disconnected",
            },
        ),
    };
    tracing::info!(
        schema = 1,
        status,
        phase,
        cause,
        executor,
        validation,
        generation,
        outputs = targets,
        rollback_targets = targets,
        focused,
        "native desktop output candidate admitted"
    );
    drop(resolved);
    let refused =
        matches!(validation, "busy" | "rejected" | "unbuildable") || executor == "unresolved";
    Ok(StartupOutputActivation {
        capabilities,
        // An unavailable cross-device test is not an acceptance, but its
        // geometry must still reach the ordinary transactional apply path.
        plan: (!refused).then_some(activation),
        refused,
        validation: if executor == "unresolved" {
            "unresolved"
        } else {
            validation
        },
    })
}

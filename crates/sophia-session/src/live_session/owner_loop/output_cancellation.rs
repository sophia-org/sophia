#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeOutputCancellationRequest {
    AbortPreparation,
    Rollback,
}

/// Cancel through the physical owner before notifying policy. The caller
/// retains the resulting execution phase even when policy notification fails,
/// so completion can finish an already accepted rollback.
fn cancel_output_topology_execution<Native, Policy>(
    phase: &mut LiveOutputTopologyExecutionPhase,
    request_native: Native,
    reject_policy: Policy,
) -> Result<(), Box<dyn std::error::Error>>
where
    Native: FnOnce(
        NativeOutputCancellationRequest,
    ) -> Result<
        Option<sophia_backend_live::LiveProductionNativeTopologyPreparationPhase>,
        Box<dyn std::error::Error>,
    >,
    Policy: FnOnce() -> Result<(), Box<dyn std::error::Error>>,
{
    use LiveOutputTopologyExecutionPhase as Execution;
    use NativeOutputCancellationRequest as Request;
    use sophia_backend_live::LiveProductionNativeTopologyPreparationPhase as NativePhase;

    let request = match *phase {
        // Quiescence cancellation has its own reducer, before native work.
        Execution::WaitingForQuiescence | Execution::RollingBack => return Ok(()),
        Execution::Preparing | Execution::Applying => Request::AbortPreparation,
        Execution::AwaitingFirstPresentation | Execution::Reconciling => Request::Rollback,
    };
    let native_phase = request_native(request)?;
    match *phase {
        Execution::Preparing => {
            // The preparation service drains resources and reports its
            // terminal failure before policy settles the candidate.
            Ok(())
        }
        Execution::Applying => match native_phase {
            Some(NativePhase::RollingBack) => {
                *phase = Execution::RollingBack;
                reject_policy()
            }
            Some(NativePhase::Failed) => {
                // No card was mutated. Reuse preparation's resource cleanup
                // and terminal path, without claiming rollback has completed.
                *phase = Execution::Preparing;
                Ok(())
            }
            phase => Err(format!(
                "output cancellation produced an invalid native phase: {phase:?}"
            )
            .into()),
        },
        Execution::AwaitingFirstPresentation | Execution::Reconciling => {
            *phase = Execution::RollingBack;
            reject_policy()
        }
        Execution::WaitingForQuiescence | Execution::RollingBack => unreachable!(),
    }
}

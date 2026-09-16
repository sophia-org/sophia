// Borrow the real policy owner's admission state. Tests may provide committed
// publication facts without constructing a process supervisor; publication
// validation, transaction minting and bounded queue admission remain shared.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LiveIndicatorAdmissionResult {
    admission: LiveWmRequestAdmission,
    // Minted by this exact queue attempt; absence means no serial was minted.
    activation_serial: Option<u64>,
    policy_connection_epoch: u64,
}

impl LiveIndicatorAdmissionResult {
    fn unavailable() -> Self {
        Self {
            admission: LiveWmRequestAdmission::Duplicate,
            activation_serial: None,
            policy_connection_epoch: 0,
        }
    }
}

struct LiveIndicatorAdmission<'a> {
    policy_connection_epoch: u64,
    publication: &'a sophia_engine::PolicyIndicatorPublication,
    outputs: &'a [sophia_engine::HeadlessOutput],
    output_generations: &'a BTreeMap<sophia_protocol::OutputId, u64>,
    capabilities: u64,
    next_transaction: &'a mut u64,
    queue: &'a mut VecDeque<LivePublicPolicyCause>,
    in_flight_source: Option<LiveWmProposalSource>,
    in_flight: bool,
}

fn mint_public_policy_transaction(
    next: &mut u64,
) -> Result<TransactionId, Box<dyn std::error::Error>> {
    let transaction = TransactionId::from_raw(*next);
    *next = next
        .checked_add(1)
        .ok_or("public WM transaction identity exhausted")?;
    Ok(transaction)
}

impl LiveIndicatorAdmission<'_> {
    fn enqueue(
        &mut self,
        action: WmActionId,
        output: sophia_protocol::OutputId,
    ) -> Result<LiveIndicatorAdmissionResult, Box<dyn std::error::Error>> {
        if self.capabilities & sophia_protocol::SOPHIA_WM_CAPABILITY_OUTPUT_ACTIONS == 0
            || !self
                .publication
                .indicators
                .iter()
                .any(|indicator| indicator.output == output && indicator.action == Some(action))
        {
            return Ok(LiveIndicatorAdmissionResult::unavailable());
        }
        let Some(output_generation) = self
            .output_generations
            .get(&output)
            .copied()
            .filter(|g| *g != 0)
        else {
            return Ok(LiveIndicatorAdmissionResult::unavailable());
        };
        if !self.outputs.iter().any(|o| o.id == output) {
            return Ok(LiveIndicatorAdmissionResult::unavailable());
        }
        let activation_serial = mint_public_policy_transaction(self.next_transaction)?.raw();
        let mut affected_outputs = self
            .outputs
            .iter()
            .map(|output| output.id)
            .collect::<Vec<_>>();
        affected_outputs.sort_by_key(|output| output.raw());
        let admission = enqueue_public_policy_cause(
            self.queue,
            self.in_flight_source,
            self.in_flight,
            LivePublicPolicyCause {
                source: LiveWmProposalSource::Action(action),
                cause: sophia_protocol::PolicyRequestCause::OutputAction {
                    activation_serial,
                    action,
                    output,
                    output_generation,
                },
                affected_outputs,
            },
        );
        Ok(LiveIndicatorAdmissionResult {
            admission,
            activation_serial: Some(activation_serial),
            policy_connection_epoch: self.policy_connection_epoch,
        })
    }
}

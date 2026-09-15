// Borrow the real policy owner's admission state. Tests may provide committed
// publication facts without constructing a process supervisor; publication
// validation, transaction minting and bounded queue admission remain shared.
struct LiveIndicatorAdmission<'a> {
    publication: &'a sophia_engine::PolicyIndicatorPublication,
    outputs: &'a [sophia_engine::HeadlessOutput],
    active_output: sophia_protocol::OutputId,
    next_transaction: &'a mut u64,
    queue: &'a mut VecDeque<LivePublicPolicyCause>,
    in_flight_source: Option<LiveWmProposalSource>,
    in_flight: bool,
}

fn mint_public_policy_transaction(next: &mut u64) -> Result<TransactionId, Box<dyn std::error::Error>> {
    let transaction = TransactionId::from_raw(*next);
    *next = next.checked_add(1).ok_or("public WM transaction identity exhausted")?;
    Ok(transaction)
}

impl LiveIndicatorAdmission<'_> {
    fn enqueue(
        &mut self,
        action: WmActionId,
        output: sophia_protocol::OutputId,
    ) -> Result<LiveWmRequestAdmission, Box<dyn std::error::Error>> {
        if !self.publication.indicators.iter().any(|indicator| {
            indicator.output == output && indicator.action == Some(action)
        }) {
            return Ok(LiveWmRequestAdmission::Duplicate);
        }
        let activation_serial = mint_public_policy_transaction(self.next_transaction)?.raw();
        let mut affected_outputs = self.outputs.iter().map(|output| output.id).collect::<Vec<_>>();
        affected_outputs.sort_by_key(|output| output.raw());
        if let Some(index) = affected_outputs.iter().position(|output| *output == self.active_output) {
            affected_outputs.swap(0, index);
        }
        Ok(enqueue_public_policy_cause(
            self.queue, self.in_flight_source, self.in_flight,
            LivePublicPolicyCause {
                source: LiveWmProposalSource::Action(action),
                cause: sophia_protocol::PolicyRequestCause::Action { activation_serial, action },
                affected_outputs,
            },
        ))
    }
}

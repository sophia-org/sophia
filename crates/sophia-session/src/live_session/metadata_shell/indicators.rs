use super::LiveMetadataShell;
use sophia_protocol::{
    OutputId, ShellIndicator, ShellIndicatorSnapshot, ShellOutputStatus,
    encode_shell_indicator_snapshot,
};

impl LiveMetadataShell {
    /// Republish the indicator set the Engine already holds, plus which output
    /// the user is actually on.
    ///
    /// The active output is carried explicitly because it cannot be inferred
    /// from the indicators: an output that is focused while holding no window
    /// contributes a status and no entries, and that is exactly the case a bar
    /// has to show. It is sourced from committed public policy rather than the
    /// startup activation plan, which records a topology fact rather than live
    /// focus.
    pub(in crate::live_session) fn service_indicators(
        &mut self,
        publication: Option<&sophia_engine::PolicyIndicatorPublication>,
        active_output: Option<OutputId>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.connected || !self.transport.supports_indicators() {
            return Ok(());
        }
        let Some(publication) = publication else {
            return Ok(());
        };
        self.transport.poll_io()?;

        let snapshot = indicator_snapshot(
            publication,
            active_output,
            self.transport.connection_epoch(),
        );

        // Republishing an unchanged set would wake a shell for nothing on every
        // committed frame.
        if self.indicators.last_published.as_ref() == Some(&snapshot) {
            return Ok(());
        }

        let tx = self.take_transaction()?;
        let frames = encode_shell_indicator_snapshot(tx, &snapshot)
            .map_err(sophia_runtime::ShellTransportError::Codec)?;
        for frame in frames {
            self.transport.send_async(frame)?;
        }
        self.indicators.last_published = Some(snapshot);
        // Evidence of the exact enqueued publication, not peer consumption.
        // Repeat publications may share a generation when only focus moves;
        // the indicator identities and state must still agree for that revision.
        let published = self
            .indicators
            .last_published
            .as_ref()
            .expect("just retained");
        for indicator in &published.indicators {
            crate::session_println!(
                "sophia_shell_indicator_state schema=1 connection_epoch={} indicator_generation={} output={} indicator={} action={} slot={} state_bits={} entries={}",
                published.connection_epoch,
                published.generation,
                indicator.output.raw(),
                indicator.indicator,
                indicator.action,
                indicator.slot,
                indicator.state_bits,
                published.indicators.len(),
            );
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(in crate::live_session) enum IndicatorServiceError {
    Poll(Box<dyn std::error::Error>),
    Completion(Box<dyn std::error::Error>),
}

impl LiveMetadataShell {
    pub(in crate::live_session) fn service_indicator_activation(
        &mut self,
        admit: impl FnOnce(
            sophia_protocol::WmActionId,
            OutputId,
        ) -> Result<
            crate::live_session::LiveIndicatorAdmissionResult,
            Box<dyn std::error::Error>,
        >,
    ) -> Result<bool, IndicatorServiceError> {
        if !self.connected || !self.transport.supports_indicator_activation() {
            return Ok(false);
        }
        self.content.service_indicator_request(
            &mut self.transport.connection(),
            &mut self.indicators,
            admit,
        )
    }
}

impl LiveIndicatorState {
    pub(in crate::live_session) fn poll_request(
        &mut self,
        transport: &mut sophia_runtime::ShellTransportConnection<'_>,
        input_enabled: bool,
    ) -> Result<Option<LiveIndicatorActivationRequest>, Box<dyn std::error::Error>> {
        let Some((transaction, activation)) = transport.poll_indicator_activation()? else {
            return Ok(None);
        };
        let mut status = classify_indicator_activation(self.last_published.as_ref(), &activation);
        if status == sophia_protocol::ShellIndicatorActivationStatus::Accepted && !input_enabled {
            if activation.event_id <= self.direct_event_high_water {
                status = sophia_protocol::ShellIndicatorActivationStatus::Stale;
            } else {
                self.direct_event_high_water = activation.event_id;
            }
        }
        Ok(Some(LiveIndicatorActivationRequest {
            transaction,
            activation,
            status,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::live_session) struct LiveIndicatorActivationRequest {
    pub(in crate::live_session) transaction: sophia_protocol::TransactionId,
    pub(in crate::live_session) activation: sophia_protocol::ShellIndicatorActivation,
    pub(in crate::live_session) status: sophia_protocol::ShellIndicatorActivationStatus,
}

/// Project a policy indicator publication onto the wire snapshot.
///
/// Extracted from the transport so the conformance host publishes through the
/// same code the session does. A host that built its own snapshot would be
/// re-implementing exactly the mapping most worth checking, and agreement would
/// then prove only that two encoders match each other.
pub fn indicator_snapshot(
    publication: &sophia_engine::PolicyIndicatorPublication,
    active_output: Option<OutputId>,
    connection_epoch: u64,
) -> ShellIndicatorSnapshot {
    ShellIndicatorSnapshot {
        connection_epoch,
        generation: publication.generation,
        active_output,
        statuses: publication
            .output_statuses
            .iter()
            .map(|status| ShellOutputStatus {
                output: status.output,
                focus_bits: status.focus_bits,
                layout: status.layout.clone(),
            })
            .collect(),
        indicators: publication
            .indicators
            .iter()
            .map(|indicator| ShellIndicator {
                output: indicator.output,
                indicator: indicator.indicator,
                // Identities are allocated from one, so zero is free to mean
                // "not activatable" and can never collide with a real action.
                action: indicator.action.map_or(0, sophia_protocol::WmActionId::raw),
                slot: indicator.slot,
                state_bits: indicator.state_bits,
                label: indicator.label.clone(),
            })
            .collect(),
    }
}

/// Decide what an activation is worth against the set the shell was last sent.
///
/// Split out from the transport so it can be exercised directly: these are the
/// paths that decide whether a click reaches policy, and they should not be
/// reachable only through a socket.
pub(in crate::live_session) fn classify_indicator_activation(
    published: Option<&ShellIndicatorSnapshot>,
    activation: &sophia_protocol::ShellIndicatorActivation,
) -> sophia_protocol::ShellIndicatorActivationStatus {
    use sophia_protocol::ShellIndicatorActivationStatus as Status;
    let Some(snapshot) = published else {
        return Status::Stale;
    };
    // The shell must have been looking at the same screen the session was. A
    // pill clicked against a replaced set points at a view that may have moved.
    if snapshot.connection_epoch != activation.connection_epoch
        || snapshot.generation != activation.snapshot_generation
    {
        return Status::Stale;
    }
    let Some(indicator) = snapshot.indicators.iter().find(|indicator| {
        indicator.output == activation.output
            && indicator.indicator == activation.indicator
            && indicator.action == activation.action
    }) else {
        return Status::Unknown;
    };
    // A pill published without an action is not activatable, and zero is how
    // that is spelled. Honouring it would invent authority the publication
    // never granted.
    if indicator.action == 0 {
        return Status::Unauthorized;
    }
    Status::Accepted
}

#[derive(Default)]
pub(in crate::live_session) struct LiveIndicatorState {
    pub(in crate::live_session) last_published: Option<ShellIndicatorSnapshot>,
    direct_event_high_water: u64,
}

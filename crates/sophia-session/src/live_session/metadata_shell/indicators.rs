use super::LiveMetadataShell;
use crate::shell_indicator_projection::indicator_snapshot;
use sophia_protocol::{OutputId, ShellIndicatorSnapshot, encode_shell_indicator_snapshot};

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
        Ok(())
    }
}

impl LiveMetadataShell {
    /// Take one activation the shell sent, if it is still meaningful.
    ///
    /// The shell must name the snapshot it was presenting. A pill clicked
    /// against a set that has since been replaced refers to a view that may
    /// have moved, so it is answered stale rather than applied late. The
    /// action itself is validated again by the WM against what was published;
    /// this check only establishes that the shell was looking at the same
    /// screen the session was.
    pub(in crate::live_session) fn take_indicator_activation(
        &mut self,
    ) -> Result<
        Option<(sophia_protocol::OutputId, sophia_protocol::WmActionId)>,
        Box<dyn std::error::Error>,
    > {
        if !self.connected || !self.transport.supports_indicator_activation() {
            return Ok(None);
        }
        let Some(frame) = self
            .transport
            .poll_kind(sophia_protocol::IpcMessageKind::ShellIndicatorActivate)?
        else {
            return Ok(None);
        };
        let (tx, activation) = sophia_protocol::decode_shell_indicator_activation(&frame)
            .map_err(sophia_runtime::ShellTransportError::Codec)?;

        let status =
            classify_indicator_activation(self.indicators.last_published.as_ref(), &activation);

        let outcome = sophia_protocol::ShellIndicatorActivationOutcome {
            connection_epoch: self.transport.connection_epoch(),
            snapshot_generation: activation.snapshot_generation,
            event_id: activation.event_id,
            status,
            reason: 0,
        };
        self.transport.send_async(
            sophia_protocol::encode_shell_indicator_activation_outcome(tx, &outcome)
                .map_err(sophia_runtime::ShellTransportError::Codec)?,
        )?;

        if status == sophia_protocol::ShellIndicatorActivationStatus::Accepted {
            return Ok(Some((
                activation.output,
                sophia_protocol::WmActionId::from_raw(activation.action),
            )));
        }
        Ok(None)
    }
}

/// Project a policy indicator publication onto the wire snapshot.
///
/// Extracted from the transport so the conformance host publishes through the
/// same code the session does. A host that built its own snapshot would be
/// re-implementing exactly the mapping most worth checking, and agreement would
/// then prove only that two encoders match each other.

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
}

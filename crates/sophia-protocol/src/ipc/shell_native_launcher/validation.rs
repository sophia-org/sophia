use super::SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES;
use super::records::*;
use crate::{
    ContentAllocationId, ContentGrant, ContentMargins, ContentOutputId, IpcCodecError,
    SOPHIA_SHELL_MAX_APPLICATIONS, SOPHIA_SHELL_MAX_LAUNCHER_ROWS, ShellContentRecord,
};

fn require(ok: bool, field: &'static str) -> Result<(), IpcCodecError> {
    if ok {
        Ok(())
    } else {
        Err(IpcCodecError::InvalidRecord(field))
    }
}
fn grant(v: ContentGrant) -> Result<(), IpcCodecError> {
    require(
        v.connection_epoch > 0 && v.content_grant_epoch > 0,
        "native launcher grant",
    )
}
fn output(v: ContentOutputId) -> Result<(), IpcCodecError> {
    require(v.id > 0 && v.generation > 0, "native launcher output")
}
fn binding(v: &NativeLauncherBinding) -> Result<(), IpcCodecError> {
    grant(v.grant)?;
    output(v.output)?;
    require(
        [
            v.opening,
            v.allocation.id,
            v.allocation.generation,
            v.catalog_generation,
            v.candidate_generation,
            v.presentation_epoch,
            v.interaction_generation,
            v.state_revision,
            v.focus_lease,
        ]
        .into_iter()
        .all(|v| v > 0),
        "native launcher binding",
    )
}
fn event(v: &NativeLauncherEvent) -> Result<(), IpcCodecError> {
    binding(&v.binding)?;
    require(
        v.event_id > 0 && v.state_revision >= v.binding.state_revision,
        "native launcher event",
    )
}
fn activation(v: &NativeLauncherActivation) -> Result<(), IpcCodecError> {
    event(&v.event)?;
    require(
        (1..=2).contains(&v.cause)
            && v.slot > 0
            && usize::from(v.slot) <= SOPHIA_SHELL_MAX_APPLICATIONS
            && v.event.state_revision == v.event.binding.state_revision,
        "native launcher activation",
    )
}
fn reason(v: u16) -> Result<(), IpcCodecError> {
    require((1..=12).contains(&v), "native launcher reason")
}

pub(super) fn validate(record: &ShellNativeLauncherRecord) -> Result<(), IpcCodecError> {
    use ShellNativeLauncherRecord::*;
    match record {
        Opening(v) => {
            grant(v.grant)?;
            output(v.output)?;
            require(
                v.opening > 0 && v.catalog_generation > 0 && v.state_revision == 1,
                "native launcher opening",
            )
        }
        AllocationRequest(v) => {
            grant(v.grant)?;
            output(v.output)?;
            require(
                v.opening > 0
                    && v.request_id > 0
                    && (1..=3).contains(&v.operation)
                    && (1..=4).contains(&v.edge),
                "native launcher allocation",
            )?;
            require(
                if v.operation == 1 {
                    v.prior == ContentAllocationId::default()
                } else {
                    v.prior.id > 0 && v.prior.generation > 0
                },
                "native launcher allocation prior",
            )?;
            require(
                [
                    v.margins.top,
                    v.margins.right,
                    v.margins.bottom,
                    v.margins.left,
                ]
                .into_iter()
                .all(|v| (-512..=512).contains(&v)),
                "native launcher margins",
            )?;
            require(
                if v.operation == 3 {
                    v.desired_width == 0
                        && v.desired_height == 0
                        && v.margins == ContentMargins::default()
                } else {
                    v.desired_width > 0 && v.desired_height > 0
                },
                "native launcher allocation dimensions",
            )
        }
        CandidateBegin(v) => {
            crate::ipc::shell_content::validation::validate(&ShellContentRecord::CandidateBegin(
                v.content.clone(),
            ))?;
            require(
                v.opening > 0
                    && v.catalog_generation > 0
                    && v.state_revision > 0
                    && v.content.surface_count == 1
                    && v.content.placement_count > 0
                    && v.content.target_count as usize == v.rows.len()
                    && v.rows.len() <= SOPHIA_SHELL_MAX_LAUNCHER_ROWS,
                "native launcher candidate",
            )?;
            require(
                v.rows.iter().enumerate().all(|(i, slot)| {
                    *slot > 0
                        && usize::from(*slot) <= SOPHIA_SHELL_MAX_APPLICATIONS
                        && !v.rows[..i].contains(slot)
                }),
                "native launcher catalog rows",
            )?;
            require(
                if v.rows.is_empty() {
                    v.selected == 0
                } else {
                    v.rows.contains(&v.selected)
                },
                "native launcher selection",
            )
        }
        CandidateChunk(v) => {
            crate::ipc::shell_content::validation::validate_candidate_chunk(v, true)
        }
        Focus(v) => binding(v),
        FocusRevoked(v) => {
            binding(&v.binding)?;
            reason(v.reason)
        }
        Input(v) => {
            event(&v.event)?;
            require(v.issued_mono_usec > 0, "native launcher issuance")?;
            require(
                if v.kind == NativeLauncherInputKind::Accept {
                    v.event.state_revision == v.event.binding.state_revision
                } else {
                    v.event.state_revision > v.event.binding.state_revision
                },
                "native launcher input revision",
            )?;
            require(
                if v.kind == NativeLauncherInputKind::Text {
                    !v.text.is_empty()
                        && crate::shell_launcher_text_valid(
                            &v.text,
                            SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES,
                        )
                } else {
                    v.text.is_empty()
                },
                "native launcher text",
            )
        }
        InputAck(v) => {
            event(&v.event)?;
            require(
                (1..=2).contains(&v.disposition),
                "native launcher acknowledgement",
            )
        }
        Activate(v) => activation(v),
        ActivationOutcome(v) => {
            activation(&v.activation)?;
            require(
                (1..=5).contains(&v.status) && v.reason <= 12 && (v.status == 1) == (v.reason == 0),
                "native launcher activation outcome",
            )
        }
        Closed(v) => {
            grant(v.grant)?;
            require(v.opening > 0, "native launcher closed opening")?;
            reason(v.reason)
        }
    }
}

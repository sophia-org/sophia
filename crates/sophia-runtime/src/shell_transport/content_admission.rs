use sophia_protocol::{
    ContentAdmissionRefused, ContentGrant, ContentResourceId,
    SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT, SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
    SOPHIA_SHELL_CONTENT_REVISION, ShellContentRecord,
};

pub(super) const PERMISSION_DENIED: u16 = 1;
pub(super) const UNAVAILABLE: u16 = 4;

/// Operator and implementation decision for the revision-5 content workflow.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ShellContentAdmissionPolicy {
    /// The host has no complete content service. This is the production default
    /// until the owner loop and renderer lifecycle are wired end to end.
    #[default]
    Unavailable,
    /// The implementation exists but the effective operator profile denies it.
    Denied,
    /// CPU content is admitted. Discrete input remains an independent grant.
    Granted { discrete_input: bool },
}

pub(super) fn record_grant(record: &ShellContentRecord) -> Option<ContentGrant> {
    match record {
        ShellContentRecord::AdmissionRefused(_) => None,
        ShellContentRecord::Limits(value) => Some(value.grant),
        ShellContentRecord::OutputFacts(value) => Some(value.grant),
        ShellContentRecord::AllocationRequest(value) => Some(value.grant),
        ShellContentRecord::AllocationResult(value) => Some(value.grant),
        ShellContentRecord::ResourceBegin(value) => Some(value.grant),
        ShellContentRecord::ResourceStatus(value) => Some(value.grant),
        ShellContentRecord::ResourceChunk(value) => Some(value.grant),
        ShellContentRecord::ResourceEnd(value) => Some(value.grant),
        ShellContentRecord::ResourceCancel(value) => Some(value.grant),
        ShellContentRecord::ResourceRetire(value) => Some(value.grant),
        ShellContentRecord::ResourceReleased(value) => Some(value.grant),
        ShellContentRecord::CandidateBegin(value) => Some(value.grant),
        ShellContentRecord::CandidateChunk(value) => Some(value.grant),
        ShellContentRecord::CandidateEnd(value) => Some(value.grant),
        ShellContentRecord::CandidateOutcome(value) => Some(value.grant),
        ShellContentRecord::FrameDemand(value) => Some(value.grant),
        ShellContentRecord::FramePermit(value) => Some(value.grant),
        ShellContentRecord::FrameDemandCancel(value) => Some(value.grant),
        ShellContentRecord::Action(value) => Some(value.grant),
        ShellContentRecord::ActionAck(value) => Some(value.grant),
    }
}

pub(super) fn resource_identity(record: &ShellContentRecord) -> Option<ContentResourceId> {
    match record {
        ShellContentRecord::ResourceBegin(value) => Some(value.resource),
        ShellContentRecord::ResourceChunk(value) => Some(value.resource),
        ShellContentRecord::ResourceEnd(value) => Some(value.resource),
        ShellContentRecord::ResourceCancel(value) => Some(value.resource),
        ShellContentRecord::ResourceRetire(value) => Some(value.resource),
        _ => None,
    }
}

pub(super) fn client_record(record: &ShellContentRecord) -> bool {
    matches!(
        record,
        ShellContentRecord::AllocationRequest(_)
            | ShellContentRecord::ResourceBegin(_)
            | ShellContentRecord::ResourceChunk(_)
            | ShellContentRecord::ResourceEnd(_)
            | ShellContentRecord::ResourceCancel(_)
            | ShellContentRecord::ResourceRetire(_)
            | ShellContentRecord::CandidateBegin(_)
            | ShellContentRecord::CandidateChunk(_)
            | ShellContentRecord::CandidateEnd(_)
            | ShellContentRecord::FrameDemand(_)
            | ShellContentRecord::FrameDemandCancel(_)
            | ShellContentRecord::ActionAck(_)
    )
}

pub(super) fn server_record(record: &ShellContentRecord) -> bool {
    matches!(
        record,
        ShellContentRecord::OutputFacts(_)
            | ShellContentRecord::AllocationResult(_)
            | ShellContentRecord::ResourceStatus(_)
            | ShellContentRecord::ResourceReleased(_)
            | ShellContentRecord::CandidateOutcome(_)
            | ShellContentRecord::FramePermit(_)
            | ShellContentRecord::Action(_)
    )
}

pub(super) enum ContentAdmissionDecision {
    NotRequested,
    Granted(u64),
    Refused(ContentAdmissionRefused),
}

pub(super) fn decide(
    revision: u16,
    requested: u64,
    policy: ShellContentAdmissionPolicy,
) -> ContentAdmissionDecision {
    if requested == 0 || revision < SOPHIA_SHELL_CONTENT_REVISION {
        return ContentAdmissionDecision::NotRequested;
    }
    let refused = |reason, denied_capabilities| {
        ContentAdmissionDecision::Refused(ContentAdmissionRefused {
            reason,
            denied_capabilities,
        })
    };
    match policy {
        ShellContentAdmissionPolicy::Unavailable => refused(UNAVAILABLE, requested),
        ShellContentAdmissionPolicy::Denied => refused(PERMISSION_DENIED, requested),
        ShellContentAdmissionPolicy::Granted { discrete_input } => {
            let denied_input =
                requested & SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT != 0 && !discrete_input;
            if denied_input {
                refused(
                    PERMISSION_DENIED,
                    SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT,
                )
            } else {
                ContentAdmissionDecision::Granted(
                    SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                        | (requested & SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT),
                )
            }
        }
    }
}

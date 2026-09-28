// The desktop SDK owns these passive values; keep the historical facade.
pub use sophia_shell_protocol::shell::descriptor::{
    SOPHIA_SHELL_MAX_DESCRIPTORS, SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS,
    SOPHIA_SHELL_MAX_RESERVATION_THICKNESS_PX, ShellV1Activation, ShellV1ActivationAck,
    ShellV1ActivationDisposition, ShellV1Candidate, ShellV1CandidateEntry, ShellV1CandidateOutcome,
    ShellV1CandidateOutcomeKind, ShellV1Descriptor, ShellV1DescriptorSnapshot,
    ShellV1ReservationEdge, ShellV1WorkAreaReservation, ToplevelActionCapabilityRef,
};

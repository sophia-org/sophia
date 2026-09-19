//! Approved cause codes for a refused shell-component start.
//!
//! A start failure reports `reason={error}`, which is free text that evidence
//! reduction drops -- correctly, since arbitrary error text must not be
//! retained. The consequence was a session holding 841 identical records that
//! named neither the component nor why it refused, and a cause that had to be
//! recovered by changing the configuration and restarting rather than by
//! reading the log.
//!
//! These codes close that gap without retaining free text. They classify the
//! bounded set of refusals the start path can raise, so the retained record
//! distinguishes a GPU identity change from a missing protection domain from a
//! launch specification a profile got wrong.
//!
//! Classification is by message because the errors arrive as `Box<dyn Error>`
//! from several layers, and a typed cause would have to be threaded through
//! every one of them. The accompanying test pins every literal the start path
//! can produce, so an error added without a code fails rather than silently
//! reporting `other`.

/// Why a component start was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartCause {
    /// Presentation is unavailable, or a previous attempt's cleanup is still
    /// retained. The slot is not startable yet.
    Presentation,
    /// A prior attempt on this slot is still owned and was not revoked.
    AttemptOwned,
    /// The selection itself is unusable -- an unknown slot, or a role set the
    /// session cannot hold.
    Selection,
    /// The launch specification a profile produced is invalid: a relative
    /// executable, an absent allowance, a zero grant.
    LaunchSpec,
    /// The GPU grant could not be constituted: no admitted render device, a
    /// zero epoch, or a device carried against a denied policy.
    GpuGrant,
    /// The render node's identity did not survive revalidation between
    /// admission and launch.
    GpuIdentity,
    /// The protected domain or its bounded sysfs projection could not be built.
    GpuDomain,
    /// Launch evidence was missing, stale, or unconnected.
    Evidence,
    /// Anything the codes above do not name. A record carrying this is a gap
    /// in this table, not a kind of failure.
    Other,
}

impl StartCause {
    /// The retained token. Reduction admits exactly these values.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Presentation => "presentation",
            Self::AttemptOwned => "attempt_owned",
            Self::Selection => "selection",
            Self::LaunchSpec => "launch_spec",
            Self::GpuGrant => "gpu_grant",
            Self::GpuIdentity => "gpu_identity",
            Self::GpuDomain => "gpu_domain",
            Self::Evidence => "evidence",
            Self::Other => "other",
        }
    }

    /// Every admitted token, for the reduction allowlist and its test.
    pub(crate) const ALL: [&'static str; 9] = [
        "presentation",
        "attempt_owned",
        "selection",
        "launch_spec",
        "gpu_grant",
        "gpu_identity",
        "gpu_domain",
        "evidence",
        "other",
    ];
}

/// Classify a refusal the component start path raised.
///
/// Matching is on the distinctive part of each message rather than the whole,
/// so a message that gains a trailing detail keeps its code.
pub(crate) fn classify(message: &str) -> StartCause {
    // Identity is checked before the domain is built, so its more specific
    // phrases are tested first.
    if message.contains("render node identity changed")
        || message.contains("render node physical device changed")
        || message.contains("render node physical identity")
    {
        return StartCause::GpuIdentity;
    }
    if message.contains("requires a protection domain")
        || message.contains("protected filesystem")
        || message.contains("sysfs")
    {
        return StartCause::GpuDomain;
    }
    if message.contains("has no admitted render device")
        || message.contains("GPU grant epoch")
        || message.contains("denied shell GPU policy carried a device")
        || message.contains("requires native client rendering")
    {
        return StartCause::GpuGrant;
    }
    if message.contains("launch evidence") {
        return StartCause::Evidence;
    }
    if message.contains("must be absolute")
        || message.contains("positive panel allowance")
        || message.contains("reserved nonzero grant")
        || message.contains("explicit positive reservation")
        || message.contains("absolute parent")
        || message.contains("allowance absent")
    {
        return StartCause::LaunchSpec;
    }
    if message.contains("presentation unavailable") {
        return StartCause::Presentation;
    }
    if message.contains("attempt still owned") {
        return StartCause::AttemptOwned;
    }
    if message.contains("unknown component selection") || message.contains("selected roles") {
        return StartCause::Selection;
    }
    StartCause::Other
}

mod tests;

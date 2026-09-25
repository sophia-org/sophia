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
    /// A fresh content grant cannot fit its useful floor or epoch capacity.
    ContentBudget,
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
    /// The process layer refused the attempt: a slot still owned by a previous
    /// process, an unknown component, or a launch that could not be protected.
    ///
    /// Distinct from `Presentation`, which is the session declining to start
    /// anything, and from `LaunchSpec`, which is a specification a profile got
    /// wrong. This is the layer that owns the child refusing to make one.
    Process,
    /// Anything the codes above do not name. A record carrying this is a gap
    /// in this table, not a kind of failure.
    Other,
}

impl StartCause {
    /// The retained token. Reduction admits exactly these values.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ContentBudget => "content_budget",
            Self::Presentation => "presentation",
            Self::AttemptOwned => "attempt_owned",
            Self::Selection => "selection",
            Self::LaunchSpec => "launch_spec",
            Self::GpuGrant => "gpu_grant",
            Self::GpuIdentity => "gpu_identity",
            Self::GpuDomain => "gpu_domain",
            Self::Evidence => "evidence",
            Self::Process => "process",
            Self::Other => "other",
        }
    }

    /// Every admitted token, for the reduction allowlist and its test.
    pub(crate) const ALL: [&'static str; 11] = [
        "content_budget",
        "presentation",
        "attempt_owned",
        "selection",
        "launch_spec",
        "gpu_grant",
        "gpu_identity",
        "gpu_domain",
        "evidence",
        "process",
        "other",
    ];
}

/// Classify a refusal the component start path raised.
///
/// Matching is on the distinctive part of each message rather than the whole,
/// so a message that gains a trailing detail keeps its code.
pub(crate) fn classify(message: &str) -> StartCause {
    if message.contains("ContentStore(Budget)") {
        return StartCause::ContentBudget;
    }
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
        // The three the coordinator itself raises when it cannot name one
        // device for a direct grant. These reach the record now that a
        // component declaring direct access is refused at prepare rather
        // than failing session start.
        || message.contains("active render device is unavailable")
        || message.contains("absent from the admitted inventory")
        || message.contains("ambiguous active render device")
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
    // After the selection test, deliberately. "unknown component selection" is
    // a selection the session cannot hold; the bare "unknown component" here is
    // the process layer not recognising a slot. The longer phrase contains the
    // shorter one, so testing this first would swallow it.
    if message.contains("component process busy or unknown")
        || message.contains("requires protected launch")
        || message.contains("component protection evidence")
        || message.contains("stale component process")
        || message.contains("unknown component")
    {
        return StartCause::Process;
    }
    StartCause::Other
}

#[path = "../tests/support/component_start_cause.rs"]
mod tests;

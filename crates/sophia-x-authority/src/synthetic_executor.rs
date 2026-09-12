//! Executing admitted synthetic input, once, under one guard.
//!
//! Everything that decides whether a press may happen, and everything that
//! makes it happen, runs inside a single hold on the common authority. A
//! design that checked first and acted afterwards would leave a transition
//! free to land in between, which is precisely the window this closes.

use sophia_input_authority::{
    ConnectionIdentity, HoldIncarnation, Input, InputKind, Recipient, RegistrationError,
    ReleaseOutcome, RequestCompletion, RequestToken,
};
use sophia_protocol::NamespaceId;

/// What an admitted request asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntheticAction {
    Press,
    Release,
}

/// Where a press is delivered, as resolved at execution.
///
/// The grab that decides this can be taken or dropped between admission and
/// the moment a request becomes runnable, so resolving it earlier would name
/// a recipient the press never reached. Later releases use what is recorded
/// here rather than asking the question again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedRecipient {
    pub recipient: Recipient,
    /// Whether a grab, rather than ordinary focus, chose it.
    pub grabbed: bool,
}

/// Resolve where input in this namespace goes right now.
///
/// A device grab redirects what would otherwise follow the route. A SERVER
/// grab does not appear here at all: it schedules requests, deciding who may
/// proceed while others wait, and says nothing about who is entitled to
/// receive input. Treating its holder as the recipient would hand a client
/// that merely asked to be impervious every event on the seat.
pub(crate) fn resolve_recipient(
    authority: &crate::XInputAuthorityState,
    namespace: NamespaceId,
    focused: Option<u64>,
    connection_generation: u64,
    input: Input,
) -> Option<ResolvedRecipient> {
    // The input classifies itself. Carrying a separate device alongside it
    // meant two values that could disagree, and nothing to say which one was
    // right when they did.
    let grab = match input.kind() {
        InputKind::Key => authority.keyboard_grab(namespace),
        InputKind::Button => authority.pointer_grab(namespace),
    };
    if let Some(grab) = grab {
        return Some(ResolvedRecipient {
            recipient: Recipient {
                recipient: grab.owner,
                connection_generation,
            },
            grabbed: true,
        });
    }
    focused.map(|focused| ResolvedRecipient {
        recipient: Recipient {
            recipient: focused,
            connection_generation,
        },
        grabbed: false,
    })
}

/// What a press recorded, once it succeeded.
///
/// The recipient here is the LEDGER's, taken from the hold the press joined or
/// began, not the one resolution proposed. Those differ whenever a press joins
/// an existing hold: the route may point somewhere new, while the hold still
/// answers to where the first press went. Reporting the proposal would name a
/// client that will never receive the matching release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntheticRecord {
    pub incarnation: HoldIncarnation,
    /// Whether this press began the hold rather than joining one.
    ///
    /// A duplicate or a join moves the ledger without being a delivery, so a
    /// caller that treats every success as an event would emit input twice.
    pub first_press: bool,
    /// How resolution chose the target it proposed.
    ///
    /// Kept for the press that began the hold; for a join it describes a
    /// proposal the ledger did not adopt.
    pub proposed: ResolvedRecipient,
}

/// What an execution attempt produced.
///
/// `completion` reports a ledger transition and nothing else. It is not an
/// XTEST reply, and it says nothing about XKB state, routing, or anything a
/// writer did: no delivery has happened at this point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntheticOutcome {
    pub completion: RequestCompletion,
    /// Present only for a press that succeeded.
    pub record: Option<SyntheticRecord>,
    /// Present only for a release that was applied.
    pub release: Option<ReleaseOutcome>,
}

/// A request that could not be executed, and why, without having touched
/// anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntheticRefusal {
    /// No client is entitled to this input right now.
    NoRecipient,
    /// The authority refused before any effect.
    Authority(RegistrationError),
}

/// The identity a synthetic request executes against.
#[derive(Debug, Clone, Copy)]
pub struct SyntheticRequest {
    pub token: RequestToken,
    pub connection: ConnectionIdentity,
    pub namespace: NamespaceId,
    /// Session's, not the client's: X mints no such value.
    pub connection_generation: u64,
    pub action: SyntheticAction,
    pub input: Input,
}

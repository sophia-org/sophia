//! Executing admitted synthetic input, once, under one guard.
//!
//! Everything that decides whether a press may happen, and everything that
//! makes it happen, runs inside a single hold on the common authority. A
//! design that checked first and acted afterwards would leave a transition
//! free to land in between, which is precisely the window this closes.

use sophia_input_authority::{
    ConnectionIdentity, Input, Recipient, RegistrationError, RequestCompletion, RequestToken,
};
use sophia_protocol::NamespaceId;

/// Which device an admitted request speaks for.
///
/// Carried rather than derived: the authority's `Input` deliberately does not
/// say whether it is a key or a button, and the caller decoding the request
/// already knows. Asking the input to tell us would mean widening a type whose
/// narrowness is the point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntheticDevice {
    Keyboard,
    Pointer,
}

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
/// A server grab outranks a device grab, and either outranks focus: an
/// impervious holder is the only client entitled to see input while it holds
/// the server, and a device grab redirects what would otherwise follow focus.
pub(crate) fn resolve_recipient(
    authority: &crate::XInputAuthorityState,
    namespace: NamespaceId,
    focused: Option<u64>,
    connection_generation: u64,
    device: SyntheticDevice,
) -> Option<ResolvedRecipient> {
    if let Some(owner) = authority.server_owner(namespace) {
        return Some(ResolvedRecipient {
            recipient: Recipient {
                recipient: owner,
                connection_generation,
            },
            grabbed: true,
        });
    }
    let grab = match device {
        SyntheticDevice::Keyboard => authority.keyboard_grab(namespace),
        SyntheticDevice::Pointer => authority.pointer_grab(namespace),
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

/// What an execution attempt produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntheticOutcome {
    pub completion: RequestCompletion,
    /// Absent when nothing was delivered, which is not the same as refused.
    pub recipient: Option<ResolvedRecipient>,
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
    pub device: SyntheticDevice,
    pub action: SyntheticAction,
    pub input: Input,
}

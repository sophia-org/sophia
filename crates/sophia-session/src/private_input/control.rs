//! The narrow bridge from Engine-committed state to real controls.
//!
//! WHY THIS IS NOT A SENDER. What crosses here is a named effect for a named
//! connection, minted into a transaction Session owns. A caller cannot reach
//! the control producer, the broker or the routing registry, and cannot choose
//! its own transaction id: two callers that picked the same one would make the
//! acknowledgements ambiguous, and the ambiguity would be in the evidence
//! rather than in the code that caused it.

use sophia_protocol::{Rect, SurfaceId, TransactionId};
use sophia_x_authority::XAuthorityControlKind;

use super::submission::PrivateInputConnection;

/// One Engine-committed effect, as Session will submit it.
///
/// A CLOSED SET, NOT A PASSTHROUGH. These are the effects the headless
/// coordinator commits and the host drives. A control this does not name is
/// one nothing here has committed, so there is deliberately no escape hatch
/// that would let an arbitrary command reach the producer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PrivateInputControl {
    /// Admit the surface with its committed geometry. The map effect.
    AdmitSurface {
        surface: SurfaceId,
        geometry: Rect,
    },
    ConfigureSurface {
        surface: SurfaceId,
        geometry: Rect,
    },
    FocusSurface {
        surface: SurfaceId,
    },
    ClearFocus {
        surface: SurfaceId,
    },
    WithdrawSurface {
        surface: SurfaceId,
    },
    CloseSurface {
        surface: SurfaceId,
    },
}

impl PrivateInputControl {
    pub fn surface(self) -> SurfaceId {
        match self {
            Self::AdmitSurface { surface, .. }
            | Self::ConfigureSurface { surface, .. }
            | Self::FocusSurface { surface }
            | Self::ClearFocus { surface }
            | Self::WithdrawSurface { surface }
            | Self::CloseSurface { surface } => surface,
        }
    }

    pub fn kind(self) -> XAuthorityControlKind {
        match self {
            Self::AdmitSurface { .. } => XAuthorityControlKind::AdmitSurface,
            Self::ConfigureSurface { .. } => XAuthorityControlKind::ConfigureSurface,
            Self::FocusSurface { .. } => XAuthorityControlKind::FocusSurface,
            Self::ClearFocus { .. } => XAuthorityControlKind::ClearFocus,
            Self::WithdrawSurface { .. } => XAuthorityControlKind::WithdrawSurface,
            Self::CloseSurface { .. } => XAuthorityControlKind::CloseSurface,
        }
    }
}

/// What the order took, and how to recognise its acknowledgement.
///
/// The transaction is Session's own, so a caller correlates the real
/// acknowledgement that later arrives on the drain against this exact value
/// rather than against a number it chose and hoped was unused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivateInputControlAccepted {
    pub transaction: TransactionId,
    pub surface: SurfaceId,
    pub kind: XAuthorityControlKind,
}

/// Why a control was not taken.
#[derive(Debug)]
pub enum PrivateInputControlError {
    /// The boundary has no live connection for that admission.
    ConnectionGone,
    /// The order refused it, with its own reason and the command kept.
    Refused(
        sophia_x_authority::AdmissionRefusal,
        sophia_x_authority::XAuthorityClientControlCommand,
    ),
    /// The service has ended.
    Ended,
    /// A lock this needed could not be read. Never reported as a refusal: a
    /// boundary that could not be asked has not declined anything.
    Unavailable,
}

/// What one coordinator step did.
///
/// COUNTS OF DIFFERENT THINGS, KEPT APART. Transactions observed, transactions
/// the coordinator committed, and controls the commit called for are three
/// numbers that only agree when nothing was refused, and an evidence reader
/// needs to see when they do not.
#[derive(Debug, Default)]
pub struct PrivateInputCommitted {
    pub transactions_observed: usize,
    pub transactions_committed: usize,
    pub controls: Vec<PrivateInputControlAccepted>,
    /// Controls the committed state called for and the order did not take.
    /// Present rather than summarised, because a step that committed state and
    /// then failed to apply it is the case this whole path exists to expose.
    pub refused: Vec<PrivateInputControlError>,
}

/// The connection a control is submitted for, named exactly.
///
/// Re-exported here so a caller submitting a control and a caller submitting
/// input name a connection the same way.
pub type PrivateInputControlTarget = PrivateInputConnection;

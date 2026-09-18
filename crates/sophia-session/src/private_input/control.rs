//! Session's own actions, and the effects that come only from a commit.
//!
//! THESE ARE TWO DIFFERENT THINGS AND THE TYPES SAY SO. An explicit focus
//! decision is Session's policy: nothing commits it, Session simply decides it
//! and is answerable for the acknowledgement. A map or a configure is not
//! policy. It is what the coordinator committed, and its geometry belongs to
//! the committed state rather than to whoever asked.
//!
//! WHAT AN EARLIER VERSION OF THIS FILE GOT WRONG. It offered one closed enum
//! covering both, with caller-supplied geometry, and claimed that being a
//! closed set meant no uncommitted command could reach the producer. That was
//! false. A closed set constrains which commands exist, not whether the state
//! they describe was ever committed, and a caller could have asked for a
//! configure with any geometry at any time. The commitment is now structural:
//! a committed effect has private fields, so the only way one exists is for
//! the coordinator to have produced it.

use sophia_protocol::{Rect, SurfaceId, TransactionId};
use sophia_x_authority::XAuthorityControlKind;

/// An action Session decides on its own authority.
///
/// Focus policy only. These are not committed surface state and are not
/// pretending to be: Session chooses them, submits them, and checks the real
/// acknowledgement that comes back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateInputAction {
    FocusSurface { surface: SurfaceId },
    ClearFocus { surface: SurfaceId },
}

impl PrivateInputAction {
    pub fn surface(self) -> SurfaceId {
        match self {
            Self::FocusSurface { surface } | Self::ClearFocus { surface } => surface,
        }
    }

    pub fn kind(self) -> XAuthorityControlKind {
        match self {
            Self::FocusSurface { .. } => XAuthorityControlKind::FocusSurface,
            Self::ClearFocus { .. } => XAuthorityControlKind::ClearFocus,
        }
    }
}

/// What the order took, and how to recognise its acknowledgement.
///
/// The transaction is Session's own, so a caller matches the real
/// acknowledgement against this exact submission rather than against a number
/// it chose and hoped was unused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivateInputSubmitted {
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

/// One effect the coordinator committed, and what Session did with it.
///
/// UNFORGEABLE, NOT MERELY UNDOCUMENTED. An earlier version said there was no
/// public constructor while leaving every field public, which is not the same
/// thing: a struct with public fields is constructible by writing it out. The
/// fields are private and read through methods, so the only way a value of
/// this type exists is for `apply_committed` to have built it from a
/// `TransactionCommit` whose outcome was `Committed`, with geometry taken from
/// the coordinator's committed surface state.
///
/// It is still report data, and nothing accepts it back. No path anywhere
/// treats being handed one of these as authority to route anything.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrivateInputCommittedEffect {
    committed_transaction: TransactionId,
    surface: SurfaceId,
    kind: XAuthorityControlKind,
    geometry: Option<Rect>,
    submitted: Option<PrivateInputSubmitted>,
}

impl PrivateInputCommittedEffect {
    #[expect(
        dead_code,
        reason = "built by apply_committed; the unfulfilled expectation is what removes this"
    )]
    pub(super) fn new(
        committed_transaction: TransactionId,
        surface: SurfaceId,
        kind: XAuthorityControlKind,
        geometry: Option<Rect>,
        submitted: Option<PrivateInputSubmitted>,
    ) -> Self {
        Self {
            committed_transaction,
            surface,
            kind,
            geometry,
            submitted,
        }
    }

    /// The authority transaction the coordinator committed.
    pub fn committed_transaction(&self) -> TransactionId {
        self.committed_transaction
    }

    pub fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Which effect the commit called for.
    ///
    /// READ FROM THE BATCH'S OWN FACTS, never inferred from presence in
    /// committed state. Two edges admit, and they are not the same edge.
    ///
    /// An ordinary map arrives as `mapped` in `surface_presentations`, taken
    /// from the runtime's surface state. A policy-managed deferred map does
    /// not: while policy maps are deferred, MapWindow records the surface as
    /// pending rather than mapped, so `mapped` stays false and the batch
    /// carries a `Request` intent instead. That Request is what authorises the
    /// admission, and `mapped` becomes true only once the admission succeeds.
    /// Requiring `mapped` for that branch would wait for a fact the admission
    /// itself produces, which is a deadlock rather than a stricter check.
    ///
    /// So: `mapped`, or a `Request` intent, with no admission outstanding for
    /// that surface, is an admission. A later committed update to a surface
    /// already admitted is a configure. A `Withdraw` intent or an intake
    /// removal is a withdrawal. A surface with neither a mapping nor a Request
    /// is never admitted, which is what keeps an unmapped passive helper out:
    /// the transport makes the same distinction, giving an owner route only to
    /// mapped surfaces.
    ///
    /// The surface is identified by its whole `SurfaceId`, whose own
    /// generation is the incarnation. The `generation` on a presentation
    /// observation is the drawing clock and advances with content, so keying
    /// on it would re-admit a surface on every draw.
    pub fn kind(&self) -> XAuthorityControlKind {
        self.kind
    }

    /// The committed geometry, read from the coordinator's committed surface
    /// state. `None` for a withdrawal, which commits no geometry.
    pub fn geometry(&self) -> Option<Rect> {
        self.geometry
    }

    /// What the order took for it, when it took it.
    pub fn submitted(&self) -> Option<PrivateInputSubmitted> {
        self.submitted
    }
}

/// What one coordinator step did.
///
/// COUNTS OF DIFFERENT THINGS, KEPT APART. Batches observed, commits the
/// coordinator returned, commits whose outcome was `Committed`, and effects
/// the order actually took are four numbers that agree only when nothing was
/// rejected or refused. An evidence reader needs to see when they do not.
#[derive(Debug, Default)]
pub struct PrivateInputCommitted {
    pub batches_observed: usize,
    /// Commits the coordinator returned, whatever their outcome.
    pub commits: usize,
    /// Of those, the ones whose outcome was `Committed`. A rejected or timed
    /// out transaction is a commit result too, and counting it as committed
    /// would be the whole failure this reports.
    pub committed: usize,
    /// Every effect the committed state called for, each carrying what the
    /// order did with it.
    pub effects: Vec<PrivateInputCommittedEffect>,
    /// Effects the committed state called for and the order did not take.
    /// Present rather than summarised: a step that committed state and then
    /// failed to apply it is what this path exists to expose.
    pub refused: Vec<PrivateInputControlError>,
}

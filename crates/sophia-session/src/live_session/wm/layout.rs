/// Re-drive attempts against one standing target before the session says so.
///
/// A standing target is re-injected until the surface commits at exactly that
/// size, so a surface that commits at any other size loops forever. The loop is
/// idempotent and cheap, which is why it went unnoticed; this is the point at
/// which repeating stops being indistinguishable from converging. Reporting, not
/// a cap: bounding the loop before knowing why it does not converge would trade a
/// visible symptom for an invisible one.
const STANDING_TARGET_REDRIVE_REPORT_THRESHOLD: u32 = 8;

struct PendingLiveWmLayout {
    transaction: TransactionId,
    layers: Vec<LayerSnapshot>,
    requested_sizes: BTreeMap<SurfaceId, Size>,
    presentation_states: BTreeMap<SurfaceId, sophia_protocol::PolicyPresentationState>,
    presentation_settlements: BTreeSet<SurfaceId>,
    configure_deliveries: usize,
    focus: Option<SurfaceId>,
    deadline: Instant,
    update: WmTransactionUpdate,
    moved_surfaces: usize,
    staged_transactions: BTreeMap<SurfaceId, SurfaceTransaction>,
    admission_surfaces: BTreeSet<SurfaceId>,
    source: Option<LiveWmProposalSource>,
    policy_settlement: Option<LivePolicySettlementIdentity>,
}

struct LiveAuthorityLayoutObservation {
    new_surfaces: Vec<SurfaceId>,
    withdrawn_surfaces: Vec<SurfaceId>,
    output_reservations_changed: bool,
    admission_group_error: Option<&'static str>,
    admission_group_overflowed: bool,
    client_route_invalid: bool,
}

enum LiveLayoutProgress {
    Blocked,
    DeferredReady,
    Committed(LiveWmCommitResult),
}

/// Whether an escaped Present names the same content as an admission candidate.
///
/// The buffer is part of the comparison because one transaction and surface can
/// carry more than one source, and a backing snapshot must not be read as the
/// client's Present.
fn escaped_key_names_candidate(
    key: sophia_protocol::DmaBufPresentKey,
    candidate: sophia_protocol::SurfaceTransactionKey,
) -> bool {
    candidate.transaction == key.transaction
        && candidate.surface == key.surface
        && matches!(
            candidate.target_buffer,
            sophia_protocol::BufferSource::DmaBuf { handle, .. }
                if handle == key.buffer.raw()
        )
}

const PRE_ADMISSION_GROUP_CAPACITY: usize = 256;

/// How many escaped pre-admission Presents are remembered at once.
///
/// One per escaped Present, not per window, held until production skips it or
/// it stops being eligible. A client outstanding more than this many unskipped
/// pre-map frames at once is not a case worth carrying unbounded state for.
const ESCAPED_PRE_ADMISSION_CAPACITY: usize = 64;

/// A Present that reached production without passing through admission,
/// because its surface was inactive when the batch carrying it was observed.
///
/// A client that presents its first frame before mapping its window produces
/// one of these: `surface_requires_admission` is false while the surface is
/// inactive, so the frame is never quarantined, and a later map does not
/// replay the batch that carried it. Production owns the frame, admission has
/// no claim on it, and nothing waiting on presentation order can release it.
///
/// "Inactive at intake" rather than "never admitted": withdrawal clears
/// admission state, so inactive is a fact about that moment and not a history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EscapedPreAdmissionPresent {
    /// The exact Present, buffer included. A transaction and surface pair is
    /// not identity here -- several sources can share one, and the buffer is
    /// what stops a backing snapshot being mistaken for this frame.
    key: sophia_protocol::DmaBufPresentKey,
    /// Set when the authority confirms the map for this surface, which is the
    /// point the client can be expected to draw again after a skip.
    ready: bool,
}

/// One committed policy answer that placed nothing.
///
/// A blind window manager may decline a surface indefinitely -- a monocle layout
/// places one window however many it is shown. Recording which facts the answer
/// was given against is what separates "policy already said no" from "ask again",
/// and keeps the session from re-asking a settled question every owner-loop turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LiveManageSettlement {
    connection_epoch: u64,
    scene_generation: u64,
}

#[derive(Default)]
struct PersistentLiveLayout {
    layers: BTreeMap<SurfaceId, LayerSnapshot>,
    planning_surfaces: BTreeMap<SurfaceId, sophia_engine::SurfaceLayoutFacts>,
    authority_surface_facts: BTreeMap<SurfaceId, sophia_engine::SurfaceLayoutFacts>,
    admissions: sophia_engine::SurfaceAdmissionTable,
    dma_buf_sizes: BTreeMap<sophia_protocol::BufferHandle, Size>,
    cpu_buffer_sizes: BTreeMap<u64, Size>,
    deferred_dma_buf_releases: BTreeSet<sophia_protocol::BufferHandle>,
    deferred_fence_releases: BTreeSet<sophia_protocol::FenceHandle>,
    layout_epochs: LayoutEpochCoordinator,
    client_routes: XAuthorityClientSurfaceRoutes,
    presentation_roles: BTreeMap<SurfaceId, sophia_protocol::SurfacePresentationRole>,
    presentation_owners: BTreeMap<SurfaceId, SurfaceId>,
    surface_kinds: BTreeMap<SurfaceId, sophia_protocol::LayoutNodeKind>,
    placement_preferences:
        BTreeMap<SurfaceId, sophia_protocol::SurfacePlacementPreference>,
    authority_stack_ranks: BTreeMap<SurfaceId, u32>,
    mapped_surfaces: BTreeSet<SurfaceId>,
    pre_admission_groups: VecDeque<LiveAdmissionAuthorityGroup>,
    escaped_pre_admission: VecDeque<EscapedPreAdmissionPresent>,
    released_admission_groups: VecDeque<LiveAdmissionAuthorityGroup>,
    output_reservations: sophia_engine::SurfaceOutputReservationState,
    unmanaged_surfaces: BTreeSet<SurfaceId>,
    admission_retries: BTreeMap<SurfaceId, u8>,
    /// Surfaces whose admission this coordinator gave up on, awaiting
    /// whoever was waiting on them. Drained rather than read, so an
    /// abandoned launch is told once and not once per owner-loop turn.
    withdrawn_admissions: Vec<SurfaceId>,
    /// Surfaces whose `Manage` request the window manager answered by placing
    /// nothing. Keyed by the facts it answered against, so a fact change retires
    /// the entry rather than any timer.
    manage_settlements: BTreeMap<SurfaceId, LiveManageSettlement>,
    pending: Option<PendingLiveWmLayout>,
    focus_to_apply: Option<(TransactionId, SurfaceId)>,
    retirement_focus:
        BTreeMap<SurfaceId, (sophia_protocol::SurfaceTransactionKey, TransactionId)>,
    bypass_policy_admission: bool,
    /// The Engine, not an external policy client, owns initial placement --
    /// true exactly in the Direct policy-map mode (no external window manager).
    /// A surface then has no policy-assigned output owner, so it must route to
    /// an output by its geometry instead.
    engine_owns_initial_placement: bool,
    stage_new_surfaces_offset: bool,
    center_first_surface_in: Option<Size>,
    constraint_relayout_required: bool,
    awaiting_visual_commits: ResizeVisualCommitTracker,
    committed_policy_presentations:
        BTreeMap<SurfaceId, sophia_protocol::PolicyPresentationState>,
}

include!("layout/authority_observation.rs");
include!("layout/surface_ownership.rs");
include!("layout/staging.rs");
include!("layout/pending_settlement.rs");
include!("layout/admission.rs");
include!("layout/commit.rs");

#ifndef SOPHIA_SHELL_NATIVE_SESSION_H
#define SOPHIA_SHELL_NATIVE_SESSION_H
#include "sophia_shell_session.h"

/* Native launcher (r7) lifecycle over one READY sophia_ss, following
 * spec/sophia-shell-files.md "Role family outcomes (normative)" and "Native
 * launcher lifecycle and clocks (normative)". It builds only the
 * typed file records of sophia-shell-files-v1 and never renders, owns widgets
 * or reads a clock: callers pass CLOCK_MONOTONIC milliseconds. Nothing is
 * presented, focused or activated until the server's own event says so, and
 * nothing is replayed. The ss stays the caller's: poll, dispatch, event
 * acknowledgement and object fetches remain there. While an ns is attached,
 * events are read through sophia_ns_next, and every launcher record is
 * submitted through this API, which owns the transaction IDs it names.
 * Return values are sophia_9p_result codes: AGAIN (no event), BUSY (nothing
 * changed; retry after dispatch), INVALID (refused locally: not current, not
 * permitted, or a value the codec refuses), ARGUMENT, or a final status. */
#define SOPHIA_NS_RESOURCES 16u
#define SOPHIA_NS_ROWS 32u
/* NativeCandidate.interaction_generation: production Session supplies 1; it
 * is not a client counter. Presented bindings carry it back. */
#define SOPHIA_NS_INTERACTION_GENERATION 1u

enum sophia_ns_state {
    SOPHIA_NS_LIVE,
    /* Final. ENDED: the ss is final; custody of anything not SUBMITTED is
     * unknown and nothing is presented any more. FAILED: the server sent a
     * record this client cannot correlate; close the ss. */
    SOPHIA_NS_ENDED,
    SOPHIA_NS_FAILED
};
enum sophia_ns_event_kind {
    /* Not a launcher record (for example ObjectPublished). Finish with done. */
    SOPHIA_NS_EV_OTHER,
    SOPHIA_NS_EV_OPENING,
    SOPHIA_NS_EV_CLOSED,
    SOPHIA_NS_EV_ALLOCATION,
    SOPHIA_NS_EV_RESOURCE,
    SOPHIA_NS_EV_RESOURCE_RELEASED,
    SOPHIA_NS_EV_PERMIT,
    SOPHIA_NS_EV_CANDIDATE,
    SOPHIA_NS_EV_FOCUS,
    SOPHIA_NS_EV_FOCUS_REVOKED,
    /* Finish with input_reserve then input_commit (or input_apply). */
    SOPHIA_NS_EV_INPUT,
    /* Finish with action_ack (or done when no acknowledgement is owed). */
    SOPHIA_NS_EV_ACTION,
    SOPHIA_NS_EV_ACTIVATION
};
struct sophia_ns_event {
    enum sophia_ns_event_kind kind;
    /* Borrowed until the event is finished; its text too. */
    const struct sophia_sf_record *record;
    /* The record names this client's current opening, allocation, candidate,
     * focus or activation; a non-current record changed nothing current. */
    uint8_t current;
    /* FOCUS: held when every binding field equals the shown candidate's at
     * the current revision, in event order, and the opening has no Admitted
     * activation. The contract ("minted only after an actual Presented")
     * gives no lease freshness or ordering rule, so none is checked: a later
     * NativeFocus supersedes the held one. INPUT: the UI may
     * edit (non-Accept, bound to the held focus, opening current). */
    uint8_t held, editable;
    /* INPUT Accept: a keyboard NativeActivate for the presented selection may
     * be reserved with it. ACTION kind 1 on the shown candidate naming one of
     * its displayed rows: a pointer NativeActivate may go with its ack. */
    uint8_t can_activate;
    /* ACTION kinds 1-2: an ActionAck is owed. Kind 3 is a cancellation and
     * owes nothing; finish it with done. */
    uint8_t ack_owed;
};
struct sophia_ns_config {
    /* First transaction ID this client assigns; nonzero. */
    uint64_t first_transaction;
    /* The permit deadline is ADVISORY scheduling only ("Permit time"): no
     * local bound proves the server still holds a permit. A candidate is refused locally unless
     * now_ms + margin is before the FrameDemand's enqueue time + ttl_ms. A permit past that bound
     * is skipped, never cancelled; a fresh demand waits for the server's own FramePermit ending the
     * old one. The residual race (the server expiring a permit the bound still allows) is not
     * hidden. */
    uint32_t permit_margin_ms;
};
struct sophia_ns_allocation_params {
    uint16_t edge;
    uint32_t width, height;
    int16_t margin_top, margin_right, margin_bottom, margin_left;
};
/* One whole native candidate: placements on the one surface, one target per
 * displayed row. The session fills identities, surface, permit and
 * generations; surface_index, action_kind and action_id (the row's slot) are
 * set here. */
struct sophia_ns_scene {
    uint16_t placement_count, row_count, selected;
    uint16_t rows[SOPHIA_NS_ROWS];
    struct sophia_sf_content_placement placements[32];
    struct sophia_sf_content_target targets[SOPHIA_NS_ROWS];
};
/* Deadlines are Session service time: the server judges them when it
 * services the record, so a client send before a deadline guarantees nothing
 * (transport race). A missing ack for Action kind 1 produces a kind-3
 * cancellation after the deadline; a missing NativeInputAck closes the opening (reason 6). Stale
 * acks are ignored only within the grant: a wrong-grant ack is fatal, so every ack here echoes the
 * current grant. */
struct sophia_ns_obligations {
    /* Earliest nonzero deadline below, 0 when none. */
    uint64_t next_ms;
    /* Head NativeInput ack: issued_mono_usec / 1000 + action_ack_timeout_ms,
     * an early scheduling deadline in Session's host CLOCK_MONOTONIC. A
     * missed ack closes the opening with reason 6. */
    uint64_t input_due_ms;
    /* Head Action ack: receipt + action_ack_timeout_ms (receipt time is only
     * a local scheduling hint; it does not restart the server's deadline). */
    uint64_t action_due_ms;
    /* Advisory local permit bound (demand enqueue + ttl_ms), 0 without a
     * usable permit. Not a server validity guarantee. */
    uint64_t permit_expires_ms;
    /* Informational: the server decides these outcomes. */
    uint64_t allocation_due_ms, candidate_due_ms;
    /* A presented candidate is older than the state revision, or none is
     * presented while opening, catalog, allocation are current. */
    uint8_t needs_candidate;
    /* The opening's catalog, or the fetched output facts, are not the latest
     * announced: a newer announcement disarms the local scene. This reduces
     * stale work; it cannot exclude a later server update. */
    uint8_t needs_catalog, needs_facts;
    /* A closed opening's allocation awaits invalidation (status 4). It
     * arrives only once the old pixels are removed and the attach is still
     * live; no deadline bounds it, and a final session ends the wait. */
    uint8_t awaiting_invalidation;
    /* Owed acknowledgement records refused at submit (not resendable). */
    uint8_t lost_acks;
};
enum sophia_ns_resource_state {
    SOPHIA_NS_RESOURCE_FREE,
    SOPHIA_NS_RESOURCE_BEGUN,
    SOPHIA_NS_RESOURCE_ADMITTED,
    SOPHIA_NS_RESOURCE_ACCEPTED,
    SOPHIA_NS_RESOURCE_RETIRING
};
struct sophia_ns_resource {
    uint64_t id, generation, transaction, ticket;
    uint8_t state;
};
struct sophia_ns_allocation {
    /* The latest granted AllocationResult while has_grant. */
    struct sophia_sf_allocation_result granted;
    uint64_t request_id, ticket, requested_ms, opening;
    uint16_t operation, edge, request_edge;
    /* pending: a request awaits its result. orphaned: its opening closed and
     * it is no longer used. Invalidation (status 4) follows only once the old
     * pixels are removed and the attach is still live; nothing bounds when it
     * arrives, and a final session ends the wait. Until then no new allocation
     * is requested. */
    uint8_t pending, has_grant, orphaned;
};
struct sophia_ns_permit {
    uint64_t demand_id, ticket, permit_id, demand_ms, expires_ms, last_demand_id;
    uint32_t max_candidate_bytes;
    /* demand: standing or granted, awaiting its end. granted: a permit
     * usable until expires_ms. */
    uint8_t demand, granted, cancelling;
};
/* Key fields of one submitted or presented candidate. */
struct sophia_ns_shown {
    uint64_t transaction, ticket, generation, presentation_epoch, submitted_ms;
    uint64_t opening, catalog_generation, state_revision, interaction_generation;
    uint64_t allocation_id, allocation_generation, output_id, output_generation;
    uint16_t selected, row_count;
    uint16_t rows[SOPHIA_NS_ROWS];
    uint8_t valid, prepared;
};
/* The head input's owed records (an optional keyboard activation, then the
 * ack), reserved before any local effect. */
struct sophia_ns_response {
    struct sophia_ss_reservation reservation;
    struct sophia_sf_record records[2];
    uint8_t count, active, activate;
};
#define SOPHIA_NS_ACKS 16u
/* Private state exposed for caller-owned allocation. Do not modify it. */
struct sophia_ns {
    struct sophia_ss *ss;
    enum sophia_ns_state state;
    struct sophia_sf_limits limits;
    struct sophia_ns_config config;
    uint64_t next_transaction, next_request, next_demand, next_generation;
    uint64_t catalog_announced, catalog_installed, facts_announced, facts_installed, revision;
    uint64_t last_opening;
    uint64_t head_ms;
    struct sophia_sf_native_opening opening;
    struct sophia_sf_native_binding focus;
    uint8_t open, focused, launched, have_head, lost_acks, ack_count;
    uint16_t closed_reason;
    struct sophia_ns_event head;
    struct sophia_ns_allocation allocation;
    struct sophia_ns_permit permit;
    struct sophia_ns_shown pending, shown;
    struct sophia_ns_response response;
    struct {
        struct sophia_sf_native_activate value;
        uint64_t transaction, ticket;
        uint8_t active;
    } activation;
    uint64_t acks[SOPHIA_NS_ACKS];
    struct sophia_ns_resource resources[SOPHIA_NS_RESOURCES];
    unsigned upload;
    uint8_t uploading;
    /* Candidate record scratch; not state. */
    struct sophia_sf_record scratch;
};

/* ss must be READY with its limits fetched (BUSY until then). It is borrowed,
 * not owned, and must outlive the ns; nothing else may submit on it. */
int sophia_ns_init(struct sophia_ns *, struct sophia_ss *, const struct sophia_ns_config *);
enum sophia_ns_state sophia_ns_state(const struct sophia_ns *);
/* Poll submit outcomes and local permit expiry; mirror a final ss. Call after
 * each dispatch. REFUSED or DROPPED_UNSENT clears the item that owed it; an
 * ack refused at submit is lost (see obligations). */
int sophia_ns_service(struct sophia_ns *, uint64_t now_ms);
/* The next event, applied once to local state. It stays the head (returned
 * again, not reapplied) until finished: done for most kinds, input_commit for
 * INPUT, action_ack or done for ACTION. AGAIN when none. */
int sophia_ns_next(struct sophia_ns *, uint64_t now_ms, struct sophia_ns_event *);
int sophia_ns_done(struct sophia_ns *);
/* The caller fetched and installed this catalog generation. An opening stays
 * bound to its own catalog generation and never adopts another: candidates
 * and activations need that generation installed and not superseded by a
 * newer announcement; otherwise they are disarmed while inputs are still
 * acknowledged, awaiting close. */
int sophia_ns_catalog(struct sophia_ns *, uint64_t generation);
/* The caller fetched this Outputs facts_generation. Candidates name it and
 * need it to equal the latest announcement. */
int sophia_ns_facts(struct sophia_ns *, uint64_t generation);

/* Semantic input. Reserve before any UI effect: BUSY changes nothing and the
 * caller must not edit. After a successful reserve the caller edits at most
 * once (only when editable) and then commits exactly once with disposition 1
 * Consumed or 2 Stale; commit cannot fail for capacity and consumes the
 * event. activate (Accept with can_activate only) adds the keyboard
 * NativeActivate for the presented selection, ordered before the ack. Cancel
 * only before any edit: it returns the reservation and keeps the event. */
int sophia_ns_input_reserve(struct sophia_ns *, int activate);
int sophia_ns_input_commit(struct sophia_ns *, uint16_t disposition, uint64_t *first_ticket);
int sophia_ns_input_cancel(struct sophia_ns *);
/* Reserve, call edit once when editable (nonzero: consumed), commit. The
 * callback must not reenter the ns or ss nor retain the borrowed text. */
typedef int (*sophia_ns_edit)(void *user, const struct sophia_sf_native_input *);
int sophia_ns_input_apply(struct sophia_ns *, int activate, sophia_ns_edit, void *user,
                          uint64_t *first_ticket);
/* ActionAck for the head ACTION event (kinds 1-2), echoing it; consumes it.
 * activate (can_activate only) first submits a pointer NativeActivate
 * (cause 2) naming the held focus, the current revision, the Action's
 * event_id and its row slot, in the same local submission (local atomicity
 * does not make server custody or effects atomic). first_ticket is the first
 * record's. */
int sophia_ns_action_ack(struct sophia_ns *, uint16_t disposition, int activate,
                         uint64_t *first_ticket);

/* NativeAllocationRequest: acquire when none is granted, else resize; one
 * request at a time, for the current opening. */
int sophia_ns_allocate(struct sophia_ns *, const struct sophia_ns_allocation_params *,
                       uint64_t now_ms, uint64_t *ticket);
int sophia_ns_release(struct sophia_ns *, uint64_t now_ms, uint64_t *ticket);
/* Upload wrappers: transaction and grant are filled here; chunks go through
 * sophia_ss_upload_chunk. Only an Accepted resource may be placed. */
int sophia_ns_upload_begin(struct sophia_ns *, struct sophia_sf_resource_begin, uint64_t *ticket);
int sophia_ns_upload_end(struct sophia_ns *, uint64_t *ticket);
int sophia_ns_upload_cancel(struct sophia_ns *, uint64_t *ticket);
int sophia_ns_retire(struct sophia_ns *, uint64_t resource_id, uint64_t generation,
                     uint64_t *ticket);
/* FrameDemand for the granted allocation: refused while the output's permit
 * is unused or a candidate is outstanding (assembling), and never queued on
 * the local TTL alone. Reason 1/2 dirty or animation
 * work, 3 withdrawal. demand_id strictly rises; one demand at a time (never
 * replacing a standing one); each frame needs a fresh demand once the server
 * ended the last one or a candidate consumed its permit. Cancel only names a
 * granted permit well inside its advisory bound: a cancel naming nothing
 * current revokes authority. */
int sophia_ns_demand(struct sophia_ns *, uint16_t reason, uint64_t now_ms, uint64_t *ticket);
int sophia_ns_demand_cancel(struct sophia_ns *, uint64_t now_ms, uint64_t *ticket);
/* One candidate at a time per output (local policy). The Session revalidates
 * facts, interaction, opening, catalog, state_revision and allocation binding
 * at renderer handoff; a mismatch there is fatal, not a Rejected outcome.
 * One whole NativeCandidate against the current opening, installed catalog
 * and facts, state revision, granted allocation and unconsumed permit inside
 * its local bound, whose placements name Accepted resources. Admission consumes the permit. One
 * candidate is outstanding until its CandidateOutcome or submit refusal. */
int sophia_ns_present(struct sophia_ns *, const struct sophia_ns_scene *, uint64_t now_ms,
                      uint64_t *ticket);

int sophia_ns_obligations(const struct sophia_ns *, struct sophia_ns_obligations *);
/* Borrowed views of current state; NULL when absent. */
const struct sophia_sf_native_opening *sophia_ns_opening(const struct sophia_ns *);
const struct sophia_sf_native_binding *sophia_ns_focus(const struct sophia_ns *);
const struct sophia_ns_shown *sophia_ns_presented(const struct sophia_ns *);
const struct sophia_sf_allocation_result *sophia_ns_allocation(const struct sophia_ns *);
uint64_t sophia_ns_revision(const struct sophia_ns *);
#endif

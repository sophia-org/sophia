#ifndef SOPHIA_SHELL_SESSION_H
#define SOPHIA_SHELL_SESSION_H
#include "sophia_shell_files_client.h"

/* Native shell session over one admitted fd. It owns every submission and
 * upload of its file client; nothing is replayed and there is no reconnect.
 * Local admission is not custody: custody is the server's Submitted event for
 * that submission, observed by this session. Tickets are local and monotonic.
 * The library reads no clock: callers pass CLOCK_MONOTONIC milliseconds. */
#define SOPHIA_SS_SLOTS 64u
#define SOPHIA_SS_OUTCOMES 256u
#define SOPHIA_SS_REQUESTS 8u
#define SOPHIA_SS_FIDS 32u
/* The file client's single transaction buffer bounds one encoded record. */
#define SOPHIA_SS_RECORD_BYTES 8192u
/* journal_ack_progress_timeout from the shell files contract. */
#define SOPHIA_SS_ACK_PROGRESS_MS 2000u
enum sophia_ss_state {
    SOPHIA_SS_NEGOTIATING,
    SOPHIA_SS_READY,
    /* Final states. Close the fd and open a new session; nothing is replayed. */
    SOPHIA_SS_REFUSED_NEGOTIATION,
    /* ESTALE from events, submit or ack: the stream is unavailable. Export
     * revocation looks like this, but ESTALE alone does not prove it. */
    SOPHIA_SS_STALE,
    SOPHIA_SS_CLOSED,
    SOPHIA_SS_FAILED
};
enum sophia_ss_outcome {
    /* Evicted from the last SOPHIA_SS_OUTCOMES tickets; never another's. */
    SOPHIA_SS_UNAVAILABLE,
    SOPHIA_SS_ADMITTED_LOCAL,
    SOPHIA_SS_IN_FLIGHT,
    /* Final outcomes. SUBMITTED is custody only and survives any later end. */
    SOPHIA_SS_SUBMITTED,
    SOPHIA_SS_REFUSED,
    SOPHIA_SS_DROPPED_UNSENT,
    SOPHIA_SS_UNKNOWN_DISCONNECTED
};
struct sophia_ss_config {
    struct sophia_sf_negotiate offer;
    enum sophia_sf_profile profile;
    uint32_t msize;
    /* At most SOPHIA_SS_SLOTS records in SOPHIA_SS_RECORD_BYTES to
     * SOPHIA_SS_SLOTS * SOPHIA_SS_RECORD_BYTES encoded bytes. */
    uint16_t queue_slots;
    size_t queue_bytes;
    /* Optional object scratch, as sophia_sf_client_init_profile. */
    void *object_storage;
    size_t object_capacity;
};
struct sophia_ss_reservation {
    uint64_t serial;
    uint16_t slots;
    size_t bytes;
};
struct sophia_ss_obligations {
    /* Owed ack progress by ack_due_ms (0: none owed). objects has bit
     * kind - 1 for each announced object not yet fetched; blocked means
     * consumed events cannot be acknowledged until those are fetched. */
    uint64_t consumed, acked, ack_limit, ack_due_ms;
    uint8_t objects, blocked;
};
/* Private state exposed for caller-owned allocation. Do not modify it. */
struct sophia_ss_entry {
    uint64_t ticket;
    size_t bytes;
};
struct sophia_ss_ticket {
    uint64_t ticket;
    uint32_t error;
    uint8_t outcome;
};
struct sophia_ss_hold {
    uint64_t sequence, before, generation, qid;
    uint8_t active;
};
struct sophia_ss {
    struct sophia_9p_client wire;
    struct sophia_sf_client files;
    enum sophia_ss_state state;
    uint8_t *queue;
    size_t queue_capacity, queue_used, reserved_bytes;
    uint16_t slots, queued, reserved_slots, refused_reason;
    uint64_t next_ticket, flight_ticket, flight_id, last_consumed, acked_seen;
    uint64_t now_ms, retry_at, ack_clock, refused_denied, reservation;
    uint32_t retry_delay;
    uint8_t flight, waiting, progress, object_requested, refused, reserved;
    struct sophia_ss_entry entries[SOPHIA_SS_SLOTS];
    /* holds: announcements owed a fetch; seen: the last verified fetch. */
    struct sophia_ss_hold holds[4], seen[4];
    struct sophia_ss_ticket tickets[SOPHIA_SS_OUTCOMES];
};
/* 9P request storage plus the encoded queue; 0 for invalid sizes. */
size_t sophia_ss_storage_bytes(uint32_t msize, size_t queue_bytes);
/* fd is admitted and connected; the caller keeps ownership and closes it
 * after sophia_ss_close or a final state. storage must be disjoint from the
 * session and outlive it. Starts version, attach and negotiation. */
int sophia_ss_open_fd(struct sophia_ss *, int fd, const struct sophia_ss_config *, void *storage,
                      size_t bytes);
int sophia_ss_poll_fd(const struct sophia_ss *);
/* POLLIN while live, plus POLLOUT only while 9P output is queued; 0 when final. */
short sophia_ss_poll_events(const struct sophia_ss *);
/* -1, or milliseconds until a submit deferred by EAGAIN may be retried. It
 * reports nothing about health: see obligations. */
int sophia_ss_timeout(const struct sophia_ss *, uint64_t now_ms);
/* One service pass: bounded (at most byte_budget bytes and 32 syscalls each
 * way) and nonblocking whatever revents says; revents is used only for
 * POLLNVAL, which ends the session as CLOSED. A submit refused with
 * EAGAIN is retried with the same epoch, id and staged transaction, never in
 * the pass that saw the refusal: only after event consume or ack progress, or
 * once now_ms reaches its backoff deadline (4 ms doubling to 256 ms). now_ms
 * must not decrease: a smaller value is ARGUMENT and changes nothing.
 * Deadlines saturate at UINT64_MAX. Returns 0 while live, else a negative
 * sophia_9p_result; see state. */
int sophia_ss_dispatch(struct sophia_ss *, short revents, size_t byte_budget, uint64_t now_ms);
enum sophia_ss_state sophia_ss_state(const struct sophia_ss *);
/* The live epoch after negotiation, and the granted limits; else 0 and NULL. */
uint64_t sophia_ss_epoch(const struct sophia_ss *);
const struct sophia_sf_limits *sophia_ss_limits(const struct sophia_ss *);
/* AGAIN unless a Refused event was received; its reason and denied bits. */
int sophia_ss_refusal(const struct sophia_ss *, uint16_t *reason, uint64_t *denied);
/* Encoded size of one candidate record, 0 when invalid or not a candidate. */
size_t sophia_ss_record_bytes(const struct sophia_sf_record *);
/* READY only. Validates and encodes every record (header epoch, submission
 * and sequence are assigned here and at hand-off) before any state changes:
 * all are admitted, or none. BUSY: not READY, or no slot/byte room now.
 * INVALID: a record fails validation. ARGUMENT: bad kind, count or size. */
int sophia_ss_submit(struct sophia_ss *, const struct sophia_sf_record *records, size_t count,
                     uint64_t *first_ticket);
/* Reserve room before an irreversible local step (for example a UI edit
 * whose acknowledgement must follow). One reservation at a time; BUSY
 * changes nothing. Reserved room is unavailable to submit until commit or
 * cancel, and hand-off only frees room. Commit cannot fail for capacity: it
 * admits all records or none, keeping the reservation on INVALID/ARGUMENT
 * (more records or bytes than reserved is ARGUMENT). If the session became
 * final, commit admits nothing, releases it and returns the final status. */
int sophia_ss_reserve(struct sophia_ss *, uint16_t slots, size_t bytes,
                      struct sophia_ss_reservation *);
int sophia_ss_commit(struct sophia_ss *, struct sophia_ss_reservation *,
                     const struct sophia_sf_record *records, size_t count, uint64_t *first_ticket);
int sophia_ss_cancel(struct sophia_ss *, struct sophia_ss_reservation *);
/* ARGUMENT for an unissued ticket. error is the submit Rlerror of a REFUSED
 * ticket (any valid Rlerror except EAGAIN, EALREADY and ESTALE, unknown errno
 * included, with no meaning attached), else 0; it may be NULL. */
int sophia_ss_outcome(const struct sophia_ss *, uint64_t ticket, enum sophia_ss_outcome *,
                      uint32_t *error);
/* Application events in order. Negotiated, Refused and Submitted are consumed
 * by the session. Borrow until consume. Consuming ObjectPublished obliges the
 * application to fetch that object before later events can be acknowledged.
 * Consume returns 0, or the final status when the session ended in its pass;
 * the event is consumed either way. Events received before an end remain
 * readable after it. */
int sophia_ss_event(struct sophia_ss *, const struct sophia_sf_record **);
int sophia_ss_consume(struct sophia_ss *);
/* Highest sequence safe to acknowledge: consumed, and below every unfetched
 * ObjectPublished. Ack sends it when it advances; BUSY while one is pending.
 * The server closes a reader whose acknowledgement makes no progress within
 * SOPHIA_SS_ACK_PROGRESS_MS while its journal is full. Consume, fetch
 * announced objects and ack before ack_due_ms. Frame permit and action ack
 * deadlines arrive in delivered events and remain the application's. */
uint64_t sophia_ss_ack_limit(const struct sophia_ss *);
int sophia_ss_ack(struct sophia_ss *);
int sophia_ss_obligations(const struct sophia_ss *, struct sophia_ss_obligations *);
/* One object fetch at a time, as sophia_sf_client_object. A fetch releases an
 * announcement only when the opened qid and decoded generation equal the
 * announced ones; a failed or different fetch leaves it owed. Either order
 * works: an announcement consumed after its object was already fetched and
 * verified is discharged at once, together with any older one it supersedes. */
int sophia_ss_object(struct sophia_ss *, uint16_t kind, uint64_t generation, uint64_t qid);
int sophia_ss_object_result(struct sophia_ss *, const struct sophia_sf_record **);
/* As sophia_sf_client_upload_*, including chunk borrowing. Begin, End and
 * Cancel share the single submission slot and are ticketed like any record;
 * they are accepted only in READY with nothing queued, reserved or in flight. */
int sophia_ss_upload_begin(struct sophia_ss *, struct sophia_sf_resource_begin, uint64_t *ticket);
int sophia_ss_upload_chunk(struct sophia_ss *, const void *, size_t);
int sophia_ss_upload_ready(const struct sophia_ss *);
int sophia_ss_upload_pending(const struct sophia_ss *);
int sophia_ss_upload_end(struct sophia_ss *, uint64_t transaction, uint64_t *ticket);
int sophia_ss_upload_cancel(struct sophia_ss *, uint64_t transaction, uint64_t *ticket);
/* Ends the session locally without closing fd: queued records become
 * DROPPED_UNSENT, an issued submit without Submitted UNKNOWN_DISCONNECTED. */
void sophia_ss_close(struct sophia_ss *);
#endif

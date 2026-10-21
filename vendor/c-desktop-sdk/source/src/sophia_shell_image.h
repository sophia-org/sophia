#ifndef SOPHIA_SHELL_IMAGE_H
#define SOPHIA_SHELL_IMAGE_H
#include "sophia_image_producer.h"
#include "sophia_owned_ring.h"
#include "sophia_shell_session.h"

/* Image adapter for a content-granted shell session: legacy sealed ISB
 * offers at revision 9, or one owned ring at revision 10 (see "Owned ring"
 * below). The mode follows the offer the session was opened with.
 *
 * ISB image adapter for a content-granted shell session at revision 9.
 * The caller owns this struct, attaches it to one live session before image
 * use, and keeps both alive and unmoved until the session
 * is final and the adapter is closed. Nothing here reads a clock: every call
 * takes CLOCK_MONOTONIC milliseconds, as the session does.
 *
 * Once r9 is negotiated with sealed images (bit 13), the adapter reads the
 * image-channel, image-limits and image-formats files through the session's
 * own 9P wire, one complete set per pass, and hands the set to the producer.
 * After discovery it connects the endpoint beneath endpoint_dir and attaches
 * the socket. Image journal events are consumed by the session and routed to
 * the producer; they never reach sophia_ss_event. ResourceStatus for a live
 * image offer is routed too and still delivered. ImageBegin is queued as an
 * ordinary session candidate with its own ticket, and so is a ResourceCancel
 * naming an offer.
 *
 * DMA-BUF is never requested. Default sessions never touch this file. */
#define SOPHIA_SS_IMAGE_TEXT_BYTES 2048u
#define SOPHIA_SS_IMAGE_LIMITS_BYTES 136u
/* Header plus one row: the smallest useful formats buffer. */
#define SOPHIA_SS_IMAGE_FORMATS_MIN 64u
/* ImageBegin: 32-byte header and the producer's 96-byte body. */
#define SOPHIA_SS_IMAGE_BEGIN_BYTES 128u

struct sophia_ss_image_config {
    /* role, revision and connection_epoch are set from the session;
     * granted_kinds from the negotiated capabilities. The rest is the
     * caller's: bounds, transfer timeout and startup deadline. */
    struct sophia_ip_config producer;
    /* Borrowed directory fd holding the role's 9P socket. */
    int endpoint_dir;
    /* Caller storage for the formats object, at least FORMATS_MIN bytes. */
    void *formats;
    size_t formats_capacity;
    /* r10 ring geometry; 0 selects 1 slot and the granted max_resource_bytes
     * rounded down to a multiple of four. Ignored at r9. */
    uint32_t ring_slots;
    uint64_t ring_capacity;
};

/* Pseudo record kind under which D4E1 journal events are consumed inside
 * the session; never delivered by sophia_ss_event. */
#define SOPHIA_SS_RING_EVENT 0xd4e1u

/* The connection's single ring. REFUSED and FAILED are final for the 9P
 * epoch; recovery is a new connection with its own registration. */
enum sophia_ss_ring_state {
    SOPHIA_SS_RING_IDLE,       /* channel not READY yet */
    SOPHIA_SS_RING_REGISTERING,/* Register queued or written; awaiting RingAdmitted */
    SOPHIA_SS_RING_ADMITTED,   /* source budget held; source packet pending or sent */
    SOPHIA_SS_RING_READY,      /* RingReady: slots may be offered */
    SOPHIA_SS_RING_REFUSED,    /* RingRefused or a refused Register write; see reason */
    SOPHIA_SS_RING_FAILED      /* closed channel, contradiction, send failure, journal loss */
};
/* Slot custody. Only FREE slots may be written; only SlotReleased returns an
 * offered slot. REFUSED, RELEASED and UNKNOWN wait for forget. */
enum sophia_ss_ring_slot_state {
    SOPHIA_SS_SLOT_FREE,
    SOPHIA_SS_SLOT_OFFERING, /* Offer queued or written; no reply yet */
    SOPHIA_SS_SLOT_OFFERED,  /* the control file accepted it */
    SOPHIA_SS_SLOT_REFUSED,  /* the control write was definitively refused; nothing journaled */
    SOPHIA_SS_SLOT_RELEASED, /* the exact SlotReleased arrived */
    SOPHIA_SS_SLOT_UNKNOWN   /* journal lost before release; never reused */
};
/* What the caller supplies per offer; the SDK assigns offer id and slot
 * generation and fills the identity from the session and ring. */
struct sophia_ss_ring_image {
    uint64_t resource_id, resource_generation;
    uint64_t allocation_id, allocation_generation, output_id, output_generation;
    uint64_t topology_generation;
    uint32_t width, height;
};
struct sophia_ss_ring_offer_info {
    struct sophia_or_offer offer;
    enum sophia_ss_ring_slot_state state;
    /* 0 until a terminal ResourceStatus for this grant and resource. Failed
     * ResourceRetire/Cancel replies share that record (always Rejected), so
     * while OFFERING/OFFERED this is evidence only (ACCEPTED once any
     * Accepted is seen, else the latest). After RELEASED it is the outcome the exact
     * SlotReleased selected, given a matching kind was seen: Accepted for
     * reason 0, Cancelled for 11, else Rejected. */
    enum sophia_ip_status status;
    uint32_t control_error;       /* errno of a refused Offer write */
    uint16_t reason;              /* SlotReleased reason */
    uint8_t changed_generation, cancel_requested;
    /* The role's transfer deadline passed while OFFERING or OFFERED. Local
     * only: custody still waits for the final status and SlotReleased. */
    uint8_t deadline_passed;
    /* An observed Accepted and the matching SlotReleased with reason 0 and no
     * change: the only offers native adoption may consume. */
    uint8_t adoptable;
};

/* Private state exposed for caller-owned allocation. Do not modify it. */
struct sophia_ss_image_ticket {
    uint64_t ticket, offer;
};
struct sophia_ss_ring_slot {
    struct sophia_or_offer offer;
    uint64_t deadline_ms;
    uint32_t control_error;
    /* Terminal ResourceStatus kinds seen, never cleared. Failed control
     * replies are Rejected too, so only SlotReleased picks the outcome, and
     * it must match a kind seen here. */
    uint16_t reason;
    uint8_t state, status, accepted, rejected, cancelled_seen, changed, cancelled,
        deadline_passed;
};
struct sophia_ss_image_ring {
    struct sophia_ip_ring memory;
    struct sophia_or_registration registration;
    struct sophia_ss_ring_slot slots[SOPHIA_OR_MAX_SLOTS];
    uint64_t next_offer, generation[SOPHIA_OR_MAX_SLOTS], retry_at;
    uint32_t retry_delay, control_error;
    uint16_t reason;
    /* control: 0 idle, 1 Register, 2 + slot for an Offer. */
    uint8_t state, control, control_written, source_sent;
    uint8_t control_bytes[SOPHIA_OR_OFFER_BYTES];
    uint8_t control_used;
};
struct sophia_ss_image {
    struct sophia_ss_image_config config;
    struct sophia_ip producer;
    struct sophia_ss *session;
    struct sophia_ss_image_ticket tickets[SOPHIA_IP_MAX_OFFERS];
    uint64_t pass, retry_at, status_seen;
    uint32_t retry_delay, read_error;
    size_t text_used, limits_used, formats_used;
    uint8_t stage, file, started, lost;
    uint8_t text[SOPHIA_SS_IMAGE_TEXT_BYTES];
    uint8_t limits[SOPHIA_SS_IMAGE_LIMITS_BYTES];
    struct sophia_ss_image_ring ring;
};

/* Validates and copies config; touches no session. */
int sophia_ss_image_init(struct sophia_ss_image *, const struct sophia_ss_image_config *);
/* After sophia_ss_open_*, during negotiation or in READY. Attaching after
 * negotiation lets callers derive bounds and transfer timeout from Limits.
 * Sealed mode: the offer must span revision 9, require content and bit 13
 * but not bit 14; a completed negotiation must grant r9, content and bit 13
 * without bit 14. Owned-ring mode: the offer must span revision 10 and
 * require content and bit 15 but neither 13 nor 14; negotiation must grant
 * r10, content and bit 15 without 13 or 14.
 * Role/grant checks remain with the
 * session. One adapter per session, never replaced after closure. ARGUMENT or
 * BUSY changes nothing. ChannelReady still gates every image offer. */
int sophia_ss_image_attach(struct sophia_ss *, struct sophia_ss_image *);
/* The producer's socket, or -1. Poll it beside the session's fd. */
int sophia_ss_image_poll_fd(const struct sophia_ss_image *);
short sophia_ss_image_poll_events(const struct sophia_ss_image *);
/* -1, or milliseconds until the producer or a discovery/connect retry is
 * due. On expiry call sophia_ss_image_dispatch, then sophia_ss_dispatch. */
int64_t sophia_ss_image_timeout(const struct sophia_ss_image *, uint64_t now_ms);
/* Producer socket service plus discovery and connect progress. Returns the
 * producer's changed mask (SOPHIA_IP_CHANGED_*), or -ARGUMENT. */
int sophia_ss_image_dispatch(struct sophia_ss_image *, short revents, uint64_t now_ms);
/* Queues one sealed source as an ImageBegin candidate. READY sessions only.
 * On OK the producer owns source_fd, *offer names it and *ticket is the
 * session ticket for its ImageBegin. BUSY (no queue room), AGAIN (channel
 * not ready) and every refusal leave source_fd with the caller. Shell r10
 * (owned ring) refuses every call. */
int sophia_ss_image_begin(struct sophia_ss_image *, struct sophia_ic_metadata *, int source_fd,
                          uint64_t now_ms, uint64_t *offer, uint64_t *ticket);
/* Queues ResourceCancel naming offer: transaction is the offer ID, the
 * grant's content epoch is the metadata's admission epoch. READY sessions
 * only. Queue room is checked first; then the producer marks the offer
 * (sophia_ip_cancel), so its source is never sent afterward. The cancel may
 * queue behind its ImageBegin. On OK *ticket is the session ticket, for the
 * caller only: whatever its outcome, the mark stays and custody is unchanged.
 * Session end is journal loss as for any offer. BUSY (no room),
 * REFUSED (a status, a mark, or no live custody) and ARGUMENT (unknown
 * offer) change nothing. Acceptance may still win; settlement is only
 * SourceReleased. */
int sophia_ss_image_cancel(struct sophia_ss_image *, uint64_t offer, uint64_t now_ms,
                           uint64_t *ticket);
/* The producer, for offer, path, cause and forget queries. */
struct sophia_ip *sophia_ss_image_producer(struct sophia_ss_image *);

/* Owned ring (r10, spec/sophia-owned-ring.md).
 * Once ChannelReady grants exactly kind 4, the adapter creates the ring
 * memory and writes Register to owned-ring-v1 (eager: before any image).
 * On RingAdmitted it sends the D4F1 source packet with the ring fd on the
 * channel socket; RingReady makes slots offerable. A budget refusal is
 * RingRefused, final for this connection: handle it, there is no fallback.
 * EAGAIN control replies are retried with the session's 4..256 ms backoff,
 * resending the identical bytes; other errnos refuse definitively.
 *
 * Per image: take a FREE slot's pixels, write width*height*4 premultiplied
 * BGRA bytes from its start, then offer it. The SDK writes the slot's
 * generation word before the Offer and then never touches the slot; the
 * caller must not either until it is released and forgotten. ResourceStatus
 * is matched by grant and resource identity, never by transaction alone (a
 * cancel may carry its own). SlotReleased must name the exact identity,
 * offer, slot, slot generation and resource, after that offer's final
 * status; anything else fails the ring and keeps every slot's custody.
 * Failed ResourceCancel/ResourceRetire replies are ordinary submission
 * outcomes and never settle a slot. Journal loss makes live slots UNKNOWN.
 * Calls other than state queries need r10 mode: ARGUMENT otherwise. */
enum sophia_ss_ring_state sophia_ss_image_ring_state(const struct sophia_ss_image *,
                                                     uint16_t *reason);
/* The registered geometry, or 0/NULL before memory exists. */
uint64_t sophia_ss_image_ring_capacity(const struct sophia_ss_image *, uint32_t *slots);
/* A FREE slot's pixel bytes in a READY ring, else NULL. */
void *sophia_ss_image_ring_pixels(struct sophia_ss_image *, uint32_t slot);
/* Offers a FREE slot. AGAIN: ring not READY (or the session is not), or a
 * control write is still active. REFUSED: a check failed locally (slot not
 * free, zero or over-limit dimensions, width*height*4 above capacity, a
 * resource already live in another slot); nothing was spent. On OK *offer
 * names it and the slot is OFFERING. */
int sophia_ss_image_ring_offer(struct sophia_ss_image *, uint32_t slot,
                               const struct sophia_ss_ring_image *, uint64_t now_ms,
                               uint64_t *offer);
int sophia_ss_image_ring_info(const struct sophia_ss_image *, uint64_t offer,
                              struct sophia_ss_ring_offer_info *);
/* Non-FREE slots in slot order; *cursor starts at 0. Returns 0 when done. */
int sophia_ss_image_ring_next(const struct sophia_ss_image *, unsigned *cursor,
                              struct sophia_ss_ring_offer_info *);
/* Queues ResourceCancel for an OFFERED slot with no final status; the
 * transaction is the offer id. The mark is permanent; acceptance may still
 * win, and the slot still waits for SlotReleased. BUSY (no queue room),
 * REFUSED (status, mark or wrong state), ARGUMENT (unknown offer). */
int sophia_ss_image_ring_cancel(struct sophia_ss_image *, uint64_t offer, uint64_t now_ms,
                                uint64_t *ticket);
/* Frees a REFUSED, RELEASED or UNKNOWN slot for reuse (UNKNOWN only ever
 * on a failed ring, so it is never offered again). */
int sophia_ss_image_ring_forget(struct sophia_ss_image *, uint64_t offer);
/* Closes the producer socket and unmaps the ring. Sophia keeps its own
 * source reference; slot custody states remain readable. Idempotent. */
void sophia_ss_image_close(struct sophia_ss_image *);
#endif

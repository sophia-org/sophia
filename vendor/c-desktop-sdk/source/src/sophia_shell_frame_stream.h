#ifndef SOPHIA_SHELL_FRAME_STREAM_H
#define SOPHIA_SHELL_FRAME_STREAM_H
#include "sophia_shell_session.h"

/* One-credit SFT owner. Zero initialization is required. Route every SFT event
 * here in journal order; disconnect on authority/transport loss. This owns frame
 * credit, not source slots or resources. Fields are SDK-owned; do not copy or
 * reset an initialized owner. Never reset Unknown to evade custody. */
enum sophia_ss_frame_state {
    SOPHIA_SS_FRAME_CLOSED,
    SOPHIA_SS_FRAME_OPENING,
    SOPHIA_SS_FRAME_OPEN,
    SOPHIA_SS_FRAME_CLOSING,
    SOPHIA_SS_FRAME_UNKNOWN
};
struct sophia_ss_frame_stream {
    enum sophia_ss_frame_state state;
    struct sophia_sft_key key;
    uint64_t opening, closing, last_stream, last_candidate;
    uint64_t pending_transaction, pending_generation;
    uint8_t credit, closed_notice, prepared;
};
/* Admit Open before mutating the stream. Reopen requires Closed and every old
 * frame drained, the same grant, and a stream id above the last accepted id.
 * Refusal preserves stream and ticket. Generations stay spent after a refusal. */
int sophia_ss_frame_open(struct sophia_ss *, struct sophia_ss_frame_stream *,
                        uint64_t transaction, const struct sophia_sft_key *, uint64_t *ticket);
/* A whole Commit uses one credit only after queue acceptance. The producer must
 * first join any image-slot ownership; this API does not submit a separate Offer.
 * No credit: BUSY; wrong binding/direction/generation: ARGUMENT; queue/value
 * errors follow sophia_ss_submit. Failure preserves stream and ticket. */
int sophia_ss_frame_submit(struct sophia_ss *, struct sophia_ss_frame_stream *,
                          const struct sophia_sf_record *, uint64_t *ticket);
/* Close stops admissions, not accepted work. A refused Close can be retried;
 * reopening waits for both the close notice and all accepted frame outcomes.
 * Close needs a transaction distinct from Open so automatic expiry is unambiguous. */
int sophia_ss_frame_close(struct sophia_ss *, struct sophia_ss_frame_stream *,
                         uint64_t transaction, uint64_t *ticket);
int sophia_ss_frame_has_credit(const struct sophia_ss_frame_stream *);
/* Only frame/stream work; source and resource owners are independent. */
int sophia_ss_frame_drained(const struct sophia_ss_frame_stream *);
/* Every event exactly once, before consume/ACK. Invalid identity, order or value
 * returns INVALID, enters Unknown and retains outstanding work. Prepared and
 * custody receipts never return credit. Owner deadlines stay opaque. */
int sophia_ss_frame_observe(struct sophia_ss_frame_stream *, const struct sophia_sf_record *);
void sophia_ss_frame_disconnect(struct sophia_ss_frame_stream *);
/* Join a filled free ring slot to one whole Commit. No legacy Offer is sent.
 * Requires r13 and exactly two slots; record must have has_image == 0.
 * The SDK inserts the exact offer while encoding and restores the record.
 * On refusal no credit, slot generation, offer or ticket is spent. On success
 * stop writing the slot until exact SlotReleased (frame completion is not release).
 * Returns 9P queue/argument errors; image metadata/availability is validated first.
 * now_ms is the session's monotonic domain. The caller must not drive concurrently. */
struct sophia_ss_image;
struct sophia_ss_ring_image;
int sophia_ss_frame_submit_ring(struct sophia_ss_image *, struct sophia_ss_frame_stream *,
                               uint32_t slot, const struct sophia_ss_ring_image *,
                               struct sophia_sf_record *, uint64_t now_ms,
                               uint64_t *ticket, uint64_t *offer);
/* Establish retained images before this owner's first Open using ordinary image
 * settlement. Keep one owner per output binding; settle sources before opening.
 * This neither grants credit nor selects fallback. The host still checks that no
 * live stream owns the output. Requires an unused owner and matching key/metadata;
 * UNKNOWN or any previously queued Open refuses. Returns SOPHIA_IPR_* with the same ownership,
 * source-custody and failure rules as sophia_ss_frame_legacy_ring_offer. */
int sophia_ss_frame_initial_ring_offer(struct sophia_ss_image *, const struct sophia_ss_frame_stream *,
                                      const struct sophia_sft_key *, uint32_t slot,
                                      const struct sophia_ss_ring_image *, uint64_t now_ms, uint64_t *offer);
/* Legacy Offer after frame/close work AND SFT sources for the output have drained.
 * The metadata must match the drained key. Registration and accepted resources
 * survive; follow normal image settlement and demand/permit/candidate handling.
 * Keep this output binding on the legacy path; reconsider SFT only on a new binding.
 * Returns SOPHIA_IPR_* as sophia_ss_image_ring_offer: wrong phase/binding is ARGUMENT,
 * pending SFT source is BUSY, uncertain source/closed session is CLOSED.
 * Failure leaves the pixels and offer result unchanged. Caller synchronizes with
 * its worker and retains the mapping exactly as for ordinary ring offers. */
int sophia_ss_frame_legacy_ring_offer(struct sophia_ss_image *, const struct sophia_ss_frame_stream *,
                                     uint32_t slot, const struct sophia_ss_ring_image *,
                                     uint64_t now_ms, uint64_t *offer);
#endif

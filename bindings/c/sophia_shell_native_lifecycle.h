#ifndef SOPHIA_SHELL_NATIVE_LIFECYCLE_H
#define SOPHIA_SHELL_NATIVE_LIFECYCLE_H
#include "sophia_shell_native_launcher.h"
#include "sophia_shell_content_feedback.h"
#include "sophia_shell_outbox.h"
#ifdef __cplusplus
extern "C" {
#endif
struct sophia_shell_native_lifecycle;
struct sophia_shell_native_lifecycle_snapshot {
    int open, presented, focused, candidate_pending, activation_pending, closing;
    struct sophia_shell_native_opening opening;
    uint64_t state_revision, candidate_generation, presentation_epoch;
    struct sophia_shell_native_binding focus;
    uint16_t selected;
};
/* The serialized callback applies a validated semantic edit to the UI and
 * returns nonzero for consumed, zero for refused. Called at most once, after
 * ACK storage is reserved. It must not reenter the client, use its socket or
 * retain borrowed text. Accept/activation never invokes this callback. */
typedef int (*sophia_shell_native_edit)(void *user, uint16_t kind,
                                      const uint8_t *text, size_t bytes);
int sophia_shell_native_lifecycle_new(const struct sophia_shell_frame *limits,
    uint64_t committed_catalog_generation, struct sophia_shell_outbox *outbox,
    struct sophia_shell_native_lifecycle **out);
/* Catalog installation is the caller's separate validated catalog/menu owner.
 * Replacement disarms old interaction; it does not recreate an Opening. */
int sophia_shell_native_lifecycle_catalog(struct sophia_shell_native_lifecycle *owner,
                                        uint64_t committed_generation);
/* Queue the actual Begin and complete one-surface chunk atomically, retaining
 * copied scene/row/target identities until the matching terminal outcome.
 * Both candidate_generation fields must be zero; this owner mints a grant-wide
 * generation only on successful FIFO admission. Caller supplies the captured
 * opening/revision and an actual allocation, resident resources and permit.
 * This helper checks wire/profile/state coherence, not those separate owners. */
int sophia_shell_native_lifecycle_offer(struct sophia_shell_native_lifecycle *owner,
    const struct sophia_shell_native_candidate *begin,
    const struct sophia_shell_native_chunk *chunk, uint64_t *next_transaction);
/* Queue retained CandidateEnd, at most one frame. No GPU work or socket I/O. */
int sophia_shell_native_lifecycle_pump(struct sophia_shell_native_lifecycle *owner,
                                     uint64_t *next_transaction);
/* Dispatch native inbound kinds and common CandidateOutcome/Action in ORIGINAL
 * socket FIFO order. On BUSY retain this exact frame and retry it; no second edit
 * occurs. No action-before-Presented promotion, no ACK for cancellation, and no
 * optimistic local launch. Uses the caller's single shared transaction source.
 * Other content/catalog records belong to their respective client owners. */
int sophia_shell_native_lifecycle_receive(struct sophia_shell_native_lifecycle *owner,
    const struct sophia_shell_frame *frame, uint64_t *next_transaction,
    sophia_shell_native_edit apply, void *user);
int sophia_shell_native_lifecycle_inspect(const struct sophia_shell_native_lifecycle *owner,
                                        struct sophia_shell_native_lifecycle_snapshot *out);
/* Disconnect-only disposal; owned outbox and resource lifetimes are separate. */
void sophia_shell_native_lifecycle_dispose(struct sophia_shell_native_lifecycle *owner);
#ifdef __cplusplus
}
#endif
#endif

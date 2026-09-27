#ifndef SOPHIA_SHELL_UPLOAD_H
#define SOPHIA_SHELL_UPLOAD_H
#include "sophia_shell_content_limits.h"
#include "sophia_shell_content_resource.h"
#include "sophia_shell_outbox.h"
#ifdef __cplusplus
extern "C" {
#endif
#define SOPHIA_SHELL_UPLOAD_SLOTS 2u
struct sophia_shell_upload;
enum sophia_shell_upload_state {
    SOPHIA_UPLOAD_EMPTY, SOPHIA_UPLOAD_STAGED, SOPHIA_UPLOAD_BEGIN_PENDING,
    SOPHIA_UPLOAD_CHUNKS, SOPHIA_UPLOAD_END_PENDING, SOPHIA_UPLOAD_RESIDENT,
    SOPHIA_UPLOAD_CANCEL_READY, SOPHIA_UPLOAD_CANCEL_PENDING,
    SOPHIA_UPLOAD_RETIRE_READY, SOPHIA_UPLOAD_RELEASE_PENDING
};
struct sophia_shell_upload_snapshot {
    enum sophia_shell_upload_state state;
    struct sophia_shell_resource_key key;
    uint64_t bytes;
    uint32_t chunks_queued;
};
/* Two immutable copied-raster slots for one connection grant. Validates the
 * actual Limits record, allocates fixed bounded bookkeeping, performs no I/O.
 * One owner must allocate all resource IDs for this connection. */
int sophia_shell_upload_new(const struct sophia_shell_frame *limits,
                           struct sophia_shell_upload **out);
/* Tight BGRA8 premultiplied rows. Copies only after limits/scale/byte/slot checks.
 * Failed admission leaves slot/result untouched. IDs and generations are minted
 * lazily when Begin is owned by the outbox, not while staging pixels. */
int sophia_shell_upload_stage(struct sophia_shell_upload *owner,
    uint32_t width, uint32_t height, uint32_t scale_numerator, uint32_t scale_denominator,
    const uint8_t *pixels, size_t bytes, unsigned *slot);
/* At most one owned frame per visit, round-robin between slots. Caller supplies
 * its shared next transaction (>0); it advances only after FIFO admission.
 * BUSY preserves the exact pending record, pixels and generation. Flush the
 * same outbox separately with bounded service, retaining control reservations. */
int sophia_shell_upload_pump(struct sophia_shell_upload *owner,
    struct sophia_shell_outbox *outbox, uint64_t *next_transaction);
/* Exact wire reply/transaction/grant/resource matching. No Prepared/Presented
 * handling here. Rejected Retire leaves RESIDENT; only matched Released frees
 * a retiring slot. A rejected Begin still consumed its enqueued generation. */
int sophia_shell_upload_reply(struct sophia_shell_upload *owner,
                             const struct sophia_shell_frame *frame);
/* Withdraw staging/transfer, or request retirement of a resident. Caller must
 * first stop referencing that resource in new candidates. Existing server
 * consumers remain independent; this never synthesizes ResourceReleased. */
int sophia_shell_upload_cancel(struct sophia_shell_upload *owner, unsigned slot);
int sophia_shell_upload_retire(struct sophia_shell_upload *owner, unsigned slot);
int sophia_shell_upload_inspect(const struct sophia_shell_upload *owner, unsigned slot,
                               struct sophia_shell_upload_snapshot *out);
/* Only after connection termination. Releases local copies, NOT remote owners
 * or grant credit; Session independently retains any outstanding consumers. */
void sophia_shell_upload_dispose(struct sophia_shell_upload *owner);
#ifdef __cplusplus
}
#endif
#endif

#ifndef SOPHIA_SHELL_FRAME_TYPES_H
#define SOPHIA_SHELL_FRAME_TYPES_H
/* Shared passive SFT values; codec and owning file client keep explicit r13 gates. */
#include "sophia_owned_ring.h"
#include "sophia_shell_files_content.h"

#define SOPHIA_SFT_REVISION 13u
#define SOPHIA_SFT_CAPABILITY (UINT64_C(1) << 18)
/* Required r13 option; internal preparation remains mandatory. */
#define SOPHIA_SFT_TERMINAL_ONLY_CAPABILITY (UINT64_C(1) << 19)
#define SOPHIA_SFT_MAX_BYTES 8192u
enum sophia_sft_kind {
    SOPHIA_SFT_OPENED = 55,
    SOPHIA_SFT_CLOSED = 56,
    SOPHIA_SFT_REFUSED = 57,
    SOPHIA_SFT_OUTCOME = 58,
    SOPHIA_SFT_OPEN = 279,
    SOPHIA_SFT_CLOSE = 280,
    SOPHIA_SFT_COMMIT = 281
};
enum sophia_sft_class { SOPHIA_SFT_REQUEST, SOPHIA_SFT_EVENT };
struct sophia_sft_key {
    uint64_t connection_epoch, content_epoch, output_id, output_generation;
    uint64_t allocation_id, allocation_generation, facts_generation, stream_id;
};
/* ttl_ms is used only by Opened (credit is exactly one); reason by Closed.
 * Other kinds ignore these fields. All key words and transaction are nonzero. */
struct sophia_sft_stream {
    uint64_t transaction;
    struct sophia_sft_key key;
    uint32_t ttl_ms;
    uint16_t reason;
};
struct sophia_sft_retire {
    uint64_t transaction, resource_id, resource_generation;
};
struct sophia_sft_commit {
    struct sophia_sf_candidate candidate;
    uint8_t has_image, has_retire;
    struct sophia_or_offer image;
    struct sophia_sft_retire retire;
};
struct sophia_sft_refused {
    uint64_t transaction, connection_epoch, content_epoch, output_id, output_generation;
    uint64_t stream_id, candidate_generation;
    uint16_t request_kind, reason;
};
struct sophia_sft_outcome {
    struct sophia_sf_candidate_outcome candidate;
    uint64_t stream_id, allocation_id, allocation_generation, facts_generation;
    /* Owner-clock deadline, opaque to clients. Zero means no returned credit. */
    uint64_t credit_expires_msec;
};
union sophia_sft_value {
    struct sophia_sft_stream stream;
    struct sophia_sft_commit commit;
    struct sophia_sft_refused refused;
    struct sophia_sft_outcome outcome;
};
#endif

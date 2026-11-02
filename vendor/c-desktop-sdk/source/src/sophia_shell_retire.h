#ifndef SOPHIA_SHELL_RETIRE_H
#define SOPHIA_SHELL_RETIRE_H
#include "sophia_shell_files.h"

enum sophia_sr_status {
    SOPHIA_SR_OK = 0,
    SOPHIA_SR_ARGUMENT = -1,
    SOPHIA_SR_DUPLICATE = -2,
    SOPHIA_SR_WRONG_GRANT = -3,
    SOPHIA_SR_MISATTRIBUTED = -4,
    SOPHIA_SR_CAPACITY = -5
};
enum sophia_sr_progress_kind {
    SOPHIA_SR_UNRELATED,
    SOPHIA_SR_RETIRING,
    SOPHIA_SR_KEPT,
    SOPHIA_SR_RELEASED
};
struct sophia_sr_progress {
    enum sophia_sr_progress_kind kind;
    uint64_t resource_id, resource_generation;
};
/* Caller-owned bounded storage; fields are implementation state. */
struct sophia_sr_entry {
    uint64_t output_id, output_generation, candidate_generation;
    struct sophia_sf_candidate_retire retire;
    uint8_t state;
};
struct sophia_sr {
    uint64_t connection_epoch, content_epoch;
    struct sophia_sr_entry *entries;
    size_t capacity;
};
/* Does no I/O or allocation. Storage must remain live until the tracker is
 * discarded. Name only locally admitted intents; observe decoded records in
 * journal order. A held predecessor must not be explicitly retired or reused.
 * Revoke on the end of this grant; initialize a new tracker for its successor. */
int sophia_sr_init(struct sophia_sr *, uint64_t connection_epoch, uint64_t content_epoch,
    struct sophia_sr_entry *, size_t capacity);
int sophia_sr_name(struct sophia_sr *, uint64_t output_id, uint64_t output_generation,
    uint64_t candidate_generation, const struct sophia_sf_candidate_retire *);
int sophia_sr_holds(const struct sophia_sr *, uint64_t resource_id, uint64_t resource_generation);
int sophia_sr_observe(struct sophia_sr *, const struct sophia_sf_record *, struct sophia_sr_progress *);
void sophia_sr_revoke(struct sophia_sr *);
#endif

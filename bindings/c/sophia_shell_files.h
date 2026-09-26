#ifndef SOPHIA_SHELL_FILES_H
#define SOPHIA_SHELL_FILES_H
#include "sophia_shell_files_content.h"
#include <stddef.h>
#include <stdint.h>
#define SOPHIA_SF_HEADER_BYTES 32u
#define SOPHIA_SF_MAX_RECORD 65536u
enum sophia_sf_kind {
    SOPHIA_SF_LIMITS = 1,
    SOPHIA_SF_OUTPUTS = 2,
    SOPHIA_SF_NEGOTIATED = 16,
    SOPHIA_SF_REFUSED = 17,
    SOPHIA_SF_SUBMITTED = 18,
    SOPHIA_SF_OBJECT_PUBLISHED = 19,
    SOPHIA_SF_ALLOCATION_RESULT = 32,
    SOPHIA_SF_RESOURCE_STATUS = 33,
    SOPHIA_SF_RESOURCE_RELEASED = 34,
    SOPHIA_SF_CANDIDATE_OUTCOME = 35,
    SOPHIA_SF_FRAME_PERMIT = 36,
    SOPHIA_SF_ACTION = 37,
    SOPHIA_SF_NEGOTIATE = 256,
    SOPHIA_SF_ALLOCATION_REQUEST = 257,
    SOPHIA_SF_RESOURCE_BEGIN = 258,
    SOPHIA_SF_RESOURCE_END = 259,
    SOPHIA_SF_RESOURCE_CANCEL = 260,
    SOPHIA_SF_RESOURCE_RETIRE = 261,
    SOPHIA_SF_CANDIDATE = 262,
    SOPHIA_SF_FRAME_DEMAND = 263,
    SOPHIA_SF_FRAME_DEMAND_CANCEL = 264,
    SOPHIA_SF_ACTION_ACK = 265,
};
struct sophia_sf_header {
    uint16_t kind;
    uint64_t epoch, submission, sequence;
};
struct sophia_sf_record {
    struct sophia_sf_header header;
    union {
        struct sophia_sf_limits limits;
        struct sophia_sf_outputs outputs;
        struct sophia_sf_negotiated negotiated;
        struct sophia_sf_refused refused;
        struct sophia_sf_submitted submitted;
        struct sophia_sf_object_published object_published;
        struct sophia_sf_allocation_result allocation_result;
        struct sophia_sf_resource_status resource_status;
        struct sophia_sf_resource_released resource_released;
        struct sophia_sf_candidate_outcome candidate_outcome;
        struct sophia_sf_frame_permit frame_permit;
        struct sophia_sf_action action;
        struct sophia_sf_negotiate negotiate;
        struct sophia_sf_allocation_request allocation_request;
        struct sophia_sf_resource_begin resource_begin;
        struct sophia_sf_resource_end resource_end;
        struct sophia_sf_resource_cancel resource_cancel;
        struct sophia_sf_resource_retire resource_retire;
        struct sophia_sf_candidate candidate;
        struct sophia_sf_frame_demand frame_demand;
        struct sophia_sf_frame_demand_cancel frame_demand_cancel;
        struct sophia_sf_action_ack action_ack;
    } value;
};
/* Validate complete records; output arguments and destination stay unchanged on error.
 * Return 0 on success, -1 for invalid wire/value, -4 for arguments/capacity.
 * Semantic admission, grants and presentation remain with their owners. */
int sophia_sf_encode(void *, size_t, const struct sophia_sf_record *, size_t *);
int sophia_sf_decode(const void *, size_t, struct sophia_sf_record *);
int sophia_sf_submit_encode(uint8_t dst[24], uint64_t epoch, uint64_t submission, uint32_t bytes);
int sophia_sf_ack_encode(uint8_t dst[16], uint64_t epoch, uint64_t sequence);
#endif

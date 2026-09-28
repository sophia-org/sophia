#ifndef SOPHIA_SF_SESSION_INTERNAL_H
#define SOPHIA_SF_SESSION_INTERNAL_H
#include "../sophia_shell_files_client.h"
#include "internal.h"
int sf_api_epoch(const uint8_t *, size_t, uint64_t *);
int sf_api_profile(const uint8_t *, size_t, enum sophia_sf_profile);
int sf_descriptor_offer(const struct sophia_sf_negotiate *);
int sf_descriptor_welcome(const struct sophia_sf_client *, const struct sophia_sf_negotiated *);
int sf_descriptor_object_allowed(const struct sophia_sf_client *, uint16_t);
int sf_descriptor_record_allowed(const struct sophia_sf_client *, uint16_t);
int sf_session_candidate_allowed(const struct sophia_sf_client *, uint16_t);
int sf_session_event_allowed(const struct sophia_sf_client *, const struct sophia_sf_record *);
int sf_session_queue(struct sophia_sf_client *, const struct sophia_sf_record *);
int sf_session_drive(struct sophia_sf_client *);
int sf_session_event_parse(struct sophia_sf_client *);
int sf_session_object_reply(struct sophia_sf_client *, const struct sophia_9p_reply *);
int sf_session_upload_reply(struct sophia_sf_client *, const struct sophia_9p_reply *);
int sf_session_upload_drive(struct sophia_sf_client *);
int sf_session_upload_event(struct sophia_sf_client *, const struct sophia_sf_record *);
int sf_session_object_drive(struct sophia_sf_client *);
static inline int sf_same(struct sophia_sf_operation *op, struct sophia_9p_handle h)
{
    return op->active && op->handle.slot == h.slot && op->handle.serial == h.serial;
}
static inline int sf_started(struct sophia_sf_operation *op, int result)
{
    if (!result)
        op->active = 1;
    return result == SOPHIA_9P_BUSY ? 0 : result;
}
#endif

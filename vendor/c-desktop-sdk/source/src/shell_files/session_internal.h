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
int sf_session_attached_wire(const struct sophia_9p_client *, uint32_t root, uint16_t fids);
int sf_session_event_parse(struct sophia_sf_client *);
int sf_session_object_reply(struct sophia_sf_client *, const struct sophia_9p_reply *);
int sf_session_object_drive(struct sophia_sf_client *);
/* Shell r9 with the image adapter attached and sealed images granted. */
int sf_session_image_r9(const struct sophia_sf_client *);
int sf_session_image_kind(uint16_t kind);
/* An encoded ImageBegin candidate for this epoch, whole and canonical. */
int sf_session_image_begin_valid(const struct sophia_sf_client *, const uint8_t *, size_t);
/* Decodes one journal record with the negotiated codec and checks epoch,
 * event kind and profile; image events fill only the header. 0 or -1. */
int sf_session_event_decode(const struct sophia_sf_client *, const uint8_t *, size_t,
                            struct sophia_sf_record *);
/* One whole small file from the root: walk, open, read to EOF, clunk.
 * BUSY while another read is active. The result is 0 with *bytes, or
 * AGAIN/INVALID with the Rlerror in *error (0 when the file overflowed or
 * the reply was malformed). */
int sf_session_file_read(struct sophia_sf_client *, const char *name, void *buf, size_t cap);
int sf_session_file_result(struct sophia_sf_client *, size_t *bytes, uint32_t *error);
int sf_session_file_drive(struct sophia_sf_client *);
int sf_session_file_reply(struct sophia_sf_client *, const struct sophia_9p_reply *);
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

#ifndef SOPHIA_LOCK_CLIENT_INTERNAL_H
#define SOPHIA_LOCK_CLIENT_INTERNAL_H
#include "../sophia_lock_client.h"
#include <string.h>

/* Bootstrap steps, one request each; reads repeat until EOF. */
enum {
  LC_VERSION,
  LC_ATTACH,
  LC_API_WALK,
  LC_API_OPEN,
  LC_API_READ,
  LC_API_CLUNK,
  LC_LIMITS_WALK,
  LC_LIMITS_OPEN,
  LC_LIMITS_READ,
  LC_LIMITS_CLUNK,
  LC_STREAMS, /* walk then open events, transaction, submit, ack */
  LC_BOOTED = LC_STREAMS + 8
};
/* Submission: stage the record, hand it over, then renew the transaction. */
enum {
  LC_TX_IDLE,
  LC_TX_WRITE,
  LC_TX_SUBMIT,
  LC_TX_WAIT,
  LC_TX_CLUNK,
  LC_TX_WALK,
  LC_TX_OPEN
};
enum { LC_OBJECT_IDLE, LC_OBJECT_WALK, LC_OBJECT_OPEN, LC_OBJECT_READ,
       LC_OBJECT_CLUNK };
enum {
  LC_UPLOAD_IDLE,
  LC_UPLOAD_BEGUN, /* ResourceBegin submitted; waiting for admitted */
  LC_UPLOAD_WALK,
  LC_UPLOAD_OPEN,
  LC_UPLOAD_READY,
  LC_UPLOAD_WRITE,
  LC_UPLOAD_ENDING, /* End or Cancel submitted; waiting for its status */
  LC_UPLOAD_CLUNK
};
#define LC_EVENTS 0
#define LC_TRANSACTION 1
#define LC_SUBMIT 2
#define LC_ACK 3

static inline uint64_t lc_get(const uint8_t *p, size_t n) {
  uint64_t v = 0;
  size_t i;
  for (i = 0; i < n; ++i)
    v |= (uint64_t)p[i] << (i * 8);
  return v;
}
static inline int lc_same(const struct sophia_lc_operation *op,
                          struct sophia_9p_handle h) {
  return op->active && op->handle.slot == h.slot &&
         op->handle.serial == h.serial;
}
static inline int lc_started(struct sophia_lc_operation *op, int result) {
  if (!result)
    op->active = 1;
  return result == SOPHIA_9P_BUSY ? 0 : result;
}
static inline uint32_t lc_cap(const struct sophia_lc_client *c, size_t want,
                              uint32_t overhead, uint32_t iounit) {
  size_t n = want, wire = c->wire->msize - overhead;
  if (n > wire)
    n = wire;
  if (iounit && n > iounit)
    n = iounit;
  return (uint32_t)n;
}
/* ESTALE on a stream marks the connection epoch revoked, not a violation. */
static inline int lc_remote(struct sophia_lc_client *c, uint32_t error) {
  c->remote_error = error;
  if (error == 116)
    c->stale = 1;
  return SOPHIA_9P_INVALID;
}
int sophia_lc_internal_queue(struct sophia_lc_client *,
                             const struct sophia_lf_record *);
int sophia_lc_internal_parse(struct sophia_lc_client *);
int sophia_lc_internal_object_drive(struct sophia_lc_client *);
int sophia_lc_internal_object_reply(struct sophia_lc_client *,
                                    const struct sophia_9p_reply *);
int sophia_lc_internal_upload_drive(struct sophia_lc_client *);
int sophia_lc_internal_upload_reply(struct sophia_lc_client *,
                                    const struct sophia_9p_reply *);
int sophia_lc_internal_upload_write_reply(struct sophia_lc_client *,
                                          const struct sophia_9p_reply *);
void sophia_lc_internal_upload_status(struct sophia_lc_client *,
                                      const struct sophia_lf_resource_status *);
void sophia_lc_internal_upload_refused(struct sophia_lc_client *,
                                       uint16_t kind);
#endif

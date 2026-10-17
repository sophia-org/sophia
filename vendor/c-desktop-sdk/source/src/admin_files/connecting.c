#include "../sophia_admin_connecting.h"
#include <string.h>
enum { VERSION = 1, FILES, CATALOG, TAKEN };
static int disjoint(const void *a, size_t an, const void *b, size_t bn) {
  uintptr_t av = (uintptr_t)a, bv = (uintptr_t)b;
  return av <= UINTPTR_MAX - an && bv <= UINTPTR_MAX - bn &&
         (av + an <= bv || bv + bn <= av);
}
static int fail(struct sophia_ad_connecting *c, int error, int component) {
  if (!c->error) {
    c->error = error;
    c->component_error = component;
  }
  if (c->phase == FILES)
    sophia_ao_close(&c->opening);
  if (c->phase == CATALOG)
    sophia_ad_close(c->session);
  return c->error;
}
int sophia_ad_connect_init(struct sophia_ad_connecting *c,
                           struct sophia_9p_client *wire,
                           struct sophia_ad *session, uint64_t now,
                           uint64_t deadline) {
  size_t i, bytes;
  int r;
  if (!c || !wire || !session || wire->phase || wire->terminal ||
      wire->fid_count || wire->rx_used || wire->capacity < 2 ||
      wire->capacity > SOPHIA_9P_REQUESTS || wire->fid_limit < 4 ||
      wire->msize < 4096 || wire->msize > 65536)
    return SOPHIA_AD_INVALID;
  if (deadline <= now)
    return SOPHIA_AD_SNAPSHOT;
  bytes = sophia_9p_storage_bytes(wire->offered, wire->capacity);
  if (!bytes || !disjoint(c, sizeof *c, session, sizeof *session) ||
      !disjoint(c, sizeof *c, wire, sizeof *wire) ||
      !disjoint(c, sizeof *c, wire->storage, bytes) ||
      !disjoint(session, sizeof *session, wire, sizeof *wire) ||
      !disjoint(session, sizeof *session, wire->storage, bytes))
    return SOPHIA_AD_INVALID;
  for (i = 0; i < 2u * wire->capacity; ++i)
    if (wire->slots[i].state)
      return SOPHIA_AD_INVALID;
  memset(c, 0, sizeof *c);
  c->wire = wire;
  c->session = session;
  c->now = now;
  c->deadline = deadline;
  c->phase = VERSION;
  r = sophia_9p_version(wire, &c->version);
  return r ? fail(c, SOPHIA_AD_WIRE, r) : 0;
}
int sophia_ad_connect_ready(const struct sophia_ad_connecting *c) {
  return c && !c->error && c->phase == CATALOG &&
         sophia_ad_catalog(c->session) != NULL;
}
int sophia_ad_connect_dispatch(struct sophia_ad_connecting *c, uint64_t now,
                               size_t budget) {
  int r;
  if (!c)
    return SOPHIA_AD_INVALID;
  if (now < c->now)
    return SOPHIA_AD_CLOCK_REGRESSION;
  if (c->error)
    return c->error;
  c->now = now;
  if (c->phase == TAKEN)
    return SOPHIA_AD_CLOSED;
  if (sophia_ad_connect_ready(c))
    return 0;
  if (now >= c->deadline)
    return fail(c, SOPHIA_AD_SNAPSHOT, SOPHIA_AS_DEADLINE);
  switch (c->phase) {
  case VERSION: {
    struct sophia_9p_reply reply;
    r = sophia_9p_service(c->wire, budget);
    if (r < 0)
      return fail(c, SOPHIA_AD_WIRE, r);
    r = sophia_9p_peek(c->wire, &reply);
    if (r == SOPHIA_9P_AGAIN)
      return 0;
    if (r)
      return fail(c, SOPHIA_AD_WIRE, r);
    if (reply.type != 101 || reply.handle.slot != c->version.slot ||
        reply.handle.serial != c->version.serial)
      return fail(c, SOPHIA_AD_INVALID, 0);
    r = sophia_9p_consume(c->wire, reply.handle);
    if (r)
      return fail(c, SOPHIA_AD_WIRE, r);
    r = sophia_ao_init(&c->opening, c->wire, now, c->deadline);
    if (r)
      return fail(c, SOPHIA_AD_SNAPSHOT, r);
    c->phase = FILES;
    return 0;
  }
  case FILES:
    r = sophia_ao_dispatch(&c->opening, now, budget);
    if (r)
      return fail(c, SOPHIA_AD_SNAPSHOT, r);
    if (sophia_ao_ready(&c->opening)) {
      r = sophia_ad_init(c->session, &c->opening);
      if (r)
        return fail(c, r, 0);
      c->phase = CATALOG;
    }
    return 0;
  case CATALOG:
    r = sophia_ad_dispatch(c->session, now, budget);
    return r ? fail(c, r, c->session->component_error) : 0;
  default:
    return fail(c, SOPHIA_AD_INVALID, 0);
  }
}
int sophia_ad_connect_next_wake(const struct sophia_ad_connecting *c,
                                uint64_t *time) {
  uint64_t next;
  if (!c || !time || c->error || c->phase == TAKEN ||
      sophia_ad_connect_ready(c))
    return 0;
  *time = c->deadline;
  if (c->phase == CATALOG && sophia_ad_next_wake(c->session, &next) &&
      next < *time)
    *time = next;
  return 1;
}
int sophia_ad_connect_wants_write(const struct sophia_ad_connecting *c) {
  return c && !c->error && c->phase != TAKEN && !sophia_ad_connect_ready(c) &&
         sophia_9p_wants_write(c->wire);
}
struct sophia_ad *
sophia_ad_connect_take_session(struct sophia_ad_connecting *c) {
  if (!sophia_ad_connect_ready(c))
    return NULL;
  c->phase = TAKEN;
  return c->session;
}
void sophia_ad_connect_close(struct sophia_ad_connecting *c) {
  if (c)
    (void)fail(c, SOPHIA_AD_CLOSED, 0);
}

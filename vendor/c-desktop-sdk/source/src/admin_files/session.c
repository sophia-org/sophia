#include "../sophia_admin_session.h"
#include <string.h>

static int active_command(const struct sophia_ad *s) {
  return s->have_command && (s->command.custody == SOPHIA_AC_QUEUED ||
                             s->command.custody == SOPHIA_AC_ISSUED ||
                             s->command.custody == SOPHIA_AC_ACCEPTED);
}
static int fail(struct sophia_ad *s, int kind, int component) {
  if (!s->error) {
    s->error = kind;
    s->component_error = component;
  }
  if (s->have_command)
    sophia_ac_close(&s->command);
  s->fetching = s->active = 0;
  return s->error;
}
static int clock_check(struct sophia_ad *s, uint64_t now) {
  if (!s)
    return SOPHIA_AD_INVALID;
  if (now < s->now)
    return SOPHIA_AD_CLOCK_REGRESSION;
  if (s->error)
    return s->error;
  s->now = now;
  return 0;
}
static int drive(struct sophia_ad *s) {
  int r;
  if (s->active)
    return 0;
  if (!s->fetching && s->have_command) {
    uint64_t wake, deadline;
    r = sophia_ac_advance(&s->command, s->now);
    if (r)
      return fail(s, SOPHIA_AD_COMMAND, r);
    if (s->command.custody == SOPHIA_AC_QUEUED) {
      r = sophia_9p_write(s->wire, s->ctl, 0, s->command.bytes,
                          s->command.length, &s->pending);
      if (r)
        return fail(s, SOPHIA_AD_WIRE, r);
      r = sophia_ac_issued(&s->command, s->now);
      if (r)
        return fail(s, SOPHIA_AD_COMMAND, r);
      s->active = 1;
      return 0;
    }
    if (s->command.custody == SOPHIA_AC_ACCEPTED &&
        sophia_ac_next_wake(&s->command, &wake) && s->now >= wake) {
      r = sophia_ac_start_status(&s->command, s->now, &deadline);
      if (r)
        return fail(s, SOPHIA_AD_COMMAND, r);
      r = sophia_as_init(&s->fetch, SOPHIA_AS_STATUS, s->scratch,
                         sizeof s->scratch, s->now, deadline);
      if (r)
        return fail(s, SOPHIA_AD_SNAPSHOT, r);
      s->fetching = 1;
      s->file = SOPHIA_AS_STATUS;
    }
  }
  if (s->fetching) {
    struct sophia_as_operation op;
    uint32_t fid = SOPHIA_9P_NOFID;
    r = sophia_as_operation(&s->fetch, s->now, s->wire->msize, &op);
    if (r != 1)
      return fail(s, SOPHIA_AD_SNAPSHOT, r ? r : SOPHIA_AS_INVALID);
    switch (op.kind) {
    case SOPHIA_AS_WALK: {
      const char *name = sophia_as_name(op.file);
      r = sophia_9p_walk(s->wire, s->root, &name, 1, &s->pending, &fid);
      break;
    }
    case SOPHIA_AS_OPEN:
      r = sophia_9p_lopen(s->wire, op.fid, 0, &s->pending);
      break;
    case SOPHIA_AS_READ:
      r = sophia_9p_read(s->wire, op.fid, op.offset, op.count, &s->pending);
      break;
    case SOPHIA_AS_CLUNK:
      r = sophia_9p_clunk(s->wire, op.fid, &s->pending);
      break;
    default:
      return fail(s, SOPHIA_AD_INVALID, 0);
    }
    if (r)
      return fail(s, SOPHIA_AD_WIRE, r);
    r = sophia_as_issued(&s->fetch, s->now, s->wire->msize, fid);
    if (r)
      return fail(s, SOPHIA_AD_SNAPSHOT, r);
    s->active = 1;
  }
  return 0;
}
static int disjoint(const void *a, size_t an, const void *b, size_t bn) {
  uintptr_t av = (uintptr_t)a, bv = (uintptr_t)b;
  return av <= UINTPTR_MAX - an && bv <= UINTPTR_MAX - bn &&
         (av + an <= bv || bv + bn <= av);
}
int sophia_ad_init(struct sophia_ad *s, struct sophia_ao *opening) {
  size_t i, bytes;
  struct sophia_9p_client *wire;
  if (!s || !sophia_ao_ready(opening))
    return SOPHIA_AD_INVALID;
  wire = opening->wire;
  bytes = sophia_9p_storage_bytes(wire->offered, wire->capacity);
  if (!bytes || !disjoint(s, sizeof *s, opening, sizeof *opening) ||
      !disjoint(s, sizeof *s, wire, sizeof *wire) ||
      !disjoint(s, sizeof *s, wire->storage, bytes) || wire->terminal ||
      wire->fid_count != 2 || wire->rx_used)
    return SOPHIA_AD_INVALID;
  for (i = 0; i < 2u * wire->capacity; ++i)
    if (wire->slots[i].state)
      return SOPHIA_AD_INVALID;
  memset(s, 0, sizeof *s);
  s->wire = wire;
  s->root = opening->root;
  s->ctl = opening->ctl;
  s->info = opening->info;
  s->now = opening->now;
  s->next_request = 1;
  sophia_ao_close(opening);
  return sophia_ad_refresh_catalog(s, s->now);
}
int sophia_ad_refresh_catalog(struct sophia_ad *s, uint64_t now) {
  int r = clock_check(s, now);
  if (r)
    return r;
  if (s->fetching || s->active || active_command(s))
    return SOPHIA_AD_BUSY;
  if (now > UINT64_MAX - s->info.frame_ms)
    return SOPHIA_AD_INVALID;
  s->valid = 0;
  r = sophia_as_init(&s->fetch, SOPHIA_AS_COMMANDS, s->catalog_bytes,
                     sizeof s->catalog_bytes, now, now + s->info.frame_ms);
  if (r)
    return fail(s, SOPHIA_AD_SNAPSHOT, r);
  s->fetching = 1;
  s->file = SOPHIA_AS_COMMANDS;
  return drive(s);
}
int sophia_ad_invoke(struct sophia_ad *s,
                     const struct sophia_af_command *command, uint64_t now,
                     uint64_t *request) {
  int r;
  if (!command || !request)
    return SOPHIA_AD_INVALID;
  r = clock_check(s, now);
  if (r)
    return r;
  if (s->fetching || s->active || active_command(s))
    return SOPHIA_AD_BUSY;
  if (!s->valid)
    return SOPHIA_AD_MISSING_CATALOG;
  if (!sophia_af_catalog_contains(&s->catalog, command))
    return SOPHIA_AD_UNADVERTISED;
  if (s->next_request == UINT64_MAX)
    return SOPHIA_AD_EXHAUSTED;
  r = sophia_ac_init(&s->command, &s->info, &s->catalog, command,
                     s->next_request, now);
  if (r)
    return SOPHIA_AD_INVALID;
  s->have_command = 1;
  *request = s->next_request++;
  return drive(s);
}
static int accept(struct sophia_ad *s, const struct sophia_9p_reply *reply) {
  int r;
  if (s->fetching) {
    const void *bytes;
    size_t length;
    r = sophia_as_reply(&s->fetch, reply, s->now);
    if (r)
      return fail(s, SOPHIA_AD_SNAPSHOT, r);
    if (sophia_as_result(&s->fetch, &bytes, &length)) {
      if (s->file == SOPHIA_AS_COMMANDS) {
        if (sophia_af_decode_catalog(bytes, length, &s->catalog))
          return fail(s, SOPHIA_AD_INVALID, 0);
        s->valid = 1;
      } else {
        r = sophia_ac_status(&s->command, bytes, length, s->now);
        if (r)
          return fail(s, SOPHIA_AD_COMMAND, r);
        if (sophia_ac_invalidate_catalog(&s->command))
          s->valid = 0;
      }
      s->fetching = 0;
    }
  } else {
    if (!s->have_command || (reply->type != 119 && reply->type != 7))
      return fail(s, SOPHIA_AD_INVALID, 0);
    r = sophia_ac_write_reply(&s->command, reply->type == 7,
                              reply->type == 7 ? reply->error : reply->count,
                              s->now);
    if (r)
      return fail(s, SOPHIA_AD_COMMAND, r);
  }
  return 0;
}
int sophia_ad_dispatch(struct sophia_ad *s, uint64_t now, size_t budget) {
  struct sophia_9p_reply reply;
  int r = clock_check(s, now);
  if (r)
    return r;
  if (s->have_command && (r = sophia_ac_advance(&s->command, now)))
    return fail(s, SOPHIA_AD_COMMAND, r);
  if (s->fetching) {
    struct sophia_as_operation op;
    r = sophia_as_operation(&s->fetch, now, s->wire->msize, &op);
    if (r < 0)
      return fail(s, SOPHIA_AD_SNAPSHOT, r);
  }
  r = drive(s);
  if (r)
    return r;
  r = sophia_9p_service(s->wire, budget);
  if (r < 0)
    return fail(s, SOPHIA_AD_WIRE, r);
  r = sophia_9p_peek(s->wire, &reply);
  if (r == SOPHIA_9P_AGAIN)
    return 0;
  if (r)
    return fail(s, SOPHIA_AD_WIRE, r);
  if (!s->active || reply.handle.slot != s->pending.slot ||
      reply.handle.serial != s->pending.serial)
    return fail(s, SOPHIA_AD_INVALID, 0);
  r = accept(s, &reply);
  if (r)
    return r;
  r = sophia_9p_consume(s->wire, reply.handle);
  if (r)
    return fail(s, SOPHIA_AD_WIRE, r);
  s->active = 0;
  return drive(s);
}
const struct sophia_af_catalog *sophia_ad_catalog(const struct sophia_ad *s) {
  return s && s->valid ? &s->catalog : NULL;
}
const struct sophia_ac *sophia_ad_latest(const struct sophia_ad *s) {
  return s && s->have_command ? &s->command : NULL;
}
int sophia_ad_next_wake(const struct sophia_ad *s, uint64_t *time) {
  uint64_t wake;
  int have = 0;
  if (!s || !time || s->error)
    return 0;
  if (s->fetching) {
    *time = s->fetch.deadline;
    have = 1;
  }
  if (s->have_command && sophia_ac_next_wake(&s->command, &wake) &&
      (!have || wake < *time)) {
    *time = wake;
    have = 1;
  }
  return have;
}
int sophia_ad_wants_write(const struct sophia_ad *s) {
  return s && !s->error && sophia_9p_wants_write(s->wire);
}
void sophia_ad_close(struct sophia_ad *s) {
  if (s)
    (void)fail(s, SOPHIA_AD_CLOSED, 0);
}

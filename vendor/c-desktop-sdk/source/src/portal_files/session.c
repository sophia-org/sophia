#include "../sophia_portal_session.h"
#include "frame_internal.h"
#include "request_internal.h"
#include <string.h>
enum { WALK = 1, OPEN, READ, CLUNK, READY };
static int disjoint(const void *a, size_t an, const void *b, size_t bn) {
  uintptr_t av = (uintptr_t)a, bv = (uintptr_t)b;
  return av <= UINTPTR_MAX - an && bv <= UINTPTR_MAX - bn &&
         (av + an <= bv || bv + bn <= av);
}
static int fail(struct sophia_pcs *s, int error) {
  if (!error)
    return 0;
  if (!s->error)
    s->error = error;
  if (s->capture_present)
    sophia_pcr_close(&s->capture);
  if (s->frame_present)
    sophia_pfr_close(&s->frame);
  return s->error;
}
static int wire_error(struct sophia_pcs *s, int r) {
  s->wire_error = r;
  return SOPHIA_PO_WIRE_ERROR;
}
static int same(struct sophia_9p_handle a, struct sophia_9p_handle b) {
  return a.slot == b.slot && a.serial == b.serial;
}
static int drive(struct sophia_pcs *s) {
  struct sophia_9p_client *w = s->opening->wire;
  int r;
  if (s->frame_present) {
    r = sophia_pfr_step(&s->frame, s->now);
    if (r)
      return r;
  }
  if (s->capture_present) {
    r = sophia_pcr_step(&s->capture, s->now);
    if (r)
      return r;
  }
  if (s->stage && s->stage != READY && !s->watch_active && !s->flushing) {
    switch (s->stage) {
    case WALK: {
      const char *name = "watch";
      r = sophia_9p_walk(w, s->opening->root, &name, 1, &s->watch_tag,
                         &s->watch_fid);
      break;
    }
    case OPEN:
      r = sophia_9p_lopen(w, s->watch_fid, 0, &s->watch_tag);
      break;
    case READ:
      r = sophia_9p_read(w, s->watch_fid, 0, 1, &s->watch_tag);
      break;
    case CLUNK:
      r = sophia_9p_clunk(w, s->watch_fid, &s->watch_tag);
      break;
    default:
      return SOPHIA_PS_INVALID;
    }
    if (r)
      return wire_error(s, r);
    s->watch_active = 1;
  }
  if (s->fetch_present && !s->fetch_active) {
    struct sophia_ps_operation op;
    uint32_t fid = SOPHIA_9P_NOFID;
    r = sophia_ps_operation(&s->fetch, s->now, w->msize, &op);
    if (r <= 0)
      return r;
    switch (op.kind) {
    case SOPHIA_PS_WALK: {
      const char *name = sophia_ps_name(op.file);
      r = sophia_9p_walk(w, s->opening->root, &name, 1, &s->fetch_tag, &fid);
      break;
    }
    case SOPHIA_PS_OPEN:
      r = sophia_9p_lopen(w, op.fid, 0, &s->fetch_tag);
      break;
    case SOPHIA_PS_READ:
      r = sophia_9p_read(w, op.fid, op.offset, op.count, &s->fetch_tag);
      break;
    case SOPHIA_PS_CLUNK:
      r = sophia_9p_clunk(w, op.fid, &s->fetch_tag);
      break;
    default:
      return SOPHIA_PS_INVALID;
    }
    if (r)
      return wire_error(s, r);
    r = sophia_ps_issued(&s->fetch, s->now, w->msize, fid);
    if (r)
      return r;
    s->fetch_active = 1;
  }
  return 0;
}
int sophia_pcs_init(struct sophia_pcs *s, struct sophia_po *o) {
  struct sophia_9p_client *w;
  size_t bytes, i;
  if (!s || !o || !sophia_po_ready(o))
    return SOPHIA_PS_INVALID;
  w = o->wire;
  if (!w || w->terminal || w->phase != 2 || w->fid_count != 1 ||
      w->fids[0] != o->root || o->root == SOPHIA_9P_NOFID || w->rx_used ||
      w->capacity < 4 || w->capacity > SOPHIA_9P_REQUESTS || w->fid_limit < 3 ||
      w->msize < 512 || w->msize > 65536)
    return SOPHIA_PS_INVALID;
  bytes = sophia_9p_storage_bytes(w->offered, w->capacity);
  if (!bytes || !disjoint(s, sizeof *s, o, sizeof *o) ||
      !disjoint(s, sizeof *s, w, sizeof *w) ||
      !disjoint(s, sizeof *s, w->storage, bytes))
    return SOPHIA_PS_INVALID;
  for (i = 0; i < 2u * w->capacity; ++i)
    if (w->slots[i].state)
      return SOPHIA_PS_INVALID;
  memset(s, 0, sizeof *s);
  s->opening = o;
  s->now = o->now;
  sophia_po_close(o);
  return 0;
}
int sophia_pcs_watch(struct sophia_pcs *s, uint64_t deadline) {
  if (!s)
    return SOPHIA_PS_INVALID;
  if (s->error)
    return s->error;
  if (s->stage)
    return SOPHIA_PS_BUSY;
  if (deadline <= s->now)
    return SOPHIA_PS_DEADLINE;
  s->stage = WALK;
  s->watch_fid = SOPHIA_9P_NOFID;
  s->watch_deadline = deadline;
  return fail(s, drive(s));
}
int sophia_pcs_cancel_watch(struct sophia_pcs *s) {
  if (!s || !s->stage)
    return SOPHIA_PS_INVALID;
  if (s->error)
    return s->error;
  if (s->stage != READY)
    s->cancel = 1;
  return fail(s, drive(s));
}
int sophia_pcs_watch_waiting(const struct sophia_pcs *s) {
  return s && !s->error && s->stage == READ && !s->cancel;
}
int sophia_pcs_watch_event(const struct sophia_pcs *s,
                           enum sophia_pcs_event *event, uint32_t *remote) {
  if (!s || s->error || s->stage != READY || !event || !remote)
    return 0;
  *event = s->event;
  *remote = s->watch_errno;
  return 1;
}
int sophia_pcs_release_watch(struct sophia_pcs *s) {
  if (!s)
    return SOPHIA_PS_INVALID;
  if (s->error)
    return s->error;
  if (s->stage != READY)
    return SOPHIA_PS_BUSY;
  s->stage = s->cancel = 0;
  s->event = 0;
  s->watch_errno = 0;
  return 0;
}
int sophia_pcs_snapshot(struct sophia_pcs *s, enum sophia_ps_file file,
                        void *storage, size_t capacity, uint64_t deadline) {
  struct sophia_9p_client *w;
  size_t bytes;
  int r;
  if (!s || !storage)
    return SOPHIA_PS_INVALID;
  if (s->error)
    return s->error;
  if (s->fetch_present || s->capture_present || s->frame_present)
    return SOPHIA_PS_BUSY;
  w = s->opening->wire;
  bytes = sophia_9p_storage_bytes(w->offered, w->capacity);
  if (!disjoint(storage, capacity, s, sizeof *s) ||
      !disjoint(storage, capacity, s->opening, sizeof *s->opening) ||
      !disjoint(storage, capacity, w, sizeof *w) ||
      !disjoint(storage, capacity, w->storage, bytes))
    return SOPHIA_PS_INVALID;
  r = sophia_ps_init(&s->fetch, file, storage, capacity, s->now, deadline);
  if (r)
    return r;
  s->fetch_present = 1;
  return fail(s, drive(s));
}
int sophia_pcs_snapshot_result(const struct sophia_pcs *s, const void **bytes,
                               size_t *length) {
  return s && !s->error && s->fetch_present &&
         sophia_ps_result(&s->fetch, bytes, length);
}
int sophia_pcs_release_snapshot(struct sophia_pcs *s) {
  const void *bytes;
  size_t length;
  if (!s)
    return SOPHIA_PS_INVALID;
  if (s->error)
    return s->error;
  if (!sophia_pcs_snapshot_result(s, &bytes, &length))
    return SOPHIA_PS_BUSY;
  s->fetch_present = 0;
  return 0;
}
static int accept(struct sophia_pcs *s, const struct sophia_9p_reply *r) {
  if (s->frame_present && sophia_pfr_matches(&s->frame, r->handle))
    return sophia_pfr_accept(&s->frame, r);
  if (s->capture_present && s->capture.active &&
      same(r->handle, s->capture.pending))
    return sophia_pcr_accept(&s->capture, r);
  if (s->fetch_active && same(r->handle, s->fetch_tag)) {
    s->fetch_active = 0;
    return sophia_ps_reply(&s->fetch, r, s->now);
  }
  if (s->flushing && same(r->handle, s->flush_tag)) {
    if (r->type != 109)
      return SOPHIA_PS_INVALID;
    s->flushing = s->watch_active = 0;
    if (!s->event)
      s->event = SOPHIA_PCS_CANCELLED;
    s->stage = CLUNK;
    return 0;
  }
  if (!s->watch_active || !same(r->handle, s->watch_tag))
    return SOPHIA_PS_INVALID;
  s->watch_active = 0;
  if (r->type == 7) {
    if (s->stage == CLUNK)
      return SOPHIA_PS_INVALID;
    s->event = r->error == 116 && s->stage != WALK ? SOPHIA_PCS_REVOKED
                                                   : SOPHIA_PCS_REFUSED;
    s->watch_errno = s->event == SOPHIA_PCS_REFUSED ? r->error : 0;
    s->stage = s->stage == WALK ? READY : CLUNK;
    return 0;
  }
  switch (s->stage) {
  case WALK:
    if (r->type != 111 || r->count != 1 || r->qid.type)
      return SOPHIA_PS_INVALID;
    s->stage = OPEN;
    break;
  case OPEN:
    if (r->type != 13 || r->qid.type)
      return SOPHIA_PS_INVALID;
    s->stage = READ;
    break;
  case CLUNK:
    if (r->type != 121)
      return SOPHIA_PS_INVALID;
    s->stage = READY;
    break;
  default:
    return SOPHIA_PS_INVALID;
  }
  if (s->cancel && (s->stage == OPEN || s->stage == READ)) {
    s->event = SOPHIA_PCS_CANCELLED;
    s->stage = CLUNK;
  }
  return 0;
}
static int deadline(const struct sophia_pcs *s, uint64_t *deadline,
                    int capture_wake) {
  const void *bytes;
  size_t length;
  int present = 0;
  if (!s || s->error || !deadline)
    return 0;
  if (s->stage && s->stage != READY) {
    *deadline = s->watch_deadline;
    present = 1;
  }
  if (s->fetch_present && !sophia_ps_result(&s->fetch, &bytes, &length) &&
      (!present || s->fetch.deadline < *deadline)) {
    *deadline = s->fetch.deadline;
    present = 1;
  }
  if (capture_wake && s->capture_present) {
    uint64_t wake;
    if (sophia_pcr_next_wake(&s->capture, &wake) &&
        (!present || wake < *deadline)) {
      *deadline = wake;
      present = 1;
    }
  }
  if (s->frame_present) {
    struct sophia_pf_frame_info info;
    if (!sophia_pfr_result(&s->frame, &info, &bytes, &length) &&
        (!present || s->frame.deadline < *deadline)) {
      *deadline = s->frame.deadline;
      present = 1;
    }
  }
  return present;
}
int sophia_pcs_deadline(const struct sophia_pcs *s, uint64_t *wake) {
  return deadline(s, wake, 1);
}
int sophia_pcs_dispatch(struct sophia_pcs *s, uint64_t now, size_t budget) {
  struct sophia_9p_client *w;
  struct sophia_9p_reply reply;
  uint64_t hard_deadline;
  unsigned i;
  int r;
  if (!s)
    return SOPHIA_PS_INVALID;
  if (now < s->now)
    return SOPHIA_PS_CLOCK_REGRESSION;
  if (s->error)
    return s->error;
  s->now = now;
  if (deadline(s, &hard_deadline, 0) && now >= hard_deadline)
    return fail(s, SOPHIA_PS_DEADLINE);
  r = drive(s);
  if (r)
    return fail(s, r);
  w = s->opening->wire;
  r = sophia_9p_service(w, budget);
  if (r < 0)
    return fail(s, wire_error(s, r));
  for (i = 0; i < 32; ++i) {
    r = sophia_9p_peek(w, &reply);
    if (r == SOPHIA_9P_AGAIN) {
      /* Drain decoded replies before flushing: a settled tag is no longer
       * flushable, and its observed outcome takes precedence over cancellation.
       */
      if (s->cancel && s->stage == READ && !s->flushing && s->watch_active) {
        r = sophia_9p_flush(w, s->watch_tag, &s->flush_tag);
        if (r)
          return fail(s, wire_error(s, r));
        s->flushing = 1;
      }
      return 0;
    }
    if (r)
      return fail(s, wire_error(s, r));
    r = accept(s, &reply);
    if (r)
      return fail(s, r);
    r = sophia_9p_consume(w, reply.handle);
    if (r)
      return fail(s, wire_error(s, r));
    r = drive(s);
    if (r)
      return fail(s, r);
  }
  return 0;
}
int sophia_pcs_wants_write(const struct sophia_pcs *s) {
  return s && !s->error && sophia_9p_wants_write(s->opening->wire);
}
int sophia_pcs_immediate(const struct sophia_pcs *s) {
  return s && !s->error && s->cancel && s->stage == READ && !s->flushing;
}
int sophia_pcs_finish(struct sophia_pcs *s) {
  if (!s)
    return SOPHIA_PS_INVALID;
  if (s->error)
    return s->error;
  if (s->stage || s->fetch_present || s->capture_present || s->frame_present)
    return SOPHIA_PS_BUSY;
  s->opening->now = s->now;
  s->opening->error = 0;
  s->opening = NULL;
  s->error = SOPHIA_PS_CLOSED;
  return 0;
}
void sophia_pcs_close(struct sophia_pcs *s) {
  if (s && !s->error)
    (void)fail(s, SOPHIA_PS_CLOSED);
}

int sophia_pcs_capture(struct sophia_pcs *s,
                       struct sophia_pf_capture_request request,
                       uint64_t previous_serial, uint64_t deadline) {
  int r;
  if (!s)
    return SOPHIA_PS_INVALID;
  if (s->error)
    return s->error;
  if (s->fetch_present || s->capture_present || s->frame_present)
    return SOPHIA_PS_BUSY;
  if (s->opening->info.request != SOPHIA_PF_OUTPUT_CAPTURE_V1)
    return SOPHIA_PCR_UNSUPPORTED;
  r = sophia_pcr_prepare(&s->capture, s->opening->wire, s->opening->root,
                         request, previous_serial, s->now, deadline);
  if (r)
    return r;
  s->capture_present = 1;
  r = drive(s);
  return r ? fail(s, r) : 0;
}
int sophia_pcs_capture_custody(const struct sophia_pcs *s,
                               enum sophia_pc_custody *custody) {
  if (!s || !s->capture_present || !custody)
    return 0;
  *custody = s->capture.capture.custody;
  return 1;
}
int sophia_pcs_capture_result(const struct sophia_pcs *s,
                              enum sophia_pf_capture_state *outcome,
                              uint64_t *serial) {
  if (!s || s->error || !s->capture_present || !serial ||
      !sophia_pcr_result(&s->capture, outcome))
    return 0;
  *serial = s->capture.serial;
  return 1;
}
int sophia_pcs_release_capture(struct sophia_pcs *s) {
  enum sophia_pf_capture_state outcome;
  uint64_t serial;
  if (!s)
    return SOPHIA_PS_INVALID;
  if (s->error)
    return s->error;
  if (!sophia_pcs_capture_result(s, &outcome, &serial))
    return SOPHIA_PS_BUSY;
  s->capture_present = 0;
  return 0;
}

int sophia_pcs_frame(struct sophia_pcs *s, void *image, size_t capacity,
                     uint64_t deadline) {
  struct sophia_9p_client *w;
  size_t bytes;
  int r;
  if (!s || !image || !capacity)
    return SOPHIA_PS_INVALID;
  if (s->error)
    return s->error;
  if (s->fetch_present || s->capture_present || s->frame_present)
    return SOPHIA_PS_BUSY;
  w = s->opening->wire;
  if (w->capacity < 6 || w->fid_limit < 4)
    return SOPHIA_PS_INVALID;
  bytes = sophia_9p_storage_bytes(w->offered, w->capacity);
  if (!disjoint(image, capacity, s, sizeof *s) ||
      !disjoint(image, capacity, s->opening, sizeof *s->opening) ||
      !disjoint(image, capacity, w, sizeof *w) ||
      !disjoint(image, capacity, w->storage, bytes))
    return SOPHIA_PS_INVALID;
  r = sophia_pfr_prepare(&s->frame, w, s->opening->root, image, capacity,
                         s->now, deadline);
  if (r)
    return r;
  s->frame_present = 1;
  r = drive(s);
  return r ? fail(s, r) : 0;
}
uint64_t sophia_pcs_frame_received(const struct sophia_pcs *s) {
  return s && s->frame_present ? sophia_pfr_received(&s->frame) : 0;
}
int sophia_pcs_frame_result(const struct sophia_pcs *s,
                            struct sophia_pf_frame_info *info,
                            const void **pixels, size_t *length) {
  return s && !s->error && s->frame_present &&
         sophia_pfr_result(&s->frame, info, pixels, length);
}
int sophia_pcs_release_frame(struct sophia_pcs *s) {
  struct sophia_pf_frame_info info;
  const void *pixels;
  size_t length;
  if (!s)
    return SOPHIA_PS_INVALID;
  if (s->error)
    return s->error;
  if (!sophia_pcs_frame_result(s, &info, &pixels, &length))
    return SOPHIA_PS_BUSY;
  s->frame_present = 0;
  return 0;
}

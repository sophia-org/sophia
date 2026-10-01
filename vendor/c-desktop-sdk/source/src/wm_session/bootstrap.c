#include "internal.h"

static const char api[] =
    "sophia-wm-files version=1 output_transport=9p2000.L\n";
static const char *const files[] = {"events", "submit", "ack"};
static uint32_t *fid(struct sophia_ws *s, unsigned index) {
  if (!index)
    return &s->events_fid;
  return index == 1 ? &s->submit_fid : &s->ack_fid;
}
static int bootstrap_drive(struct sophia_ws *s) {
  int r;
  if (s->boot_op.active || s->bootstrap == 16)
    return 0;
  switch (s->bootstrap) {
  case 0:
    r = sophia_9p_version(&s->wire, &s->boot_op.handle);
    break;
  case 1:
    r = sophia_9p_attach(&s->wire, "", "", &s->boot_op.handle, &s->root);
    break;
  case 2:
  case 6: {
    const char *name = s->bootstrap == 2 ? "api" : "limits";
    s->boot_used = 0;
    r = sophia_9p_walk(&s->wire, s->root, &name, 1, &s->boot_op.handle,
                       &s->boot_fid);
    break;
  }
  case 3:
  case 7:
    r = sophia_9p_lopen(&s->wire, s->boot_fid, 0, &s->boot_op.handle);
    break;
  case 4:
  case 8: {
    uint32_t n = (s->bootstrap == 4 ? 257u : 65u) - (uint32_t)s->boot_used;
    if (s->boot_iounit && n > s->boot_iounit)
      n = s->boot_iounit;
    r = sophia_9p_read(&s->wire, s->boot_fid, s->boot_used, n,
                       &s->boot_op.handle);
    break;
  }
  case 5:
  case 9:
    r = sophia_9p_clunk(&s->wire, s->boot_fid, &s->boot_op.handle);
    break;
  default: {
    unsigned index = (s->bootstrap - 10u) / 2u;
    if (!(s->bootstrap % 2))
      r = sophia_9p_walk(&s->wire, s->root, &files[index], 1,
                         &s->boot_op.handle, fid(s, index));
    else
      r = sophia_9p_lopen(&s->wire, *fid(s, index), index ? 1 : 0,
                          &s->boot_op.handle);
    break;
  }
  }
  return ws_started(&s->boot_op, r);
}
int ws_boot_reply(struct sophia_ws *s, const struct sophia_9p_reply *r) {
  if (r->type == 7) {
    s->remote_error = r->error;
    return SOPHIA_9P_INVALID;
  }
  if (r->type == 111 && (r->count != 1 || r->qid.type))
    return SOPHIA_9P_INVALID;
  if (r->type == 13 && r->qid.type)
    return SOPHIA_9P_INVALID;
  if (s->bootstrap == 4 || s->bootstrap == 8) {
    size_t maximum = s->bootstrap == 4 ? 256u : 64u;
    if (r->type != 117 || r->count > maximum - s->boot_used)
      return SOPHIA_9P_INVALID;
    if (r->count) {
      memcpy(s->boot_bytes + s->boot_used, r->data, r->count);
      s->boot_used += r->count;
      return 0;
    }
    if (s->bootstrap == 4) {
      /* This advertises the separate output role's current contract. It
       * does not select or connect any output wire for the WM client. The
       * comparison is exact: an export still naming current_ipc is refused. */
      if (s->boot_used != sizeof(api) - 1 ||
          memcmp(s->boot_bytes, api, sizeof(api) - 1))
        return SOPHIA_9P_INVALID;
    } else {
      struct sophia_wf_record value;
      if (sophia_wf_decode(s->boot_bytes, s->boot_used, 0, &value) ||
          value.header.kind != SOPHIA_WF_LIMITS)
        return SOPHIA_9P_INVALID;
      s->epoch = value.header.epoch;
      s->limits = value.value.limits;
      /* Refuse an impossible offer before opening the candidate lane. */
      if ((s->config.offer.required & ~s->limits.capability_ceiling) ||
          (s->limits.profile_required &&
           !((s->config.offer.required | s->config.offer.optional) &
             SOPHIA_WF_CAP_PROFILE_ACTIVATION)))
        return SOPHIA_9P_INVALID;
      s->have_limits = 1;
    }
  }
  if (s->bootstrap == 3 || s->bootstrap == 7)
    s->boot_iounit = r->iounit;
  if (s->bootstrap == 11)
    s->events_iounit = r->iounit;
  if ((s->bootstrap == 13 && r->iounit && r->iounit < 24) ||
      (s->bootstrap == 15 && r->iounit && r->iounit < 16))
    return SOPHIA_9P_INVALID;
  ++s->bootstrap;
  if (s->bootstrap == 16) {
    struct sophia_wf_record value;
    uint64_t ticket;
    memset(&value, 0, sizeof(value));
    value.header.kind = SOPHIA_WF_NEGOTIATE;
    value.value.negotiate = s->config.offer;
    return ws_queue(s, &value, s->bootstrap_deadline, &ticket);
  }
  return 0;
}
int ws_drive(struct sophia_ws *s) {
  int r = bootstrap_drive(s);
  if (r || s->bootstrap != 16)
    return r;
  if (!s->event_op.active && s->journal_used < WS_JOURNAL_BYTES) {
    size_t n = WS_JOURNAL_BYTES - s->journal_used;
    if (n > s->wire.msize - 11u)
      n = s->wire.msize - 11u;
    if (s->events_iounit && n > s->events_iounit)
      n = s->events_iounit;
    r = ws_started(&s->event_op,
                   sophia_9p_read(&s->wire, s->events_fid, s->event_offset,
                                  (uint32_t)n, &s->event_op.handle));
    if (r)
      return r;
  }
  if (!s->ack_op.active && s->consumed > s->acked) {
    uint8_t b[16];
    if (sophia_wf_ack_encode(b, s->epoch, s->consumed))
      return SOPHIA_9P_INVALID;
    r = sophia_9p_write(&s->wire, s->ack_fid, 0, b, 16, &s->ack_op.handle);
    if (!r) {
      s->ack_op.active = 1;
      s->ack_pending = s->consumed;
    } else if (r != SOPHIA_9P_BUSY)
      return r;
  }
  r = ws_tx_drive(s);
  return r ? r : ws_object_drive(s);
}

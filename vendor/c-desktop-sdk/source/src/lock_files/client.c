#include "client_internal.h"

static const char *const stream_names[] = {"events", "transaction", "submit",
                                           "ack"};
/* events read, transaction read-write, submit and ack write-only. */
static const uint32_t stream_modes[] = {0, 2, 1, 1};

static int offer_valid(const struct sophia_lf_negotiate *offer) {
  struct sophia_lf_record r;
  uint8_t b[SOPHIA_LF_MAX_CANDIDATE];
  size_t n;
  memset(&r, 0, sizeof(r));
  r.header.kind = SOPHIA_LF_NEGOTIATE;
  r.header.epoch = 1;
  r.header.submission = 1;
  r.value.negotiate = *offer;
  return offer->minimum_revision &&
         offer->minimum_revision <= offer->maximum_revision &&
         (offer->capabilities & SOPHIA_LF_CAP_PRESENT) &&
         !(offer->capabilities &
           ~(uint64_t)(SOPHIA_LF_CAP_PRESENT | SOPHIA_LF_CAP_CHORDS)) &&
         (!offer->chord_count || (offer->capabilities & SOPHIA_LF_CAP_CHORDS)) &&
         !sophia_lf_encode(b, sizeof(b), &r, &n);
}
int sophia_lc_init(struct sophia_lc_client *c, struct sophia_9p_client *wire,
                   const struct sophia_lf_negotiate *offer) {
  if (!c || !wire || !offer || wire->phase || wire->capacity < 6 ||
      !offer_valid(offer))
    return SOPHIA_9P_ARGUMENT;
  memset(c, 0, sizeof(*c));
  c->wire = wire;
  c->upload_window = 1;
  c->offer = *offer;
  c->next_submission = 1;
  c->object_fid = c->upload_fid = UINT32_MAX;
  return lc_started(&c->boot_op, sophia_9p_version(wire, &c->boot_op.handle));
}

/* The whole api file, exactly one line: a valid prefix followed by anything
 * else never yields an epoch. */
static int api_epoch(const uint8_t *b, size_t n, uint64_t *epoch) {
  static const char prefix[] = "sophia-lock-files version=1 epoch=";
  size_t at = sizeof(prefix) - 1;
  uint64_t value = 0;
  if (n <= at || memcmp(b, prefix, at) || b[at] < '1' || b[at] > '9')
    return SOPHIA_9P_INVALID;
  while (at < n && b[at] >= '0' && b[at] <= '9') {
    unsigned digit = (unsigned)(b[at++] - '0');
    if (value > (UINT64_MAX - digit) / 10)
      return SOPHIA_9P_INVALID;
    value = value * 10 + digit;
  }
  if (at + 1 != n || b[at] != '\n')
    return SOPHIA_9P_INVALID;
  *epoch = value;
  return 0;
}
static int limits_read(struct sophia_lc_client *c) {
  struct sophia_lf_record r;
  if (sophia_lf_decode(c->object, c->object_used, &r) ||
      r.header.kind != SOPHIA_LF_LIMITS || r.header.epoch != c->epoch)
    return SOPHIA_9P_INVALID;
  c->limits = r.value.limits;
  c->object_used = 0;
  return 0;
}

static int bootstrap_drive(struct sophia_lc_client *c) {
  const char *name;
  unsigned stream;
  int r;
  if (c->boot_op.active || c->bootstrap >= LC_BOOTED)
    return 0;
  switch (c->bootstrap) {
  case LC_ATTACH:
    r = sophia_9p_attach(c->wire, "", "", &c->boot_op.handle, &c->root);
    break;
  case LC_API_WALK:
  case LC_LIMITS_WALK:
    name = c->bootstrap == LC_API_WALK ? "api" : "limits";
    r = sophia_9p_walk(c->wire, c->root, &name, 1, &c->boot_op.handle,
                       &c->fids[0]);
    break;
  case LC_API_OPEN:
  case LC_LIMITS_OPEN:
    r = sophia_9p_lopen(c->wire, c->fids[0], 0, &c->boot_op.handle);
    break;
  case LC_API_READ:
    r = sophia_9p_read(c->wire, c->fids[0], c->boot_used,
                       lc_cap(c, sizeof(c->boot) - c->boot_used, 11,
                              c->boot_iounit),
                       &c->boot_op.handle);
    break;
  case LC_LIMITS_READ:
    r = sophia_9p_read(c->wire, c->fids[0], c->object_used,
                       lc_cap(c, sizeof(c->object) - c->object_used, 11,
                              c->boot_iounit),
                       &c->boot_op.handle);
    break;
  case LC_API_CLUNK:
  case LC_LIMITS_CLUNK:
    r = sophia_9p_clunk(c->wire, c->fids[0], &c->boot_op.handle);
    break;
  default:
    stream = (c->bootstrap - LC_STREAMS) / 2;
    if (!((c->bootstrap - LC_STREAMS) % 2))
      r = sophia_9p_walk(c->wire, c->root, &stream_names[stream], 1,
                         &c->boot_op.handle, &c->fids[stream]);
    else
      r = sophia_9p_lopen(c->wire, c->fids[stream], stream_modes[stream],
                          &c->boot_op.handle);
    break;
  }
  return lc_started(&c->boot_op, r);
}
static int boot_reply(struct sophia_lc_client *c,
                      const struct sophia_9p_reply *r) {
  if (r->type == 7)
    return lc_remote(c, r->error);
  if (r->type == 111 && r->count != 1)
    return SOPHIA_9P_INVALID;
  if (c->bootstrap == LC_API_READ || c->bootstrap == LC_LIMITS_READ) {
    uint8_t *into = c->bootstrap == LC_API_READ ? c->boot : c->object;
    size_t *used = c->bootstrap == LC_API_READ ? &c->boot_used : &c->object_used;
    size_t room = c->bootstrap == LC_API_READ ? sizeof(c->boot)
                                              : sizeof(c->object);
    if (r->type != 117 || r->count > room - *used)
      return SOPHIA_9P_INVALID;
    if (r->count) {
      /* A full buffer can only be followed by EOF: the next read asks for
       * nothing, so anything more is refused as overlong. */
      if (*used + r->count == room)
        return SOPHIA_9P_INVALID;
      memcpy(into + *used, r->data, r->count);
      *used += r->count;
      return 0;
    }
    if (c->bootstrap == LC_API_READ
            ? api_epoch(c->boot, c->boot_used, &c->epoch)
            : limits_read(c))
      return SOPHIA_9P_INVALID;
  }
  if (r->type == 13) {
    if (c->bootstrap < LC_STREAMS)
      c->boot_iounit = r->iounit;
    else {
      unsigned stream = (c->bootstrap - LC_STREAMS) / 2;
      if (r->iounit && ((stream == LC_SUBMIT && r->iounit < 24) ||
                        (stream == LC_ACK && r->iounit < 16)))
        return SOPHIA_9P_INVALID;
      c->iounit[stream] = r->iounit;
    }
  }
  if (++c->bootstrap == LC_BOOTED) {
    struct sophia_lf_record negotiate;
    memset(&negotiate, 0, sizeof(negotiate));
    negotiate.header.kind = SOPHIA_LF_NEGOTIATE;
    negotiate.value.negotiate = c->offer;
    return sophia_lc_internal_queue(c, &negotiate);
  }
  return 0;
}

static int submit_drive(struct sophia_lc_client *c) {
  int r;
  if (!c->submit_stage || c->submit_op.active ||
      (c->submit_stage == LC_TX_SUBMIT && c->submit_wait))
    return 0;
  switch (c->submit_stage) {
  case LC_TX_WRITE:
    r = sophia_9p_write(c->wire, c->fids[LC_TRANSACTION], c->tx_offset,
                        c->tx + c->tx_offset,
                        lc_cap(c, c->tx_size - c->tx_offset, 23,
                               c->iounit[LC_TRANSACTION]),
                        &c->submit_op.handle);
    break;
  case LC_TX_SUBMIT: {
    uint8_t b[SOPHIA_LF_SUBMIT_BYTES];
    if (sophia_lf_submit_encode(b, c->epoch, c->next_submission - 1,
                                (uint32_t)c->tx_size))
      return SOPHIA_9P_INVALID;
    r = sophia_9p_write(c->wire, c->fids[LC_SUBMIT], 0, b, sizeof(b),
                        &c->submit_op.handle);
    if (!r)
      c->submit_sent = 1;
    break;
  }
  case LC_TX_CLUNK:
    r = sophia_9p_clunk(c->wire, c->fids[LC_TRANSACTION], &c->submit_op.handle);
    break;
  case LC_TX_WALK:
    r = sophia_9p_walk(c->wire, c->root, &stream_names[LC_TRANSACTION], 1,
                       &c->submit_op.handle, &c->fids[LC_TRANSACTION]);
    break;
  case LC_TX_OPEN:
    /* Custody is released only by acknowledging its Submitted. */
    if (c->submitted && c->acked < c->submitted_sequence)
      return 0;
    r = sophia_9p_lopen(c->wire, c->fids[LC_TRANSACTION], 2,
                        &c->submit_op.handle);
    break;
  default:
    return 0;
  }
  return lc_started(&c->submit_op, r);
}
static int submit_reply(struct sophia_lc_client *c,
                        const struct sophia_9p_reply *r) {
  if (r->type == 7 && c->submit_stage == LC_TX_SUBMIT) {
    /* EAGAIN transferred nothing; it waits for submit_retry. */
    if (r->error == 11) {
      c->submit_sent = 0;
      c->submit_wait = 1;
      return 0;
    }
    /* EALREADY after observed custody is that custody. */
    if (r->error == 114 && c->submitted) {
      c->submit_replied = 1;
      c->submit_stage = LC_TX_CLUNK;
      return 0;
    }
    /* Any other errno but ESTALE refuses the record: nothing was journaled
     * and clunking the transaction discards the staging. Negotiation has no
     * such outcome; its refusal arrives as a Refused event. */
    if (r->error != 114 && r->error != 116 && c->negotiated && !c->submitted) {
      c->submit_sent = 0;
      c->submit_error = r->error ? r->error : UINT32_MAX;
      c->submit_stage = LC_TX_CLUNK;
      sophia_lc_internal_upload_refused(c, (uint16_t)lc_get(c->tx + 6, 2));
      return 0;
    }
  }
  if (r->type == 7)
    return lc_remote(c, r->error);
  switch (c->submit_stage) {
  case LC_TX_WRITE:
    if (r->type != 119 || !r->count || r->count > c->tx_size - c->tx_offset)
      return SOPHIA_9P_INVALID;
    c->tx_offset += r->count;
    if (c->tx_offset == c->tx_size)
      c->submit_stage = LC_TX_SUBMIT;
    return 0;
  case LC_TX_SUBMIT:
    if (r->type != 119 || r->count != SOPHIA_LF_SUBMIT_BYTES)
      return SOPHIA_9P_INVALID;
    c->submit_replied = 1;
    c->submit_stage = c->submitted ? LC_TX_CLUNK : LC_TX_WAIT;
    return 0;
  case LC_TX_CLUNK:
    c->submit_stage = LC_TX_WALK;
    return 0;
  case LC_TX_WALK:
    if (r->type != 111 || r->count != 1)
      return SOPHIA_9P_INVALID;
    c->submit_stage = LC_TX_OPEN;
    return 0;
  case LC_TX_OPEN:
    c->iounit[LC_TRANSACTION] = r->iounit;
    c->submit_stage = LC_TX_IDLE;
    return 0;
  default:
    return SOPHIA_9P_INVALID;
  }
}

static int ack_drive(struct sophia_lc_client *c) {
  uint8_t b[SOPHIA_LF_ACK_BYTES];
  int r;
  if (c->ack_op.active || c->consumed == c->acked)
    return 0;
  if (sophia_lf_ack_encode(b, c->epoch, c->consumed))
    return SOPHIA_9P_INVALID;
  r = sophia_9p_write(c->wire, c->fids[LC_ACK], 0, b, sizeof(b),
                      &c->ack_op.handle);
  if (!r)
    c->ack_pending = c->consumed;
  return lc_started(&c->ack_op, r);
}
static int events_drive(struct sophia_lc_client *c) {
  uint32_t n;
  if (c->event_op.active)
    return 0;
  n = lc_cap(c, sizeof(c->events) - c->event_used, 11, c->iounit[LC_EVENTS]);
  if (!n)
    return 0;
  return lc_started(&c->event_op,
                    sophia_9p_read(c->wire, c->fids[LC_EVENTS], c->event_offset,
                                   n, &c->event_op.handle));
}
static int drive(struct sophia_lc_client *c) {
  int r = bootstrap_drive(c);
  if (r || c->bootstrap < LC_BOOTED)
    return r;
  if ((r = events_drive(c)) || (r = ack_drive(c)) || (r = submit_drive(c)) ||
      (r = sophia_lc_internal_object_drive(c)))
    return r;
  if ((r = sophia_lc_internal_upload_drive(c)))
    return r;
  /* Draining the last upload reply can queue Cancel here. Materialize its
   * wire request before returning, so poll sees POLLOUT without a timer. */
  return submit_drive(c);
}
static int receive(struct sophia_lc_client *c, const struct sophia_9p_reply *r) {
  if (lc_same(&c->boot_op, r->handle)) {
    c->boot_op.active = 0;
    return boot_reply(c, r);
  }
  if (lc_same(&c->submit_op, r->handle)) {
    c->submit_op.active = 0;
    return submit_reply(c, r);
  }
  if (lc_same(&c->event_op, r->handle)) {
    c->event_op.active = 0;
    if (r->type == 7)
      return lc_remote(c, r->error);
    if (r->type != 117 || !r->count ||
        r->count > sizeof(c->events) - c->event_used ||
        r->count > UINT64_MAX - c->event_offset)
      return SOPHIA_9P_INVALID;
    memcpy(c->events + c->event_used, r->data, r->count);
    c->event_used += r->count;
    c->event_offset += r->count;
    return sophia_lc_internal_parse(c);
  }
  if (lc_same(&c->ack_op, r->handle)) {
    c->ack_op.active = 0;
    if (r->type == 7)
      return lc_remote(c, r->error);
    if (r->type != 119 || r->count != SOPHIA_LF_ACK_BYTES)
      return SOPHIA_9P_INVALID;
    c->acked = c->ack_pending;
    return 0;
  }
  if (lc_same(&c->object_op, r->handle)) {
    c->object_op.active = 0;
    return sophia_lc_internal_object_reply(c, r);
  }
  if (lc_same(&c->upload_op, r->handle)) {
    c->upload_op.active = 0;
    return sophia_lc_internal_upload_reply(c, r);
  }
  return sophia_lc_internal_upload_write_reply(c, r);
}
int sophia_lc_service(struct sophia_lc_client *c, size_t budget) {
  struct sophia_9p_reply reply;
  int r;
  unsigned i;
  if (!c)
    return SOPHIA_9P_ARGUMENT;
  if (c->terminal)
    return c->terminal;
  if ((r = drive(c)) || (r = sophia_9p_service(c->wire, budget)))
    return c->terminal = r;
  for (i = 0; i < 64; ++i) {
    r = sophia_9p_peek(c->wire, &reply);
    if (r == SOPHIA_9P_AGAIN)
      break;
    if (r || (r = receive(c, &reply)) ||
        (r = sophia_9p_consume(c->wire, reply.handle)))
      return c->terminal = r;
  }
  if ((r = drive(c)))
    c->terminal = r;
  return r;
}

int sophia_lc_internal_queue(struct sophia_lc_client *c,
                             const struct sophia_lf_record *value) {
  struct sophia_lf_record r;
  size_t n;
  if (c->submit_stage)
    return SOPHIA_9P_BUSY;
  if (c->next_submission == UINT64_MAX)
    return SOPHIA_9P_ARGUMENT;
  r = *value;
  r.header.epoch = c->epoch;
  r.header.submission = c->next_submission;
  r.header.sequence = 0;
  if (sophia_lf_encode(c->tx, sizeof(c->tx), &r, &n))
    return SOPHIA_9P_ARGUMENT;
  c->next_submission++;
  c->tx_size = n;
  c->tx_offset = 0;
  c->submit_stage = LC_TX_WRITE;
  c->submitted = c->submit_replied = c->submit_wait = c->submit_sent = 0;
  c->submitted_sequence = 0;
  c->submit_error = 0;
  return 0;
}
int sophia_lc_submit(struct sophia_lc_client *c,
                     const struct sophia_lf_record *r) {
  if (!c || !r || (r->header.kind != SOPHIA_LF_CANDIDATE &&
                   r->header.kind != SOPHIA_LF_FRAME_DEMAND &&
                   r->header.kind != SOPHIA_LF_RESOURCE_RETIRE))
    return SOPHIA_9P_ARGUMENT;
  if (c->terminal)
    return c->terminal;
  if (!sophia_lc_ready(c))
    return SOPHIA_9P_BUSY;
  return sophia_lc_internal_queue(c, r);
}
int sophia_lc_submission(const struct sophia_lc_client *c, uint64_t *id,
                         enum sophia_lc_submission *stage, uint32_t *error) {
  if (!c || !id || !stage)
    return SOPHIA_9P_ARGUMENT;
  *id = c->next_submission - 1;
  if (error)
    *error = c->submit_error;
  if (!*id)
    *stage = SOPHIA_LC_SUBMISSION_NONE;
  else if (c->submitted)
    *stage = SOPHIA_LC_SUBMISSION_CUSTODIED;
  else if (c->submit_error)
    *stage = SOPHIA_LC_SUBMISSION_REFUSED;
  else if (c->submit_stage == LC_TX_WRITE ||
           (c->submit_stage == LC_TX_SUBMIT && !c->submit_sent))
    *stage = SOPHIA_LC_SUBMISSION_STAGED;
  else if (c->submit_stage == LC_TX_SUBMIT || c->submit_stage == LC_TX_WAIT)
    *stage = SOPHIA_LC_SUBMISSION_ISSUED;
  else
    *stage = SOPHIA_LC_SUBMISSION_NONE;
  return 0;
}
int sophia_lc_submit_retry(struct sophia_lc_client *c) {
  if (!c)
    return SOPHIA_9P_ARGUMENT;
  if (c->terminal)
    return c->terminal;
  if (c->submit_stage != LC_TX_SUBMIT || !c->submit_wait)
    return SOPHIA_9P_ARGUMENT;
  c->submit_wait = 0;
  return 0;
}

enum sophia_lc_state sophia_lc_state(const struct sophia_lc_client *c) {
  if (!c)
    return SOPHIA_LC_FAILED;
  if (c->refusal)
    return SOPHIA_LC_REFUSED;
  if (c->stale)
    return SOPHIA_LC_STALE;
  if (c->terminal)
    return SOPHIA_LC_FAILED;
  return c->negotiated ? SOPHIA_LC_READY : SOPHIA_LC_BOOTSTRAP;
}
int sophia_lc_ready(const struct sophia_lc_client *c) {
  return sophia_lc_state(c) == SOPHIA_LC_READY;
}
uint64_t sophia_lc_epoch(const struct sophia_lc_client *c) {
  return c ? c->epoch : 0;
}
uint16_t sophia_lc_refusal(const struct sophia_lc_client *c) {
  return c ? c->refusal : 0;
}
uint32_t sophia_lc_remote_error(const struct sophia_lc_client *c) {
  return c ? c->remote_error : 0;
}
const struct sophia_lf_limits *sophia_lc_limits(const struct sophia_lc_client *c) {
  return sophia_lc_ready(c) ? &c->limits : NULL;
}
const struct sophia_lf_negotiated *
sophia_lc_welcome(const struct sophia_lc_client *c) {
  return sophia_lc_ready(c) ? &c->welcome : NULL;
}

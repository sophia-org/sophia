#include "internal.h"

void os_set(struct sophia_os *s, uint64_t id, enum sophia_os_custody custody,
            uint32_t error) {
  struct os_ticket *t = &s->tickets[id % OS_OUTCOMES];
  if (t->id == id && t->custody >= SOPHIA_OS_SUBMITTED)
    return;
  t->id = id;
  t->custody = custody;
  t->error = error;
}
int sophia_os_outcome(const struct sophia_os *s, uint64_t id,
                      enum sophia_os_custody *custody, uint32_t *error) {
  const struct os_ticket *t;
  if (!s || !id || !custody || !error)
    return SOPHIA_9P_ARGUMENT;
  t = &s->tickets[id % OS_OUTCOMES];
  *custody = t->id == id ? t->custody : SOPHIA_OS_UNAVAILABLE;
  *error = t->id == id ? t->error : 0;
  return 0;
}
int os_queue(struct sophia_os *s, const struct sophia_of_record *record,
             uint64_t deadline, uint64_t *ticket) {
  struct sophia_of_record r;
  size_t bytes;
  int result;
  if (s->tx_stage != OS_TX_IDLE)
    return SOPHIA_9P_BUSY;
  if (!s->next_ticket || !ticket || deadline <= s->now)
    return SOPHIA_9P_ARGUMENT;
  r = *record;
  r.header.epoch = s->epoch;
  r.header.submission = s->next_ticket;
  r.header.sequence = 0;
  result = sophia_of_encode(s->tx, SOPHIA_OF_MAX_CANDIDATE, &r, &bytes);
  if (result)
    return result;
  s->tx_ticket = s->next_ticket;
  s->next_ticket = s->next_ticket == UINT64_MAX ? 0 : s->next_ticket + 1;
  s->tx_domain =
      r.header.kind == SOPHIA_OF_PROPOSAL ? r.value.proposal.transaction : 0;
  s->tx_size = bytes;
  s->tx_written = 0;
  s->tx_deadline = deadline;
  s->assembly_deadline = 0;
  s->retry_at = 0;
  s->retry_delay = 0;
  s->tx_replied = 0;
  s->tx_stage = OS_TX_WALK;
  os_set(s, s->tx_ticket, SOPHIA_OS_ADMITTED_LOCAL, 0);
  *ticket = s->tx_ticket;
  return 0;
}
int sophia_os_submit(struct sophia_os *s,
                     const struct sophia_of_proposal *proposal,
                     uint64_t deadline, uint64_t *ticket) {
  struct sophia_of_record record;
  int r;
  if (!s || !proposal || !ticket)
    return SOPHIA_9P_ARGUMENT;
  if (os_final(s))
    return s->terminal;
  if (s->state != SOPHIA_OS_READY)
    return SOPHIA_9P_BUSY;
  /* An observe-only grant cannot configure; do not spend an identity. */
  if (!(s->selected & SOPHIA_OF_CAP_CONFIGURE))
    return SOPHIA_9P_ARGUMENT;
  memset(&record, 0, sizeof(record));
  record.header.kind = SOPHIA_OF_PROPOSAL;
  record.value.proposal = *proposal;
  r = os_queue(s, &record, deadline, ticket);
  if (!r)
    s->drive_pending = 1;
  return r;
}
int sophia_os_next_transaction(struct sophia_os *s, uint64_t *transaction) {
  if (!s || !transaction || !s->next_domain)
    return SOPHIA_9P_ARGUMENT;
  if (os_final(s))
    return s->terminal;
  *transaction = s->next_domain;
  s->next_domain = s->next_domain == UINT64_MAX ? 0 : s->next_domain + 1;
  return 0;
}
int os_tx_drive(struct sophia_os *s) {
  int r;
  if (s->tx_op.active || s->tx_stage == OS_TX_IDLE || s->tx_stage == OS_TX_WAIT)
    return 0;
  if (s->tx_stage == OS_TX_WALK) {
    const char *name = "transaction";
    /* The export holds one staged candidate: the previous Submitted must be
     * acknowledged (and its fid clunked, via IDLE) before the next opens. */
    if (s->acked < s->submitted_sequence)
      return 0;
    r = sophia_9p_walk(&s->wire, s->root, &name, 1, &s->tx_op.handle,
                       &s->tx_fid);
  } else if (s->tx_stage == OS_TX_OPEN)
    r = sophia_9p_lopen(&s->wire, s->tx_fid, 2, &s->tx_op.handle);
  else if (s->tx_stage == OS_TX_WRITE) {
    size_t n = s->tx_size - s->tx_written, maximum = s->wire.msize - 23u;
    if (s->tx_iounit && maximum > s->tx_iounit)
      maximum = s->tx_iounit;
    if (n > maximum)
      n = maximum;
    r = sophia_9p_write(&s->wire, s->tx_fid, s->tx_written,
                        s->tx + s->tx_written, n, &s->tx_op.handle);
    if (!r && !s->assembly_deadline)
      s->assembly_deadline = os_add(s->now, s->limits.assembly_timeout_ms);
  } else if (s->tx_stage == OS_TX_SUBMIT) {
    uint8_t b[SOPHIA_OF_SUBMIT_BYTES];
    if (s->retry_at &&
        (s->pass <= s->retry_pass ||
         (s->now < s->retry_at && s->consumed == s->retry_consumed &&
          s->acked == s->retry_acked)))
      return 0;
    if (sophia_of_submit_encode(b, s->epoch, s->tx_ticket,
                                (uint32_t)s->tx_size))
      return SOPHIA_9P_INVALID;
    r = sophia_9p_write(&s->wire, s->submit_fid, 0, b, sizeof(b),
                        &s->tx_op.handle);
    if (!r) {
      s->retry_at = 0;
      os_set(s, s->tx_ticket, SOPHIA_OS_ISSUED, 0);
    }
  } else
    r = sophia_9p_clunk(&s->wire, s->tx_fid, &s->tx_op.handle);
  return os_started(&s->tx_op, r);
}
/* Called after the dispatch batch's events have been validated. EAGAIN has
 * transferred nothing and consumed no identity: retry the identical submit.
 * ESTALE ends the attach. Other refusals (ENOSPC, EACCES, EINVAL) settle
 * this ticket; accepted work remains settleable. */
int os_tx_reply(struct sophia_os *s, const struct sophia_9p_reply *r) {
  enum sophia_os_custody custody =
      s->tickets[s->tx_ticket % OS_OUTCOMES].custody;
  if (r->type == 7) {
    s->remote_error = r->error;
    if (s->tx_stage != OS_TX_SUBMIT || custody == SOPHIA_OS_SUBMITTED ||
        r->error == 116)
      return SOPHIA_9P_INVALID;
    if (r->error == 11) {
      os_set(s, s->tx_ticket, SOPHIA_OS_ADMITTED_LOCAL, 0);
      s->retry_delay = os_backoff(s->retry_delay);
      s->retry_at = os_add(s->now, s->retry_delay);
      s->retry_pass = s->pass;
      s->retry_consumed = s->consumed;
      s->retry_acked = s->acked;
      return 0;
    }
    os_set(s, s->tx_ticket, SOPHIA_OS_REFUSED_SUBMIT, r->error);
    if (!s->negotiated)
      return SOPHIA_9P_INVALID;
    s->tx_stage = OS_TX_CLUNK;
    return 0;
  }
  switch (s->tx_stage) {
  case OS_TX_WALK:
    if (r->type != 111 || r->count != 1 || r->qid.type)
      return SOPHIA_9P_INVALID;
    s->tx_stage = OS_TX_OPEN;
    break;
  case OS_TX_OPEN:
    if (r->type != 13 || r->qid.type)
      return SOPHIA_9P_INVALID;
    s->tx_iounit = r->iounit;
    s->tx_stage = OS_TX_WRITE;
    break;
  case OS_TX_WRITE:
    if (r->type != 119 || !r->count || r->count > s->tx_size - s->tx_written)
      return SOPHIA_9P_INVALID;
    s->tx_written += r->count;
    if (s->tx_written == s->tx_size)
      s->tx_stage = OS_TX_SUBMIT;
    break;
  case OS_TX_SUBMIT:
    if (r->type != 119 || r->count != SOPHIA_OF_SUBMIT_BYTES)
      return SOPHIA_9P_INVALID;
    s->tx_replied = 1;
    s->tx_stage = custody == SOPHIA_OS_SUBMITTED ? OS_TX_CLUNK : OS_TX_WAIT;
    break;
  case OS_TX_CLUNK:
    if (r->type != 121)
      return SOPHIA_9P_INVALID;
    s->tx_stage = OS_TX_IDLE;
    s->tx_deadline = s->assembly_deadline = s->retry_at = 0;
    break;
  default:
    return SOPHIA_9P_INVALID;
  }
  return 0;
}

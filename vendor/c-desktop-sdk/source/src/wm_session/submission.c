#include "internal.h"

void ws_set(struct sophia_ws *s, uint64_t id, enum sophia_ws_custody custody,
            uint32_t error) {
  struct ws_ticket *t = &s->tickets[id % WS_OUTCOMES];
  if (t->id == id && t->custody >= SOPHIA_WS_SUBMITTED)
    return;
  t->id = id;
  t->custody = custody;
  t->error = error;
}
int sophia_ws_outcome(const struct sophia_ws *s, uint64_t id,
                      enum sophia_ws_custody *custody, uint32_t *error) {
  const struct ws_ticket *t;
  if (!s || !id || !custody || !error)
    return SOPHIA_9P_ARGUMENT;
  t = &s->tickets[id % WS_OUTCOMES];
  *custody = t->id == id ? t->custody : SOPHIA_WS_UNAVAILABLE;
  *error = t->id == id ? t->error : 0;
  return 0;
}
int ws_queue(struct sophia_ws *s, const struct sophia_wf_record *record,
             uint64_t deadline, uint64_t *ticket) {
  struct sophia_wf_record r;
  uint64_t domain = 0;
  size_t bytes;
  int result;
  if (s->tx_stage != WS_TX_IDLE)
    return SOPHIA_9P_BUSY;
  if (!s->next_ticket || !ticket || deadline <= s->now)
    return SOPHIA_9P_ARGUMENT;
  if (record->header.kind < 256 || record->header.kind > 263)
    return SOPHIA_9P_ARGUMENT;
  if (record->header.kind == SOPHIA_WF_CONFIGURATION)
    domain = record->value.configuration.transaction;
  if (record->header.kind == SOPHIA_WF_PROJECTION)
    domain = record->value.projection.transaction;
  if (record->header.kind == SOPHIA_WF_SESSION_OPERATION)
    domain = record->value.session_operation.transaction;
  if (domain && domain <= s->submitted_domain)
    return SOPHIA_9P_INVALID;
  r = *record;
  r.header.epoch = s->epoch;
  r.header.submission = s->next_ticket;
  r.header.sequence = 0;
  result =
      sophia_wf_encode(s->tx, SOPHIA_WF_MAX_RECORD, s->selected, &r, &bytes);
  if (result)
    return result;
  s->tx_ticket = s->next_ticket;
  s->next_ticket = s->next_ticket == UINT64_MAX ? 0 : s->next_ticket + 1;
  s->tx_domain = domain;
  s->tx_size = bytes;
  s->tx_written = 0;
  s->tx_deadline = deadline;
  s->assembly_deadline = 0;
  s->retry_at = 0;
  s->retry_delay = 0;
  s->tx_replied = 0;
  s->tx_stage = WS_TX_WALK;
  ws_set(s, s->tx_ticket, SOPHIA_WS_ADMITTED_LOCAL, 0);
  *ticket = s->tx_ticket;
  return 0;
}
int sophia_ws_submit(struct sophia_ws *s, const struct sophia_wf_record *record,
                     uint64_t deadline, uint64_t *ticket) {
  if (!s || !record || !ticket || record->header.kind == SOPHIA_WF_NEGOTIATE)
    return SOPHIA_9P_ARGUMENT;
  if (ws_final(s))
    return s->terminal;
  if (s->state != SOPHIA_WS_READY)
    return SOPHIA_9P_BUSY;
  return ws_queue(s, record, deadline, ticket);
}
int sophia_ws_next_transaction(struct sophia_ws *s, uint64_t *transaction) {
  if (!s || !transaction || !s->next_domain)
    return SOPHIA_9P_ARGUMENT;
  if (ws_final(s))
    return s->terminal;
  *transaction = s->next_domain;
  s->next_domain = s->next_domain == UINT64_MAX ? 0 : s->next_domain + 1;
  return 0;
}
int ws_tx_drive(struct sophia_ws *s) {
  int r;
  if (s->tx_op.active || s->tx_stage == WS_TX_IDLE || s->tx_stage == WS_TX_WAIT)
    return 0;
  if (s->tx_stage == WS_TX_WALK) {
    const char *name = "transaction";
    if (s->acked < s->submitted_sequence)
      return 0;
    r = sophia_9p_walk(&s->wire, s->root, &name, 1, &s->tx_op.handle,
                       &s->tx_fid);
  } else if (s->tx_stage == WS_TX_OPEN)
    r = sophia_9p_lopen(&s->wire, s->tx_fid, 2, &s->tx_op.handle);
  else if (s->tx_stage == WS_TX_WRITE) {
    size_t n = s->tx_size - s->tx_written, maximum = s->wire.msize - 23u;
    if (s->tx_iounit && maximum > s->tx_iounit)
      maximum = s->tx_iounit;
    if (n > maximum)
      n = maximum;
    r = sophia_9p_write(&s->wire, s->tx_fid, s->tx_written,
                        s->tx + s->tx_written, n, &s->tx_op.handle);
    if (!r && !s->assembly_deadline)
      s->assembly_deadline = ws_add(s->now, s->limits.assembly_timeout_ms);
  } else if (s->tx_stage == WS_TX_SUBMIT) {
    uint8_t b[24];
    if (s->retry_at &&
        (s->pass <= s->retry_pass ||
         (s->now < s->retry_at && s->consumed == s->retry_consumed &&
          s->acked == s->retry_acked)))
      return 0;
    if (sophia_wf_submit_encode(b, s->epoch, s->tx_ticket,
                                (uint32_t)s->tx_size))
      return SOPHIA_9P_INVALID;
    r = sophia_9p_write(&s->wire, s->submit_fid, 0, b, 24, &s->tx_op.handle);
    if (!r) {
      s->retry_at = 0;
      ws_set(s, s->tx_ticket, SOPHIA_WS_ISSUED, 0);
    }
  } else
    r = sophia_9p_clunk(&s->wire, s->tx_fid, &s->tx_op.handle);
  return ws_started(&s->tx_op, r);
}
/* Called only after the dispatch batch's event bytes have been appended and
 * validated. A Submitted in that batch therefore wins either reply ordering. */
int ws_tx_reply(struct sophia_ws *s, const struct sophia_9p_reply *r) {
  enum sophia_ws_custody custody =
      s->tickets[s->tx_ticket % WS_OUTCOMES].custody;
  if (r->type == 7) {
    s->remote_error = r->error;
    if (s->tx_stage != WS_TX_SUBMIT)
      return SOPHIA_9P_INVALID;
    if (r->error == 114 && custody == SOPHIA_WS_SUBMITTED) {
      s->tx_replied = 1;
      s->tx_stage = WS_TX_CLUNK;
      return 0;
    }
    if (custody == SOPHIA_WS_SUBMITTED || r->error == 114 || r->error == 116)
      return SOPHIA_9P_INVALID;
    if (r->error == 11) {
      ws_set(s, s->tx_ticket, SOPHIA_WS_ADMITTED_LOCAL, 0);
      s->retry_delay = s->retry_delay ? s->retry_delay * 2 : 4;
      if (s->retry_delay > 256)
        s->retry_delay = 256;
      s->retry_at = ws_add(s->now, s->retry_delay);
      s->retry_pass = s->pass;
      s->retry_consumed = s->consumed;
      s->retry_acked = s->acked;
      return 0;
    }
    ws_set(s, s->tx_ticket, SOPHIA_WS_REFUSED, r->error);
    if (!s->negotiated)
      return SOPHIA_9P_INVALID;
    s->tx_stage = WS_TX_CLUNK;
    return 0;
  }
  switch (s->tx_stage) {
  case WS_TX_WALK:
    if (r->type != 111 || r->count != 1 || r->qid.type)
      return SOPHIA_9P_INVALID;
    s->tx_stage = WS_TX_OPEN;
    break;
  case WS_TX_OPEN:
    if (r->type != 13 || r->qid.type)
      return SOPHIA_9P_INVALID;
    s->tx_iounit = r->iounit;
    s->tx_stage = WS_TX_WRITE;
    break;
  case WS_TX_WRITE:
    if (r->type != 119 || !r->count || r->count > s->tx_size - s->tx_written)
      return SOPHIA_9P_INVALID;
    s->tx_written += r->count;
    if (s->tx_written == s->tx_size)
      s->tx_stage = WS_TX_SUBMIT;
    break;
  case WS_TX_SUBMIT:
    if (r->type != 119 || r->count != 24)
      return SOPHIA_9P_INVALID;
    s->tx_replied = 1;
    s->tx_stage = custody == SOPHIA_WS_SUBMITTED ? WS_TX_CLUNK : WS_TX_WAIT;
    break;
  case WS_TX_CLUNK:
    if (r->type != 121)
      return SOPHIA_9P_INVALID;
    s->tx_stage = WS_TX_IDLE;
    s->tx_deadline = s->assembly_deadline = s->retry_at = 0;
    break;
  default:
    return SOPHIA_9P_INVALID;
  }
  return 0;
}

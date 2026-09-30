#include "internal.h"

int os_event_append(struct sophia_os *s, const struct sophia_9p_reply *r) {
  if (r->type != 117 || !r->count ||
      r->count > OS_JOURNAL_BYTES - s->journal_used ||
      r->count > UINT64_MAX - s->event_offset)
    return SOPHIA_9P_INVALID;
  if (s->journal_used == s->journal_validated)
    s->fragment_deadline = os_add(s->now, s->limits.ack_timeout_ms);
  memcpy(s->journal + s->journal_used, r->data, r->count);
  s->journal_used += r->count;
  s->event_offset += r->count;
  return 0;
}
static int negotiated(struct sophia_os *s, uint64_t granted) {
  const struct sophia_of_negotiate *offer = &s->config.offer;
  /* The grant must be a subset of this request; the codec already required
   * observe and refused unknown bits. */
  if (s->negotiated || s->refusal || (granted & ~offer->capabilities) ||
      offer->minimum_revision > SOPHIA_OF_REVISION ||
      offer->maximum_revision < SOPHIA_OF_REVISION)
    return -1;
  s->selected = granted;
  s->negotiated = 1;
  s->state = SOPHIA_OS_READY;
  return 0;
}
/* Validate all received events in order, even behind an application-held
 * head. Only negotiation and file custody have effects here. A malformed
 * record stops the scan; no later bytes can establish custody. */
int os_events_validate(struct sophia_os *s) {
  struct sophia_of_record r;
  size_t before = s->journal_validated;
  while (s->journal_used - s->journal_validated >= 4) {
    const uint8_t *p = s->journal + s->journal_validated;
    size_t n = (size_t)of_get(p, 4);
    if (n < SOPHIA_OF_HEADER_BYTES || n > OS_EVENT_BYTES)
      return SOPHIA_9P_INVALID;
    if (n > s->journal_used - s->journal_validated)
      break;
    if (sophia_of_decode(p, n, &r) || r.header.epoch != s->epoch ||
        r.header.sequence <= s->sequence || r.header.kind < 16 ||
        r.header.kind >= 256 || s->journal_records == OS_MAX_EVENTS ||
        s->refused_sequence)
      return SOPHIA_9P_INVALID;
    switch (r.header.kind) {
    case SOPHIA_OF_NEGOTIATED:
      if (negotiated(s, r.value.granted_capabilities))
        return SOPHIA_9P_INVALID;
      break;
    case SOPHIA_OF_REFUSED:
      if (s->negotiated)
        return SOPHIA_9P_INVALID;
      s->refusal = r.value.refusal;
      s->refused_sequence = r.header.sequence;
      break;
    case SOPHIA_OF_SUBMITTED: {
      struct os_ticket *t = &s->tickets[s->tx_ticket % OS_OUTCOMES];
      if (!s->tx_ticket || t->id != s->tx_ticket ||
          t->custody != SOPHIA_OS_ISSUED ||
          r.value.submitted.submission != s->tx_ticket ||
          r.value.submitted.kind != of_get(s->tx + 6, 2))
        return SOPHIA_9P_INVALID;
      os_set(s, s->tx_ticket, SOPHIA_OS_SUBMITTED, 0);
      s->submitted_sequence = r.header.sequence;
      if (s->tx_domain && s->next_domain && s->next_domain <= s->tx_domain)
        s->next_domain = s->tx_domain == UINT64_MAX ? 0 : s->tx_domain + 1;
      if (s->tx_replied)
        s->tx_stage = OS_TX_CLUNK;
      break;
    }
    default:
      /* ObjectPublished and Outcome follow a successful negotiation. */
      if (!s->negotiated)
        return SOPHIA_9P_INVALID;
    }
    s->sequence = r.header.sequence;
    s->journal_validated += n;
    ++s->journal_records;
  }
  if (s->journal_used == s->journal_validated)
    s->fragment_deadline = 0;
  else if (s->journal_validated != before)
    s->fragment_deadline = os_add(s->now, s->limits.ack_timeout_ms);
  return 0;
}
static void remove_head(struct sophia_os *s) {
  size_t n = (size_t)of_get(s->journal, 4);
  s->consumed = of_get(s->journal + 24, 8);
  s->journal_used -= n;
  s->journal_validated -= n;
  --s->journal_records;
  memmove(s->journal, s->journal + n, s->journal_used);
  s->event_ready = 0;
}
/* Internal records are consumed here. An ObjectPublished head starts the
 * fetch of exactly that object and is presented only once the fetch is
 * complete, so its acknowledgement can never precede the full read. */
int os_event_head(struct sophia_os *s) {
  unsigned visits;
  if (s->event_ready)
    return 0;
  for (visits = 0; visits < OS_MAX_EVENTS && s->journal_validated; ++visits) {
    uint16_t kind = (uint16_t)of_get(s->journal + 6, 2);
    if (kind == SOPHIA_OF_NEGOTIATED || kind == SOPHIA_OF_SUBMITTED ||
        kind == SOPHIA_OF_REFUSED) {
      remove_head(s);
      s->drive_pending = 1;
      continue;
    }
    if (sophia_of_decode(s->journal, (size_t)of_get(s->journal, 4),
                         &s->event))
      return SOPHIA_9P_INVALID;
    if (kind == SOPHIA_OF_OBJECT_PUBLISHED) {
      const struct sophia_of_published *p = &s->event.value.published;
      if (s->object_stage == OS_OBJECT_IDLE) {
        s->object_qid = p->qid_path;
        s->object_epoch = p->topology_epoch;
        s->object_used = 0;
        s->object_retry_at = 0;
        s->object_retry_delay = 0;
        /* Reading an announced object is part of acknowledging it. */
        s->object_deadline = os_add(s->now, s->limits.ack_timeout_ms);
        s->object_stage = OS_OBJECT_WALK;
        s->drive_pending = 1;
        return 0;
      }
      if (s->object_stage != OS_OBJECT_READY || s->object_qid != p->qid_path)
        return 0;
    }
    s->event_ready = 1;
    break;
  }
  return 0;
}
int sophia_os_event(struct sophia_os *s, const struct sophia_of_record **out) {
  if (!s || !out)
    return SOPHIA_9P_ARGUMENT;
  if (os_final(s))
    return s->terminal;
  if (!s->event_ready)
    return SOPHIA_9P_AGAIN;
  *out = &s->event;
  return 0;
}
int sophia_os_consume(struct sophia_os *s) {
  int r;
  if (!s)
    return SOPHIA_9P_ARGUMENT;
  if (os_final(s))
    return s->terminal;
  if (!s->event_ready)
    return SOPHIA_9P_ARGUMENT;
  if (s->event.header.kind == SOPHIA_OF_OBJECT_PUBLISHED)
    s->object_stage = OS_OBJECT_IDLE;
  remove_head(s);
  r = os_event_head(s);
  if (!r)
    s->drive_pending = 1;
  return r ? os_finish(s, r) : 0;
}

#include "internal.h"

int ws_event_append(struct sophia_ws *s, const struct sophia_9p_reply *r) {
  if (r->type != 117 || !r->count ||
      r->count > WS_JOURNAL_BYTES - s->journal_used ||
      r->count > UINT64_MAX - s->event_offset)
    return SOPHIA_9P_INVALID;
  if (s->journal_used == s->journal_validated)
    s->fragment_deadline = ws_add(s->now, s->limits.send_timeout_ms);
  memcpy(s->journal + s->journal_used, r->data, r->count);
  s->journal_used += r->count;
  s->event_offset += r->count;
  return 0;
}
/* Validate all received events in order, even behind an application-held head.
 * Only negotiation and transport custody have effects here. A malformed record
 * stops this scan; no later bytes can establish custody. */
int ws_events_validate(struct sophia_ws *s) {
  struct sophia_wf_record r;
  size_t before = s->journal_validated;
  while (s->journal_used - s->journal_validated >= 4) {
    const uint8_t *p = s->journal + s->journal_validated;
    size_t n = (size_t)wf_get(p, 4);
    if (n < 32 || n > WS_EVENT_BYTES)
      goto fault;
    if (n > s->journal_used - s->journal_validated)
      break;
    if (sophia_wf_decode(p, n, s->selected, &r) || r.header.epoch != s->epoch ||
        r.header.sequence <= s->sequence || r.header.kind < 16 ||
        r.header.kind > 25 || s->journal_records == 64)
      goto fault;
    if (r.header.kind == SOPHIA_WF_NEGOTIATED) {
      uint64_t caps = r.value.selected_capabilities;
      uint64_t offer = s->config.offer.required | s->config.offer.optional;
      if (s->negotiated || (caps & ~offer) ||
          (caps & ~s->limits.capability_ceiling) ||
          (caps & s->config.offer.required) != s->config.offer.required ||
          ((caps & SOPHIA_WF_CAP_PRESENTATION_ACTIONS) &&
           !(caps & SOPHIA_WF_CAP_SURFACE_INSTANCES)) ||
          ((caps & SOPHIA_WF_CAP_ACTION_LIFECYCLE) &&
           (~caps & (SOPHIA_WF_CAP_ACTIONS | SOPHIA_WF_CAP_CONFIGURATION))) ||
          ((caps & SOPHIA_WF_CAP_OUTPUT_LAUNCH_CONTEXT) &&
           !(caps & SOPHIA_WF_CAP_LAUNCH_ORIGIN)) ||
          (s->limits.profile_required &&
           !(caps & SOPHIA_WF_CAP_PROFILE_ACTIVATION)))
        goto fault;
      s->selected = caps;
      s->negotiated = 1;
      s->state = SOPHIA_WS_READY;
    } else if (r.header.kind == SOPHIA_WF_SUBMITTED) {
      struct ws_ticket *t = &s->tickets[s->tx_ticket % WS_OUTCOMES];
      if (!s->tx_ticket || t->id != s->tx_ticket ||
          t->custody != SOPHIA_WS_ISSUED ||
          r.value.submitted.submission != s->tx_ticket ||
          r.value.submitted.kind != wf_get(s->tx + 6, 2))
        goto fault;
      ws_set(s, s->tx_ticket, SOPHIA_WS_SUBMITTED, 0);
      s->submitted_sequence = r.header.sequence;
      if (s->tx_domain) {
        s->submitted_domain = s->tx_domain;
        if (s->next_domain && s->next_domain <= s->tx_domain)
          s->next_domain = s->tx_domain == UINT64_MAX ? 0 : s->tx_domain + 1;
      }
      if (s->tx_replied)
        s->tx_stage = WS_TX_CLUNK;
    } else if (!s->negotiated)
      goto fault;
    s->sequence = r.header.sequence;
    s->journal_validated += n;
    ++s->journal_records;
  }
  if (s->journal_used == s->journal_validated)
    s->fragment_deadline = 0;
  else if (s->journal_validated != before)
    s->fragment_deadline = ws_add(s->now, s->limits.send_timeout_ms);
  return 0;
fault:
  s->event_fault = 1;
  return SOPHIA_9P_INVALID;
}
static void remove_head(struct sophia_ws *s) {
  size_t n = (size_t)wf_get(s->journal, 4);
  s->consumed = wf_get(s->journal + 24, 8);
  s->journal_used -= n;
  s->journal_validated -= n;
  --s->journal_records;
  memmove(s->journal, s->journal + n, s->journal_used);
  s->event_ready = 0;
}
int ws_event_head(struct sophia_ws *s) {
  unsigned visits;
  if (s->event_ready)
    return 0;
  for (visits = 0; visits < 64 && s->journal_validated; ++visits) {
    uint16_t kind = (uint16_t)wf_get(s->journal + 6, 2);
    if (kind == SOPHIA_WF_NEGOTIATED || kind == SOPHIA_WF_SUBMITTED) {
      remove_head(s);
      continue;
    }
    if (sophia_wf_decode(s->journal, (size_t)wf_get(s->journal, 4), s->selected,
                         &s->event))
      return SOPHIA_9P_INVALID;
    s->event_ready = 1;
    break;
  }
  return 0;
}
int sophia_ws_event(struct sophia_ws *s, const struct sophia_wf_record **out) {
  if (!s || !out)
    return SOPHIA_9P_ARGUMENT;
  if (ws_final(s))
    return s->terminal;
  if (!s->event_ready)
    return SOPHIA_9P_AGAIN;
  *out = &s->event;
  return 0;
}
int sophia_ws_consume(struct sophia_ws *s) {
  int r;
  if (!s)
    return SOPHIA_9P_ARGUMENT;
  if (ws_final(s))
    return s->terminal;
  if (!s->event_ready)
    return SOPHIA_9P_ARGUMENT;
  remove_head(s);
  r = ws_event_head(s);
  if (!r)
    s->drive_pending = 1;
  return r ? ws_finish(s, r) : 0;
}

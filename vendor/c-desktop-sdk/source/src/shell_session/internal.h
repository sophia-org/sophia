#ifndef SOPHIA_SS_INTERNAL_H
#define SOPHIA_SS_INTERNAL_H
#include "../sophia_shell_session.h"
#include <string.h>
static inline void ss_set(struct sophia_ss *s, uint64_t ticket, enum sophia_ss_outcome outcome,
                          uint32_t error)
{
    struct sophia_ss_ticket *t = &s->tickets[ticket % SOPHIA_SS_OUTCOMES];
    t->ticket = ticket;
    t->error = error;
    t->outcome = (uint8_t)outcome;
}
static inline int ss_final(const struct sophia_ss *s) { return s->state > SOPHIA_SS_READY; }
/* Consumption is progress; the first unacknowledged one starts the ack clock. */
static inline void ss_consumed(struct sophia_ss *s, uint64_t sequence)
{
    if (s->last_consumed <= s->files.acked_sequence)
        s->ack_clock = s->now_ms;
    s->last_consumed = sequence;
    s->progress = 1;
}
int ss_status(const struct sophia_ss *);
int ss_pump(struct sophia_ss *, size_t budget);
int ss_handoff(struct sophia_ss *);
#endif

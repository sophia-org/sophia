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
/* Free queue slot and bytes outside any reservation. */
int ss_room(const struct sophia_ss *, size_t bytes);
/* Admits one already-encoded candidate (submission 0) with a new ticket. */
int ss_admit_encoded(struct sophia_ss *, const uint8_t *, size_t, uint64_t *ticket);
/* Image adapter hooks; each does nothing without an attached adapter. */
void ss_image_drive(struct sophia_ss *, uint64_t now_ms);
int ss_image_event(struct sophia_ss *, const struct sophia_sf_record *);
void ss_image_status(struct sophia_ss *);
int ss_pump(struct sophia_ss *, size_t budget);
/* Owned-ring hooks (shell r10), called by the image adapter. */
struct sophia_ss_image;
int ss_ring_mode(const struct sophia_ss_image *);
void ss_ring_drive(struct sophia_ss_image *, uint64_t now_ms);
void ss_ring_lost(struct sophia_ss_image *);
void ss_ring_event(struct sophia_ss_image *, const uint8_t *, size_t);
void ss_ring_status(struct sophia_ss_image *, const struct sophia_sf_record *);
int ss_ring_poll_out(const struct sophia_ss_image *);
int64_t ss_ring_timeout(const struct sophia_ss_image *, uint64_t now_ms);
int ss_handoff(struct sophia_ss *);
#endif

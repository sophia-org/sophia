#include "internal.h"
#include "../shell_files/session_internal.h"

/* Encode with the live epoch and submission 0; a stand-in id satisfies
 * validation and hand-off assigns the real one. BUSY: no room. */
static int encode(const struct sophia_sf_record *record, uint64_t epoch, uint8_t *dst,
                  size_t capacity, size_t *n)
{
    struct sophia_sf_record r;
    int status;
    if (record->header.kind <= SOPHIA_SF_NEGOTIATE)
        return SOPHIA_9P_ARGUMENT;
    r = *record;
    r.header.epoch = epoch;
    r.header.submission = 1;
    r.header.sequence = 0;
    status = sophia_sf_encode(dst, capacity, &r, n);
    if (status)
        return status == -1 ? SOPHIA_9P_INVALID : SOPHIA_9P_BUSY;
    memset(dst + 16, 0, 8);
    return 0;
}
size_t sophia_ss_record_bytes(const struct sophia_sf_record *record)
{
    struct sophia_sf_record r;
    if (!record || record->header.kind <= SOPHIA_SF_NEGOTIATE)
        return 0;
    r = *record;
    r.header.epoch = 1;
    r.header.submission = 1;
    r.header.sequence = 0;
    switch (r.header.kind) {
    case SOPHIA_SF_DESCRIPTOR_CANDIDATE:
        r.header.epoch = r.value.descriptor_candidate.connection_epoch;
        break;
    case SOPHIA_SF_DESCRIPTOR_ACTIVATION_ACK:
        r.header.epoch = r.value.descriptor_activation_ack.connection_epoch;
        break;
    case SOPHIA_SF_TABS_CANDIDATE:
        r.header.epoch = r.value.tabs_candidate.connection_epoch;
        break;
    case SOPHIA_SF_REFERENCE_CANDIDATE:
        r.header.epoch = r.value.reference_candidate.connection_epoch;
        break;
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_CANDIDATE:
        r.header.epoch = r.value.descriptor_launcher_candidate.connection_epoch;
        break;
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION_ACK:
        r.header.epoch = r.value.descriptor_launcher_activation_ack.grant.connection_epoch;
        break;
    }
    return sophia_sf_record_bytes(&r);
}
/* Encode the whole group into free queue bytes before any state changes.
 * Bytes past queue_used are scratch, so a refused group leaves no trace. */
static int admit(struct sophia_ss *s, const struct sophia_sf_record *records, size_t count,
                 size_t slots, size_t room, int reserved, uint64_t *first)
{
    size_t i, n, used = 0, sizes[SOPHIA_SS_SLOTS];
    int status;
    if (!records || !count || !first || count > s->slots)
        return SOPHIA_9P_ARGUMENT;
    if (count > slots)
        return reserved ? SOPHIA_9P_ARGUMENT : SOPHIA_9P_BUSY;
    for (i = 0; i < count; i++) {
        size_t capacity = room - used;
        if (!sf_session_candidate_allowed(&s->files, records[i].header.kind))
            return SOPHIA_9P_ARGUMENT;
        if (capacity > s->files.tx_capacity)
            capacity = s->files.tx_capacity;
        status = encode(&records[i], s->files.epoch, s->queue + s->queue_used + used, capacity,
                        &n);
        /* Only a record that could never fit, or overruns a reservation, is
         * an argument error; otherwise the queue is merely full now. */
        if (status == SOPHIA_9P_BUSY &&
            (reserved || room - used >= s->files.tx_capacity ||
             sophia_ss_record_bytes(&records[i]) > s->files.tx_capacity ||
             sophia_ss_record_bytes(&records[i]) > s->queue_capacity))
            status = SOPHIA_9P_ARGUMENT;
        if (status)
            return status;
        sizes[i] = n;
        used += n;
    }
    *first = s->next_ticket;
    for (i = 0; i < count; i++) {
        s->entries[s->queued].ticket = s->next_ticket;
        s->entries[s->queued++].bytes = sizes[i];
        ss_set(s, s->next_ticket++, SOPHIA_SS_ADMITTED_LOCAL, 0);
    }
    s->queue_used += used;
    return 0;
}
int sophia_ss_submit(struct sophia_ss *s, const struct sophia_sf_record *records, size_t count,
                     uint64_t *first_ticket)
{
    int r;
    if (!s)
        return SOPHIA_9P_ARGUMENT;
    if (ss_final(s))
        return ss_status(s);
    if (s->state != SOPHIA_SS_READY)
        return SOPHIA_9P_BUSY;
    /* Tickets promised to an outstanding reservation stay available. */
    if (count > s->slots || UINT64_MAX - s->next_ticket < (uint64_t)count + s->reserved_slots)
        return SOPHIA_9P_ARGUMENT;
    r = admit(s, records, count, (size_t)(s->slots - s->queued - s->reserved_slots),
              s->queue_capacity - s->queue_used - s->reserved_bytes, 0, first_ticket);
    if (!r)
        (void)ss_pump(s, 0);
    return r;
}
int sophia_ss_reserve(struct sophia_ss *s, uint16_t slots, size_t bytes,
                      struct sophia_ss_reservation *out)
{
    if (!s || !out || !slots || slots > s->slots || bytes > s->queue_capacity)
        return SOPHIA_9P_ARGUMENT;
    if (ss_final(s))
        return ss_status(s);
    if (s->state != SOPHIA_SS_READY || s->reserved || slots > s->slots - s->queued ||
        bytes > s->queue_capacity - s->queue_used)
        return SOPHIA_9P_BUSY;
    if (UINT64_MAX - s->next_ticket < slots || s->reservation == UINT64_MAX)
        return SOPHIA_9P_ARGUMENT;
    s->reserved = 1;
    s->reserved_slots = slots;
    s->reserved_bytes = bytes;
    out->serial = ++s->reservation;
    out->slots = slots;
    out->bytes = bytes;
    return 0;
}
static int release(struct sophia_ss *s, struct sophia_ss_reservation *v)
{
    if (!s || !v || !s->reserved || v->serial != s->reservation)
        return SOPHIA_9P_ARGUMENT;
    s->reserved = 0;
    s->reserved_slots = 0;
    s->reserved_bytes = 0;
    memset(v, 0, sizeof(*v));
    return 0;
}
int sophia_ss_commit(struct sophia_ss *s, struct sophia_ss_reservation *v,
                     const struct sophia_sf_record *records, size_t count, uint64_t *first_ticket)
{
    int r;
    if (!s || !v || !s->reserved || v->serial != s->reservation)
        return SOPHIA_9P_ARGUMENT;
    if (ss_final(s)) {
        (void)release(s, v);
        return ss_status(s);
    }
    r = admit(s, records, count, s->reserved_slots, s->reserved_bytes, 1, first_ticket);
    if (r)
        return r;
    (void)release(s, v);
    (void)ss_pump(s, 0);
    return 0;
}
int sophia_ss_cancel(struct sophia_ss *s, struct sophia_ss_reservation *v)
{
    return release(s, v);
}
/* Hand the queue head to the file client, which copies it and assigns its id. */
int ss_handoff(struct sophia_ss *s)
{
    enum sophia_sf_submission stage;
    size_t n;
    int r;
    if (s->state != SOPHIA_SS_READY || s->flight || !s->queued)
        return 0;
    n = s->entries[0].bytes;
    r = sophia_sf_client_submit_bytes(&s->files, s->queue, n);
    if (r == SOPHIA_9P_BUSY)
        return 0;
    if (r || sophia_sf_client_submission(&s->files, &s->flight_id, &stage))
        return r ? r : SOPHIA_9P_INVALID;
    s->flight = 1;
    s->flight_ticket = s->entries[0].ticket;
    ss_set(s, s->flight_ticket, SOPHIA_SS_IN_FLIGHT, 0);
    s->queue_used -= n;
    memmove(s->queue, s->queue + n, s->queue_used);
    s->queued--;
    memmove(s->entries, s->entries + 1, s->queued * sizeof(s->entries[0]));
    return 0;
}
int sophia_ss_outcome(const struct sophia_ss *s, uint64_t ticket, enum sophia_ss_outcome *out,
                      uint32_t *error)
{
    const struct sophia_ss_ticket *t;
    if (!s || !out || !ticket || ticket >= s->next_ticket)
        return SOPHIA_9P_ARGUMENT;
    t = &s->tickets[ticket % SOPHIA_SS_OUTCOMES];
    *out = t->ticket == ticket ? (enum sophia_ss_outcome)t->outcome : SOPHIA_SS_UNAVAILABLE;
    if (error)
        *error = t->ticket == ticket ? t->error : 0;
    return 0;
}
static int upload_gate(struct sophia_ss *s, uint64_t *ticket)
{
    if (!s || !ticket)
        return SOPHIA_9P_ARGUMENT;
    if (ss_final(s))
        return ss_status(s);
    if (s->state != SOPHIA_SS_READY || s->flight || s->queued || s->reserved)
        return SOPHIA_9P_BUSY;
    return s->next_ticket == UINT64_MAX ? SOPHIA_9P_ARGUMENT : 0;
}
/* The file client staged an upload record itself; track it like a hand-off. */
static int adopt(struct sophia_ss *s, int status, uint64_t *ticket)
{
    enum sophia_sf_submission stage;
    if (status)
        return status;
    (void)sophia_sf_client_submission(&s->files, &s->flight_id, &stage);
    s->flight = 1;
    s->flight_ticket = s->next_ticket++;
    ss_set(s, s->flight_ticket, SOPHIA_SS_IN_FLIGHT, 0);
    *ticket = s->flight_ticket;
    (void)ss_pump(s, 0);
    return 0;
}
int sophia_ss_upload_begin(struct sophia_ss *s, struct sophia_sf_resource_begin v,
                           uint64_t *ticket)
{
    int r = upload_gate(s, ticket);
    return r ? r : adopt(s, sophia_sf_client_upload_begin(&s->files, v), ticket);
}
int sophia_ss_upload_end(struct sophia_ss *s, uint64_t transaction, uint64_t *ticket)
{
    int r = upload_gate(s, ticket);
    return r ? r : adopt(s, sophia_sf_client_upload_end(&s->files, transaction), ticket);
}
int sophia_ss_upload_cancel(struct sophia_ss *s, uint64_t transaction, uint64_t *ticket)
{
    int r = upload_gate(s, ticket);
    return r ? r : adopt(s, sophia_sf_client_upload_cancel(&s->files, transaction), ticket);
}
int sophia_ss_upload_chunk(struct sophia_ss *s, const void *bytes, size_t count)
{
    int r;
    if (!s)
        return SOPHIA_9P_ARGUMENT;
    if (ss_final(s))
        return ss_status(s);
    r = sophia_sf_client_upload_chunk(&s->files, bytes, count);
    if (!r)
        (void)ss_pump(s, 0);
    return r;
}
int sophia_ss_upload_ready(const struct sophia_ss *s)
{
    return s && !ss_final(s) && sophia_sf_client_upload_ready(&s->files);
}
int sophia_ss_upload_pending(const struct sophia_ss *s)
{
    return s && !ss_final(s) && sophia_sf_client_upload_pending(&s->files);
}

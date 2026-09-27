/* Scripted in-process 9P shell files peer for session tests, included once by
 * each test binary (static inline: tests use different subsets). Requests are
 * answered as they arrive except where a test holds them; events come only
 * from the scripted journal. */
#include "../sophia_shell_session.h"
#include "shell_files_vectors.h"
#include <assert.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

enum { F_NONE, F_ROOT, F_API, F_EVENTS, F_TX, F_SUBMIT, F_ACK, F_LIMITS, F_OUTPUTS, F_UPLOAD };
/* ACCEPT: Rwrite then Submitted. SILENT: Rwrite only. HOLD: no reply.
 * ERROR: Rlerror. CUSTODY: Submitted journaled while the Rwrite is held. */
enum { P_ACCEPT, P_SILENT, P_HOLD, P_ERROR, P_CUSTODY };
#define PEER_EPOCH 17u
#define OUTPUTS_QID 0x77u
struct peer {
    int fd;
    uint8_t in[16384];
    size_t in_used;
    uint32_t fids[64];
    uint8_t files[64];
    unsigned fid_count;
    uint8_t journal[16384];
    size_t journal_used;
    uint64_t sequence, acked;
    int event_held, tx_held, submit_held, hold_tx, refuse_negotiation, policy, once;
    uint16_t event_tag, tx_tag, submit_tag;
    uint32_t event_count, event_error, tx_count, error;
    uint64_t event_offset;
    uint8_t staged[8192], last_submit[24];
    size_t staged_used;
    unsigned tx_writes, submits, acks, upload_bytes;
    uint64_t outputs_generation;
    /* Rlerror for walks of outputs or upload/N: node-specific failures. */
    uint32_t object_error, upload_error;
    /* outputs: append one trailing byte; cap each Rread (0: no cap). */
    int object_trailing;
    uint32_t object_read_max;
};
static inline uint64_t peer_get(const uint8_t *p, size_t n)
{
    size_t i;
    uint64_t v = 0;
    for (i = 0; i < n; i++)
        v |= (uint64_t)p[i] << (8 * i);
    return v;
}
static inline void peer_put(uint8_t *p, uint64_t v, size_t n)
{
    size_t i;
    for (i = 0; i < n; i++)
        p[i] = (uint8_t)(v >> (8 * i));
}
static inline void peer_send(struct peer *p, unsigned type, uint16_t tag, const void *body,
                             size_t n)
{
    uint8_t b[16384];
    size_t at = 0;
    ssize_t r;
    assert(n + 7 <= sizeof(b));
    peer_put(b, n + 7, 4);
    b[4] = (uint8_t)type;
    peer_put(b + 5, tag, 2);
    if (n)
        memcpy(b + 7, body, n);
    while (at < n + 7) {
        r = send(p->fd, b + at, n + 7 - at, MSG_DONTWAIT | MSG_NOSIGNAL);
        assert(r > 0);
        at += (size_t)r;
    }
}
static inline void peer_error(struct peer *p, uint16_t tag, uint32_t error)
{
    uint8_t b[4];
    peer_put(b, error, 4);
    peer_send(p, 7, tag, b, 4);
}
static inline void peer_count(struct peer *p, uint16_t tag, uint32_t count)
{
    uint8_t b[4];
    peer_put(b, count, 4);
    peer_send(p, 119, tag, b, 4);
}
/* Append one event through the library encoder with the next sequence. */
static inline void peer_record(struct peer *p, struct sophia_sf_record *r)
{
    size_t n;
    r->header.epoch = PEER_EPOCH;
    r->header.submission = 0;
    r->header.sequence = ++p->sequence;
    assert(!sophia_sf_encode(p->journal + p->journal_used, sizeof(p->journal) - p->journal_used,
                             r, &n));
    p->journal_used += n;
}
static inline void peer_raw(struct peer *p, const void *bytes, size_t n)
{
    assert(n <= sizeof(p->journal) - p->journal_used);
    memcpy(p->journal + p->journal_used, bytes, n);
    p->journal_used += n;
}
static inline void peer_submitted(struct peer *p, uint64_t id, uint16_t kind)
{
    struct sophia_sf_record r = {0};
    r.header.kind = SOPHIA_SF_SUBMITTED;
    r.value.submitted.submission_id = id;
    r.value.submitted.candidate_kind = kind;
    peer_record(p, &r);
}
static inline void peer_app_event(struct peer *p)
{
    struct sophia_sf_record r;
    assert(!sophia_sf_decode(vector_AllocationResult, sizeof(vector_AllocationResult), &r));
    peer_record(p, &r);
}
static inline void peer_published(struct peer *p, uint16_t kind, uint64_t generation,
                                  uint64_t qid)
{
    struct sophia_sf_record r = {0};
    r.header.kind = SOPHIA_SF_OBJECT_PUBLISHED;
    r.value.object_published.object_kind = kind;
    r.value.object_published.generation = generation;
    r.value.object_published.qid = qid;
    peer_record(p, &r);
}
static inline void peer_resource_status(struct peer *p, const struct sophia_sf_resource_begin *b,
                                        uint16_t status)
{
    struct sophia_sf_record r = {0};
    struct sophia_sf_resource_status *v = &r.value.resource_status;
    r.header.kind = SOPHIA_SF_RESOURCE_STATUS;
    v->transaction = b->transaction;
    v->grant_connection_epoch = b->grant_connection_epoch;
    v->grant_content_epoch = b->grant_content_epoch;
    v->resource_id = b->resource_id;
    v->resource_generation = b->resource_generation;
    v->status = status;
    v->admitted_bytes = status == 2 ? b->total_bytes : 0;
    peer_record(p, &r);
}
static inline size_t peer_object(struct peer *p, uint8_t file, uint8_t *b, size_t capacity)
{
    struct sophia_sf_record r;
    size_t n;
    if (file == F_LIMITS) {
        assert(!sophia_sf_decode(vector_Limits, sizeof(vector_Limits), &r));
        r.value.limits.grant_connection_epoch = PEER_EPOCH;
    } else {
        assert(!sophia_sf_decode(vector_Outputs, sizeof(vector_Outputs), &r));
        r.value.outputs.grant_connection_epoch = PEER_EPOCH;
        r.value.outputs.facts_generation = p->outputs_generation;
    }
    r.header.epoch = PEER_EPOCH;
    assert(!sophia_sf_encode(b, capacity, &r, &n));
    return n;
}
static inline uint8_t peer_file(const struct peer *p, uint32_t fid)
{
    unsigned i;
    for (i = 0; i < p->fid_count; i++)
        if (p->fids[i] == fid)
            return p->files[i];
    return F_NONE;
}
static inline void peer_bind(struct peer *p, uint32_t fid, uint8_t file)
{
    assert(p->fid_count < 64);
    p->fids[p->fid_count] = fid;
    p->files[p->fid_count++] = file;
}
static inline void peer_unbind(struct peer *p, uint32_t fid)
{
    unsigned i;
    for (i = 0; i < p->fid_count; i++)
        if (p->fids[i] == fid) {
            p->fids[i] = p->fids[--p->fid_count];
            p->files[i] = p->files[p->fid_count];
            return;
        }
}
static inline void peer_qid(uint8_t *b, uint8_t file)
{
    memset(b, 0, 13);
    peer_put(b + 5, file == F_OUTPUTS ? OUTPUTS_QID : file, 8);
}
static inline void peer_answer_events(struct peer *p)
{
    uint8_t b[4 + 16384];
    size_t n;
    if (!p->event_held)
        return;
    if (p->event_error) {
        p->event_held = 0;
        peer_error(p, p->event_tag, p->event_error);
        return;
    }
    if (p->journal_used <= p->event_offset)
        return;
    n = p->journal_used - (size_t)p->event_offset;
    if (n > p->event_count)
        n = p->event_count;
    peer_put(b, n, 4);
    memcpy(b + 4, p->journal + p->event_offset, n);
    p->event_held = 0;
    peer_send(p, 117, p->event_tag, b, 4 + n);
}
static inline void peer_answer_submit(struct peer *p, uint32_t error)
{
    assert(p->submit_held);
    p->submit_held = 0;
    if (error)
        peer_error(p, p->submit_tag, error);
    else
        peer_count(p, p->submit_tag, 24);
}
static inline void peer_submit(struct peer *p, uint16_t tag, const uint8_t *b)
{
    uint16_t kind = (uint16_t)peer_get(p->staged + 6, 2);
    uint64_t id = peer_get(b + 8, 8);
    int policy = p->policy;
    assert(peer_get(b, 8) == PEER_EPOCH && peer_get(b + 16, 4) == p->staged_used);
    p->submits++;
    memcpy(p->last_submit, b, 24);
    if (kind == SOPHIA_SF_NEGOTIATE) {
        struct sophia_sf_record r = {0};
        peer_count(p, tag, 24);
        peer_submitted(p, id, kind);
        if (p->refuse_negotiation) {
            r.header.kind = SOPHIA_SF_REFUSED;
            r.value.refused.reason = 4;
            r.value.refused.denied_capabilities = 0x10;
        } else {
            r.header.kind = SOPHIA_SF_NEGOTIATED;
            r.value.negotiated.selected_revision = 6;
            r.value.negotiated.connection_epoch = PEER_EPOCH;
            r.value.negotiated.capabilities = 0x41;
            r.value.negotiated.limits_published = 1;
        }
        peer_record(p, &r);
        return;
    }
    if (p->once)
        p->policy = P_ACCEPT;
    p->once = 0;
    if (policy == P_ACCEPT || policy == P_SILENT)
        peer_count(p, tag, 24);
    else if (policy == P_ERROR)
        peer_error(p, tag, p->error);
    else {
        p->submit_held = 1;
        p->submit_tag = tag;
    }
    if (policy == P_ACCEPT || policy == P_CUSTODY)
        peer_submitted(p, id, kind);
}
static inline void peer_request(struct peer *p, const uint8_t *m, size_t size)
{
    static const char api[] = "sophia-shell-files version=1 role=bar epoch=17 fd_transfer=none\n";
    static const char *const names[] = {"",       "",       "api",  "events", "transaction",
                                        "submit", "ack",    "limits", "outputs"};
    uint8_t type = m[4], b[16384], file;
    uint16_t tag = (uint16_t)peer_get(m + 5, 2);
    const uint8_t *body = m + 7;
    uint64_t offset;
    uint32_t count;
    size_t n;
    unsigned i;
    (void)size;
    switch (type) {
    case 100:
        peer_put(b, peer_get(body, 4), 4);
        peer_put(b + 4, 8, 2);
        memcpy(b + 6, "9P2000.L", 8);
        peer_send(p, 101, tag, b, 14);
        return;
    case 104:
        peer_bind(p, (uint32_t)peer_get(body, 4), F_ROOT);
        peer_qid(b, F_ROOT);
        b[0] = 0x80;
        peer_send(p, 105, tag, b, 13);
        return;
    case 110:
        count = (uint32_t)peer_get(body + 8, 2);
        n = (size_t)peer_get(body + 10, 2);
        file = F_NONE;
        if (count == 2 && n == 6 && !memcmp(body + 12, "upload", 6))
            file = F_UPLOAD;
        for (i = 2; count == 1 && i < sizeof(names) / sizeof(names[0]); i++)
            if (strlen(names[i]) == n && !memcmp(body + 12, names[i], n))
                file = (uint8_t)i;
        assert(file != F_NONE);
        if (file == F_OUTPUTS && p->object_error) {
            peer_error(p, tag, p->object_error);
            return;
        }
        if (file == F_UPLOAD && p->upload_error) {
            peer_error(p, tag, p->upload_error);
            return;
        }
        peer_bind(p, (uint32_t)peer_get(body + 4, 4), file);
        peer_put(b, count, 2);
        for (i = 0; i < count; i++)
            peer_qid(b + 2 + 13 * i, file);
        peer_send(p, 111, tag, b, 2 + 13 * count);
        return;
    case 12:
        peer_qid(b, peer_file(p, (uint32_t)peer_get(body, 4)));
        peer_put(b + 13, 0, 4);
        peer_send(p, 13, tag, b, 17);
        return;
    case 120:
        if (peer_file(p, (uint32_t)peer_get(body, 4)) == F_TX)
            p->staged_used = 0;
        peer_unbind(p, (uint32_t)peer_get(body, 4));
        peer_send(p, 121, tag, NULL, 0);
        return;
    case 116:
        file = peer_file(p, (uint32_t)peer_get(body, 4));
        offset = peer_get(body + 4, 8);
        count = (uint32_t)peer_get(body + 12, 4);
        if (file == F_EVENTS) {
            assert(!p->event_held && offset <= p->journal_used);
            p->event_held = 1;
            p->event_tag = tag;
            p->event_offset = offset;
            p->event_count = count;
            peer_answer_events(p);
            return;
        }
        if (file == F_API) {
            n = sizeof(api) - 1;
            memcpy(b + 4, api, n);
        } else {
            n = peer_object(p, file, b + 4, sizeof(b) - 5);
            if (file == F_OUTPUTS && p->object_trailing)
                b[4 + n++] = 0xee;
        }
        n = offset >= n ? 0 : n - (size_t)offset;
        if (n > count)
            n = count;
        if (file != F_API && p->object_read_max && n > p->object_read_max)
            n = p->object_read_max;
        memmove(b + 4, b + 4 + offset, n);
        peer_put(b, n, 4);
        peer_send(p, 117, tag, b, 4 + n);
        return;
    case 118:
        file = peer_file(p, (uint32_t)peer_get(body, 4));
        offset = peer_get(body + 4, 8);
        count = (uint32_t)peer_get(body + 12, 4);
        if (file == F_TX) {
            assert(offset == p->staged_used && count <= sizeof(p->staged) - p->staged_used);
            memcpy(p->staged + p->staged_used, body + 16, count);
            p->staged_used += count;
            p->tx_writes++;
            if (p->hold_tx) {
                p->tx_held = 1;
                p->tx_tag = tag;
                p->tx_count = count;
            } else
                peer_count(p, tag, count);
        } else if (file == F_UPLOAD) {
            p->upload_bytes += count;
            peer_count(p, tag, count);
        } else if (file == F_SUBMIT) {
            assert(count == 24);
            peer_submit(p, tag, body + 16);
        } else {
            assert(file == F_ACK && count == 16);
            p->acks++;
            p->acked = peer_get(body + 24, 8);
            peer_count(p, tag, 16);
        }
        return;
    default:
        fprintf(stderr, "peer: unexpected request type %u\n", type);
        abort();
    }
}
/* Read and answer every complete request now available, then any held read. */
static inline void peer_pump(struct peer *p)
{
    ssize_t r;
    size_t n;
    for (;;) {
        r = recv(p->fd, p->in + p->in_used, sizeof(p->in) - p->in_used, MSG_DONTWAIT);
        if (r <= 0)
            break;
        p->in_used += (size_t)r;
    }
    while (p->in_used >= 4 && (n = (size_t)peer_get(p->in, 4)) <= p->in_used) {
        assert(n >= 7);
        peer_request(p, p->in, n);
        p->in_used -= n;
        memmove(p->in, p->in + n, p->in_used);
    }
    peer_answer_events(p);
}
static inline void peer_close(struct peer *p)
{
    close(p->fd);
    p->fd = -1;
}

/* A session and its peer over one socket pair, driven by explicit passes. */
struct rig {
    struct sophia_ss s;
    struct peer p;
    int fd[2];
    void *storage;
    uint64_t now;
};
static const struct sophia_ss_config rig_config = {{5, 6, 0}, SOPHIA_SF_BAR, 4096, 4, 8192,
                                                   NULL, 0};
static inline void rig_step(struct rig *r)
{
    int status = sophia_ss_dispatch(&r->s, POLLIN, 65536, r->now);
    (void)status;
    if (r->p.fd >= 0)
        peer_pump(&r->p);
}
static inline void rig_run(struct rig *r, unsigned passes)
{
    unsigned i;
    for (i = 0; i < passes; i++)
        rig_step(r);
}
/* Open without driving: the peer answers only once passes run. */
static inline void rig_start(struct rig *r, const struct sophia_ss_config *config)
{
    size_t bytes = sophia_ss_storage_bytes(config->msize, config->queue_bytes);
    memset(r, 0, sizeof(*r));
    assert(bytes && !socketpair(AF_UNIX, SOCK_STREAM, 0, r->fd));
    r->storage = malloc(bytes);
    assert(r->storage);
    r->p.fd = r->fd[1];
    r->now = 1000;
    assert(!sophia_ss_open_fd(&r->s, r->fd[0], config, r->storage, bytes));
    assert(sophia_ss_state(&r->s) == SOPHIA_SS_NEGOTIATING);
}
static inline void rig_settle(struct rig *r)
{
    unsigned i;
    for (i = 0; i < 64 && sophia_ss_state(&r->s) == SOPHIA_SS_NEGOTIATING; i++)
        rig_step(r);
}
static inline void rig_ready(struct rig *r, const struct sophia_ss_config *config)
{
    rig_start(r, config);
    rig_settle(r);
    if (sophia_ss_state(&r->s) != SOPHIA_SS_READY)
        fprintf(stderr, "rig: state=%d remote=%u boot=%u\n", sophia_ss_state(&r->s),
                r->s.files.remote_error, r->s.files.bootstrap);
    assert(sophia_ss_state(&r->s) == SOPHIA_SS_READY && sophia_ss_epoch(&r->s) == PEER_EPOCH);
    /* Negotiation settled: its Submitted and Negotiated were consumed. */
    rig_run(r, 4);
}
static inline void rig_close(struct rig *r)
{
    close(r->fd[0]);
    if (r->p.fd >= 0)
        close(r->p.fd);
    free(r->storage);
}
/* A valid content record for this epoch; header fields are the session's. */
static inline struct sophia_sf_record rig_request(uint64_t id)
{
    struct sophia_sf_record r;
    assert(!sophia_sf_decode(vector_AllocationRequest, sizeof(vector_AllocationRequest), &r));
    r.value.allocation_request.allocation_request_id = id;
    return r;
}
static inline enum sophia_ss_outcome rig_outcome(const struct rig *r, uint64_t ticket)
{
    enum sophia_ss_outcome o;
    assert(!sophia_ss_outcome(&r->s, ticket, &o, NULL));
    return o;
}
/* Drive until ticket leaves IN_FLIGHT/ADMITTED_LOCAL or passes run out. */
static inline enum sophia_ss_outcome rig_until(struct rig *r, uint64_t ticket, unsigned passes)
{
    unsigned i;
    for (i = 0; i < passes; i++) {
        enum sophia_ss_outcome o = rig_outcome(r, ticket);
        if (o != SOPHIA_SS_IN_FLIGHT && o != SOPHIA_SS_ADMITTED_LOCAL)
            return o;
        rig_step(r);
    }
    return rig_outcome(r, ticket);
}
/* The submission id the peer saw in the latest Tsubmit. */
static inline uint64_t rig_last_id(const struct rig *r)
{
    return peer_get(r->p.last_submit + 8, 8);
}

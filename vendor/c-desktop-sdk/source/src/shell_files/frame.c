#include "../sophia_shell_frames.h"
#include "internal.h"

static int class_of(uint16_t kind)
{
    if (kind >= SOPHIA_SFT_OPEN && kind <= SOPHIA_SFT_COMMIT)
        return SOPHIA_SFT_REQUEST;
    if (kind >= SOPHIA_SFT_OPENED && kind <= SOPHIA_SFT_OUTCOME)
        return SOPHIA_SFT_EVENT;
    return -1;
}
static int key_check(const struct sophia_sft_key *v)
{
    return v->connection_epoch && v->content_epoch && v->output_id && v->output_generation &&
           v->allocation_id && v->allocation_generation && v->facts_generation && v->stream_id;
}
static void key_put(uint8_t *b, const struct sophia_sft_key *v)
{
    sf_put(b, v->connection_epoch, 8);
    sf_put(b + 8, v->content_epoch, 8);
    sf_put(b + 16, v->output_id, 8);
    sf_put(b + 24, v->output_generation, 8);
    sf_put(b + 32, v->allocation_id, 8);
    sf_put(b + 40, v->allocation_generation, 8);
    sf_put(b + 48, v->facts_generation, 8);
    sf_put(b + 56, v->stream_id, 8);
}
static void key_take(const uint8_t *b, struct sophia_sft_key *v)
{
    v->connection_epoch = sf_get(b, 8);
    v->content_epoch = sf_get(b + 8, 8);
    v->output_id = sf_get(b + 16, 8);
    v->output_generation = sf_get(b + 24, 8);
    v->allocation_id = sf_get(b + 32, 8);
    v->allocation_generation = sf_get(b + 40, 8);
    v->facts_generation = sf_get(b + 48, 8);
    v->stream_id = sf_get(b + 56, 8);
}
static size_t scene_size(const struct sophia_sf_candidate *v)
{
    return 72u + 64u * v->surface_count + 32u * v->placement_count + 48u * v->target_count;
}
static int commit_check(const struct sophia_sft_commit *v)
{
    const struct sophia_sf_candidate *c = &v->candidate;
    const struct sophia_or_offer *o = &v->image;
    uint8_t offer[SOPHIA_OR_OFFER_BYTES];
    size_t i;
    int found = 0;
    if (v->has_image > 1 || v->has_retire > 1 || c->surface_count != 1 || sf_validate_candidate(c))
        return 0;
    if (v->has_image) {
        if (sophia_or_offer_encode(offer, o) != SOPHIA_IC_OK || o->slot >= 2 ||
            o->identity.connection_epoch != c->grant_connection_epoch ||
            o->identity.content_epoch != c->grant_content_epoch ||
            o->allocation_id != c->surfaces[0].allocation_id ||
            o->allocation_generation != c->surfaces[0].allocation_generation ||
            o->output_id != c->output_id || o->output_generation != c->output_generation ||
            o->topology_generation != c->facts_generation)
            return 0;
        for (i = 0; i < c->placement_count; i++)
            found |= c->placements[i].resource_id == o->resource_id &&
                     c->placements[i].resource_generation == o->resource_generation;
        if (!found)
            return 0;
    }
    if (v->has_retire) {
        if (!v->retire.transaction || v->retire.transaction == c->transaction ||
            !v->retire.resource_id || !v->retire.resource_generation)
            return 0;
        for (i = 0; i < c->placement_count; i++)
            if (c->placements[i].resource_id == v->retire.resource_id &&
                c->placements[i].resource_generation == v->retire.resource_generation)
                return 0;
    }
    return 1;
}
size_t sophia_sft_record_bytes(const struct sophia_sft_record *r, uint16_t revision)
{
    size_t size;
    uint64_t epoch;
    int class;
    if (!r || revision != SOPHIA_SFT_REVISION || !r->header.epoch)
        return 0;
    class = class_of(r->header.kind);
    if (class < 0 || (class == SOPHIA_SFT_REQUEST ? (!r->header.submission || r->header.sequence)
                                                  : (r->header.submission || !r->header.sequence)))
        return 0;
    switch (r->header.kind) {
    case SOPHIA_SFT_OPEN:
    case SOPHIA_SFT_CLOSE:
    case SOPHIA_SFT_OPENED:
    case SOPHIA_SFT_CLOSED: {
        const struct sophia_sft_stream *v = &r->value.stream;
        if (!v->transaction || !key_check(&v->key))
            return 0;
        if (r->header.kind == SOPHIA_SFT_OPENED && (!v->ttl_ms || v->ttl_ms > 250))
            return 0;
        if (r->header.kind == SOPHIA_SFT_CLOSED && (!v->reason || v->reason > 12))
            return 0;
        epoch = v->key.connection_epoch;
        size = class == SOPHIA_SFT_REQUEST ? 104 : 112;
        break;
    }
    case SOPHIA_SFT_REFUSED: {
        const struct sophia_sft_refused *v = &r->value.refused;
        if (!v->transaction || !v->connection_epoch || !v->content_epoch || !v->output_id ||
            !v->output_generation || !v->stream_id || !v->reason || v->reason > 12 ||
            class_of(v->request_kind) != SOPHIA_SFT_REQUEST ||
            ((v->candidate_generation != 0) != (v->request_kind == SOPHIA_SFT_COMMIT)))
            return 0;
        epoch = v->connection_epoch;
        size = 96;
        break;
    }
    case SOPHIA_SFT_COMMIT: {
        const struct sophia_sft_commit *v = &r->value.commit;
        if (!commit_check(v))
            return 0;
        epoch = v->candidate.grant_connection_epoch;
        size = 48 + scene_size(&v->candidate) + 152u * v->has_image + 24u * v->has_retire;
        break;
    }
    case SOPHIA_SFT_OUTCOME: {
        const struct sophia_sft_outcome *v = &r->value.outcome;
        if (sf_validate_candidate_outcome(&v->candidate) || !v->stream_id || !v->allocation_id ||
            !v->allocation_generation || !v->facts_generation ||
            (v->candidate.kind == 1 && v->credit_expires_msec))
            return 0;
        epoch = v->candidate.grant_connection_epoch;
        size = 148;
        break;
    }
    default:
        return 0;
    }
    return epoch == r->header.epoch && size <= SOPHIA_SFT_MAX_BYTES ? size : 0;
}

static void commit_put(uint8_t *b, const struct sophia_sft_commit *v)
{
    const struct sophia_sf_candidate *c = &v->candidate;
    uint8_t header[80];
    size_t i, at = 88;
    sf_put_candidate(header, c);
    memcpy(b, header, 8);
    sf_put(b + 8, scene_size(c), 4);
    sf_put(b + 12, v->has_image | (v->has_retire << 1), 2);
    sf_put(b + 14, 0, 2);
    memcpy(b + 16, header + 8, 72);
    for (i = 0; i < c->surface_count; i++, at += 64)
        sf_put_content_surface(b + at, &c->surfaces[i]);
    for (i = 0; i < c->placement_count; i++, at += 32)
        sf_put_content_placement(b + at, &c->placements[i]);
    for (i = 0; i < c->target_count; i++, at += 48)
        sf_put_content_target(b + at, &c->targets[i]);
    if (v->has_image) {
        (void)sophia_or_offer_encode(b + at, &v->image);
        at += 152;
    }
    if (v->has_retire) {
        sf_put(b + at, v->retire.transaction, 8);
        sf_put(b + at + 8, v->retire.resource_id, 8);
        sf_put(b + at + 16, v->retire.resource_generation, 8);
    }
}
int sophia_sft_encode(void *dst, size_t capacity, uint16_t revision,
                      const struct sophia_sft_record *r, size_t *written)
{
    uint8_t *b = dst;
    size_t size;
    if (!dst || !r || !written)
        return -4;
    size = sophia_sft_record_bytes(r, revision);
    if (!size)
        return -1;
    if (size > capacity)
        return -4;
    sf_put(b, size, 4);
    sf_put(b + 4, 1, 2);
    sf_put(b + 6, r->header.kind, 2);
    sf_put(b + 8, r->header.epoch, 8);
    sf_put(b + 16, r->header.submission, 8);
    sf_put(b + 24, r->header.sequence, 8);
    b += 32;
    switch (r->header.kind) {
    case SOPHIA_SFT_OPEN:
    case SOPHIA_SFT_CLOSE:
    case SOPHIA_SFT_OPENED:
    case SOPHIA_SFT_CLOSED: {
        const struct sophia_sft_stream *v = &r->value.stream;
        sf_put(b, v->transaction, 8);
        key_put(b + 8, &v->key);
        if (r->header.kind == SOPHIA_SFT_OPENED) {
            sf_put(b + 72, 1, 4);
            sf_put(b + 76, v->ttl_ms, 4);
        }
        if (r->header.kind == SOPHIA_SFT_CLOSED) {
            sf_put(b + 72, v->reason, 2);
            memset(b + 74, 0, 6);
        }
        break;
    }
    case SOPHIA_SFT_REFUSED: {
        const struct sophia_sft_refused *v = &r->value.refused;
        sf_put(b, v->transaction, 8);
        sf_put(b + 8, v->connection_epoch, 8);
        sf_put(b + 16, v->content_epoch, 8);
        sf_put(b + 24, v->output_id, 8);
        sf_put(b + 32, v->output_generation, 8);
        sf_put(b + 40, v->stream_id, 8);
        sf_put(b + 48, v->candidate_generation, 8);
        sf_put(b + 56, v->request_kind, 2);
        sf_put(b + 58, v->reason, 2);
        memset(b + 60, 0, 4);
        break;
    }
    case SOPHIA_SFT_COMMIT:
        commit_put(b, &r->value.commit);
        break;
    case SOPHIA_SFT_OUTCOME: {
        const struct sophia_sft_outcome *v = &r->value.outcome;
        sf_put_candidate_outcome(b, &v->candidate);
        sf_put(b + 76, v->stream_id, 8);
        sf_put(b + 84, v->allocation_id, 8);
        sf_put(b + 92, v->allocation_generation, 8);
        sf_put(b + 100, v->facts_generation, 8);
        sf_put(b + 108, v->credit_expires_msec, 8);
        break;
    }
    }
    *written = size;
    return 0;
}

static int commit_take(const uint8_t *b, size_t size, struct sophia_sft_commit *v)
{
    uint8_t header[80];
    uint64_t scene;
    uint16_t flags;
    size_t at = 88, i;
    struct sophia_sf_candidate *c = &v->candidate;
    if (size < 152)
        return -1;
    scene = sf_get(b + 8, 4);
    flags = (uint16_t)sf_get(b + 12, 2);
    if ((flags & ~3u) || !sf_zero(b + 14, 2) || scene < 136 || scene > SOPHIA_SFT_MAX_BYTES ||
        size != 16 + scene + ((flags & 1) ? 152u : 0u) + ((flags & 2) ? 24u : 0u))
        return -1;
    memcpy(header, b, 8);
    memcpy(header + 8, b + 16, 72);
    if (sf_take_candidate(header, c) || c->surface_count != 1 || scene != scene_size(c))
        return -1;
    for (i = 0; i < c->surface_count; i++, at += 64)
        if (sf_take_content_surface(b + at, &c->surfaces[i]))
            return -1;
    for (i = 0; i < c->placement_count; i++, at += 32)
        if (sf_take_content_placement(b + at, &c->placements[i]))
            return -1;
    for (i = 0; i < c->target_count; i++, at += 48)
        if (sf_take_content_target(b + at, &c->targets[i]))
            return -1;
    v->has_image = (uint8_t)(flags & 1);
    v->has_retire = (uint8_t)((flags >> 1) & 1);
    if (v->has_image) {
        struct sophia_or_registration registration;
        int kind;
        if (sophia_or_control_decode(b + at, 152, &kind, &registration, &v->image) !=
                SOPHIA_IC_OK ||
            kind != 2)
            return -1;
        at += 152;
    }
    if (v->has_retire) {
        v->retire.transaction = sf_get(b + at, 8);
        v->retire.resource_id = sf_get(b + at + 8, 8);
        v->retire.resource_generation = sf_get(b + at + 16, 8);
    }
    return 0;
}
int sophia_sft_decode(const void *src, size_t size, enum sophia_sft_class expected,
                      uint16_t revision, size_t max_bytes, struct sophia_sft_record *out)
{
    const uint8_t *b = src;
    struct sophia_sft_record r;
    size_t body;
    if (!src || !out)
        return -4;
    if (revision != SOPHIA_SFT_REVISION || size < 32 || size > SOPHIA_SFT_MAX_BYTES ||
        size > max_bytes || sf_get(b, 4) != size || sf_get(b + 4, 2) != 1)
        return -1;
    memset(&r, 0, sizeof r);
    r.header.kind = (uint16_t)sf_get(b + 6, 2);
    r.header.epoch = sf_get(b + 8, 8);
    r.header.submission = sf_get(b + 16, 8);
    r.header.sequence = sf_get(b + 24, 8);
    if (class_of(r.header.kind) < 0 || class_of(r.header.kind) != (int)expected)
        return -1;
    b += 32;
    body = size - 32;
    switch (r.header.kind) {
    case SOPHIA_SFT_OPEN:
    case SOPHIA_SFT_CLOSE:
    case SOPHIA_SFT_OPENED:
    case SOPHIA_SFT_CLOSED: {
        struct sophia_sft_stream *v = &r.value.stream;
        if (body != (expected == SOPHIA_SFT_REQUEST ? 72u : 80u))
            return -1;
        v->transaction = sf_get(b, 8);
        key_take(b + 8, &v->key);
        if (r.header.kind == SOPHIA_SFT_OPENED) {
            if (sf_get(b + 72, 4) != 1)
                return -1;
            v->ttl_ms = (uint32_t)sf_get(b + 76, 4);
        }
        if (r.header.kind == SOPHIA_SFT_CLOSED) {
            if (!sf_zero(b + 74, 6))
                return -1;
            v->reason = (uint16_t)sf_get(b + 72, 2);
        }
        break;
    }
    case SOPHIA_SFT_REFUSED: {
        struct sophia_sft_refused *v = &r.value.refused;
        if (body != 64 || !sf_zero(b + 60, 4))
            return -1;
        v->transaction = sf_get(b, 8);
        v->connection_epoch = sf_get(b + 8, 8);
        v->content_epoch = sf_get(b + 16, 8);
        v->output_id = sf_get(b + 24, 8);
        v->output_generation = sf_get(b + 32, 8);
        v->stream_id = sf_get(b + 40, 8);
        v->candidate_generation = sf_get(b + 48, 8);
        v->request_kind = (uint16_t)sf_get(b + 56, 2);
        v->reason = (uint16_t)sf_get(b + 58, 2);
        break;
    }
    case SOPHIA_SFT_COMMIT:
        if (commit_take(b, body, &r.value.commit))
            return -1;
        break;
    case SOPHIA_SFT_OUTCOME: {
        struct sophia_sft_outcome *v = &r.value.outcome;
        if (body != 116 || sf_take_candidate_outcome(b, &v->candidate))
            return -1;
        v->stream_id = sf_get(b + 76, 8);
        v->allocation_id = sf_get(b + 84, 8);
        v->allocation_generation = sf_get(b + 92, 8);
        v->facts_generation = sf_get(b + 100, 8);
        v->credit_expires_msec = sf_get(b + 108, 8);
        break;
    }
    default:
        return -1;
    }
    if (sophia_sft_record_bytes(&r, revision) != size)
        return -1;
    *out = r;
    return 0;
}

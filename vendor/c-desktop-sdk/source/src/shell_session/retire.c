#include "../sophia_shell_retire.h"
#include <string.h>

enum { FREE, NAMED, RETIRING };

int sophia_sr_init(struct sophia_sr *s, uint64_t connection, uint64_t content,
    struct sophia_sr_entry *entries, size_t capacity)
{
    if (!s || !connection || !content || !entries || !capacity ||
        capacity > SIZE_MAX / sizeof(*entries))
        return SOPHIA_SR_ARGUMENT;
    s->connection_epoch = connection;
    s->content_epoch = content;
    s->entries = entries;
    s->capacity = capacity;
    memset(entries, 0, capacity * sizeof(*entries));
    return SOPHIA_SR_OK;
}

int sophia_sr_name(struct sophia_sr *s, uint64_t output, uint64_t output_generation,
    uint64_t generation, const struct sophia_sf_candidate_retire *retire)
{
    size_t i, free_slot;
    if (!s || !retire || !output || !output_generation || !generation ||
        !retire->transaction || !retire->resource_id || !retire->resource_generation)
        return SOPHIA_SR_ARGUMENT;
    free_slot = s->capacity;
    for (i = 0; i < s->capacity; ++i) {
        const struct sophia_sr_entry *e = &s->entries[i];
        if (e->state == FREE) {
            if (free_slot == s->capacity)
                free_slot = i;
        } else if ((e->state == NAMED && e->candidate_generation == generation) ||
            e->retire.transaction == retire->transaction ||
            (e->retire.resource_id == retire->resource_id &&
             e->retire.resource_generation == retire->resource_generation)) {
            return SOPHIA_SR_DUPLICATE;
        }
    }
    if (free_slot == s->capacity)
        return SOPHIA_SR_CAPACITY;
    s->entries[free_slot] = (struct sophia_sr_entry){output, output_generation, generation,
        *retire, NAMED};
    return SOPHIA_SR_OK;
}

int sophia_sr_holds(const struct sophia_sr *s, uint64_t id, uint64_t generation)
{
    size_t i;
    if (!s)
        return 0;
    for (i = 0; i < s->capacity; ++i) {
        const struct sophia_sr_entry *e = &s->entries[i];
        if (e->state != FREE && e->retire.resource_id == id &&
            e->retire.resource_generation == generation)
            return 1;
    }
    return 0;
}

int sophia_sr_observe(struct sophia_sr *s, const struct sophia_sf_record *r,
    struct sophia_sr_progress *out)
{
    size_t i;
    if (!s || !r || !out)
        return SOPHIA_SR_ARGUMENT;
    *out = (struct sophia_sr_progress){SOPHIA_SR_UNRELATED, 0, 0};
    if (r->header.kind == SOPHIA_SF_CANDIDATE_OUTCOME) {
        const struct sophia_sf_candidate_outcome *v = &r->value.candidate_outcome;
        if (v->grant_connection_epoch != s->connection_epoch || v->grant_content_epoch != s->content_epoch)
            return SOPHIA_SR_WRONG_GRANT;
        for (i = 0; i < s->capacity; ++i) {
            struct sophia_sr_entry *e = &s->entries[i];
            if (e->state != NAMED || e->candidate_generation != v->candidate_generation)
                continue;
            if (e->output_id != v->output_id || e->output_generation != v->output_generation)
                return SOPHIA_SR_MISATTRIBUTED;
            if (v->kind == 1)
                return SOPHIA_SR_OK;
            *out = (struct sophia_sr_progress){v->kind == 2 ? SOPHIA_SR_RETIRING : SOPHIA_SR_KEPT,
                e->retire.resource_id, e->retire.resource_generation};
            e->state = v->kind == 2 ? RETIRING : FREE;
            return SOPHIA_SR_OK;
        }
    } else if (r->header.kind == SOPHIA_SF_RESOURCE_RELEASED) {
        const struct sophia_sf_resource_released *v = &r->value.resource_released;
        if (v->grant_connection_epoch != s->connection_epoch || v->grant_content_epoch != s->content_epoch)
            return SOPHIA_SR_WRONG_GRANT;
        for (i = 0; i < s->capacity; ++i) {
            struct sophia_sr_entry *e = &s->entries[i];
            int resource = e->retire.resource_id == v->resource_id &&
                e->retire.resource_generation == v->resource_generation;
            int transaction = e->retire.transaction == v->transaction;
            if (e->state == FREE || (!resource && !transaction))
                continue;
            if (e->state != RETIRING || !resource || !transaction)
                return SOPHIA_SR_MISATTRIBUTED;
            *out = (struct sophia_sr_progress){SOPHIA_SR_RELEASED, v->resource_id, v->resource_generation};
            e->state = FREE;
            return SOPHIA_SR_OK;
        }
    }
    return SOPHIA_SR_OK;
}

void sophia_sr_revoke(struct sophia_sr *s)
{
    if (s)
        memset(s->entries, 0, s->capacity * sizeof(*s->entries));
}

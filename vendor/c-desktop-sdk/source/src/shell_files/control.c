/* KDL field transcription; cross-field validation is in validation.c. */
#include "internal.h"
int sf_check_negotiate(const struct sophia_sf_negotiate *v)
{
    (void)v;
    return 0;
}
void sf_put_negotiate(uint8_t *b, const struct sophia_sf_negotiate *v)
{
    memset(b, 0, 16);
    sf_put(b + 0, (uint64_t)v->minimum_revision, 2);
    sf_put(b + 2, (uint64_t)v->maximum_revision, 2);
    sf_put(b + 8, (uint64_t)v->required_capabilities, 8);
}
int sf_take_negotiate(const uint8_t *b, struct sophia_sf_negotiate *v)
{
    v->minimum_revision = (uint16_t)sf_get(b + 0, 2);
    v->maximum_revision = (uint16_t)sf_get(b + 2, 2);
    if (!sf_zero(b + 4, 4))
        return -1;
    v->required_capabilities = (uint64_t)sf_get(b + 8, 8);
    return sf_check_negotiate(v);
}
int sf_check_negotiated(const struct sophia_sf_negotiated *v)
{
    if (!v->connection_epoch || v->limits_published > 1 ||
        !v->max_descriptors || v->max_descriptors > 16 ||
        !v->max_label_bytes || v->max_label_bytes > 128 ||
        !v->max_pending_activations || v->max_pending_activations > 16)
        return -1;
    return 0;
}
void sf_put_negotiated(uint8_t *b, const struct sophia_sf_negotiated *v)
{
    memset(b, 0, 32);
    sf_put(b + 0, (uint64_t)v->selected_revision, 2);
    sf_put(b + 4, (uint64_t)v->connection_epoch, 8);
    sf_put(b + 12, (uint64_t)v->capabilities, 8);
    sf_put(b + 20, (uint64_t)v->max_descriptors, 2);
    sf_put(b + 22, (uint64_t)v->max_label_bytes, 2);
    sf_put(b + 24, (uint64_t)v->max_pending_activations, 2);
    sf_put(b + 26, (uint64_t)v->limits_published, 2);
}
int sf_take_negotiated(const uint8_t *b, struct sophia_sf_negotiated *v)
{
    v->selected_revision = (uint16_t)sf_get(b + 0, 2);
    if (!sf_zero(b + 2, 2))
        return -1;
    v->connection_epoch = (uint64_t)sf_get(b + 4, 8);
    v->capabilities = (uint64_t)sf_get(b + 12, 8);
    v->max_descriptors = (uint16_t)sf_get(b + 20, 2);
    v->max_label_bytes = (uint16_t)sf_get(b + 22, 2);
    v->max_pending_activations = (uint16_t)sf_get(b + 24, 2);
    v->limits_published = (uint16_t)sf_get(b + 26, 2);
    if (!sf_zero(b + 28, 4))
        return -1;
    return sf_check_negotiated(v);
}
int sf_check_refused(const struct sophia_sf_refused *v)
{
    if (v->reason < 1 || v->reason > 4)
        return -1;
    return 0;
}
void sf_put_refused(uint8_t *b, const struct sophia_sf_refused *v)
{
    memset(b, 0, 16);
    sf_put(b + 0, (uint64_t)v->reason, 2);
    sf_put(b + 8, (uint64_t)v->denied_capabilities, 8);
}
int sf_take_refused(const uint8_t *b, struct sophia_sf_refused *v)
{
    v->reason = (uint16_t)sf_get(b + 0, 2);
    if (!sf_zero(b + 2, 2))
        return -1;
    if (!sf_zero(b + 4, 4))
        return -1;
    v->denied_capabilities = (uint64_t)sf_get(b + 8, 8);
    return sf_check_refused(v);
}
int sf_check_submitted(const struct sophia_sf_submitted *v)
{
    if (!v->submission_id || v->candidate_kind < 256 || v->candidate_kind > 272)
        return -1;
    return 0;
}
void sf_put_submitted(uint8_t *b, const struct sophia_sf_submitted *v)
{
    memset(b, 0, 16);
    sf_put(b + 0, (uint64_t)v->submission_id, 8);
    sf_put(b + 8, (uint64_t)v->candidate_kind, 2);
}
int sf_take_submitted(const uint8_t *b, struct sophia_sf_submitted *v)
{
    v->submission_id = (uint64_t)sf_get(b + 0, 8);
    v->candidate_kind = (uint16_t)sf_get(b + 8, 2);
    if (!sf_zero(b + 10, 6))
        return -1;
    return sf_check_submitted(v);
}
int sf_check_object_published(const struct sophia_sf_object_published *v)
{
    if (v->object_kind < 1 || v->object_kind > 4 || !v->qid)
        return -1;
    return 0;
}
void sf_put_object_published(uint8_t *b, const struct sophia_sf_object_published *v)
{
    memset(b, 0, 24);
    sf_put(b + 0, (uint64_t)v->object_kind, 2);
    sf_put(b + 8, (uint64_t)v->generation, 8);
    sf_put(b + 16, (uint64_t)v->qid, 8);
}
int sf_take_object_published(const uint8_t *b, struct sophia_sf_object_published *v)
{
    v->object_kind = (uint16_t)sf_get(b + 0, 2);
    if (!sf_zero(b + 2, 6))
        return -1;
    v->generation = (uint64_t)sf_get(b + 8, 8);
    v->qid = (uint64_t)sf_get(b + 16, 8);
    return sf_check_object_published(v);
}

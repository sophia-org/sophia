#include "roles_internal.h"

/* Explicit host member -> KDL offset mapping; no struct is a wire image. */
#define BINDING(X)                                                                                 \
    X(transaction, 0)                                                                              \
    X(grant_connection_epoch, 8)                                                                   \
    X(grant_content_epoch, 16) X(opening, 24) X(output_id, 32) X(output_generation, 40)            \
        X(allocation_id, 48) X(allocation_generation, 56) X(catalog_generation, 64)                \
            X(candidate_generation, 72) X(presentation_epoch, 80) X(interaction_generation, 88)    \
                X(state_revision, 96) X(focus_lease, 104)
static void put_binding(uint8_t *b, const struct sophia_sf_native_binding *v)
{
#define PUT(name, at) sf_put(b + at, v->name, 8);
    BINDING(PUT)
#undef PUT
}
static void take_binding(const uint8_t *b, struct sophia_sf_native_binding *v)
{
#define TAKE(name, at) v->name = sf_get(b + at, 8);
    BINDING(TAKE)
#undef TAKE
}
#undef BINDING
#define OPENING(X)                                                                                 \
    X(transaction, 0)                                                                              \
    X(grant_connection_epoch, 8)                                                                   \
    X(grant_content_epoch, 16) X(opening, 24) X(output_id, 32) X(output_generation, 40)            \
        X(catalog_generation, 48) X(state_revision, 56)
static void put_opening(uint8_t *b, const struct sophia_sf_native_opening *v)
{
#define PUT(name, at) sf_put(b + at, v->name, 8);
    OPENING(PUT)
#undef PUT
}
static void take_opening(const uint8_t *b, struct sophia_sf_native_opening *v)
{
#define TAKE(name, at) v->name = sf_get(b + at, 8);
    OPENING(TAKE)
#undef TAKE
}
#undef OPENING
static void put_activation(uint8_t *b, const struct sophia_sf_native_activate *v)
{
    put_binding(b, &v->binding);
    sf_put(b + 112, v->event_id, 8);
    sf_put(b + 120, v->state_revision, 8);
    sf_put(b + 128, v->cause, 2);
    sf_put(b + 130, v->slot, 2);
}
static void take_activation(const uint8_t *b, struct sophia_sf_native_activate *v)
{
    take_binding(b, &v->binding);
    v->event_id = sf_get(b + 112, 8);
    v->state_revision = sf_get(b + 120, 8);
    v->cause = (uint16_t)sf_get(b + 128, 2);
    v->slot = (uint16_t)sf_get(b + 130, 2);
}
#define ALLOCATION(X)                                                                              \
    X(transaction, 0, 8)                                                                           \
    X(grant_connection_epoch, 8, 8)                                                                \
    X(grant_content_epoch, 16, 8) X(opening, 24, 8) X(output_id, 32, 8)                            \
        X(output_generation, 40, 8) X(request_id, 48, 8) X(prior_id, 56, 8)                        \
            X(prior_generation, 64, 8) X(operation, 72, 2) X(edge, 74, 2) X(desired_width, 76, 4)  \
                X(desired_height, 80, 4)
static void put_allocation(uint8_t *b, const struct sophia_sf_native_allocation_request *v)
{
#define PUT(name, at, n) sf_put(b + at, v->name, n);
    ALLOCATION(PUT)
#undef PUT
    sf_put(b + 84, (uint16_t)v->margin_top, 2);
    sf_put(b + 86, (uint16_t)v->margin_right, 2);
    sf_put(b + 88, (uint16_t)v->margin_bottom, 2);
    sf_put(b + 90, (uint16_t)v->margin_left, 2);
}
static void take_allocation(const uint8_t *b, struct sophia_sf_native_allocation_request *v)
{
#define TAKE(name, at, n) v->name = sf_get(b + at, n);
    ALLOCATION(TAKE)
#undef TAKE
    v->margin_top = (int16_t)sf_signed(b + 84, 2);
    v->margin_right = (int16_t)sf_signed(b + 86, 2);
    v->margin_bottom = (int16_t)sf_signed(b + 88, 2);
    v->margin_left = (int16_t)sf_signed(b + 90, 2);
}
#undef ALLOCATION

void sf_native_put(uint8_t *b, const struct sophia_sf_record *r)
{
    memset(b, 0, sf_role_size(r));
    switch (r->header.kind) {
    case 38:
        put_opening(b, &r->value.native_opening);
        break;
    case 39:
        put_binding(b, &r->value.native_focus);
        break;
    case 40:
        put_binding(b, &r->value.native_focus_revoked.binding);
        sf_put(b + 112, r->value.native_focus_revoked.reason, 2);
        break;
    case 41: {
        const struct sophia_sf_native_input *v = &r->value.native_input;
        put_binding(b, &v->binding);
        sf_put(b + 112, v->event_id, 8);
        sf_put(b + 120, v->state_revision, 8);
        sf_put(b + 128, v->issued_mono_usec, 8);
        sf_put(b + 136, v->kind, 2);
        sf_text_put(b + 138, 256, v->text);
        break;
    }
    case 42:
        put_activation(b, &r->value.native_activation_outcome.activation);
        sf_put(b + 132, r->value.native_activation_outcome.status, 2);
        sf_put(b + 134, r->value.native_activation_outcome.reason, 2);
        break;
    case 43: {
        const struct sophia_sf_native_closed *v = &r->value.native_closed;
        sf_put(b, v->transaction, 8);
        sf_put(b + 8, v->grant_connection_epoch, 8);
        sf_put(b + 16, v->grant_content_epoch, 8);
        sf_put(b + 24, v->opening, 8);
        sf_put(b + 32, v->reason, 2);
        break;
    }
    case 44:
        sf_put_action(b, &r->value.catalog_activation_outcome.activation.action);
        sf_put(b + 120, r->value.catalog_activation_outcome.activation.catalog_generation, 8);
        sf_put(b + 128, r->value.catalog_activation_outcome.status, 2);
        sf_put(b + 130, r->value.catalog_activation_outcome.reason, 2);
        break;
    case 45: {
        const struct sophia_sf_indicator_activation_outcome *v =
            &r->value.indicator_activation_outcome;
        sf_put(b, v->transaction, 8);
        sf_put(b + 8, v->connection_epoch, 8);
        sf_put(b + 16, v->snapshot_generation, 8);
        sf_put(b + 24, v->event_id, 8);
        sf_put(b + 32, v->status, 2);
        sf_put(b + 34, v->reason, 2);
        break;
    }
    case 266:
        put_allocation(b, &r->value.native_allocation_request);
        break;
    case 268: {
        const struct sophia_sf_native_input_ack *v = &r->value.native_input_ack;
        put_binding(b, &v->binding);
        sf_put(b + 112, v->event_id, 8);
        sf_put(b + 120, v->state_revision, 8);
        sf_put(b + 128, v->disposition, 2);
        break;
    }
    case 269:
        put_activation(b, &r->value.native_activate);
        break;
    case 271:
        sf_put_action(b, &r->value.catalog_activate.action);
        sf_put(b + 120, r->value.catalog_activate.catalog_generation, 8);
        break;
    case 272: {
        const struct sophia_sf_indicator_activate *v = &r->value.indicator_activate;
        sf_put(b, v->transaction, 8);
        sf_put(b + 8, v->connection_epoch, 8);
        sf_put(b + 16, v->snapshot_generation, 8);
        sf_put(b + 24, v->output_id, 8);
        sf_put(b + 32, v->indicator, 8);
        sf_put(b + 40, v->action, 8);
        sf_put(b + 48, v->event_id, 8);
        break;
    }
    }
}

/* Validation is separate from field extraction, so a rejected body cannot
 * expose a partially decoded binding or a borrowed text view. */
int sf_native_wire_check(unsigned, const uint8_t *, size_t);
int sf_native_check(const struct sophia_sf_record *r)
{
    uint8_t body[398];
    size_t n = sf_role_size(r);
    if (!n || n > sizeof(body))
        return -1;
    if (r->header.kind == 41 && sf_text_check(r->value.native_input.text, 256))
        return -1;
    sf_native_put(body, r);
    return sf_native_wire_check(r->header.kind, body, n);
}
int sf_native_take(const uint8_t *b, size_t n, struct sophia_sf_record *r)
{
    if (sf_native_wire_check(r->header.kind, b, n))
        return -1;
    switch (r->header.kind) {
    case 38:
        take_opening(b, &r->value.native_opening);
        break;
    case 39:
        take_binding(b, &r->value.native_focus);
        break;
    case 40:
        take_binding(b, &r->value.native_focus_revoked.binding);
        r->value.native_focus_revoked.reason = (uint16_t)sf_get(b + 112, 2);
        break;
    case 41: {
        struct sophia_sf_native_input *v = &r->value.native_input;
        take_binding(b, &v->binding);
        v->event_id = sf_get(b + 112, 8);
        v->state_revision = sf_get(b + 120, 8);
        v->issued_mono_usec = sf_get(b + 128, 8);
        v->kind = (uint16_t)sf_get(b + 136, 2);
        sf_text_take(b + 138, 256, &v->text);
        break;
    }
    case 42:
        take_activation(b, &r->value.native_activation_outcome.activation);
        r->value.native_activation_outcome.status = (uint16_t)sf_get(b + 132, 2);
        r->value.native_activation_outcome.reason = (uint16_t)sf_get(b + 134, 2);
        break;
    case 43: {
        struct sophia_sf_native_closed *v = &r->value.native_closed;
        v->transaction = sf_get(b, 8);
        v->grant_connection_epoch = sf_get(b + 8, 8);
        v->grant_content_epoch = sf_get(b + 16, 8);
        v->opening = sf_get(b + 24, 8);
        v->reason = (uint16_t)sf_get(b + 32, 2);
        break;
    }
    case 44:
        if (sf_take_action(b, &r->value.catalog_activation_outcome.activation.action))
            return -1;
        r->value.catalog_activation_outcome.activation.catalog_generation = sf_get(b + 120, 8);
        r->value.catalog_activation_outcome.status = (uint16_t)sf_get(b + 128, 2);
        r->value.catalog_activation_outcome.reason = (uint16_t)sf_get(b + 130, 2);
        break;
    case 45: {
        struct sophia_sf_indicator_activation_outcome *v = &r->value.indicator_activation_outcome;
        v->transaction = sf_get(b, 8);
        v->connection_epoch = sf_get(b + 8, 8);
        v->snapshot_generation = sf_get(b + 16, 8);
        v->event_id = sf_get(b + 24, 8);
        v->status = (uint16_t)sf_get(b + 32, 2);
        v->reason = (uint16_t)sf_get(b + 34, 2);
        break;
    }
    case 266:
        take_allocation(b, &r->value.native_allocation_request);
        break;
    case 268: {
        struct sophia_sf_native_input_ack *v = &r->value.native_input_ack;
        take_binding(b, &v->binding);
        v->event_id = sf_get(b + 112, 8);
        v->state_revision = sf_get(b + 120, 8);
        v->disposition = (uint16_t)sf_get(b + 128, 2);
        break;
    }
    case 269:
        take_activation(b, &r->value.native_activate);
        break;
    case 271:
        if (sf_take_action(b, &r->value.catalog_activate.action))
            return -1;
        r->value.catalog_activate.catalog_generation = sf_get(b + 120, 8);
        break;
    case 272: {
        struct sophia_sf_indicator_activate *v = &r->value.indicator_activate;
        v->transaction = sf_get(b, 8);
        v->connection_epoch = sf_get(b + 8, 8);
        v->snapshot_generation = sf_get(b + 16, 8);
        v->output_id = sf_get(b + 24, 8);
        v->indicator = sf_get(b + 32, 8);
        v->action = sf_get(b + 40, 8);
        v->event_id = sf_get(b + 48, 8);
        break;
    }
    default:
        return -1;
    }
    return 0;
}

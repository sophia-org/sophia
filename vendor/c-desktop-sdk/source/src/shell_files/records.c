/* Dispatch and lengths from the pinned file KDL. */
#include "internal.h"
#include "roles_internal.h"
static size_t body_size(const struct sophia_sf_record *r)
{
    if (sf_role_kind(r->header.kind))
        return sf_role_size(r);
    switch (r->header.kind) {
    case 1:
        return 264u;
    case 2:
        return 40u + 40u * r->value.outputs.output_count;
    case 256:
        return 16u;
    case 16:
        return 32u;
    case 17:
        return 16u;
    case 18:
        return 16u;
    case 19:
        return 24u;
    case 257:
        return 128u;
    case 32:
        return 168u;
    case 258:
        return 80u;
    case 259:
        return 56u;
    case 260:
        return 40u;
    case 261:
        return 40u;
    case 33:
        return 56u;
    case 34:
        return 42u;
    case 262:
        return 80u + 64u * r->value.candidate.surface_count +
               32u * r->value.candidate.placement_count + 48u * r->value.candidate.target_count;
    case 263:
        return 66u;
    case 264:
        return 56u;
    case 36:
        return 72u;
    case 37:
        return 120u;
    case 265:
        return 120u;
    case 35:
        return 76u;
    default:
        return 0;
    }
}
int sophia_sf_encode(void *dst, size_t capacity, const struct sophia_sf_record *r, size_t *written)
{
    uint8_t *b = dst;
    size_t n, i, at;
    (void)i;
    (void)at;
    if (!dst || !r || !written)
        return -4;
    if (sf_validate(r))
        return -1;
    n = 32 + body_size(r);
    if (n > capacity)
        return -4;
    sf_put(b, n, 4);
    sf_put(b + 4, 1, 2);
    sf_put(b + 6, r->header.kind, 2);
    sf_put(b + 8, r->header.epoch, 8);
    sf_put(b + 16, r->header.submission, 8);
    sf_put(b + 24, r->header.sequence, 8);
    b += 32;
    if (sf_role_kind(r->header.kind)) {
        sf_role_put(b, r);
        *written = n;
        return 0;
    }
    switch (r->header.kind) {
    case 1:
        sf_put_limits(b, &r->value.limits);
        break;
    case 2:
        sf_put_outputs(b, &r->value.outputs);
        for (i = 0; i < r->value.outputs.output_count; i++)
            sf_put_content_output_facts_entry(b + 40 + 40 * i, &r->value.outputs.outputs[i]);
        break;
    case 256:
        sf_put_negotiate(b, &r->value.negotiate);
        break;
    case 16:
        sf_put_negotiated(b, &r->value.negotiated);
        break;
    case 17:
        sf_put_refused(b, &r->value.refused);
        break;
    case 18:
        sf_put_submitted(b, &r->value.submitted);
        break;
    case 19:
        sf_put_object_published(b, &r->value.object_published);
        break;
    case 257:
        sf_put_allocation_request(b, &r->value.allocation_request);
        break;
    case 32:
        sf_put_allocation_result(b, &r->value.allocation_result);
        break;
    case 258:
        sf_put_resource_begin(b, &r->value.resource_begin);
        break;
    case 259:
        sf_put_resource_end(b, &r->value.resource_end);
        break;
    case 260:
        sf_put_resource_cancel(b, &r->value.resource_cancel);
        break;
    case 261:
        sf_put_resource_retire(b, &r->value.resource_retire);
        break;
    case 33:
        sf_put_resource_status(b, &r->value.resource_status);
        break;
    case 34:
        sf_put_resource_released(b, &r->value.resource_released);
        break;
    case 262:
        sf_put_candidate(b, &r->value.candidate);
        at = 80;
        for (i = 0; i < r->value.candidate.surface_count; i++, at += 64)
            sf_put_content_surface(b + at, &r->value.candidate.surfaces[i]);
        for (i = 0; i < r->value.candidate.placement_count; i++, at += 32)
            sf_put_content_placement(b + at, &r->value.candidate.placements[i]);
        for (i = 0; i < r->value.candidate.target_count; i++, at += 48)
            sf_put_content_target(b + at, &r->value.candidate.targets[i]);
        break;
    case 263:
        sf_put_frame_demand(b, &r->value.frame_demand);
        break;
    case 264:
        sf_put_frame_demand_cancel(b, &r->value.frame_demand_cancel);
        break;
    case 36:
        sf_put_frame_permit(b, &r->value.frame_permit);
        break;
    case 37:
        sf_put_action(b, &r->value.action);
        break;
    case 265:
        sf_put_action_ack(b, &r->value.action_ack);
        break;
    case 35:
        sf_put_candidate_outcome(b, &r->value.candidate_outcome);
        break;
    default:
        return -1;
    }
    *written = n;
    return 0;
}
int sophia_sf_decode(const void *src, size_t bytes, struct sophia_sf_record *out)
{
    const uint8_t *b = src;
    struct sophia_sf_record r;
    size_t i, at;
    (void)i;
    (void)at;
    if (!src || !out)
        return -4;
    if (bytes < 32 || bytes > SOPHIA_SF_MAX_RECORD || sf_get(b, 4) != bytes ||
        sf_get(b + 4, 2) != 1)
        return -1;
    memset(&r, 0, sizeof(r));
    r.header.kind = (uint16_t)sf_get(b + 6, 2);
    r.header.epoch = sf_get(b + 8, 8);
    r.header.submission = sf_get(b + 16, 8);
    r.header.sequence = sf_get(b + 24, 8);
    b += 32;
    bytes -= 32;
    if (sf_role_kind(r.header.kind)) {
        if (sf_role_take(b, bytes, &r) || sf_validate(&r))
            return -1;
        *out = r;
        return 0;
    }
    if (bytes + 32 > SOPHIA_SF_MAX_TRANSACTION)
        return -1;
    switch (r.header.kind) {
    case 1:
        if (bytes != 264 || sf_take_limits(b, &r.value.limits))
            return -1;
        break;
    case 2:
        if (bytes < 40 || sf_take_outputs(b, &r.value.outputs))
            return -1;
        if (bytes != body_size(&r))
            return -1;
        for (i = 0; i < r.value.outputs.output_count; i++)
            if (sf_take_content_output_facts_entry(b + 40 + 40 * i, &r.value.outputs.outputs[i]))
                return -1;
        break;
    case 256:
        if (bytes != 16 || sf_take_negotiate(b, &r.value.negotiate))
            return -1;
        break;
    case 16:
        if (bytes != 32 || sf_take_negotiated(b, &r.value.negotiated))
            return -1;
        break;
    case 17:
        if (bytes != 16 || sf_take_refused(b, &r.value.refused))
            return -1;
        break;
    case 18:
        if (bytes != 16 || sf_take_submitted(b, &r.value.submitted))
            return -1;
        break;
    case 19:
        if (bytes != 24 || sf_take_object_published(b, &r.value.object_published))
            return -1;
        break;
    case 257:
        if (bytes != 128 || sf_take_allocation_request(b, &r.value.allocation_request))
            return -1;
        break;
    case 32:
        if (bytes != 168 || sf_take_allocation_result(b, &r.value.allocation_result))
            return -1;
        break;
    case 258:
        if (bytes != 80 || sf_take_resource_begin(b, &r.value.resource_begin))
            return -1;
        break;
    case 259:
        if (bytes != 56 || sf_take_resource_end(b, &r.value.resource_end))
            return -1;
        break;
    case 260:
        if (bytes != 40 || sf_take_resource_cancel(b, &r.value.resource_cancel))
            return -1;
        break;
    case 261:
        if (bytes != 40 || sf_take_resource_retire(b, &r.value.resource_retire))
            return -1;
        break;
    case 33:
        if (bytes != 56 || sf_take_resource_status(b, &r.value.resource_status))
            return -1;
        break;
    case 34:
        if (bytes != 42 || sf_take_resource_released(b, &r.value.resource_released))
            return -1;
        break;
    case 262:
        if (bytes < 80 || sf_take_candidate(b, &r.value.candidate))
            return -1;
        if (bytes != body_size(&r))
            return -1;
        at = 80;
        for (i = 0; i < r.value.candidate.surface_count; i++, at += 64)
            if (sf_take_content_surface(b + at, &r.value.candidate.surfaces[i]))
                return -1;
        for (i = 0; i < r.value.candidate.placement_count; i++, at += 32)
            if (sf_take_content_placement(b + at, &r.value.candidate.placements[i]))
                return -1;
        for (i = 0; i < r.value.candidate.target_count; i++, at += 48)
            if (sf_take_content_target(b + at, &r.value.candidate.targets[i]))
                return -1;
        break;
    case 263:
        if (bytes != 66 || sf_take_frame_demand(b, &r.value.frame_demand))
            return -1;
        break;
    case 264:
        if (bytes != 56 || sf_take_frame_demand_cancel(b, &r.value.frame_demand_cancel))
            return -1;
        break;
    case 36:
        if (bytes != 72 || sf_take_frame_permit(b, &r.value.frame_permit))
            return -1;
        break;
    case 37:
        if (bytes != 120 || sf_take_action(b, &r.value.action))
            return -1;
        break;
    case 265:
        if (bytes != 120 || sf_take_action_ack(b, &r.value.action_ack))
            return -1;
        break;
    case 35:
        if (bytes != 76 || sf_take_candidate_outcome(b, &r.value.candidate_outcome))
            return -1;
        break;
    default:
        return -1;
    }
    if (sf_validate(&r))
        return -1;
    *out = r;
    return 0;
}
int sf_value_check(const struct sophia_sf_record *r)
{
    if (sf_role_kind(r->header.kind))
        return sf_role_check(r);
    switch (r->header.kind) {
    case 1:
        return sf_check_limits(&r->value.limits);
    case 2:
        return sf_check_outputs(&r->value.outputs);
    case 16:
        return sf_check_negotiated(&r->value.negotiated);
    case 17:
        return sf_check_refused(&r->value.refused);
    case 18:
        return sf_check_submitted(&r->value.submitted);
    case 19:
        return sf_check_object_published(&r->value.object_published);
    case 32:
        return sf_check_allocation_result(&r->value.allocation_result);
    case 33:
        return sf_check_resource_status(&r->value.resource_status);
    case 34:
        return sf_check_resource_released(&r->value.resource_released);
    case 35:
        return sf_check_candidate_outcome(&r->value.candidate_outcome);
    case 36:
        return sf_check_frame_permit(&r->value.frame_permit);
    case 37:
        return sf_check_action(&r->value.action);
    case 256:
        return sf_check_negotiate(&r->value.negotiate);
    case 257:
        return sf_check_allocation_request(&r->value.allocation_request);
    case 258:
        return sf_check_resource_begin(&r->value.resource_begin);
    case 259:
        return sf_check_resource_end(&r->value.resource_end);
    case 260:
        return sf_check_resource_cancel(&r->value.resource_cancel);
    case 261:
        return sf_check_resource_retire(&r->value.resource_retire);
    case 262:
        return sf_check_candidate(&r->value.candidate);
    case 263:
        return sf_check_frame_demand(&r->value.frame_demand);
    case 264:
        return sf_check_frame_demand_cancel(&r->value.frame_demand_cancel);
    case 265:
        return sf_check_action_ack(&r->value.action_ack);
    default:
        return -1;
    }
}
int sophia_sf_submit_encode(uint8_t b[24], uint64_t epoch, uint64_t id, uint32_t bytes)
{
    if (!b || !epoch || !id || bytes < 32 || bytes > SOPHIA_SF_MAX_TRANSACTION)
        return -4;
    sf_put(b, epoch, 8);
    sf_put(b + 8, id, 8);
    sf_put(b + 16, bytes, 4);
    sf_put(b + 20, 0, 4);
    return 0;
}
int sophia_sf_ack_encode(uint8_t b[16], uint64_t epoch, uint64_t sequence)
{
    if (!b || !epoch || !sequence)
        return -4;
    sf_put(b, epoch, 8);
    sf_put(b + 8, sequence, 8);
    return 0;
}

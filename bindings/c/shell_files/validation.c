#include "internal.h"
int sf_value_check(const struct sophia_sf_record *);

static int limits(const struct sophia_sf_limits *v)
{
    /* Scalar prototype caps have already been checked, so these sums cannot
     * overflow. These are the normative f64d670e0 KDL relationships. */
    if (v->max_resource_ids < v->max_live_resources ||
        v->max_allocations_total < v->max_allocations_per_output ||
        v->max_open_candidates_total < v->max_open_candidates_per_output ||
        v->max_pending_candidates_total < v->max_pending_candidates_per_output ||
        v->max_output_queue_bytes < v->reserved_control_queue_bytes ||
        v->max_input_queue_bytes < v->max_frame_payload + 24 ||
        v->max_chunk_bytes < v->max_width_px * 4 ||
        v->max_chunk_bytes + 48 > v->max_frame_payload ||
        v->max_staging_bytes < v->max_resource_bytes ||
        v->max_resident_bytes < v->max_resource_bytes ||
        v->max_retiring_bytes < v->max_resource_bytes ||
        v->max_session_retiring_bytes <
            v->max_staging_bytes + v->max_resident_bytes + v->max_retiring_bytes ||
        v->max_reservation_extent > v->max_panel_extent ||
        v->transfer_idle_timeout_ms > v->transfer_timeout_ms)
        return -1;
    return 0;
}
static int candidate(const struct sophia_sf_candidate *v)
{
    size_t i;
    for (i = 0; i < v->surface_count; i++) {
        const struct sophia_sf_content_surface *s = &v->surfaces[i];
        if (sf_check_content_surface(s))
            return -1;
        if (s->role == 1) {
            if (s->parent_surface_index != UINT16_MAX || s->anchor_x || s->anchor_y ||
                s->anchor_width || s->anchor_height)
                return -1;
        } else if (s->parent_surface_index >= 8 || s->reservation_extent ||
                   !sf_rect(s->anchor_x, s->anchor_y, s->anchor_width, s->anchor_height))
            return -1;
    }
    for (i = 0; i < v->placement_count; i++) {
        const struct sophia_sf_content_placement *p = &v->placements[i];
        if (sf_check_content_placement(p) || p->surface_index >= v->surface_count)
            return -1;
    }
    for (i = 0; i < v->target_count; i++) {
        const struct sophia_sf_content_target *t = &v->targets[i];
        if (sf_check_content_target(t) || t->surface_index >= v->surface_count ||
            !sf_rect(t->bounds_x, t->bounds_y, t->bounds_width, t->bounds_height))
            return -1;
    }
    return 0;
}
static int allocation_request(const struct sophia_sf_allocation_request *v)
{
    if (!sf_pair(v->prior_id, v->prior_generation) || !sf_pair(v->parent_id, v->parent_generation))
        return -1;
    if ((v->operation == 1) != (v->prior_id == 0))
        return -1;
    if (v->operation == 3) {
        if (v->parent_id || v->parent_presentation_epoch || v->anchor_x || v->anchor_y ||
            v->anchor_width || v->anchor_height || v->desired_width || v->desired_height ||
            v->margin_top || v->margin_right || v->margin_bottom || v->margin_left)
            return -1;
    } else {
        if (!v->desired_width || !v->desired_height)
            return -1;
        if (v->role == 1) {
            if (v->parent_id || v->parent_presentation_epoch || v->anchor_x || v->anchor_y ||
                v->anchor_width || v->anchor_height)
                return -1;
        } else if (!v->parent_id || !v->parent_presentation_epoch ||
                   !sf_rect(v->anchor_x, v->anchor_y, v->anchor_width, v->anchor_height))
            return -1;
    }
    return 0;
}
static int allocation_result(const struct sophia_sf_allocation_result *v)
{
    if ((v->allocation_request_id == 0) != (v->status == 4) ||
        !sf_pair(v->allocation_id, v->allocation_generation) ||
        !sf_pair(v->parent_id, v->parent_generation) || (!v->allocation_id && v->status != 2))
        return -1;
    if (v->status == 1) {
        if (v->reason || !v->scale_generation ||
            !sf_scale(v->scale_numerator, v->scale_denominator) || !v->logical_width ||
            !v->logical_height || !v->pixel_width || !v->pixel_height)
            return -1;
    } else if (v->status == 2 || v->status == 3) {
        if (v->scale_generation || v->logical_x || v->logical_y || v->logical_width ||
            v->logical_height || v->pixel_x || v->pixel_y || v->pixel_width || v->pixel_height ||
            v->scale_numerator || v->scale_denominator || v->allowed_reservation_extent ||
            v->acknowledged_anchor_x || v->acknowledged_anchor_y || v->acknowledged_anchor_width ||
            v->acknowledged_anchor_height || v->margin_top || v->margin_right || v->margin_bottom ||
            v->margin_left)
            return -1;
    }
    return 0;
}
int sf_validate(const struct sophia_sf_record *r)
{
    uint16_t k = r->header.kind;
    size_t i, j;
    if (!r->header.epoch || sf_value_check(r))
        return -1;
    if (k < 16) {
        if (r->header.submission || r->header.sequence)
            return -1;
    } else if (k < 256) {
        if (r->header.submission || !r->header.sequence)
            return -1;
    } else if (!r->header.submission || r->header.sequence)
        return -1;
    switch (k) {
    case SOPHIA_SF_NEGOTIATE:
        return !r->value.negotiate.minimum_revision ||
                       r->value.negotiate.minimum_revision > r->value.negotiate.maximum_revision
                   ? -1
                   : 0;
    case SOPHIA_SF_NEGOTIATED:
        return !r->value.negotiated.selected_revision ||
                       r->value.negotiated.connection_epoch != r->header.epoch
                   ? -1
                   : 0;
    case SOPHIA_SF_LIMITS:
        return limits(&r->value.limits);
    case SOPHIA_SF_OUTPUTS:
        for (i = 0; i < r->value.outputs.output_count; i++) {
            const struct sophia_sf_content_output_facts_entry *o = &r->value.outputs.outputs[i];
            if (sf_check_content_output_facts_entry(o) ||
                !sf_scale(o->scale_numerator, o->scale_denominator))
                return -1;
            for (j = 0; j < i; j++)
                if (o->output_id == r->value.outputs.outputs[j].output_id)
                    return -1;
        }
        return 0;
    case SOPHIA_SF_ALLOCATION_REQUEST:
        return allocation_request(&r->value.allocation_request);
    case SOPHIA_SF_ALLOCATION_RESULT:
        return allocation_result(&r->value.allocation_result);
    case SOPHIA_SF_RESOURCE_BEGIN: {
        const struct sophia_sf_resource_begin *v = &r->value.resource_begin;
        return sf_scale(v->rendered_scale_numerator, v->rendered_scale_denominator) &&
                       v->total_bytes == (uint64_t)v->width_px * 4 * v->height_px
                   ? 0
                   : -1;
    }
    case SOPHIA_SF_RESOURCE_STATUS:
        return r->value.resource_status.status <= 2 && r->value.resource_status.reason ? -1 : 0;
    case SOPHIA_SF_CANDIDATE:
        return candidate(&r->value.candidate);
    case SOPHIA_SF_FRAME_DEMAND:
        return sf_pair(r->value.frame_demand.allocation_id,
                       r->value.frame_demand.allocation_generation)
                   ? 0
                   : -1;
    case SOPHIA_SF_FRAME_PERMIT: {
        const struct sophia_sf_frame_permit *v = &r->value.frame_permit;
        return v->state == 1 &&
                       (!v->permit_id || !v->ttl_ms || !v->max_candidate_bytes || v->reason)
                   ? -1
                   : 0;
    }
    case SOPHIA_SF_CANDIDATE_OUTCOME: {
        const struct sophia_sf_candidate_outcome *v = &r->value.candidate_outcome;
        return ((v->kind == 2) != (v->presentation_epoch != 0)) || (v->kind <= 2 && v->reason) ? -1
                                                                                               : 0;
    }
    case SOPHIA_SF_ACTION: {
        const struct sophia_sf_action *v = &r->value.action;
        if (v->kind == 1 && (!v->target_id || !v->target_generation || !v->action_id || v->reason))
            return -1;
        if (v->kind == 2 && (v->target_id || v->target_generation || v->action_id || v->reason))
            return -1;
        return 0;
    }
    case SOPHIA_SF_ACTION_ACK: {
        const struct sophia_sf_action_ack *v = &r->value.action_ack;
        return ((!v->target_id) != (!v->target_generation)) || ((!v->target_id) != (!v->action_id))
                   ? -1
                   : 0;
    }
    default:
        return 0;
    }
}

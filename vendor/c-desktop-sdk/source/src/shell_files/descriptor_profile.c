/* Role selection and disclosure from spec/sophia-shell-descriptors.md. */
#include "session_internal.h"

int sf_descriptor_offer(const struct sophia_sf_negotiate *offer)
{
    static const unsigned revisions[11] = {1, 1, 2, 3, 3, 4, 4, 5, 5, 6, 6};
    uint64_t caps = offer->required_capabilities;
    unsigned revision = offer->maximum_revision < 8 ? offer->maximum_revision : 8;
    if (!offer->minimum_revision || offer->minimum_revision > revision || !(caps & 1) ||
        (caps & ~UINT64_C(0x7ff)))
        return 0;
    for (unsigned bit = 0; bit < 11; bit++)
        if ((caps & (UINT64_C(1) << bit)) && revision < revisions[bit])
            return 0;
    return (!(caps & (1u << 4)) || (caps & (1u << 3))) &&
           (!(caps & (1u << 6)) || (caps & (1u << 5))) &&
           (!(caps & (1u << 8)) || (caps & (1u << 7))) &&
           (!(caps & (1u << 10)) || (caps & (1u << 9)));
}
int sf_session_candidate_allowed(const struct sophia_sf_client *c, uint16_t kind)
{
    if (kind <= SOPHIA_SF_NEGOTIATE || kind > SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION_ACK)
        return 0;
    if (c->profile == SOPHIA_SF_DESCRIPTOR)
        return sf_descriptor_record_allowed(c, kind);
    return kind < SOPHIA_SF_DESCRIPTOR_CANDIDATE;
}
int sf_session_event_allowed(const struct sophia_sf_client *c, const struct sophia_sf_record *r)
{
    if (c->profile != SOPHIA_SF_DESCRIPTOR)
        return !((r->header.kind >= SOPHIA_SF_DESCRIPTOR_OUTCOME &&
                  r->header.kind <= SOPHIA_SF_DESCRIPTOR_LAUNCH_OUTCOME) ||
                 (r->header.kind == SOPHIA_SF_OBJECT_PUBLISHED &&
                  r->value.object_published.object_kind > SOPHIA_SF_INDICATORS));
    if (r->header.kind == SOPHIA_SF_OBJECT_PUBLISHED)
        return sf_descriptor_object_allowed(c, r->value.object_published.object_kind);
    return r->header.kind < 32 || sf_descriptor_record_allowed(c, r->header.kind);
}
int sf_descriptor_welcome(const struct sophia_sf_client *c, const struct sophia_sf_negotiated *v)
{
    unsigned revision = c->offer.maximum_revision < 8 ? c->offer.maximum_revision : 8;
    return v->connection_epoch == c->epoch && v->selected_revision == revision &&
           v->capabilities == (c->offer.required_capabilities | 2u) &&
           v->limits_published == !!(v->capabilities & (1u << 7));
}
int sf_descriptor_object_allowed(const struct sophia_sf_client *c, uint16_t kind)
{
    unsigned bit;
    switch (kind) {
    case SOPHIA_SF_LIMITS:
    case SOPHIA_SF_OUTPUTS:
        bit = 7;
        break;
    case SOPHIA_SF_CATALOG:
        bit = 5;
        break;
    case SOPHIA_SF_INDICATORS:
        bit = 9;
        break;
    case SOPHIA_SF_DESCRIPTORS:
        bit = 0;
        break;
    case SOPHIA_SF_TABS:
        bit = 2;
        break;
    case SOPHIA_SF_SHORTCUTS:
        bit = 3;
        break;
    default:
        return 0;
    }
    return c->negotiated && !!(c->welcome.capabilities & (UINT64_C(1) << bit));
}
int sf_descriptor_record_allowed(const struct sophia_sf_client *c, uint16_t kind)
{
    unsigned bit;
    switch (kind) {
    case SOPHIA_SF_DESCRIPTOR_OUTCOME:
    case SOPHIA_SF_DESCRIPTOR_ACTIVATION:
    case SOPHIA_SF_DESCRIPTOR_CANDIDATE:
    case SOPHIA_SF_DESCRIPTOR_ACTIVATION_ACK:
        bit = 0;
        break;
    case SOPHIA_SF_TABS_CANDIDATE:
        bit = 2;
        break;
    case SOPHIA_SF_REFERENCE_REQUEST:
    case SOPHIA_SF_REFERENCE_OUTCOME:
    case SOPHIA_SF_REFERENCE_CANDIDATE:
        bit = 4;
        break;
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_REQUEST:
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_OUTCOME:
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION:
    case SOPHIA_SF_DESCRIPTOR_LAUNCH_OUTCOME:
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_CANDIDATE:
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION_ACK:
        bit = 6;
        break;
    case SOPHIA_SF_ALLOCATION_RESULT:
    case SOPHIA_SF_RESOURCE_STATUS:
    case SOPHIA_SF_RESOURCE_RELEASED:
    case SOPHIA_SF_CANDIDATE_OUTCOME:
    case SOPHIA_SF_FRAME_PERMIT:
    case SOPHIA_SF_ALLOCATION_REQUEST:
    case SOPHIA_SF_RESOURCE_BEGIN:
    case SOPHIA_SF_RESOURCE_END:
    case SOPHIA_SF_RESOURCE_CANCEL:
    case SOPHIA_SF_RESOURCE_RETIRE:
    case SOPHIA_SF_CANDIDATE:
    case SOPHIA_SF_FRAME_DEMAND:
    case SOPHIA_SF_FRAME_DEMAND_CANCEL:
        bit = 7;
        break;
    case SOPHIA_SF_ACTION:
    case SOPHIA_SF_ACTION_ACK:
        bit = 8;
        break;
    case SOPHIA_SF_INDICATOR_ACTIVATION_OUTCOME:
    case SOPHIA_SF_INDICATOR_ACTIVATE:
        bit = 10;
        break;
    default:
        return 0;
    }
    return c->negotiated && !!(c->welcome.capabilities & (UINT64_C(1) << bit));
}

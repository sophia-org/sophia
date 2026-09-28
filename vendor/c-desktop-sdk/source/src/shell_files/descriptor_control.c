/* Native controls from spec/proposed/descriptor-layout.kdl, with the value
 * relationships in descriptor-records.md. No socket framing or IPC codecs. */
#include "descriptors_internal.h"

size_t sf_descriptor_size(const struct sophia_sf_record *r)
{
    if (sf_descriptor_object_kind(r->header.kind))
        return sf_descriptor_object_size(r);
    if (sf_descriptor_candidate_kind(r->header.kind))
        return sf_descriptor_candidate_size(r);
    switch (r->header.kind) {
    case SOPHIA_SF_DESCRIPTOR_OUTCOME:
        return 36;
    case SOPHIA_SF_DESCRIPTOR_ACTIVATION:
        return 84;
    case SOPHIA_SF_REFERENCE_REQUEST:
        return 60;
    case SOPHIA_SF_REFERENCE_OUTCOME:
        return 56;
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_REQUEST:
        return 320;
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_OUTCOME:
        return 44;
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION:
        return 60;
    case SOPHIA_SF_DESCRIPTOR_LAUNCH_OUTCOME:
        return 60;
    case SOPHIA_SF_DESCRIPTOR_ACTIVATION_ACK:
        return 28;
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION_ACK:
        return 60;
    default:
        return 0;
    }
}
static int identity(uint64_t transaction, uint64_t connection, uint64_t epoch)
{
    return transaction && connection == epoch && epoch ? 0 : -1;
}
static int outcome(uint16_t kind, uint64_t presentation)
{
    return kind >= 1 && kind <= 4 && (kind != 2 || presentation) ? 0 : -1;
}
static int request_check(const struct sophia_sf_reference_request *v, uint64_t epoch)
{
    return identity(v->transaction, v->connection_epoch, epoch) || !v->catalog_generation ||
                   !v->request_generation || !v->output_id || !v->output_generation ||
                   v->operation > 4
               ? -1
               : 0;
}
static int grant_check(const struct sophia_sf_descriptor_launcher_activation *v, uint64_t epoch)
{
    return identity(v->transaction, v->connection_epoch, epoch) || !v->catalog_generation ||
                   !v->request_generation || !v->candidate_generation || !v->presentation_epoch ||
                   !v->activation || !v->slot || v->slot > 4096
               ? -1
               : 0;
}
int sf_descriptor_check(const struct sophia_sf_record *r)
{
    uint64_t epoch = r->header.epoch;
    if (sf_descriptor_object_kind(r->header.kind))
        return sf_descriptor_object_check(r);
    if (sf_descriptor_candidate_kind(r->header.kind))
        return sf_descriptor_candidate_check(r);
    switch (r->header.kind) {
    case SOPHIA_SF_DESCRIPTOR_OUTCOME: {
        const struct sophia_sf_descriptor_outcome *v = &r->value.descriptor_outcome;
        return identity(v->transaction, v->connection_epoch, epoch) || !v->candidate_generation ||
                       outcome(v->kind, v->presentation_epoch) ||
                       ((v->kind == 2) != (v->presentation_epoch != 0))
                   ? -1
                   : 0;
    }
    case SOPHIA_SF_DESCRIPTOR_ACTIVATION: {
        const struct sophia_sf_descriptor_activation *v = &r->value.descriptor_activation;
        return identity(v->transaction, v->connection_epoch, epoch) || !v->candidate_generation ||
                       !v->presentation_epoch || !v->activation || !v->action_token ||
                       !v->action_issuer_epoch || !v->action_issuer_revocation_epoch ||
                       v->action_recipient_epoch != epoch || !v->action_target_slot ||
                       !v->action_target_generation
                   ? -1
                   : 0;
    }
    case SOPHIA_SF_REFERENCE_REQUEST:
        return request_check(&r->value.reference_request, epoch);
    case SOPHIA_SF_REFERENCE_OUTCOME: {
        const struct sophia_sf_reference_outcome *v = &r->value.reference_outcome;
        return identity(v->transaction, v->connection_epoch, epoch) || !v->catalog_generation ||
                       !v->request_generation || !v->candidate_generation || v->page >= v->pages ||
                       outcome(v->kind, v->presentation_epoch)
                   ? -1
                   : 0;
    }
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_REQUEST: {
        const struct sophia_sf_descriptor_launcher_request *v =
            &r->value.descriptor_launcher_request;
        return request_check(&v->request, epoch) || sf_launcher_text_check(v->query, 256) ? -1 : 0;
    }
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_OUTCOME: {
        const struct sophia_sf_descriptor_launcher_outcome *v =
            &r->value.descriptor_launcher_outcome;
        return identity(v->transaction, v->connection_epoch, epoch) || !v->request_generation ||
                       !v->candidate_generation || outcome(v->kind, v->presentation_epoch)
                   ? -1
                   : 0;
    }
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION:
        return grant_check(&r->value.descriptor_launcher_activation, epoch);
    case SOPHIA_SF_DESCRIPTOR_LAUNCH_OUTCOME: {
        const struct sophia_sf_descriptor_launch_outcome *v = &r->value.descriptor_launch_outcome;
        return grant_check(&v->grant, epoch) || v->status < 1 || v->status > 3 ? -1 : 0;
    }
    case SOPHIA_SF_DESCRIPTOR_ACTIVATION_ACK: {
        const struct sophia_sf_descriptor_activation_ack *v = &r->value.descriptor_activation_ack;
        return identity(v->transaction, v->connection_epoch, epoch) || !v->activation ||
                       v->disposition < 1 || v->disposition > 2
                   ? -1
                   : 0;
    }
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION_ACK: {
        const struct sophia_sf_descriptor_launcher_activation_ack *v =
            &r->value.descriptor_launcher_activation_ack;
        return grant_check(&v->grant, epoch) || v->consumed > 1 ? -1 : 0;
    }
    default:
        return -1;
    }
}
static void request_put(uint8_t *b, const struct sophia_sf_reference_request *v)
{
    sf_put(b, v->transaction, 8);
    sf_put(b + 8, v->connection_epoch, 8);
    sf_put(b + 16, v->catalog_generation, 8);
    sf_put(b + 24, v->request_generation, 8);
    sf_put(b + 32, v->output_id, 8);
    sf_put(b + 40, v->output_generation, 8);
    sf_put(b + 48, v->presentation_epoch, 8);
    sf_put(b + 56, v->operation, 2);
}
static void grant_put(uint8_t *b, const struct sophia_sf_descriptor_launcher_activation *v)
{
    sf_put(b, v->transaction, 8);
    sf_put(b + 8, v->connection_epoch, 8);
    sf_put(b + 16, v->catalog_generation, 8);
    sf_put(b + 24, v->request_generation, 8);
    sf_put(b + 32, v->candidate_generation, 8);
    sf_put(b + 40, v->presentation_epoch, 8);
    sf_put(b + 48, v->activation, 8);
    sf_put(b + 56, v->slot, 2);
}
void sf_descriptor_put(uint8_t *b, const struct sophia_sf_record *r)
{
    if (sf_descriptor_object_kind(r->header.kind)) {
        sf_descriptor_object_put(b, r);
        return;
    }
    if (sf_descriptor_candidate_kind(r->header.kind)) {
        sf_descriptor_candidate_put(b, r);
        return;
    }
    /* All padding is written, including query capacity beyond its length. */
    memset(b, 0, sf_descriptor_size(r));
    switch (r->header.kind) {
    case SOPHIA_SF_DESCRIPTOR_OUTCOME: {
        const struct sophia_sf_descriptor_outcome *v = &r->value.descriptor_outcome;
        sf_put(b, v->transaction, 8);
        sf_put(b + 8, v->connection_epoch, 8);
        sf_put(b + 16, v->candidate_generation, 8);
        sf_put(b + 24, v->presentation_epoch, 8);
        sf_put(b + 32, v->kind, 2);
        break;
    }
    case SOPHIA_SF_DESCRIPTOR_ACTIVATION: {
        const struct sophia_sf_descriptor_activation *v = &r->value.descriptor_activation;
        sf_put(b, v->transaction, 8);
        sf_put(b + 8, v->connection_epoch, 8);
        sf_put(b + 16, v->candidate_generation, 8);
        sf_put(b + 24, v->presentation_epoch, 8);
        sf_put(b + 32, v->activation, 8);
        sf_put(b + 40, v->action_token, 8);
        sf_put(b + 48, v->action_issuer_epoch, 8);
        sf_put(b + 56, v->action_issuer_revocation_epoch, 8);
        sf_put(b + 64, v->action_recipient_epoch, 8);
        sf_put(b + 72, v->action_target_slot, 2);
        sf_put(b + 76, v->action_target_generation, 8);
        break;
    }
    case SOPHIA_SF_REFERENCE_REQUEST:
        request_put(b, &r->value.reference_request);
        break;
    case SOPHIA_SF_REFERENCE_OUTCOME: {
        const struct sophia_sf_reference_outcome *v = &r->value.reference_outcome;
        sf_put(b, v->transaction, 8);
        sf_put(b + 8, v->connection_epoch, 8);
        sf_put(b + 16, v->catalog_generation, 8);
        sf_put(b + 24, v->request_generation, 8);
        sf_put(b + 32, v->candidate_generation, 8);
        sf_put(b + 40, v->presentation_epoch, 8);
        sf_put(b + 48, v->page, 2);
        sf_put(b + 50, v->pages, 2);
        sf_put(b + 52, v->kind, 2);
        break;
    }
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_REQUEST:
        request_put(b, &r->value.descriptor_launcher_request.request);
        sf_text_put(b + 60, 256, r->value.descriptor_launcher_request.query);
        break;
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_OUTCOME: {
        const struct sophia_sf_descriptor_launcher_outcome *v =
            &r->value.descriptor_launcher_outcome;
        sf_put(b, v->transaction, 8);
        sf_put(b + 8, v->connection_epoch, 8);
        sf_put(b + 16, v->request_generation, 8);
        sf_put(b + 24, v->candidate_generation, 8);
        sf_put(b + 32, v->presentation_epoch, 8);
        sf_put(b + 40, v->kind, 2);
        break;
    }
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION:
        grant_put(b, &r->value.descriptor_launcher_activation);
        break;
    case SOPHIA_SF_DESCRIPTOR_LAUNCH_OUTCOME:
        grant_put(b, &r->value.descriptor_launch_outcome.grant);
        sf_put(b + 58, r->value.descriptor_launch_outcome.status, 2);
        break;
    case SOPHIA_SF_DESCRIPTOR_ACTIVATION_ACK: {
        const struct sophia_sf_descriptor_activation_ack *v = &r->value.descriptor_activation_ack;
        sf_put(b, v->transaction, 8);
        sf_put(b + 8, v->connection_epoch, 8);
        sf_put(b + 16, v->activation, 8);
        sf_put(b + 24, v->disposition, 2);
        break;
    }
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION_ACK:
        grant_put(b, &r->value.descriptor_launcher_activation_ack.grant);
        sf_put(b + 58, r->value.descriptor_launcher_activation_ack.consumed, 2);
        break;
    }
}
static int request_take(const uint8_t *b, struct sophia_sf_reference_request *v)
{
    v->transaction = sf_get(b, 8);
    v->connection_epoch = sf_get(b + 8, 8);
    v->catalog_generation = sf_get(b + 16, 8);
    v->request_generation = sf_get(b + 24, 8);
    v->output_id = sf_get(b + 32, 8);
    v->output_generation = sf_get(b + 40, 8);
    v->presentation_epoch = sf_get(b + 48, 8);
    v->operation = (uint16_t)sf_get(b + 56, 2);
    return sf_zero(b + 58, 2) ? 0 : -1;
}
static void grant_take(const uint8_t *b, struct sophia_sf_descriptor_launcher_activation *v)
{
    v->transaction = sf_get(b, 8);
    v->connection_epoch = sf_get(b + 8, 8);
    v->catalog_generation = sf_get(b + 16, 8);
    v->request_generation = sf_get(b + 24, 8);
    v->candidate_generation = sf_get(b + 32, 8);
    v->presentation_epoch = sf_get(b + 40, 8);
    v->activation = sf_get(b + 48, 8);
    v->slot = (uint16_t)sf_get(b + 56, 2);
}
int sf_descriptor_take(const uint8_t *b, size_t n, struct sophia_sf_record *r)
{
    if (sf_descriptor_object_kind(r->header.kind))
        return sf_descriptor_object_take(b, n, r);
    if (sf_descriptor_candidate_kind(r->header.kind))
        return sf_descriptor_candidate_take(b, n, r);
    /* Public decode owns a temporary record and validates it before assignment. */
    if (n != sf_descriptor_size(r))
        return -1;
    switch (r->header.kind) {
    case SOPHIA_SF_DESCRIPTOR_OUTCOME: {
        struct sophia_sf_descriptor_outcome *v = &r->value.descriptor_outcome;
        v->transaction = sf_get(b, 8);
        v->connection_epoch = sf_get(b + 8, 8);
        v->candidate_generation = sf_get(b + 16, 8);
        v->presentation_epoch = sf_get(b + 24, 8);
        v->kind = (uint16_t)sf_get(b + 32, 2);
        return sf_zero(b + 34, 2) ? 0 : -1;
    }
    case SOPHIA_SF_DESCRIPTOR_ACTIVATION: {
        struct sophia_sf_descriptor_activation *v = &r->value.descriptor_activation;
        v->transaction = sf_get(b, 8);
        v->connection_epoch = sf_get(b + 8, 8);
        v->candidate_generation = sf_get(b + 16, 8);
        v->presentation_epoch = sf_get(b + 24, 8);
        v->activation = sf_get(b + 32, 8);
        v->action_token = sf_get(b + 40, 8);
        v->action_issuer_epoch = sf_get(b + 48, 8);
        v->action_issuer_revocation_epoch = sf_get(b + 56, 8);
        v->action_recipient_epoch = sf_get(b + 64, 8);
        v->action_target_slot = (uint16_t)sf_get(b + 72, 2);
        v->action_target_generation = sf_get(b + 76, 8);
        return sf_zero(b + 74, 2) ? 0 : -1;
    }
    case SOPHIA_SF_REFERENCE_REQUEST:
        return request_take(b, &r->value.reference_request);
    case SOPHIA_SF_REFERENCE_OUTCOME: {
        struct sophia_sf_reference_outcome *v = &r->value.reference_outcome;
        v->transaction = sf_get(b, 8);
        v->connection_epoch = sf_get(b + 8, 8);
        v->catalog_generation = sf_get(b + 16, 8);
        v->request_generation = sf_get(b + 24, 8);
        v->candidate_generation = sf_get(b + 32, 8);
        v->presentation_epoch = sf_get(b + 40, 8);
        v->page = (uint16_t)sf_get(b + 48, 2);
        v->pages = (uint16_t)sf_get(b + 50, 2);
        v->kind = (uint16_t)sf_get(b + 52, 2);
        return sf_zero(b + 54, 2) ? 0 : -1;
    }
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_REQUEST:
        return request_take(b, &r->value.descriptor_launcher_request.request) ||
                       sf_text_take(b + 60, 256, &r->value.descriptor_launcher_request.query)
                   ? -1
                   : 0;
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_OUTCOME: {
        struct sophia_sf_descriptor_launcher_outcome *v = &r->value.descriptor_launcher_outcome;
        v->transaction = sf_get(b, 8);
        v->connection_epoch = sf_get(b + 8, 8);
        v->request_generation = sf_get(b + 16, 8);
        v->candidate_generation = sf_get(b + 24, 8);
        v->presentation_epoch = sf_get(b + 32, 8);
        v->kind = (uint16_t)sf_get(b + 40, 2);
        return sf_zero(b + 42, 2) ? 0 : -1;
    }
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION:
        grant_take(b, &r->value.descriptor_launcher_activation);
        return sf_zero(b + 58, 2) ? 0 : -1;
    case SOPHIA_SF_DESCRIPTOR_LAUNCH_OUTCOME:
        grant_take(b, &r->value.descriptor_launch_outcome.grant);
        r->value.descriptor_launch_outcome.status = (uint16_t)sf_get(b + 58, 2);
        return 0;
    case SOPHIA_SF_DESCRIPTOR_ACTIVATION_ACK: {
        struct sophia_sf_descriptor_activation_ack *v = &r->value.descriptor_activation_ack;
        v->transaction = sf_get(b, 8);
        v->connection_epoch = sf_get(b + 8, 8);
        v->activation = sf_get(b + 16, 8);
        v->disposition = (uint16_t)sf_get(b + 24, 2);
        return sf_zero(b + 26, 2) ? 0 : -1;
    }
    case SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION_ACK:
        grant_take(b, &r->value.descriptor_launcher_activation_ack.grant);
        r->value.descriptor_launcher_activation_ack.consumed = (uint16_t)sf_get(b + 58, 2);
        return 0;
    default:
        return -1;
    }
}

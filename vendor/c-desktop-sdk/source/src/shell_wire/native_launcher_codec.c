#include "../sophia_shell_native_launcher.h"
#include "fields.h"

static struct sophia_shell_native_grant grant(const uint8_t *p)
{
    struct sophia_shell_native_grant v = {shell_get64(p), shell_get64(p+8)};
    return v;
}
static struct sophia_shell_native_id identity(const uint8_t *p)
{
    struct sophia_shell_native_id v = {shell_get64(p), shell_get64(p+8)};
    return v;
}
static struct sophia_shell_native_binding binding(const uint8_t *p)
{
    struct sophia_shell_native_binding v = {
        grant(p), shell_get64(p+16), identity(p+24), identity(p+40),
        shell_get64(p+56), shell_get64(p+64), shell_get64(p+72),
        shell_get64(p+80), shell_get64(p+88), shell_get64(p+96)
    };
    return v;
}
static struct sophia_shell_native_event event(const uint8_t *p)
{
    struct sophia_shell_native_event v = {binding(p), shell_get64(p+104), shell_get64(p+112)};
    return v;
}

int sophia_shell_native_launcher_decode(const struct sophia_shell_frame *frame,
                                       struct sophia_shell_native_message *out)
{
    if (!out) return SOPHIA_SHELL_ARGUMENT;
    int result = sophia_shell_native_launcher_validate(frame);
    if (result != SOPHIA_SHELL_OK) return result;
    const uint8_t *p = frame->payload;
    struct sophia_shell_native_message v = {0};
    v.kind = frame->kind;
    v.transaction = frame->transaction;
    switch (frame->kind) {
    case 187:
        v.value.opening.grant = grant(p);
        v.value.opening.opening = shell_get64(p+16);
        v.value.opening.output = identity(p+24);
        v.value.opening.catalog_generation = shell_get64(p+40);
        v.value.opening.state_revision = shell_get64(p+48);
        break;
    case 191: v.value.focus = binding(p); break;
    case 192:
        v.value.revoked.binding = binding(p);
        v.value.revoked.reason = shell_get16(p+104);
        break;
    case 193:
        v.value.input.event = event(p);
        v.value.input.issued_mono_usec = shell_get64(p+120);
        v.value.input.kind = shell_get16(p+128);
        v.value.input.text_bytes = shell_get16(p+130);
        v.value.input.text = p+132;
        break;
    case 196:
        v.value.outcome.activation.event = event(p);
        v.value.outcome.activation.cause = shell_get16(p+120);
        v.value.outcome.activation.slot = shell_get16(p+122);
        v.value.outcome.status = shell_get16(p+124);
        v.value.outcome.reason = shell_get16(p+126);
        break;
    case 197:
        v.value.closed.grant = grant(p);
        v.value.closed.opening = shell_get64(p+16);
        v.value.closed.reason = shell_get16(p+24);
        break;
    default: return SOPHIA_SHELL_INVALID;
    }
    *out = v;
    return SOPHIA_SHELL_OK;
}

static void put_grant(uint8_t *p, struct sophia_shell_native_grant v)
{
    shell_put64(p, v.connection_epoch);
    shell_put64(p+8, v.content_grant_epoch);
}
static void put_identity(uint8_t *p, struct sophia_shell_native_id v)
{
    shell_put64(p, v.id);
    shell_put64(p+8, v.generation);
}
static void put_event(uint8_t *p, const struct sophia_shell_native_event *v)
{
    const struct sophia_shell_native_binding *b = &v->binding;
    put_grant(p, b->grant);
    shell_put64(p+16, b->opening);
    put_identity(p+24, b->output);
    put_identity(p+40, b->allocation);
    shell_put64(p+56, b->catalog_generation);
    shell_put64(p+64, b->candidate_generation);
    shell_put64(p+72, b->presentation_epoch);
    shell_put64(p+80, b->interaction_generation);
    shell_put64(p+88, b->state_revision);
    shell_put64(p+96, b->focus_lease);
    shell_put64(p+104, v->event_id);
    shell_put64(p+112, v->state_revision);
}
static int encode(uint8_t *dst, size_t capacity, uint16_t kind, uint64_t transaction,
                  const uint8_t *payload, size_t bytes, size_t *frame_bytes)
{
    struct sophia_shell_frame frame = {kind, transaction, payload, bytes};
    int result = sophia_shell_native_launcher_validate(&frame);
    if (result != SOPHIA_SHELL_OK) return result;
    return sophia_shell_frame_encode(dst, capacity, kind, transaction, payload, bytes, frame_bytes);
}
int sophia_shell_native_allocation_encode(uint8_t *dst, size_t capacity,
    uint64_t transaction, const struct sophia_shell_native_allocation *v, size_t *frame_bytes)
{
    if (!v) return SOPHIA_SHELL_ARGUMENT;
    uint8_t p[84] = {0};
    put_grant(p, v->grant);
    shell_put64(p+16, v->opening);
    put_identity(p+24, v->output);
    shell_put64(p+40, v->request_id);
    put_identity(p+48, v->prior);
    shell_put16(p+64, v->operation);
    shell_put16(p+66, v->edge);
    shell_put32(p+68, v->desired_width);
    shell_put32(p+72, v->desired_height);
    for (size_t i=0; i<4; ++i) shell_put16(p+76+2*i, (uint16_t)v->margins[i]);
    return encode(dst, capacity, 188, transaction, p, sizeof(p), frame_bytes);
}
int sophia_shell_native_ack_encode(uint8_t *dst, size_t capacity,
    uint64_t transaction, const struct sophia_shell_native_ack *v, size_t *frame_bytes)
{
    if (!v) return SOPHIA_SHELL_ARGUMENT;
    uint8_t p[124] = {0};
    put_event(p, &v->event);
    shell_put16(p+120, v->disposition);
    return encode(dst, capacity, 194, transaction, p, sizeof(p), frame_bytes);
}
int sophia_shell_native_activation_encode(uint8_t *dst, size_t capacity,
    uint64_t transaction, const struct sophia_shell_native_activation *v, size_t *frame_bytes)
{
    if (!v) return SOPHIA_SHELL_ARGUMENT;
    uint8_t p[124] = {0};
    put_event(p, &v->event);
    shell_put16(p+120, v->cause);
    shell_put16(p+122, v->slot);
    return encode(dst, capacity, 195, transaction, p, sizeof(p), frame_bytes);
}

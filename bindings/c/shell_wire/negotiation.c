#include "fields.h"

static uint64_t revision_mask(uint16_t revision)
{
    static const uint64_t masks[] = {0, 3, 7, 31, 127, 511, 2047};
    return revision < sizeof(masks) / sizeof(*masks) ? masks[revision] : 0;
}

static int dependencies(uint64_t caps)
{
    for (unsigned dependent = 4; dependent <= 10; dependent += 2) {
        if ((caps & (UINT64_C(1) << dependent)) && !(caps & (UINT64_C(1) << (dependent - 1))))
            return 0;
    }
    return 1;
}

static int valid_request(struct sophia_shell_hello hello)
{
    return hello.minimum_revision && hello.minimum_revision <= hello.maximum_revision &&
        hello.maximum_revision <= SOPHIA_SHELL_WIRE_MAX_REVISION &&
        (hello.required_capabilities & SOPHIA_SHELL_CAP_DESCRIPTOR_SWITCHER) &&
        !(hello.required_capabilities & ~revision_mask(hello.maximum_revision)) &&
        dependencies(hello.required_capabilities);
}

int sophia_shell_hello_encode(uint8_t *dst, size_t capacity,
                             struct sophia_shell_hello hello, size_t *frame_bytes)
{
    if (!valid_request(hello))
        return SOPHIA_SHELL_INVALID;
    uint8_t payload[12];
    shell_put16(payload, hello.minimum_revision);
    shell_put16(payload + 2, hello.maximum_revision);
    shell_put64(payload + 4, hello.required_capabilities);
    return sophia_shell_frame_encode(dst, capacity, 96, 0, payload, sizeof(payload), frame_bytes);
}

int sophia_shell_welcome_decode(const struct sophia_shell_frame *frame,
                               struct sophia_shell_hello requested,
                               struct sophia_shell_welcome *welcome)
{
    if (!frame || !welcome || !frame->payload)
        return SOPHIA_SHELL_ARGUMENT;
    if (!valid_request(requested) || frame->kind != 97 || frame->transaction || frame->payload_bytes != 28)
        return SOPHIA_SHELL_INVALID;
    const uint8_t *p = frame->payload;
    struct sophia_shell_welcome value = {
        .revision = shell_get16(p), .connection_epoch = shell_get64(p + 4),
        .capabilities = shell_get64(p + 12), .max_descriptors = shell_get16(p + 20),
        .max_label_bytes = shell_get16(p + 22), .max_pending_activations = shell_get16(p + 24),
    };
    if (shell_get16(p + 2) || shell_get16(p + 26) ||
        value.revision < requested.minimum_revision || value.revision > requested.maximum_revision ||
        !value.connection_epoch || (requested.required_capabilities & ~value.capabilities) ||
        (value.capabilities & ~revision_mask(value.revision)) || !dependencies(value.capabilities) ||
        !value.max_descriptors || value.max_descriptors > 16 ||
        !value.max_label_bytes || value.max_label_bytes > 128 ||
        !value.max_pending_activations || value.max_pending_activations > 16)
        return SOPHIA_SHELL_INVALID;
    *welcome = value;
    return SOPHIA_SHELL_OK;
}

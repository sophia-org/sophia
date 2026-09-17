#include "fields.h"
#include <string.h>

int shell_header(const uint8_t *src, size_t capacity, size_t *total)
{
    if (memcmp(src, "SOPH", 4) || shell_get16(src + 4) != 1 || shell_get32(src + 20))
        return SOPHIA_SHELL_INVALID;
    uint16_t kind = shell_get16(src + 6);
    size_t payload = shell_get32(src + 16);
    if (shell_direction(kind) < 0 || !shell_transaction_valid(kind, shell_get64(src + 8)) ||
        payload > SOPHIA_SHELL_MAX_PAYLOAD_BYTES || capacity < SOPHIA_SHELL_HEADER_BYTES ||
        payload > capacity - SOPHIA_SHELL_HEADER_BYTES)
        return SOPHIA_SHELL_INVALID;
    *total = SOPHIA_SHELL_HEADER_BYTES + payload;
    return SOPHIA_SHELL_OK;
}

int sophia_shell_frame_encode(uint8_t *dst, size_t capacity, uint16_t kind,
                             uint64_t transaction, const void *payload,
                             size_t payload_bytes, size_t *frame_bytes)
{
    if (!dst || !frame_bytes || (!payload && payload_bytes))
        return SOPHIA_SHELL_ARGUMENT;
    if (shell_direction(kind) < 0 || !shell_transaction_valid(kind, transaction) ||
        payload_bytes > SOPHIA_SHELL_MAX_PAYLOAD_BYTES || capacity < SOPHIA_SHELL_HEADER_BYTES ||
        payload_bytes > capacity - SOPHIA_SHELL_HEADER_BYTES)
        return SOPHIA_SHELL_INVALID;
    /* Permit payload aliasing the destination, including its old header. */
    if (payload_bytes)
        memmove(dst + SOPHIA_SHELL_HEADER_BYTES, payload, payload_bytes);
    memcpy(dst, "SOPH", 4);
    shell_put16(dst + 4, 1);
    shell_put16(dst + 6, kind);
    shell_put64(dst + 8, transaction);
    shell_put32(dst + 16, (uint32_t)payload_bytes);
    shell_put32(dst + 20, 0);
    *frame_bytes = SOPHIA_SHELL_HEADER_BYTES + payload_bytes;
    return SOPHIA_SHELL_OK;
}

int sophia_shell_frame_decode(const uint8_t *src, size_t bytes,
                             struct sophia_shell_frame *frame)
{
    if (!src || !frame)
        return SOPHIA_SHELL_ARGUMENT;
    size_t total;
    if (bytes < SOPHIA_SHELL_HEADER_BYTES || shell_header(src, bytes, &total) || total != bytes)
        return SOPHIA_SHELL_INVALID;
    *frame = (struct sophia_shell_frame){shell_get16(src + 6), shell_get64(src + 8),
        src + SOPHIA_SHELL_HEADER_BYTES, total - SOPHIA_SHELL_HEADER_BYTES};
    return SOPHIA_SHELL_OK;
}

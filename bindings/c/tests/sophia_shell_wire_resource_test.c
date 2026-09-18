#include "../sophia_shell_content_resource.h"
#include "../shell_wire/fields.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const struct sophia_shell_resource_key key = {{11,3},{9,1}};
static const uint8_t pixels[] = {0,0,255,255,0,128,0,128};
static unsigned nibble(char c)
{
    if (c >= '0' && c <= '9') return (unsigned)(c-'0');
    if (c >= 'a' && c <= 'f') return (unsigned)(c-'a')+10;
    abort();
}
static int fixture(uint8_t *dst, size_t capacity, uint16_t kind, uint64_t tx, size_t *bytes)
{
    struct sophia_shell_resource_begin begin = {key,2,1,1,1,1,1,8};
    struct sophia_shell_resource_chunk chunk = {key,0,0,pixels,sizeof(pixels)};
    struct sophia_shell_resource_end end = {key,8,1};
    switch (kind) {
    case 165: return sophia_shell_resource_begin_encode(dst, capacity, tx, &begin, bytes);
    case 167: return sophia_shell_resource_chunk_encode(dst, capacity, tx, &chunk, bytes);
    case 168: return sophia_shell_resource_end_encode(dst, capacity, tx, &end, bytes);
    case 169: case 170: return sophia_shell_resource_control_encode(dst, capacity, kind, tx, &key, bytes);
    default: abort();
    }
}
static void check_frame(struct sophia_shell_frame f, const uint8_t *golden, size_t bytes)
{
    uint8_t dst[128], saved[128]; memset(dst, 0xa5, sizeof(dst)); memcpy(saved, dst, sizeof(dst));
    struct sophia_shell_resource_reply reply;
    memset(&reply, 0xa5, sizeof(reply));
    if (f.kind == 166 || f.kind == 171) {
        assert(sophia_shell_resource_reply_decode(&f, &reply) == SOPHIA_SHELL_OK);
        assert(reply.kind == f.kind && reply.transaction == f.transaction);
        assert(reply.key.grant.connection_epoch == 11 && reply.key.grant.content_grant_epoch == 3);
        assert(reply.key.resource.id == 9 && reply.key.resource.generation == 1);
        assert(reply.reason == 0 && reply.next_ordinal == 0);
        assert(reply.status == (f.kind == 166 ? 1 : 0));
        assert(reply.admitted_bytes == (f.kind == 166 ? 8 : 0));
        struct sophia_shell_resource_reply old;
        memcpy(&old, &reply, sizeof(old));
        for (size_t n=0; n<f.payload_bytes; ++n) {
            struct sophia_shell_frame cut = f; cut.payload_bytes = n;
            assert(sophia_shell_resource_reply_decode(&cut, &reply) != SOPHIA_SHELL_OK);
            assert(!memcmp(&old, &reply, sizeof(old)));
        }
        f.transaction = 0;
        assert(sophia_shell_resource_reply_decode(&f, &reply) == SOPHIA_SHELL_INVALID);
        assert(!memcmp(&old, &reply, sizeof(old)));
    } else {
        struct sophia_shell_resource_reply old;
        memcpy(&old, &reply, sizeof(old));
        assert(sophia_shell_resource_reply_decode(&f, &reply) == SOPHIA_SHELL_INVALID);
        assert(!memcmp(&old, &reply, sizeof(old)));
        for (size_t capacity=0; capacity<bytes; ++capacity) {
            size_t n = 999;
            assert(fixture(dst, capacity, f.kind, f.transaction, &n) == SOPHIA_SHELL_INVALID);
            assert(n == 999 && !memcmp(dst, saved, sizeof(dst)));
        }
        size_t n = 999;
        assert(fixture(dst, sizeof(dst), f.kind, 0, &n) == SOPHIA_SHELL_INVALID);
        assert(n == 999 && !memcmp(dst, saved, sizeof(dst)));
        assert(fixture(dst, sizeof(dst), f.kind, f.transaction, &n) == SOPHIA_SHELL_OK);
        assert(n == bytes && !memcmp(dst, golden, bytes));
    }
}
static void begin_bounds(void)
{
    uint8_t dst[128], saved[128]; memset(dst, 0xa5, sizeof(dst)); memcpy(saved, dst, sizeof(dst));
    struct sophia_shell_resource_begin valid = {key,1024,1024,5,4,1,69,4194304};
    for (unsigned mode=0; mode<11; ++mode) {
        struct sophia_shell_resource_begin b = valid;
        switch (mode) {
        case 0: b.width = UINT32_MAX; break;
        case 1: b.height = 0; break;
        case 2: b.height = 1025; b.total_bytes = 4198400; break;
        case 3: b.total_bytes = UINT64_MAX; break;
        case 4: b.scale_numerator = 2; b.scale_denominator = 2; break;
        case 5: b.scale_numerator = 33; break;
        case 6: b.scale_denominator = 0; break;
        case 7: b.chunk_count = 68; break;
        case 8: b.pixel_format = 2; break;
        case 9: b.key.resource.generation = 0; break;
        case 10: b.key.grant.content_grant_epoch = 0; break;
        }
        size_t n = 999;
        assert(sophia_shell_resource_begin_encode(dst, sizeof(dst), 1, &b, &n) == SOPHIA_SHELL_INVALID);
        assert(n == 999 && !memcmp(dst, saved, sizeof(dst)));
    }
    size_t n;
    assert(sophia_shell_resource_begin_encode(dst, sizeof(dst), 1, &valid, &n) == SOPHIA_SHELL_OK);
    assert(n == 88);
}
static void chunk_bounds(void)
{
    static uint8_t data[SOPHIA_SHELL_RESOURCE_MAX_CHUNK_BYTES+1];
    static uint8_t dst[SOPHIA_SHELL_MAX_FRAME_BYTES], saved[SOPHIA_SHELL_MAX_FRAME_BYTES];
    for (size_t i=0; i<sizeof(data); ++i) data[i] = (uint8_t)(i%251);
    memset(dst, 0xa5, sizeof(dst)); memcpy(saved, dst, sizeof(dst));
    struct sophia_shell_resource_chunk valid = {key,17,4194304-65488,data,65488};
    for (unsigned mode=0; mode<5; ++mode) {
        struct sophia_shell_resource_chunk c = valid;
        if (mode == 0) c.byte_count = 65489;
        if (mode == 1) c.offset = UINT64_MAX;
        if (mode == 2) ++c.offset;
        if (mode == 3) c.byte_count = 0;
        if (mode == 4) c.key.grant.connection_epoch = 0;
        size_t n = 999;
        assert(sophia_shell_resource_chunk_encode(dst, sizeof(dst), 1, &c, &n) == SOPHIA_SHELL_INVALID);
        assert(n == 999 && !memcmp(dst, saved, sizeof(dst)));
    }
    size_t n = 999;
    assert(sophia_shell_resource_chunk_encode(dst, sizeof(dst)-1, 1, &valid, &n) == SOPHIA_SHELL_INVALID);
    assert(n == 999 && !memcmp(dst, saved, sizeof(dst)));
    assert(sophia_shell_resource_chunk_encode(dst, sizeof(dst), 1, &valid, &n) == SOPHIA_SHELL_OK);
    assert(n == sizeof(dst));
    struct sophia_shell_frame f;
    assert(sophia_shell_frame_decode(dst, n, &f) == SOPHIA_SHELL_OK);
    assert(f.kind == 167 && f.payload_bytes == 65536);
    assert(shell_get32(f.payload+32) == 17 && shell_get32(f.payload+36) == 65488);
    assert(shell_get64(f.payload+40) == valid.offset && !memcmp(f.payload+48, data, 65488));
}
static void terminal_bounds(void)
{
    uint8_t dst[128], saved[128]; memset(dst, 0xa5, sizeof(dst)); memcpy(saved, dst, sizeof(dst));
    struct sophia_shell_resource_end valid = {key,8,1};
    for (unsigned mode=0; mode<5; ++mode) {
        struct sophia_shell_resource_end end = valid;
        if (mode == 0) end.total_bytes = 0;
        if (mode == 1) end.total_bytes = 4194305;
        if (mode == 2) end.chunk_count = 0;
        if (mode == 3) end.chunk_count = 4097;
        if (mode == 4) end.key.resource.id = 0;
        size_t n = 999;
        assert(sophia_shell_resource_end_encode(dst, sizeof(dst), 1, &end, &n) == SOPHIA_SHELL_INVALID);
        assert(n == 999 && !memcmp(dst, saved, sizeof(dst)));
    }
    size_t n = 999;
    assert(sophia_shell_resource_control_encode(dst, sizeof(dst), 168, 1, &key, &n) == SOPHIA_SHELL_INVALID);
    struct sophia_shell_resource_key invalid = key; invalid.resource.generation = 0;
    assert(sophia_shell_resource_control_encode(dst, sizeof(dst), 170, 1, &invalid, &n) == SOPHIA_SHELL_INVALID);
    assert(n == 999 && !memcmp(dst, saved, sizeof(dst)));
}
static void reply_bounds(void)
{
    uint8_t p[48] = {0};
    for (size_t i=0; i<4; ++i) shell_put64(p+8*i, i+1);
    shell_put16(p+32, 1);
    struct sophia_shell_frame f = {166,7,p,sizeof(p)};
    struct sophia_shell_resource_reply out;
    memset(&out, 0xa5, sizeof(out));
    struct sophia_shell_resource_reply saved;
    memcpy(&saved, &out, sizeof(saved));
    for (unsigned mode=0; mode<7; ++mode) {
        uint8_t bad[48]; memcpy(bad, p, sizeof(bad));
        if (mode < 4) shell_put64(bad+8*mode, 0);
        if (mode == 4) shell_put16(bad+32, 5);
        if (mode == 5) shell_put16(bad+34, 1);
        if (mode == 6) shell_put64(bad+40, 4194305);
        f.payload = bad;
        assert(sophia_shell_resource_reply_decode(&f, &out) == SOPHIA_SHELL_INVALID);
        assert(!memcmp(&out, &saved, sizeof(out)));
    }
    f.payload = p;
    shell_put16(p+32, 3); shell_put16(p+34, 12);
    assert(sophia_shell_resource_reply_decode(&f, &out) == SOPHIA_SHELL_OK);
    assert(out.status == 3 && out.reason == 12);
    f.kind = 171; f.payload_bytes = 34; shell_put16(p+32, 13);
    memcpy(&saved, &out, sizeof(saved));
    assert(sophia_shell_resource_reply_decode(&f, &out) == SOPHIA_SHELL_INVALID);
    assert(!memcmp(&out, &saved, sizeof(out)));
}
int main(int argc, char **argv)
{
    assert(argc == 2);
    FILE *file = fopen(argv[1], "r"); assert(file);
    char line[4096]; uint8_t bytes[2048]; unsigned count = 0;
    while (fgets(line, sizeof(line), file)) {
        char *hex = strchr(line, ' '); assert(hex); ++hex;
        size_t n = strcspn(hex, "\r\n"); assert(n%2 == 0 && n/2 <= sizeof(bytes));
        for (size_t i=0; i<n/2; ++i) bytes[i] = (uint8_t)(16*nibble(hex[2*i])+nibble(hex[2*i+1]));
        struct sophia_shell_frame f;
        assert(sophia_shell_frame_decode(bytes, n/2, &f) == SOPHIA_SHELL_OK);
        if (f.kind < 165 || f.kind > 171) continue;
        check_frame(f, bytes, n/2); ++count;
    }
    assert(!ferror(file) && count == 7); fclose(file);
    begin_bounds(); chunk_bounds(); terminal_bounds(); reply_bounds();
    puts("sophia_shell_resource_codec kinds=7 golden=pass lifecycle=not_run");
    return 0;
}

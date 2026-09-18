#include "../sophia_shell_native_launcher.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static unsigned nibble(char c)
{
    if (c >= '0' && c <= '9') return (unsigned)(c-'0');
    if (c >= 'a' && c <= 'f') return (unsigned)(c-'a')+10;
    abort();
}
static void check_binding(const struct sophia_shell_native_binding *b)
{
    assert(b->grant.connection_epoch == 2 && b->grant.content_grant_epoch == 3);
    assert(b->opening == 4 && b->output.id == 5 && b->output.generation == 6);
    assert(b->allocation.id == 7 && b->allocation.generation == 8);
    assert(b->catalog_generation == 9 && b->candidate_generation == 10);
    assert(b->presentation_epoch == 11 && b->interaction_generation == 12);
    assert(b->state_revision == 13 && b->focus_lease == 14);
}
static struct sophia_shell_native_event fixture_event(uint64_t revision)
{
    struct sophia_shell_native_event e = {
        {{2,3},4,{5,6},{7,8},9,10,11,12,13,14},15,revision
    };
    return e;
}
static void inbound(const struct sophia_shell_frame *f)
{
    struct sophia_shell_native_message out;
    memset(&out, 0xa5, sizeof(out));
    struct sophia_shell_native_message saved;
    memcpy(&saved, &out, sizeof(saved));
    int result = sophia_shell_native_launcher_decode(f, &out);
    if (f->kind == 188 || f->kind == 189 || f->kind == 190 || f->kind == 194 || f->kind == 195) {
        assert(result == SOPHIA_SHELL_INVALID && !memcmp(&out, &saved, sizeof(out)));
        return;
    }
    assert(result == SOPHIA_SHELL_OK && out.kind == f->kind && out.transaction == 25);
    switch (out.kind) {
    case 187:
        assert(out.value.opening.grant.connection_epoch == 2);
        assert(out.value.opening.grant.content_grant_epoch == 3);
        assert(out.value.opening.opening == 4 && out.value.opening.output.id == 5);
        assert(out.value.opening.output.generation == 6);
        assert(out.value.opening.catalog_generation == 9 && out.value.opening.state_revision == 1);
        break;
    case 191: check_binding(&out.value.focus); break;
    case 192:
        check_binding(&out.value.revoked.binding);
        assert(out.value.revoked.reason == 12);
        break;
    case 193:
        check_binding(&out.value.input.event.binding);
        assert(out.value.input.event.event_id == 15 && out.value.input.event.state_revision == 14);
        assert(out.value.input.issued_mono_usec == 24 && out.value.input.kind == 1);
        assert(out.value.input.text_bytes == 12 && out.value.input.text == f->payload+132);
        assert(!memcmp(out.value.input.text, "caf\xc3\xa9 \xe6\x9d\xb1\xe4\xba\xac", 12));
        break;
    case 196:
        check_binding(&out.value.outcome.activation.event.binding);
        assert(out.value.outcome.activation.event.event_id == 15);
        assert(out.value.outcome.activation.event.state_revision == 13);
        assert(out.value.outcome.activation.cause == 1 && out.value.outcome.activation.slot == 2);
        assert(out.value.outcome.status == 1 && out.value.outcome.reason == 0);
        break;
    case 197:
        assert(out.value.closed.grant.connection_epoch == 2 && out.value.closed.grant.content_grant_epoch == 3);
        assert(out.value.closed.opening == 4 && out.value.closed.reason == 11);
        break;
    default: abort();
    }
    /* Every truncation refuses without replacing the caller's previous record. */
    memcpy(&saved, &out, sizeof(saved));
    for (size_t n=0; n<f->payload_bytes; ++n) {
        struct sophia_shell_frame cut = *f;
        cut.payload_bytes = n;
        assert(sophia_shell_native_launcher_decode(&cut, &out) != SOPHIA_SHELL_OK);
        assert(!memcmp(&out, &saved, sizeof(out)));
    }
}
static int encode_fixture(uint16_t kind, uint8_t *dst, size_t capacity, size_t *bytes)
{
    struct sophia_shell_native_allocation a = {{2,3},4,{5,6},16,{0,0},1,1,640,240,{0,0,0,0}};
    struct sophia_shell_native_ack ack = {fixture_event(14),1};
    struct sophia_shell_native_activation act = {fixture_event(13),1,2};
    struct sophia_shell_native_candidate candidate = {{2,3},10,{5,6},17,18,12,1,4,9,13,2,1,{2}};
    struct sophia_shell_native_chunk chunk = {0};
    chunk.grant = a.grant; chunk.candidate_generation = 10;
    chunk.surface_count = chunk.placement_count = chunk.target_count = 1;
    chunk.surface.allocation = (struct sophia_shell_native_id){7,8};
    chunk.surface.scale_generation = 19; chunk.surface.edge = 1;
    chunk.placements[0].resource = (struct sophia_shell_native_id){20,21};
    chunk.targets[0] = (struct sophia_shell_native_target){22,23,2,0,32,640,24};
    switch (kind) {
    case 188: return sophia_shell_native_allocation_encode(dst, capacity, 25, &a, bytes);
    case 189: return sophia_shell_native_candidate_encode(dst, capacity, 25, &candidate, bytes);
    case 190: return sophia_shell_native_chunk_encode(dst, capacity, 25, &chunk, bytes);
    case 194: return sophia_shell_native_ack_encode(dst, capacity, 25, &ack, bytes);
    case 195: return sophia_shell_native_activation_encode(dst, capacity, 25, &act, bytes);
    default: abort();
    }
}
static void outbound(const struct sophia_shell_frame *f, const uint8_t *golden, size_t bytes)
{
    uint8_t encoded[256], saved[256];
    memset(encoded, 0x5a, sizeof(encoded));
    memcpy(saved, encoded, sizeof(saved));
    if (f->kind != 188 && f->kind != 189 && f->kind != 190 && f->kind != 194 && f->kind != 195) return;
    for (size_t capacity=0; capacity<bytes; ++capacity) {
        size_t count = 999;
        int result = encode_fixture(f->kind, encoded, capacity, &count);
        assert(result == SOPHIA_SHELL_INVALID && count == 999 && !memcmp(encoded, saved, sizeof(saved)));
    }
    size_t count = 0;
    assert(encode_fixture(f->kind, encoded, sizeof(encoded), &count) == SOPHIA_SHELL_OK);
    assert(count == bytes && !memcmp(encoded, golden, bytes));
}
static void refusal(void)
{
    uint8_t out[256], saved[256];
    memset(out, 0xa5, sizeof(out)); memcpy(saved, out, sizeof(out));
    size_t n = 99;
    struct sophia_shell_native_ack ack = {fixture_event(14),1};
    assert(sophia_shell_native_ack_encode(out, sizeof(out), 0, &ack, &n) == SOPHIA_SHELL_INVALID);
    ack.disposition = 3;
    assert(sophia_shell_native_ack_encode(out, sizeof(out), 25, &ack, &n) == SOPHIA_SHELL_INVALID);
    struct sophia_shell_native_activation a = {fixture_event(14),1,2};
    assert(sophia_shell_native_activation_encode(out, sizeof(out), 25, &a, &n) == SOPHIA_SHELL_INVALID);
    a.event.state_revision = 13; a.slot = 0;
    assert(sophia_shell_native_activation_encode(out, sizeof(out), 25, &a, &n) == SOPHIA_SHELL_INVALID);
    struct sophia_shell_native_allocation alloc = {{2,3},4,{5,6},16,{0,0},1,1,640,240,{0,0,0,513}};
    assert(sophia_shell_native_allocation_encode(out, sizeof(out), 25, &alloc, &n) == SOPHIA_SHELL_INVALID);
    assert(n == 99 && !memcmp(out, saved, sizeof(out)));
    alloc.margins[3] = -512;
    assert(sophia_shell_native_allocation_encode(out, sizeof(out), 25, &alloc, &n) == SOPHIA_SHELL_OK);
    assert(out[24+82] == 0 && out[24+83] == 0xfe);
}
static void content_bounds(void)
{
    /* Fixed test storage must not inflate the optimized caller stack. */
    static uint8_t out[4096], saved[4096];
    memset(out, 0xa5, sizeof(out)); memcpy(saved, out, sizeof(out));
    size_t n = 99;
    struct sophia_shell_native_candidate c = {{2,3},10,{5,6},17,18,12,1,4,9,13,2,33,{2}};
    assert(sophia_shell_native_candidate_encode(out, sizeof(out), 25, &c, &n) == SOPHIA_SHELL_INVALID);
    c.row_count = 2; c.rows[1] = 2;
    assert(sophia_shell_native_candidate_encode(out, sizeof(out), 25, &c, &n) == SOPHIA_SHELL_INVALID);
    c.rows[1] = 3; c.selected = 4;
    assert(sophia_shell_native_candidate_encode(out, sizeof(out), 25, &c, &n) == SOPHIA_SHELL_INVALID);
    struct sophia_shell_native_chunk chunk = {0};
    chunk.grant = c.grant; chunk.candidate_generation = 10; chunk.target_count = 33;
    assert(sophia_shell_native_chunk_encode(out, sizeof(out), 25, &chunk, &n) == SOPHIA_SHELL_INVALID);
    chunk.target_count = 1;
    chunk.targets[0] = (struct sophia_shell_native_target){22,23,2,-1,32,640,24};
    assert(sophia_shell_native_chunk_encode(out, sizeof(out), 25, &chunk, &n) == SOPHIA_SHELL_INVALID);
    assert(n == 99 && !memcmp(out, saved, sizeof(out)));
    c.row_count = 32; c.selected = 32;
    for (unsigned i=0; i<32; ++i) c.rows[i] = (uint16_t)(i+1);
    assert(sophia_shell_native_candidate_encode(out, sizeof(out), 25, &c, &n) == SOPHIA_SHELL_OK);
    assert(n == 24+108+64);
    c.row_count = 0; c.selected = 0;
    assert(sophia_shell_native_candidate_encode(out, sizeof(out), 25, &c, &n) == SOPHIA_SHELL_OK);
    chunk.target_count = 32;
    for (unsigned i=0; i<32; ++i)
        chunk.targets[i] = (struct sophia_shell_native_target){22+i,23,(uint16_t)(i+1),0,(int32_t)(24*i),640,24};
    assert(sophia_shell_native_chunk_encode(out, sizeof(out), 25, &chunk, &n) == SOPHIA_SHELL_OK);
    assert(n == 24+40+48*32);
    chunk.surface_count = 1;
    chunk.surface.allocation = (struct sophia_shell_native_id){7,8};
    chunk.surface.scale_generation = 19; chunk.surface.edge = 1;
    chunk.placement_count = 32;
    for (unsigned i=0; i<32; ++i)
        chunk.placements[i].resource = (struct sophia_shell_native_id){20+i,21};
    assert(sophia_shell_native_chunk_encode(out, sizeof(out), 25, &chunk, &n) == SOPHIA_SHELL_OK);
    assert(n == 24+40+64+80*32);
    memcpy(saved, out, sizeof(out));
    size_t written = 99;
    assert(sophia_shell_native_chunk_encode(out, n-1, 25, &chunk, &written) == SOPHIA_SHELL_INVALID);
    assert(written == 99 && !memcmp(out, saved, sizeof(out)));
}
int main(int argc, char **argv)
{
    assert(argc == 2);
    FILE *file = fopen(argv[1], "r"); assert(file);
    char line[4096]; uint8_t bytes[2048]; unsigned count = 0;
    while (fgets(line, sizeof(line), file)) {
        char *hex = strchr(line, ' '); assert(hex); ++hex;
        size_t size = strcspn(hex, "\r\n"); assert(size%2 == 0 && size/2 <= sizeof(bytes));
        for (size_t i=0; i<size/2; ++i) bytes[i] = (uint8_t)(16*nibble(hex[2*i])+nibble(hex[2*i+1]));
        struct sophia_shell_frame f;
        assert(sophia_shell_frame_decode(bytes, size/2, &f) == SOPHIA_SHELL_OK);
        inbound(&f); outbound(&f, bytes, size/2); ++count;
    }
    assert(!ferror(file) && count == 11); fclose(file);
    refusal(); content_bounds();
    puts("sophia_shell_native_codec inbound=6 outbound=5 golden=pass lifecycle=not_run");
    return 0;
}

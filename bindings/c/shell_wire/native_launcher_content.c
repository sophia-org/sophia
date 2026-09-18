#include "../sophia_shell_native_launcher.h"
#include "fields.h"

static void put_identity(uint8_t *p, struct sophia_shell_native_id v)
{
    shell_put64(p, v.id);
    shell_put64(p+8, v.generation);
}
static void put_grant(uint8_t *p, struct sophia_shell_native_grant v)
{
    shell_put64(p, v.connection_epoch);
    shell_put64(p+8, v.content_grant_epoch);
}
static int encode(uint8_t *dst, size_t capacity, uint16_t kind, uint64_t tx,
                  const uint8_t *p, size_t bytes, size_t *frame_bytes)
{
    struct sophia_shell_frame f = {kind, tx, p, bytes};
    int r = sophia_shell_native_launcher_validate(&f);
    if (r != SOPHIA_SHELL_OK) return r;
    return sophia_shell_frame_encode(dst, capacity, kind, tx, p, bytes, frame_bytes);
}
int sophia_shell_native_candidate_encode(uint8_t *dst, size_t capacity,
    uint64_t tx, const struct sophia_shell_native_candidate *v, size_t *frame_bytes)
{
    if (!v) return SOPHIA_SHELL_ARGUMENT;
    if (v->row_count > SOPHIA_SHELL_NATIVE_MAX_ROWS) return SOPHIA_SHELL_INVALID;
    uint8_t p[108+2*SOPHIA_SHELL_NATIVE_MAX_ROWS] = {0};
    put_grant(p, v->grant);
    shell_put64(p+16, v->candidate_generation);
    put_identity(p+24, v->output);
    shell_put64(p+40, v->facts_generation);
    shell_put64(p+48, v->pacing_permit);
    shell_put64(p+56, v->interaction_generation);
    shell_put32(p+64, 1);
    shell_put32(p+68, v->placement_count);
    shell_put32(p+72, v->row_count);
    shell_put64(p+80, v->opening);
    shell_put64(p+88, v->catalog_generation);
    shell_put64(p+96, v->state_revision);
    shell_put16(p+104, v->selected);
    shell_put16(p+106, v->row_count);
    for (size_t i=0; i<v->row_count; ++i) shell_put16(p+108+2*i, v->rows[i]);
    return encode(dst, capacity, 189, tx, p, 108+2u*v->row_count, frame_bytes);
}
int sophia_shell_native_chunk_encode(uint8_t *dst, size_t capacity,
    uint64_t tx, const struct sophia_shell_native_chunk *v, size_t *frame_bytes)
{
    if (!v) return SOPHIA_SHELL_ARGUMENT;
    if (v->surface_count > 1 || v->placement_count > SOPHIA_SHELL_NATIVE_MAX_ROWS ||
        v->target_count > SOPHIA_SHELL_NATIVE_MAX_ROWS) return SOPHIA_SHELL_INVALID;
    uint8_t p[40+64+80*SOPHIA_SHELL_NATIVE_MAX_ROWS] = {0};
    put_grant(p, v->grant);
    shell_put64(p+16, v->candidate_generation);
    shell_put32(p+24, v->ordinal);
    shell_put32(p+28, v->surface_count);
    shell_put32(p+32, v->placement_count);
    shell_put32(p+36, v->target_count);
    size_t at = 40;
    if (v->surface_count) {
        put_identity(p+at, v->surface.allocation);
        shell_put64(p+at+16, v->surface.scale_generation);
        shell_put16(p+at+24, 3);
        shell_put16(p+at+26, v->surface.edge);
        for (size_t i=0; i<4; ++i) shell_put16(p+at+28+2*i, (uint16_t)v->surface.margins[i]);
        shell_put16(p+at+40, UINT16_MAX);
        at += 64;
    }
    for (size_t i=0; i<v->placement_count; ++i, at+=32) {
        put_identity(p+at, v->placements[i].resource);
        shell_put32(p+at+20, (uint32_t)v->placements[i].x);
        shell_put32(p+at+24, (uint32_t)v->placements[i].y);
    }
    for (size_t i=0; i<v->target_count; ++i, at+=48) {
        shell_put16(p+at+2, 2);
        shell_put64(p+at+4, v->targets[i].id);
        shell_put64(p+at+12, v->targets[i].generation);
        shell_put64(p+at+20, v->targets[i].slot);
        shell_put32(p+at+28, (uint32_t)v->targets[i].x);
        shell_put32(p+at+32, (uint32_t)v->targets[i].y);
        shell_put32(p+at+36, v->targets[i].width);
        shell_put32(p+at+40, v->targets[i].height);
    }
    return encode(dst, capacity, 190, tx, p, at, frame_bytes);
}

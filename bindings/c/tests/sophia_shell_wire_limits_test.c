#include "../sophia_shell_content_limits.h"
#include "../shell_wire/fields.h"
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
static void invalid(struct sophia_shell_frame f)
{
    struct sophia_shell_content_limits out, old;
    memset(&out, 0xa5, sizeof(out)); memcpy(&old, &out, sizeof(old));
    assert(sophia_shell_content_limits_decode(&f, &out) == SOPHIA_SHELL_INVALID);
    assert(!memcmp(&out, &old, sizeof(out)));
}
static void limits(struct sophia_shell_frame f)
{
    struct sophia_shell_content_limits out;
    assert(sophia_shell_content_limits_decode(&f, &out) == SOPHIA_SHELL_OK);
    assert(out.grant.connection_epoch == 11 && out.grant.content_grant_epoch == 3);
    assert(out.limits_generation == 1 && out.max_resource_bytes == 4194304);
    assert(out.max_chunk_bytes == 65488 && out.max_width_px == 8192);
    assert(out.max_candidate_targets == 64 && out.max_candidate_bytes == 8192);
    assert(out.max_candidate_rate_millihz == 120000 && out.permit_timeout_ms == 250);
    for (size_t n=0; n<264; ++n) {
        struct sophia_shell_frame cut = f; cut.payload_bytes = n; invalid(cut);
    }
    struct sophia_shell_frame bad = f;
    bad.payload_bytes = 265; invalid(bad);
    bad = f; bad.transaction = 1; invalid(bad);
    bad = f; bad.kind = 162; invalid(bad);
    uint8_t p[264]; bad = f; bad.payload = p;
    /* Every advertised cap is independently exceeded, including reserved bits. */
    for (size_t off=24; off<=260; off += off<80 ? 8 : 4) {
        memcpy(p, f.payload, sizeof(p));
        if (off<80) shell_put64(p+off, shell_get64(p+off)+1);
        else shell_put32(p+off, shell_get32(p+off)+1);
        invalid(bad);
    }
    for (size_t off=0; off<=16; off+=8) {
        memcpy(p, f.payload, sizeof(p)); shell_put64(p+off, 0); invalid(bad);
    }
    /* Individually legal values which make the aggregate profile incoherent. */
    static const struct {size_t offset; uint32_t value;} contradictions[] = {
        {100,63},{112,3},{148,0},{156,0},{140,39},{180,65559},
        {184,65535},{176,1023},{84,32767},{80,65535},{192,511},
        {224,499},{88,0},{92,0},{104,0},{96,0},{108,0},{116,0},
        {128,0},{132,0},{152,0},{160,0},{212,0},{216,0},{256,0},
        {188,0},{168,0},{172,0},{220,0},{224,0},{228,0},{232,0},
        {236,0},{240,0},{244,0},{248,0},{252,0}
    };
    for (size_t i=0; i<sizeof(contradictions)/sizeof(contradictions[0]); ++i) {
        memcpy(p, f.payload, sizeof(p));
        shell_put32(p+contradictions[i].offset, contradictions[i].value); invalid(bad);
    }
    for (size_t off=32; off<=48; off+=8) {
        memcpy(p, f.payload, sizeof(p)); shell_put64(p+off, 4194303); invalid(bad);
    }
    memcpy(p, f.payload, sizeof(p)); shell_put64(p+24, 0); invalid(bad);
    memcpy(p, f.payload, sizeof(p)); shell_put64(p+64, 0); invalid(bad);
    memcpy(p, f.payload, sizeof(p)); shell_put64(p+56, 41943039); invalid(bad);
    /* Native profile may tighten budgets and disable optional facilities. */
    memcpy(p, f.payload, sizeof(p));
    shell_put64(p+16, UINT64_MAX);
    shell_put64(p+32, 4194304); shell_put64(p+40, 12582912); shell_put64(p+48, 8388608);
    shell_put64(p+56, 25165824);
    shell_put32(p+120, 0); shell_put32(p+124, 0); shell_put32(p+136, 0);
    shell_put32(p+144, 0); shell_put32(p+164, 0);
    assert(sophia_shell_content_limits_decode(&bad, &out) == SOPHIA_SHELL_OK);
    assert(out.limits_generation == UINT64_MAX && out.max_resident_bytes == 12582912);
    assert(!out.max_candidate_targets && !out.max_pending_actions);
}
static void refusal(struct sophia_shell_frame f)
{
    struct sophia_shell_content_refusal out, old;
    assert(sophia_shell_content_refusal_decode(&f, &out) == SOPHIA_SHELL_OK);
    assert(out.reason == 1 && out.denied_capabilities == 128);
    memcpy(&old, &out, sizeof(old));
    for (unsigned mode=0; mode<19; ++mode) {
        uint8_t p[12]; memcpy(p, f.payload, sizeof(p));
        struct sophia_shell_frame bad = f; bad.payload = p;
        if (mode<12) bad.payload_bytes = mode;
        if (mode==12) bad.payload_bytes = 13;
        if (mode==13) bad.transaction = 1;
        if (mode==14) bad.kind = 161;
        if (mode==15) shell_put16(p, 0);
        if (mode==16) shell_put16(p, 5);
        if (mode==17) shell_put16(p+2, 1);
        if (mode==18) shell_put64(p+4, 0);
        assert(sophia_shell_content_refusal_decode(&bad, &out) == SOPHIA_SHELL_INVALID);
        assert(!memcmp(&out, &old, sizeof(out)));
    }
}
int main(int argc, char **argv)
{
    assert(argc==2); FILE *file = fopen(argv[1], "r"); assert(file);
    char line[4096]; uint8_t bytes[2048]; unsigned count=0;
    while (fgets(line, sizeof(line), file)) {
        char *hex = strchr(line, ' '); assert(hex); ++hex;
        size_t n = strcspn(hex, "\r\n"); assert(n%2==0 && n/2<=sizeof(bytes));
        for (size_t i=0; i<n/2; ++i) bytes[i]=(uint8_t)(16*nibble(hex[2*i])+nibble(hex[2*i+1]));
        struct sophia_shell_frame f;
        assert(sophia_shell_frame_decode(bytes, n/2, &f)==SOPHIA_SHELL_OK);
        if (f.kind==160) {refusal(f); ++count;}
        if (f.kind==161) {limits(f); ++count;}
    }
    assert(!ferror(file) && count==2); fclose(file);
    puts("sophia_shell_content_limits golden=pass coherence=pass lifecycle=not_run");
    return 0;
}

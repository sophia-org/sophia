/* Submitted receipts naming ImageBegin (0x7000): admitted only by the
 * explicit revision entry points at shell r9 and lock r2. The plain codecs
 * and every earlier revision keep refusing the kind, and the receipt keeps
 * the real kind on the wire. */
#include "sophia_lock_files.h"
#include "sophia_shell_files.h"
#undef NDEBUG
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#define BODY 32u
#define SUBMITTED_BYTES 48u

static struct sophia_sf_record shell_receipt(uint16_t kind)
{
    struct sophia_sf_record r;
    memset(&r, 0, sizeof r);
    r.header.kind = SOPHIA_SF_SUBMITTED;
    r.header.epoch = 5, r.header.sequence = 9;
    r.value.submitted.submission_id = 7;
    r.value.submitted.candidate_kind = kind;
    return r;
}

static struct sophia_lf_record lock_receipt(uint16_t kind)
{
    struct sophia_lf_record r;
    memset(&r, 0, sizeof r);
    r.header.kind = SOPHIA_LF_SUBMITTED;
    r.header.epoch = 5, r.header.sequence = 9;
    r.value.submitted.submission = 7;
    r.value.submitted.kind = kind;
    return r;
}

static void put16(uint8_t *p, unsigned v)
{
    p[0] = (uint8_t)v, p[1] = (uint8_t)(v >> 8);
}

/* A receipt with trailing bytes is refused at the receipt revision, whether
 * or not its length field counts them. Sizes reach past every internal
 * staging copy. */
static void over_length_shell(const uint8_t *b, size_t n)
{
    static uint8_t big[4096];
    struct sophia_sf_record back;
    size_t extra;
    for (extra = 1; extra <= sizeof big - n; extra = extra * 2 + 1) {
        memset(big, 0, sizeof big);
        memcpy(big, b, n);
        big[0] = (uint8_t)(n + extra), big[1] = (uint8_t)((n + extra) >> 8);
        assert(sophia_sf_decode_revision(big, n + extra, 9, &back) == -1);
        /* Trailing bytes behind an unchanged 48-byte length field. */
        big[0] = (uint8_t)n, big[1] = 0;
        assert(sophia_sf_decode_revision(big, n + extra, 9, &back) == -1);
    }
}

static void over_length_lock(const uint8_t *b, size_t n)
{
    static uint8_t big[4096];
    struct sophia_lf_record back;
    size_t extra;
    for (extra = 1; extra <= sizeof big - n; extra = extra * 2 + 1) {
        memset(big, 0, sizeof big);
        memcpy(big, b, n);
        big[0] = (uint8_t)(n + extra), big[1] = (uint8_t)((n + extra) >> 8);
        assert(sophia_lf_decode_revision(big, n + extra, 2, &back) == -1);
        /* Trailing bytes behind an unchanged 48-byte length field. */
        big[0] = (uint8_t)n, big[1] = 0;
        assert(sophia_lf_decode_revision(big, n + extra, 2, &back) == -1);
    }
}

/* Any other 48-byte record carrying 0x7000 at the receipt's kind offset
 * decodes at the receipt revision exactly as the plain codec decodes it. */
static void raw48(uint8_t *b, unsigned kind, int candidate, unsigned head, uint64_t at8)
{
    size_t i;
    memset(b, 0, SUBMITTED_BYTES);
    b[0] = SUBMITTED_BYTES, b[4] = 1;
    put16(b + 6, kind);
    b[8] = 5;
    b[candidate ? 16 : 24] = 9;
    put16(b + BODY, head);
    for (i = 0; i < 8; i++)
        b[BODY + 8 + i] = (uint8_t)(at8 >> (8 * i));
}

static void shell_same_as_plain(unsigned kind, int candidate, unsigned head)
{
    uint8_t b[SUBMITTED_BYTES];
    struct sophia_sf_record plain, revised;
    int a, c;
    raw48(b, kind, candidate, head, SOPHIA_SF_KIND_IMAGE_BEGIN);
    memset(&plain, 0, sizeof plain), memset(&revised, 0, sizeof revised);
    a = sophia_sf_decode(b, sizeof b, &plain);
    c = sophia_sf_decode_revision(b, sizeof b, 9, &revised);
    assert(a == c && (a || !memcmp(&plain, &revised, sizeof plain)));
}

static void lock_same_as_plain(unsigned kind, int candidate, unsigned head)
{
    uint8_t b[SUBMITTED_BYTES];
    struct sophia_lf_record plain, revised;
    int a, c;
    raw48(b, kind, candidate, head, SOPHIA_LF_KIND_IMAGE_BEGIN);
    memset(&plain, 0, sizeof plain), memset(&revised, 0, sizeof revised);
    a = sophia_lf_decode(b, sizeof b, &plain);
    c = sophia_lf_decode_revision(b, sizeof b, 2, &revised);
    assert(a == c && (a || !memcmp(&plain, &revised, sizeof plain)));
}

static void other_kinds(void)
{
    shell_same_as_plain(SOPHIA_SF_NEGOTIATE, 1, 0x0101);  /* r1..r1, caps 0x7000 */
    shell_same_as_plain(SOPHIA_SF_NEGOTIATE, 1, 0x0901);
    shell_same_as_plain(SOPHIA_SF_REFUSED, 0, 1);         /* denied caps 0x7000 */
    lock_same_as_plain(SOPHIA_LF_NEGOTIATE, 1, 0x0101);
    lock_same_as_plain(SOPHIA_LF_NEGOTIATED, 0, 1);
    lock_same_as_plain(SOPHIA_LF_ENTRY, 0, 1);
    lock_same_as_plain(SOPHIA_LF_CHORD, 0, 1);
}

/* Legacy kinds behave identically through every entry point. */
static void shell_legacy(void)
{
    uint8_t plain[64], b[64];
    size_t n, m;
    unsigned kind, revision;
    for (kind = 256; kind <= 278; kind++) {
        struct sophia_sf_record r = shell_receipt((uint16_t)kind), back;
        assert(sophia_sf_encode(plain, sizeof plain, &r, &n) == 0 && n == SUBMITTED_BYTES);
        for (revision = 1; revision <= 9; revision++) {
            assert(sophia_sf_encode_revision(b, sizeof b, (uint16_t)revision, &r, &m) == 0);
            assert(m == n && !memcmp(b, plain, n));
            assert(sophia_sf_decode_revision(b, m, (uint16_t)revision, &back) == 0);
            assert(back.value.submitted.candidate_kind == kind);
        }
    }
}

static void shell_image_begin(void)
{
    struct sophia_sf_record r = shell_receipt(SOPHIA_SF_KIND_IMAGE_BEGIN), back, sentinel;
    uint8_t b[64], bad[64];
    size_t n = 0, m;
    unsigned revision, kind;
    memset(&sentinel, 0xa5, sizeof sentinel);
    assert(sophia_sf_encode(b, sizeof b, &r, &n) == -1);
    for (revision = 1; revision < 9; revision++)
        assert(sophia_sf_encode_revision(b, sizeof b, (uint16_t)revision, &r, &n) == -1);
    assert(sophia_sf_encode_revision(b, sizeof b, 9, &r, &n) == 0 && n == SUBMITTED_BYTES);
    assert(b[6] == SOPHIA_SF_SUBMITTED && b[7] == 0);
    assert(b[BODY + 8] == 0x00 && b[BODY + 9] == 0x70 && b[BODY] == 7);
    back = sentinel;
    assert(sophia_sf_decode(b, n, &back) == -1 && !memcmp(&back, &sentinel, sizeof back));
    for (revision = 1; revision < 9; revision++) {
        assert(sophia_sf_decode_revision(b, n, (uint16_t)revision, &back) == -1);
        assert(!memcmp(&back, &sentinel, sizeof back));
    }
    assert(sophia_sf_decode_revision(b, n, 9, &back) == 0);
    assert(back.header.kind == SOPHIA_SF_SUBMITTED && back.header.sequence == 9);
    assert(back.value.submitted.submission_id == 7);
    assert(back.value.submitted.candidate_kind == SOPHIA_SF_KIND_IMAGE_BEGIN);
    /* Revision bounds and capacity. */
    assert(sophia_sf_decode_revision(b, n, 0, &back) == -4);
    /* r10 has no ImageBegin receipt; revisions above 10 are unknown. */
    assert(sophia_sf_decode_revision(b, n, 10, &back) == -1);
    assert(sophia_sf_decode_revision(b, n, 11, &back) == -4);
    assert(sophia_sf_encode_revision(b, sizeof b, 0, &r, &m) == -4);
    assert(sophia_sf_encode_revision(b, sizeof b, 10, &r, &m) == -1);
    assert(sophia_sf_encode_revision(b, sizeof b, 11, &r, &m) == -4);
    assert(sophia_sf_encode_revision(bad, SUBMITTED_BYTES - 1, 9, &r, &m) == -4);
    /* Only ImageBegin: the other image kinds and the neighbors stay out. */
    for (kind = 0x7001; kind <= 0x7006; kind++) {
        struct sophia_sf_record other = shell_receipt((uint16_t)kind);
        assert(sophia_sf_encode_revision(bad, sizeof bad, 9, &other, &m) == -1);
        memcpy(bad, b, n);
        put16(bad + BODY + 8, kind);
        assert(sophia_sf_decode_revision(bad, n, 9, &back) == -1);
    }
    {
        static const unsigned outside[] = {255, 279, 0x6fff, 0};
        size_t i;
        for (i = 0; i < sizeof outside / sizeof outside[0]; i++) {
            memcpy(bad, b, n);
            put16(bad + BODY + 8, outside[i]);
            assert(sophia_sf_decode_revision(bad, n, 9, &back) == -1);
        }
    }
    /* The rest of the record keeps its legacy rules. */
    r.value.submitted.submission_id = 0;
    assert(sophia_sf_encode_revision(bad, sizeof bad, 9, &r, &m) == -1);
    memcpy(bad, b, n), bad[BODY] = 0;
    assert(sophia_sf_decode_revision(bad, n, 9, &back) == -1);
    memcpy(bad, b, n), bad[BODY + 12] = 1;
    assert(sophia_sf_decode_revision(bad, n, 9, &back) == -1);
    memcpy(bad, b, n), bad[16] = 1; /* submission id in an event header */
    assert(sophia_sf_decode_revision(bad, n, 9, &back) == -1);
    memcpy(bad, b, n), memset(bad + 24, 0, 8); /* missing sequence */
    assert(sophia_sf_decode_revision(bad, n, 9, &back) == -1);
    assert(sophia_sf_decode_revision(b, n - 1, 9, &back) == -1);
    over_length_shell(b, n);
}

static void lock_legacy(void)
{
    uint8_t plain[64], b[64];
    size_t n, m;
    unsigned kind, revision;
    for (kind = 256; kind <= 263; kind++) {
        struct sophia_lf_record r = lock_receipt((uint16_t)kind), back;
        if (kind == 257) /* no lock kind 257 */
            continue;
        assert(sophia_lf_encode(plain, sizeof plain, &r, &n) == 0 && n == SUBMITTED_BYTES);
        for (revision = 1; revision <= 2; revision++) {
            assert(sophia_lf_encode_revision(b, sizeof b, (uint16_t)revision, &r, &m) == 0);
            assert(m == n && !memcmp(b, plain, n));
            assert(sophia_lf_decode_revision(b, m, (uint16_t)revision, &back) == 0);
            assert(back.value.submitted.kind == kind);
        }
    }
}

static void lock_image_begin(void)
{
    struct sophia_lf_record r = lock_receipt(SOPHIA_LF_KIND_IMAGE_BEGIN), back, sentinel;
    uint8_t b[64], bad[64];
    size_t n = 0, m;
    unsigned kind;
    memset(&sentinel, 0xa5, sizeof sentinel);
    assert(sophia_lf_encode(b, sizeof b, &r, &n) == -1);
    assert(sophia_lf_encode_revision(b, sizeof b, 1, &r, &n) == -1);
    assert(sophia_lf_encode_revision(b, sizeof b, 2, &r, &n) == 0 && n == SUBMITTED_BYTES);
    assert(b[6] == SOPHIA_LF_SUBMITTED && b[7] == 0);
    assert(b[BODY + 8] == 0x00 && b[BODY + 9] == 0x70 && b[BODY] == 7);
    back = sentinel;
    assert(sophia_lf_decode(b, n, &back) == -1 && !memcmp(&back, &sentinel, sizeof back));
    assert(sophia_lf_decode_revision(b, n, 1, &back) == -1);
    assert(!memcmp(&back, &sentinel, sizeof back));
    assert(sophia_lf_decode_revision(b, n, 2, &back) == 0);
    assert(back.header.kind == SOPHIA_LF_SUBMITTED && back.header.sequence == 9);
    assert(back.value.submitted.submission == 7);
    assert(back.value.submitted.kind == SOPHIA_LF_KIND_IMAGE_BEGIN);
    assert(sophia_lf_decode_revision(b, n, 0, &back) == -4);
    assert(sophia_lf_decode_revision(b, n, 3, &back) == -4);
    assert(sophia_lf_encode_revision(b, sizeof b, 0, &r, &m) == -4);
    assert(sophia_lf_encode_revision(b, sizeof b, 3, &r, &m) == -4);
    assert(sophia_lf_encode_revision(bad, SUBMITTED_BYTES - 1, 2, &r, &m) == -4);
    for (kind = 0x7001; kind <= 0x7006; kind++) {
        struct sophia_lf_record other = lock_receipt((uint16_t)kind);
        assert(sophia_lf_encode_revision(bad, sizeof bad, 2, &other, &m) == -1);
        memcpy(bad, b, n);
        put16(bad + BODY + 8, kind);
        assert(sophia_lf_decode_revision(bad, n, 2, &back) == -1);
    }
    {
        static const unsigned outside[] = {255, 257, 264, 0x6fff, 0};
        size_t i;
        for (i = 0; i < sizeof outside / sizeof outside[0]; i++) {
            memcpy(bad, b, n);
            put16(bad + BODY + 8, outside[i]);
            assert(sophia_lf_decode_revision(bad, n, 2, &back) == -1);
        }
    }
    r.value.submitted.submission = 0;
    assert(sophia_lf_encode_revision(bad, sizeof bad, 2, &r, &m) == -1);
    memcpy(bad, b, n), bad[BODY] = 0;
    assert(sophia_lf_decode_revision(bad, n, 2, &back) == -1);
    memcpy(bad, b, n), bad[BODY + 12] = 1;
    assert(sophia_lf_decode_revision(bad, n, 2, &back) == -1);
    memcpy(bad, b, n), bad[16] = 1;
    assert(sophia_lf_decode_revision(bad, n, 2, &back) == -1);
    memcpy(bad, b, n), memset(bad + 24, 0, 8);
    assert(sophia_lf_decode_revision(bad, n, 2, &back) == -1);
    assert(sophia_lf_decode_revision(b, n - 1, 2, &back) == -1);
    over_length_lock(b, n);
}

/* Lock Negotiated: the plain codec stays revision 1 with PRESENT|CHORDS;
 * the _revision codec carries an explicit selection 1 or 2, preserves it,
 * and allows image bits 2 and 3 only at selection 2. */
static struct sophia_lf_record lock_welcome(uint16_t selected, uint64_t granted)
{
    struct sophia_lf_record r;
    memset(&r, 0, sizeof r);
    r.header.kind = SOPHIA_LF_NEGOTIATED;
    r.header.epoch = 5, r.header.sequence = 2;
    r.value.negotiated.granted_chords = 1;
    r.value.negotiated.granted_capabilities = granted;
    r.value.negotiated.selected_revision = selected;
    return r;
}

static void lock_negotiated(void)
{
    static const uint64_t r2_masks[] = {1, 3, 5, 7, 9, 11, 13, 15};
    uint8_t b[64], wire[64];
    size_t n, m;
    unsigned i;
    struct sophia_lf_record r, back;
    /* Plain: selection 1 (0 encodes as 1), r1 grants only. */
    r = lock_welcome(0, 3);
    assert(sophia_lf_encode(b, sizeof b, &r, &n) == 0 && n == 48);
    assert(b[32] == 1 && b[33] == 0);
    assert(sophia_lf_decode(b, n, &back) == 0 && back.value.negotiated.selected_revision == 1 &&
           back.value.negotiated.granted_capabilities == 3);
    r = lock_welcome(1, 3);
    assert(sophia_lf_encode(wire, sizeof wire, &r, &m) == 0 && m == n && !memcmp(b, wire, n));
    r = lock_welcome(2, 1);
    assert(sophia_lf_encode(b, sizeof b, &r, &n) == -1);
    r = lock_welcome(1, 5);
    assert(sophia_lf_encode(b, sizeof b, &r, &n) == -1);
    /* Revision 1 bound: byte-identical to plain, never selection 2. */
    r = lock_welcome(1, 3);
    assert(sophia_lf_encode(b, sizeof b, &r, &n) == 0);
    assert(sophia_lf_encode_revision(wire, sizeof wire, 1, &r, &m) == 0 && m == n &&
           !memcmp(b, wire, n));
    assert(sophia_lf_encode_revision(wire, sizeof wire, 2, &r, &m) == 0 && m == n &&
           !memcmp(b, wire, n));
    assert(sophia_lf_decode_revision(b, n, 2, &back) == 0 &&
           back.value.negotiated.selected_revision == 1);
    r = lock_welcome(2, 1);
    assert(sophia_lf_encode_revision(wire, sizeof wire, 1, &r, &m) == -1);
    /* Selection 2: every mask with PRESENT, carried and preserved. */
    for (i = 0; i < sizeof r2_masks / sizeof r2_masks[0]; i++) {
        r = lock_welcome(2, r2_masks[i]);
        assert(sophia_lf_encode_revision(b, sizeof b, 2, &r, &n) == 0 && n == 48);
        assert(b[32] == 2 && b[33] == 0 && b[40] == r2_masks[i]);
        assert(sophia_lf_decode_revision(b, n, 2, &back) == 0);
        assert(back.value.negotiated.selected_revision == 2 &&
               back.value.negotiated.granted_capabilities == r2_masks[i] &&
               back.value.negotiated.granted_chords == 1);
        /* Neither the plain codec nor the revision 1 bound reads it. */
        assert(sophia_lf_decode(b, n, &back) == -1);
        assert(sophia_lf_decode_revision(b, n, 1, &back) == -1);
    }
    /* Image bits at selection 1 are refused, never read as selection 2. */
    for (i = 0; i < 2; i++) {
        r = lock_welcome(1, i ? 9 : 5);
        assert(sophia_lf_encode_revision(b, sizeof b, 2, &r, &n) == -1);
        r = lock_welcome(1, 1);
        assert(sophia_lf_encode_revision(b, sizeof b, 2, &r, &n) == 0);
        b[40] = (uint8_t)(i ? 9 : 5);
        assert(sophia_lf_decode_revision(b, n, 2, &back) == -1);
    }
    /* Missing PRESENT, unknown bits, selection 0 or 3, and r1 reserved
     * bytes and chord bounds still apply at selection 2. */
    r = lock_welcome(2, 4);
    assert(sophia_lf_encode_revision(b, sizeof b, 2, &r, &n) == -1);
    r = lock_welcome(2, 0x11);
    assert(sophia_lf_encode_revision(b, sizeof b, 2, &r, &n) == -1);
    r = lock_welcome(3, 1);
    assert(sophia_lf_encode_revision(b, sizeof b, 2, &r, &n) == -1);
    r = lock_welcome(2, 5);
    assert(sophia_lf_encode_revision(b, sizeof b, 2, &r, &n) == 0);
    memcpy(wire, b, n);
    wire[40] = 4;
    assert(sophia_lf_decode_revision(wire, n, 2, &back) == -1);
    memcpy(wire, b, n);
    wire[40] = 0x15;
    assert(sophia_lf_decode_revision(wire, n, 2, &back) == -1);
    memcpy(wire, b, n);
    wire[47] = 0x80;
    assert(sophia_lf_decode_revision(wire, n, 2, &back) == -1);
    for (i = 0; i < 4; i++) {
        memcpy(wire, b, n);
        put16(wire + 32, i & 1 ? 3 : 0);
        wire[40] = (uint8_t)(i & 2 ? 5 : 1);
        assert(sophia_lf_decode_revision(wire, n, 2, &back) == -1);
    }
    /* A zero selection encodes as 1 under either bound. */
    r = lock_welcome(0, 3);
    assert(sophia_lf_encode_revision(wire, sizeof wire, 2, &r, &m) == 0 && m == 48 &&
           wire[32] == 1 && wire[40] == 3);
    r = lock_welcome(0, 5);
    assert(sophia_lf_encode_revision(wire, sizeof wire, 2, &r, &m) == -1);
    memcpy(wire, b, n);
    wire[36] = 1;
    assert(sophia_lf_decode_revision(wire, n, 2, &back) == -1);
    memcpy(wire, b, n);
    put16(wire + 34, SOPHIA_LF_MAX_CHORDS + 1);
    assert(sophia_lf_decode_revision(wire, n, 2, &back) == -1);
    r = lock_welcome(2, 5);
    r.value.negotiated.granted_chords = SOPHIA_LF_MAX_CHORDS + 1;
    assert(sophia_lf_encode_revision(b, sizeof b, 2, &r, &n) == -1);
    /* Truncated and over-long Negotiated records. */
    r = lock_welcome(2, 5);
    assert(sophia_lf_encode_revision(b, sizeof b, 2, &r, &n) == 0);
    assert(sophia_lf_decode_revision(b, n - 1, 2, &back) == -1);
    memcpy(wire, b, n);
    wire[n] = 0;
    put16(wire, (unsigned)n + 1);
    assert(sophia_lf_decode_revision(wire, n + 1, 2, &back) == -1);
    /* Capacity and bounds keep their return codes. */
    assert(sophia_lf_encode_revision(b, 47, 2, &r, &n) != 0);
    assert(sophia_lf_encode_revision(b, sizeof b, 3, &r, &n) == -4);
    assert(sophia_lf_decode_revision(b, 48, 0, &back) == -4);
}

int main(void)
{
    shell_legacy();
    shell_image_begin();
    lock_legacy();
    lock_image_begin();
    lock_negotiated();
    other_kinds();
    puts("image_receipt_test: ok");
    return 0;
}

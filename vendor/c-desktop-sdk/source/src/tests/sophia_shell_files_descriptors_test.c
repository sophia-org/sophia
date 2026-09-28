#include "sophia_shell_files.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

/* Independent literal file vectors from the native file KDL, not IPC frames or
 * output from another SDK. Designated byte offsets include the 32-byte header. */
#define TX UINT64_C(0x8877665544332211)
#define EVENT(n, k)                                                                                \
    [0] = (n) & 255, [1] = (n) >> 8, [4] = 1, [6] = (k), [8] = 9, [24] = 7, [32] = 0x11,           \
    [33] = 0x22, [34] = 0x33, [35] = 0x44, [36] = 0x55, [37] = 0x66, [38] = 0x77, [39] = 0x88,     \
    [40] = 9
#define CANDIDATE(n, k)                                                                            \
    [0] = (n), [4] = 1, [6] = (k) & 255, [7] = (k) >> 8, [8] = 9, [16] = 11, [32] = 0x11,          \
    [33] = 0x22, [34] = 0x33, [35] = 0x44, [36] = 0x55, [37] = 0x66, [38] = 0x77, [39] = 0x88,     \
    [40] = 9
#define HEADER(k) {(k), 9, 0, 7}
#define ACK_HEADER(k) {(k), 9, 11, 0}
#define GRANT {TX, 9, 17, 18, 19, 20, 21, 4096}
struct vector {
    size_t bytes;
    uint8_t wire[353];
    struct sophia_sf_record value;
    /* Body offsets of nonzero u64 identities and reserved u16s. */
    unsigned identities[10], identity_count;
    unsigned reserved[2], reserved_count;
};
static const struct vector vectors[] = {
    {68,
     {EVENT(68, 46), [48] = 17, [56] = 18, [64] = 2},
     {HEADER(SOPHIA_SF_DESCRIPTOR_OUTCOME), .value.descriptor_outcome = {TX, 9, 17, 18, 2}},
     {0, 8, 16},
     3,
     {34},
     1},
    {116,
     {EVENT(116, 47), [48] = 17, [56] = 18, [64] = 19, [72] = 20, [80] = 21, [88] = 22, [96] = 9,
      [104] = 0x23, [105] = 1, [108] = 24},
     {HEADER(SOPHIA_SF_DESCRIPTOR_ACTIVATION),
      .value.descriptor_activation = {TX, 9, 17, 18, 19, 20, 21, 22, 9, 0x123, 24}},
     {0, 8, 16, 24, 32, 40, 48, 56, 64, 76},
     10,
     {74},
     1},
    {92,
     {EVENT(92, 48), [48] = 17, [56] = 18, [64] = 19, [72] = 20, [88] = 4},
     {HEADER(SOPHIA_SF_REFERENCE_REQUEST),
      .value.reference_request = {TX, 9, 17, 18, 19, 20, 0, 4}},
     {0, 8, 16, 24, 32, 40},
     6,
     {58},
     1},
    {88,
     {EVENT(88, 49), [48] = 17, [56] = 18, [64] = 19, [72] = 20, [80] = 1, [82] = 2, [84] = 2},
     {HEADER(SOPHIA_SF_REFERENCE_OUTCOME),
      .value.reference_outcome = {TX, 9, 17, 18, 19, 20, 1, 2, 2}},
     {0, 8, 16, 24, 32},
     5,
     {54},
     1},
    {352,
     {EVENT(352, 50), [48] = 17, [56] = 18, [64] = 19, [72] = 20, [88] = 4, [92] = 3, [96] = 'a',
      [97] = 0xc3, [98] = 0xa9},
     {HEADER(SOPHIA_SF_DESCRIPTOR_LAUNCHER_REQUEST),
      .value.descriptor_launcher_request = {{TX, 9, 17, 18, 19, 20, 0, 4},
                                            {(const uint8_t *)"a\xc3\xa9", 3}}},
     {0, 8, 16, 24, 32, 40},
     6,
     {58, 62},
     2},
    {76,
     {EVENT(76, 51), [48] = 17, [56] = 18, [64] = 19, [72] = 2},
     {HEADER(SOPHIA_SF_DESCRIPTOR_LAUNCHER_OUTCOME),
      .value.descriptor_launcher_outcome = {TX, 9, 17, 18, 19, 2}},
     {0, 8, 16, 24},
     4,
     {42},
     1},
    {92,
     {EVENT(92, 52), [48] = 17, [56] = 18, [64] = 19, [72] = 20, [80] = 21, [89] = 16},
     {HEADER(SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION),
      .value.descriptor_launcher_activation = GRANT},
     {0, 8, 16, 24, 32, 40, 48},
     7,
     {58},
     1},
    {92,
     {EVENT(92, 53), [48] = 17, [56] = 18, [64] = 19, [72] = 20, [80] = 21, [89] = 16, [90] = 3},
     {HEADER(SOPHIA_SF_DESCRIPTOR_LAUNCH_OUTCOME), .value.descriptor_launch_outcome = {GRANT, 3}},
     {0, 8, 16, 24, 32, 40, 48},
     7,
     {0},
     0},
    {60,
     {CANDIDATE(60, 274), [48] = 17, [56] = 2},
     {ACK_HEADER(SOPHIA_SF_DESCRIPTOR_ACTIVATION_ACK),
      .value.descriptor_activation_ack = {TX, 9, 17, 2}},
     {0, 8, 16},
     3,
     {26},
     1},
    {92,
     {CANDIDATE(92, 278), [48] = 17, [56] = 18, [64] = 19, [72] = 20, [80] = 21, [89] = 16,
      [90] = 1},
     {ACK_HEADER(SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION_ACK),
      .value.descriptor_launcher_activation_ack = {GRANT, 1}},
     {0, 8, 16, 24, 32, 40, 48},
     7,
     {0},
     0}};
static unsigned refusals;
static void refused(const uint8_t *wire, size_t bytes)
{
    struct sophia_sf_record out, before;
    memset(&out, 0xa5, sizeof(out));
    memcpy(&before, &out, sizeof(out));
    assert(sophia_sf_decode(wire, bytes, &out) == -1);
    assert(!memcmp(&out, &before, sizeof(out)));
    refusals++;
}
static void invalid_value(const struct sophia_sf_record *r)
{
    uint8_t bytes[353], before[353];
    size_t written = 12345;
    memset(bytes, 0xa5, sizeof(bytes));
    memcpy(before, bytes, sizeof(bytes));
    assert(sophia_sf_encode(bytes, sizeof(bytes), r, &written) == -1);
    assert(written == 12345 && !memcmp(bytes, before, sizeof(bytes)));
}
static void valid_value(const struct sophia_sf_record *r)
{
    uint8_t bytes[353];
    size_t written;
    struct sophia_sf_record out;
    assert(!sophia_sf_encode(bytes, sizeof(bytes), r, &written));
    assert(!sophia_sf_decode(bytes, written, &out));
}
static void literal_vectors(void)
{
    size_t i, j;
    for (i = 0; i < sizeof(vectors) / sizeof(vectors[0]); i++) {
        const struct vector *v = &vectors[i];
        uint8_t wire[353], encoded[353];
        struct sophia_sf_record decoded, invalid;
        size_t written = 0;
        memset(encoded, 0xa5, sizeof(encoded));
        assert(!sophia_sf_encode(encoded, sizeof(encoded), &v->value, &written));
        assert(written == v->bytes && encoded[written] == 0xa5);
        assert(!memcmp(encoded, v->wire, written));
        assert(!sophia_sf_decode(v->wire, v->bytes, &decoded));
        assert(!sophia_sf_encode(encoded, sizeof(encoded), &decoded, &written));
        assert(!memcmp(encoded, v->wire, written));
        for (j = 0; j < v->bytes; j++) {
            memset(encoded, 0xa5, sizeof(encoded));
            written = 12345;
            assert(sophia_sf_encode(encoded, j, &v->value, &written) == -4);
            assert(written == 12345);
            for (size_t k = 0; k < sizeof(encoded); k++)
                assert(encoded[k] == 0xa5);
            /* Truncation with a matching header still refuses. */
            memcpy(wire, v->wire, sizeof(wire));
            wire[0] = (uint8_t)j;
            wire[1] = (uint8_t)(j >> 8);
            refused(wire, j);
        }
        memcpy(wire, v->wire, sizeof(wire));
        wire[0] = (uint8_t)(v->bytes + 1);
        wire[1] = (uint8_t)((v->bytes + 1) >> 8);
        refused(wire, v->bytes + 1);
        for (j = 0; j < v->identity_count; j++) {
            memcpy(wire, v->wire, sizeof(wire));
            memset(wire + 32 + v->identities[j], 0, 8);
            refused(wire, v->bytes);
        }
        for (j = 0; j < v->reserved_count; j++) {
            for (unsigned byte = 0; byte < 2; byte++) {
                memcpy(wire, v->wire, sizeof(wire));
                wire[32 + v->reserved[j] + byte] = 1;
                refused(wire, v->bytes);
            }
        }
        memcpy(wire, v->wire, sizeof(wire));
        wire[40] = 8;
        refused(wire, v->bytes);
        memcpy(wire, v->wire, sizeof(wire));
        wire[4] = 2;
        refused(wire, v->bytes);
        memcpy(wire, v->wire, sizeof(wire));
        wire[8] = 0;
        refused(wire, v->bytes);
        memcpy(wire, v->wire, sizeof(wire));
        wire[v->value.header.kind < 256 ? 16 : 24] = 1;
        refused(wire, v->bytes);
        memcpy(wire, v->wire, sizeof(wire));
        wire[v->value.header.kind < 256 ? 24 : 16] = 0;
        refused(wire, v->bytes);
        invalid = v->value;
        invalid.header.epoch = 8;
        invalid_value(&invalid);
        invalid = v->value;
        invalid.header.epoch = 0;
        invalid_value(&invalid);
        invalid = v->value;
        if (invalid.header.kind < 256)
            invalid.header.submission = 1;
        else
            invalid.header.sequence = 1;
        invalid_value(&invalid);
    }
}
static void outcomes(void)
{
    for (uint16_t kind = 0; kind <= 5; kind++) {
        for (unsigned presented = 0; presented <= 1; presented++) {
            struct sophia_sf_record d = vectors[0].value, r = vectors[3].value,
                                    l = vectors[5].value;
            d.value.descriptor_outcome.kind = r.value.reference_outcome.kind =
                l.value.descriptor_launcher_outcome.kind = kind;
            d.value.descriptor_outcome.presentation_epoch =
                r.value.reference_outcome.presentation_epoch =
                    l.value.descriptor_launcher_outcome.presentation_epoch =
                        presented ? UINT64_MAX : 0;
            if (kind < 1 || kind > 4 || ((kind == 2) != (presented != 0)))
                invalid_value(&d);
            else
                valid_value(&d);
            /* Reference/launcher non-Presented epochs deliberately remain allowed. */
            if (kind < 1 || kind > 4 || (kind == 2 && !presented)) {
                invalid_value(&r);
                invalid_value(&l);
            } else {
                valid_value(&r);
                valid_value(&l);
            }
        }
    }
    struct sophia_sf_record r = vectors[3].value;
    r.value.reference_outcome.pages = 0;
    invalid_value(&r);
    r.value.reference_outcome.pages = 1;
    invalid_value(&r);
    r.value.reference_outcome.pages = UINT16_MAX;
    r.value.reference_outcome.page = UINT16_MAX - 1;
    valid_value(&r);
}
static void requests_and_grants(void)
{
    struct sophia_sf_record r;
    uint8_t wire[353];
    for (uint16_t operation = 0; operation <= 5; operation++) {
        r = vectors[2].value;
        r.value.reference_request.operation = operation;
        if (operation <= 4)
            valid_value(&r);
        else
            invalid_value(&r);
        r = vectors[4].value;
        r.value.descriptor_launcher_request.request.operation = operation;
        if (operation <= 4)
            valid_value(&r);
        else
            invalid_value(&r);
    }
    for (uint16_t value = 0; value <= 4; value++) {
        r = vectors[7].value;
        r.value.descriptor_launch_outcome.status = value;
        if (value >= 1 && value <= 3)
            valid_value(&r);
        else
            invalid_value(&r);
        r = vectors[8].value;
        r.value.descriptor_activation_ack.disposition = value;
        if (value >= 1 && value <= 2)
            valid_value(&r);
        else
            invalid_value(&r);
        r = vectors[9].value;
        r.value.descriptor_launcher_activation_ack.consumed = value;
        if (value <= 1)
            valid_value(&r);
        else
            invalid_value(&r);
    }
    r = vectors[1].value;
    r.value.descriptor_activation.action_recipient_epoch = 10;
    invalid_value(&r);
    r = vectors[1].value;
    r.value.descriptor_activation.action_target_slot = 0;
    invalid_value(&r);
    for (size_t i = 6; i < 10; i++) {
        if (i == 8)
            continue;
        memcpy(wire, vectors[i].wire, sizeof(wire));
        wire[88] = 0;
        wire[89] = 0;
        refused(wire, vectors[i].bytes);
        wire[88] = 1;
        wire[89] = 16;
        refused(wire, vectors[i].bytes);
    }
    memcpy(wire, vectors[9].wire, sizeof(wire));
    wire[90] = 2;
    refused(wire, 92);
    memcpy(wire, vectors[8].wire, sizeof(wire));
    wire[56] = 3;
    refused(wire, 60);
    memcpy(wire, vectors[7].wire, sizeof(wire));
    wire[90] = 4;
    refused(wire, 92);
}
static void query_text(void)
{
    struct sophia_sf_record r = vectors[4].value, decoded;
    uint8_t text[257], wire[353];
    size_t written;
    r.value.descriptor_launcher_request.query = (struct sophia_sf_text){NULL, 0};
    valid_value(&r);
    memset(text, 'x', sizeof(text));
    r.value.descriptor_launcher_request.query = (struct sophia_sf_text){text, 256};
    valid_value(&r);
    r.value.descriptor_launcher_request.query.size = 257;
    invalid_value(&r);
    r.value.descriptor_launcher_request.query = (struct sophia_sf_text){NULL, 1};
    invalid_value(&r);
    const struct sophia_sf_text invalid[] = {
        {(const uint8_t *)"\0", 1},           {(const uint8_t *)"\n", 1},
        {(const uint8_t *)"\xc2\x85", 2},     {(const uint8_t *)"\xe2\x80\xae", 3},
        {(const uint8_t *)"\xe2\x81\xa6", 3}, {(const uint8_t *)"\xc0\xaf", 2},
        {(const uint8_t *)"\xed\xa0\x80", 3}, {(const uint8_t *)"\xf4\x90\x80\x80", 4},
        {(const uint8_t *)"\xc3", 1},         {(const uint8_t *)"\x80", 1}};
    for (size_t i = 0; i < sizeof(invalid) / sizeof(invalid[0]); i++) {
        r.value.descriptor_launcher_request.query = invalid[i];
        invalid_value(&r);
        memcpy(wire, vectors[4].wire, sizeof(wire));
        memset(wire + 96, 0, 256);
        wire[92] = (uint8_t)invalid[i].size;
        memcpy(wire + 96, invalid[i].data, invalid[i].size);
        refused(wire, 352);
    }
    memcpy(wire, vectors[4].wire, sizeof(wire));
    wire[351] = 1;
    refused(wire, 352);
    memcpy(wire, vectors[4].wire, sizeof(wire));
    wire[92] = 1;
    wire[93] = 1;
    refused(wire, 352);
    assert(!sophia_sf_decode(vectors[4].wire, 352, &decoded));
    assert(decoded.value.descriptor_launcher_request.query.data == vectors[4].wire + 96);
    assert(decoded.value.descriptor_launcher_request.query.size == 3);
    assert(!sophia_sf_encode(wire, sizeof(wire), &decoded, &written));
    assert(written == 352);
}
static void receipts_and_unknowns(void)
{
    struct sophia_sf_record r = {HEADER(SOPHIA_SF_SUBMITTED), .value.submitted = {11, 274}};
    valid_value(&r);
    r.value.submitted.candidate_kind = 278;
    valid_value(&r);
    const uint16_t unknown[] = {8, 54, 279, UINT16_MAX};
    uint8_t wire[353];
    for (size_t i = 0; i < sizeof(unknown) / sizeof(unknown[0]); i++) {
        memcpy(wire, vectors[0].wire, sizeof(wire));
        wire[6] = (uint8_t)unknown[i];
        wire[7] = (uint8_t)(unknown[i] >> 8);
        refused(wire, 68);
        r.header.kind = unknown[i];
        invalid_value(&r);
    }
}
int main(void)
{
    literal_vectors();
    outcomes();
    requests_and_grants();
    query_text();
    receipts_and_unknowns();
    printf("shell descriptor controls: 10 literal vectors, %u malformed-wire refusals passed\n",
           refusals);
    return 0;
}

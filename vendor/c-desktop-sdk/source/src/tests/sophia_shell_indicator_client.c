/* Independent revision-6 indicator decoder.
 *
 * Written from the published schema and golden frames. It borrows only
 * byte-level framing helpers from the descriptor proof; no Rust ABI, generated
 * codec, or Sophia library is involved, which is the point: if the Rust encoder
 * and this decoder agree, the wire format is described well enough for a second
 * implementation to exist.
 *
 * Reads the corpus on argv[1] and validates every record, including that a
 * cleared active-output flag carries a zeroed identity and that fixed-width
 * label padding is zero. */
#define main descriptor_proof_main
#include "sophia_shell_v1_client.c"
#undef main

#define MAX_INDICATORS 256u
#define MAX_STATUS 16u
#define LABEL_BYTES 32u

static int hex_nibble(int c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    return -1;
}

/* One "name|hexbytes" corpus line into a frame. */
static int parse_line(const char *line, uint8_t *out, size_t cap, size_t *len) {
    const char *bar = strchr(line, '|');
    size_t at = 0;
    if (!bar) return 0;
    ++bar;
    while (bar[0] && bar[0] != '\n') {
        int hi = hex_nibble((unsigned char)bar[0]);
        int lo = bar[1] ? hex_nibble((unsigned char)bar[1]) : -1;
        if (hi < 0 || lo < 0 || at >= cap) return 0;
        out[at++] = (uint8_t)((hi << 4) | lo);
        bar += 2;
    }
    *len = at;
    return 1;
}

static int check_label(const uint8_t *b, size_t at, uint16_t len) {
    size_t i;
    if (len > LABEL_BYTES) return 0;
    /* Padding beyond the declared length must be zero, or two byte strings
     * would decode to one label. */
    for (i = len; i < LABEL_BYTES; ++i) {
        if (b[at + i] != 0u) return 0;
    }
    return 1;
}

int main(int argc, char **argv) {
    FILE *corpus = argc > 1 ? fopen(argv[1], "r") : NULL;
    char line[4096];
    uint8_t frame[FRAME_CAPACITY];
    size_t len = 0;
    int snapshots = 0, activations = 0, outcomes = 0;
    int open_snapshot = 0;
    uint16_t expect_indicators = 0, expect_status = 0, saw_indicators = 0, saw_status = 0;
    uint64_t epoch = 0, generation = 0;

    if (!corpus) return 1;
    while (fgets(line, sizeof(line), corpus)) {
        const uint8_t *b;
        size_t n;
        uint16_t kind;
        if (!parse_line(line, frame, sizeof(frame), &len)) return 2;
        if (len < FRAME_HEADER_LEN) return 3;
        if (memcmp(frame, "SOPH", 4u) != 0 || read_u16(frame + 4) != 1u) return 4;
        kind = read_u16(frame + 6);
        b = frame + FRAME_HEADER_LEN;
        n = len - FRAME_HEADER_LEN;
        if (!read_u64(frame + 8)) return 5;

        switch (kind) {
        case 181u: /* IndicatorsBegin */
            if (open_snapshot || n != 32u) return 6;
            epoch = read_u64(b);
            generation = read_u64(b + 8);
            if (!epoch) return 7;
            {
                uint64_t active = read_u64(b + 16);
                uint16_t present = read_u16(b + 24);
                if (read_u16(b + 30) != 0u) return 34;
                if (present > 1u) return 8;
                /* A cleared flag must come with a zeroed identity. */
                if (!present && active) return 9;
                if (present && !active) return 10;
            }
            expect_indicators = read_u16(b + 26);
            expect_status = read_u16(b + 28);
            if (expect_indicators > MAX_INDICATORS || expect_status > MAX_STATUS) return 11;
            saw_indicators = 0;
            saw_status = 0;
            open_snapshot = 1;
            break;
        case 182u: /* IndicatorsOutputStatus */
            if (!open_snapshot || n != 64u) return 12;
            if (read_u64(b) != epoch || read_u64(b + 8) != generation) return 13;
            if (!read_u64(b + 16)) return 14;
            if (read_u32(b + 28) != 0u) return 15;
            if (!check_label(b, 32u, read_u16(b + 26))) return 16;
            if (++saw_status > expect_status) return 17;
            break;
        case 183u: /* IndicatorsEntry */
            if (!open_snapshot || n != 80u) return 18;
            if (read_u64(b) != epoch || read_u64(b + 8) != generation) return 19;
            if (!read_u64(b + 16)) return 20;
            if (!check_label(b, 48u, read_u16(b + 46))) return 21;
            if (++saw_indicators > expect_indicators) return 22;
            break;
        case 184u: /* IndicatorsEnd */
            if (!open_snapshot || n != 16u) return 23;
            if (read_u64(b) != epoch || read_u64(b + 8) != generation) return 24;
            if (saw_indicators != expect_indicators || saw_status != expect_status) return 25;
            open_snapshot = 0;
            ++snapshots;
            break;
        case 185u: /* IndicatorActivate */
            if (n != 48u) return 26;
            if (!read_u64(b + 40)) return 27;
            ++activations;
            break;
        case 186u: /* IndicatorActivateOutcome */
            if (n != 28u) return 28;
            if (read_u16(b + 24) > 3u) return 29;
            if (read_u16(b + 26) != 0u) return 30;
            ++outcomes;
            break;
        default:
            return 31;
        }
    }
    fclose(corpus);
    if (open_snapshot) return 32;
    /* The corpus carries a populated snapshot, an empty one, one activation,
     * and every outcome status. */
    if (snapshots != 2 || activations != 1 || outcomes != 4) return 33;
    printf("sophia_shell_indicator_c_client status=complete snapshots=%d activations=%d outcomes=%d\n",
           snapshots, activations, outcomes);
    return 0;
}

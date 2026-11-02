#include "sophia_shell_files.h"
#undef NDEBUG
#include <assert.h>
#include <string.h>
#include <stdio.h>
#include "shell_files_vectors.h"

static void length(uint8_t *b, size_t n)
{
    unsigned i;
    for (i = 0; i < 4; ++i)
        b[i] = (uint8_t)(n >> (8 * i));
}

static void golden(void)
{
    FILE *f = fopen("spec/golden/sophia-shell-retire-v1.tsv", "r");
    char line[17000], name[64], hex[16385];
    uint8_t bytes[8192], encoded[8192];
    struct sophia_sf_record record;
    struct sophia_sf_candidate_retire retire;
    size_t count = 0, i, n, written;
    int enabled, accepted, present, result;
    assert(f);
    while (fgets(line, sizeof line, f)) {
        if (line[0] == '#')
            continue;
        assert(sscanf(line, "%63s %d %d %16384s", name, &enabled, &accepted, hex) == 4);
        assert(strlen(hex) % 2 == 0);
        n = strlen(hex) / 2;
        for (i = 0; i < n; ++i) {
            unsigned byte;
            assert(sscanf(hex + i * 2, "%2x", &byte) == 1);
            bytes[i] = (uint8_t)byte;
        }
        result = sophia_sf_candidate_decode_with_retire(bytes, n, enabled, &record, &retire, &present);
        assert((result == 0) == accepted);
        if (!result) {
            result = present ? sophia_sf_candidate_encode_with_retire(encoded, sizeof encoded,
                &record, &retire, &written) : sophia_sf_encode(encoded, sizeof encoded, &record, &written);
            assert(!result && written == n && !memcmp(bytes, encoded, n));
        }
        ++count;
    }
    assert(!ferror(f) && count == 9);
    assert(!fclose(f));
}

int main(void)
{
    struct sophia_sf_record value, decoded;
    struct sophia_sf_candidate_retire intent = {91, 92, 93}, observed;
    uint8_t bytes[8193], changed[8193], expected[8193];
    size_t n, i;
    int present;
    (void)vectors;
    golden();
    assert(!sophia_sf_decode(vector_Candidate, sizeof vector_Candidate, &value));
    assert(value.value.candidate.surface_count && value.value.candidate.placement_count);
    assert(!sophia_sf_candidate_encode_with_retire(bytes, sizeof bytes, &value, &intent, &n));
    assert(n == sizeof vector_Candidate + 24);
    memcpy(expected, vector_Candidate, sizeof vector_Candidate);
    memset(expected + sizeof vector_Candidate, 0, 24);
    length(expected, n);
    expected[32 + 78] = 1;
    expected[n - 24] = 91;
    expected[n - 16] = 92;
    expected[n - 8] = 93;
    assert(!memcmp(expected, bytes, n));
    assert(!sophia_sf_candidate_decode_with_retire(bytes, n, 1, &decoded, &observed, &present));
    assert(present && observed.transaction == 91 && observed.resource_id == 92 &&
        observed.resource_generation == 93);
    assert(decoded.value.candidate.surface_count == value.value.candidate.surface_count);
    assert(sophia_sf_decode(bytes, n, &decoded));
    assert(sophia_sf_candidate_decode_with_retire(bytes, n, 0, &decoded, &observed, &present));
    assert(!sophia_sf_candidate_decode_with_retire(vector_Candidate, sizeof vector_Candidate,
        0, &decoded, &observed, &present));
    assert(!present);
    for (i = 0; i < n; ++i) {
        memcpy(changed, bytes, n);
        if (i >= 4)
            length(changed, i);
        assert(sophia_sf_candidate_decode_with_retire(changed, i, 1, &decoded, &observed, &present));
    }
    for (i = n - 24; i < n; i += 8) {
        memcpy(changed, bytes, n);
        memset(changed + i, 0, 8);
        assert(sophia_sf_candidate_decode_with_retire(changed, n, 1, &decoded, &observed, &present));
    }
    memcpy(changed, bytes, n);
    changed[32 + 78] = 3;
    assert(sophia_sf_candidate_decode_with_retire(changed, n, 1, &decoded, &observed, &present));
    memcpy(changed, bytes, n);
    changed[n] = 0;
    length(changed, n + 1);
    assert(sophia_sf_candidate_decode_with_retire(changed, n + 1, 1, &decoded, &observed, &present));
    assert(sophia_sf_candidate_encode_with_retire(changed, n - 1, &value, &intent, &i));
    intent.resource_generation = 0;
    assert(sophia_sf_candidate_encode_with_retire(changed, sizeof changed, &value, &intent, &i));
    return 0;
}

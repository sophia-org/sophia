#include "../sophia_shell_files.h"
#include "../shell_files/session_internal.h"
#include "shell_files_vectors.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static void api_discovery(void)
{
    static const char valid[] = "sophia-shell-files version=1 role=bar epoch=17 fd_transfer=none\n";
    static const char maximum[] = "sophia-shell-files version=1 role=bar epoch=18446744073709551615 fd_transfer=none\n";
    static const char *const invalid[] = {
        "sophia-shell-files version=1 role=bar fd_transfer=none\n",
        "sophia-shell-files version=2 role=bar epoch=17 fd_transfer=none\n",
        "sophia-shell-files version=1 role=bar epoch=0 fd_transfer=none\n",
        "sophia-shell-files version=1 role=bar epoch=017 fd_transfer=none\n",
        "sophia-shell-files version=1 role=bar epoch=-1 fd_transfer=none\n",
        "sophia-shell-files version=1 role=bar epoch=18446744073709551616 fd_transfer=none\n",
        "sophia-shell-files version=1 role= epoch=17 fd_transfer=none\n",
        "sophia-shell-files version=1 role=bar epoch=17 fd_transfer=none\nextra\n",
        "sophia-shell-files version=1 role=bar epoch=17 fd_transfer=rights\n",
    };
    uint64_t epoch = 123;
    size_t i;
    assert(!sf_api_epoch((const uint8_t *)valid, sizeof(valid) - 1, &epoch) && epoch == 17);
    assert(!sf_api_epoch((const uint8_t *)maximum, sizeof(maximum) - 1, &epoch) && epoch == UINT64_MAX);
    for (i = 0; i < sizeof(valid) - 1; i++) {
        epoch = 123;
        assert(sf_api_epoch((const uint8_t *)valid, i, &epoch) && epoch == 123);
    }
    for (i = 0; i < sizeof(invalid) / sizeof(invalid[0]); i++) {
        epoch = 123;
        assert(sf_api_epoch((const uint8_t *)invalid[i], strlen(invalid[i]), &epoch) && epoch == 123);
    }
}

static void vectors_and_truncations(void)
{
    size_t i, j, n;
    unsigned char b[8192];
    struct sophia_sf_record r, sentinel;
    for (i = 0; i < sizeof(vectors) / sizeof(vectors[0]); i++) {
        int result = sophia_sf_decode(vectors[i].bytes, vectors[i].size, &r);
        if (result)
            fprintf(stderr, "invalid vector %s\n", vectors[i].name);
        assert(!result);
        assert(!sophia_sf_encode(b, sizeof(b), &r, &n));
        assert(n == vectors[i].size && !memcmp(b, vectors[i].bytes, n));
        memset(&sentinel, 0xa5, sizeof(sentinel));
        for (j = 0; j < n; j++) {
            struct sophia_sf_record out = sentinel;
            assert(sophia_sf_decode(b, j, &out));
            assert(!memcmp(&out, &sentinel, sizeof(out)));
        }
        b[4] = 2;
        assert(sophia_sf_decode(b, n, &r));
    }
}
static void conditional_rules(void)
{
    struct sophia_sf_record r;
    unsigned char b[8192];
    size_t n;
    assert(!sophia_sf_decode(vector_FramePermit, sizeof(vector_FramePermit), &r));
    r.value.frame_permit.state = 1;
    assert(sophia_sf_encode(b, sizeof(b), &r, &n));
    r.value.frame_permit.reason = 0;
    assert(!sophia_sf_encode(b, sizeof(b), &r, &n));
    r.value.frame_permit.ttl_ms = 251;
    assert(sophia_sf_encode(b, sizeof(b), &r, &n));
    r.value.frame_permit.state = 2;
    r.value.frame_permit.ttl_ms = UINT32_MAX;
    assert(!sophia_sf_encode(b, sizeof(b), &r, &n));
    assert(!sophia_sf_decode(b, n, &r));
    assert(r.value.frame_permit.ttl_ms == UINT32_MAX);
    assert(!sophia_sf_decode(vector_ResourceStatus, sizeof(vector_ResourceStatus), &r));
    r.value.resource_status.reason = 1;
    assert(sophia_sf_encode(b, sizeof(b), &r, &n));
    r.value.resource_status.status = 2;
    assert(sophia_sf_encode(b, sizeof(b), &r, &n));
    r.value.resource_status.status = 3;
    assert(!sophia_sf_encode(b, sizeof(b), &r, &n));
    assert(!sophia_sf_decode(vector_AllocationRequest, sizeof(vector_AllocationRequest), &r));
    r.value.allocation_request.prior_id = 1;
    r.value.allocation_request.prior_generation = 1;
    assert(sophia_sf_encode(b, sizeof(b), &r, &n));
    r.value.allocation_request.operation = 2;
    assert(!sophia_sf_encode(b, sizeof(b), &r, &n));
    r.value.allocation_request.role = 2;
    assert(sophia_sf_encode(b, sizeof(b), &r, &n));
    assert(!sophia_sf_decode(vector_AllocationResult, sizeof(vector_AllocationResult), &r));
    r.value.allocation_result.margin_left = -1;
    assert(sophia_sf_encode(b, sizeof(b), &r, &n));
    r.value.allocation_result.status = 4;
    r.value.allocation_result.allocation_request_id = 0;
    r.value.allocation_result.allocation_id = 1;
    r.value.allocation_result.allocation_generation = 1;
    r.value.allocation_result.scale_numerator = UINT32_MAX;
    r.value.allocation_result.allowed_reservation_extent = UINT32_MAX;
    assert(!sophia_sf_encode(b, sizeof(b), &r, &n));
    assert(!sophia_sf_decode(b, n, &r));
    assert(r.value.allocation_result.allowed_reservation_extent == UINT32_MAX);
    assert(!sophia_sf_decode(vector_Limits, sizeof(vector_Limits), &r));
    r.value.limits.max_session_retiring_bytes = 11;
    assert(sophia_sf_encode(b, sizeof(b), &r, &n));
    assert(!sophia_sf_decode(vector_Candidate, sizeof(vector_Candidate), &r));
    r.value.candidate.placements[0].surface_index = 1;
    assert(sophia_sf_encode(b, sizeof(b), &r, &n));
    r.value.candidate.placements[0].surface_index = 0;
    r.value.candidate.surfaces[0].margin_top = -512;
    assert(!sophia_sf_encode(b, sizeof(b), &r, &n));
    assert(!sophia_sf_decode(b, n, &r));
    assert(r.value.candidate.surfaces[0].margin_top == -512);
    b[32 + 80 + 42] = 1;
    assert(sophia_sf_decode(b, n, &r));
}
static void maximum_candidate_rows(void)
{
    struct sophia_sf_record r, decoded;
    unsigned char bytes[8192];
    size_t n, i;
    assert(!sophia_sf_decode(vector_Candidate, sizeof(vector_Candidate), &r));
    r.value.candidate.surface_count = 8;
    r.value.candidate.placement_count = 32;
    r.value.candidate.target_count = 64;
    for (i = 1; i < 8; i++)
        r.value.candidate.surfaces[i] = r.value.candidate.surfaces[0];
    for (i = 1; i < 32; i++)
        r.value.candidate.placements[i] = r.value.candidate.placements[0];
    for (i = 1; i < 64; i++)
        r.value.candidate.targets[i] = r.value.candidate.targets[0];
    assert(!sophia_sf_encode(bytes, sizeof(bytes), &r, &n));
    assert(n == 32 + 80 + 8 * 64 + 32 * 32 + 64 * 48);
    assert(!sophia_sf_decode(bytes, n, &decoded));
    bytes[32 + 72] = 9;
    assert(sophia_sf_decode(bytes, n, &decoded));
    bytes[32 + 72] = 8;
    bytes[32 + 74] = 33;
    assert(sophia_sf_decode(bytes, n, &decoded));
    bytes[32 + 74] = 32;
    bytes[32 + 76] = 65;
    assert(sophia_sf_decode(bytes, n, &decoded));
}
int main(void)
{
    api_discovery();
    vectors_and_truncations();
    conditional_rules();
    maximum_candidate_rows();
    puts("shell files: 22 independent literal vectors and malformed controls passed");
    return 0;
}

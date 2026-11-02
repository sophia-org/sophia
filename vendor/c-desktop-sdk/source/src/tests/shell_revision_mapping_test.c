#include "sophia_shell_files.h"
#undef NDEBUG
#include <assert.h>
#include <stddef.h>

int main(void)
{
    static const uint16_t revisions[] = {0, 8, 9, 10, 11, 12, 13, UINT16_MAX};
    size_t i;
    unsigned mask;
    /* Independent grants for ring, draw and W2, including unsupported revisions. */
    for (i = 0; i < sizeof revisions / sizeof revisions[0]; ++i) {
        uint16_t revision = revisions[i];
        for (mask = 0; mask < 8; ++mask) {
            uint64_t caps = (uint64_t)mask << 15;
            uint16_t image = (revision == 10 || revision == 11 || revision == 12) &&
                (mask & 1) ? 10 : 0;
            int retire = revision == 12 && (mask & 4);
            assert(sophia_sf_owned_ring_image_revision(revision, caps) == image);
            assert(sophia_sf_retire_on_present_enabled(revision, caps) == !!retire);
        }
    }
    return 0;
}

#include "../sophia_shell_files.h"

int sophia_sf_retire_on_present_enabled(uint16_t revision, uint64_t capabilities)
{
    return revision == SOPHIA_SF_REVISION_RETIRE_ON_PRESENT &&
        (capabilities & SOPHIA_SF_CAP_CONTENT_RETIRE_ON_PRESENT) != 0;
}

uint16_t sophia_sf_owned_ring_image_revision(uint16_t revision, uint64_t capabilities)
{
    switch (revision) {
    case 10:
    case 11:
    case 12:
        return (capabilities & SOPHIA_SF_CAP_OWNED_RING) != 0 ?
            SOPHIA_SF_REVISION_OWNED_RING : 0;
    default:
        return 0;
    }
}

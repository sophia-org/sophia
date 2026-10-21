#define _GNU_SOURCE
#include "internal.h"
#include "../sophia_owned_ring.h"
#include <errno.h>
#ifdef __linux__
#include <fcntl.h>
#include <sys/mman.h>
#include <unistd.h>
#endif

static void cleared(struct sophia_ip_ring *r)
{
    memset(r, 0, sizeof *r);
    r->fd = -1;
}

int sophia_ip_ring_create(struct sophia_ip_ring *r, uint32_t slots, uint64_t capacity)
{
#ifdef __linux__
    size_t extent, bytes;
    void *map;
    int file, saved;
    if (!r)
        return SOPHIA_IPR_ARGUMENT;
    cleared(r);
    if (sophia_or_geometry(slots, capacity, &extent, &bytes) != SOPHIA_IC_OK)
        return SOPHIA_IPR_ARGUMENT;
    file = memfd_create("sophia-ring", MFD_CLOEXEC | MFD_ALLOW_SEALING);
    if (file < 0)
        return SOPHIA_IPR_REFUSED;
    /* As for an ISB: allocate now and keep the backing whole beneath the
     * mapping, so exhaustion is reported here rather than as SIGBUS. */
    if (fallocate(file, 0, 0, (off_t)bytes) ||
        fcntl(file, F_ADD_SEALS, F_SEAL_GROW | F_SEAL_SHRINK))
        goto refused;
    map = mmap(NULL, bytes, PROT_READ | PROT_WRITE, MAP_SHARED, file, 0);
    if (map == MAP_FAILED)
        goto refused;
    /* No writable alias but this mapping: not inherited by fork, and after
     * FUTURE_WRITE the fd allows neither pwrite nor a new writable map. */
    if (madvise(map, bytes, MADV_DONTFORK) ||
        fcntl(file, F_ADD_SEALS, F_SEAL_FUTURE_WRITE | F_SEAL_SEAL)) {
        saved = errno;
        munmap(map, bytes);
        errno = saved;
        goto refused;
    }
    r->base = map;
    r->bytes = bytes;
    r->extent = extent;
    r->capacity = capacity;
    r->slots = slots;
    r->fd = file;
    return SOPHIA_IPR_OK;
refused:
    saved = errno;
    close(file);
    errno = saved;
    return SOPHIA_IPR_REFUSED;
#else
    (void)slots;
    (void)capacity;
    if (r)
        cleared(r);
    errno = ENOTSUP;
    return SOPHIA_IPR_REFUSED;
#endif
}

void sophia_ip_ring_close(struct sophia_ip_ring *r)
{
    if (!r)
        return;
#ifdef __linux__
    if (r->base)
        munmap(r->base, r->bytes);
    if (r->fd >= 0)
        close(r->fd);
#endif
    cleared(r);
}

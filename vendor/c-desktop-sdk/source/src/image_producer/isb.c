#define _GNU_SOURCE
#include "internal.h"
#include <errno.h>
#ifdef __linux__
#include <fcntl.h>
#include <sys/mman.h>
#include <unistd.h>
#endif

#ifdef __linux__
#define ISB_SEALS (F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL)
#endif

int sophia_ip_isb_create_bounded(uint32_t width, uint32_t height, uint64_t max_bytes, int *fd,
                                 void **pixels, size_t *bytes)
{
#ifdef __linux__
    uint64_t n = (uint64_t)width * height;
    void *map;
    int file, saved;
    /* Every bound is checked before anything is allocated. The size must fit
     * size_t, a pointer difference across the mapping and a file offset. */
    if (!fd || !pixels || !bytes || !width || !height || width > UINT32_MAX / 4 ||
        n > UINT64_MAX / 4 || n * 4 > (uint64_t)SIZE_MAX || n * 4 > (uint64_t)PTRDIFF_MAX ||
        n * 4 > (uint64_t)INT64_MAX || n * 4 > max_bytes)
        return SOPHIA_IPR_ARGUMENT;
    n *= 4;
    file = memfd_create("sophia-image", MFD_CLOEXEC | MFD_ALLOW_SEALING);
    if (file < 0)
        return SOPHIA_IPR_REFUSED;
    /* Mode 0 sets the size and allocates every page now, so ordinary memory
     * exhaustion is reported here rather than as a fault while filling. The
     * size seals keep the backing whole beneath the mapping. */
    if (fallocate(file, 0, 0, (off_t)n) || fcntl(file, F_ADD_SEALS, F_SEAL_GROW | F_SEAL_SHRINK))
        goto refused;
    map = mmap(NULL, (size_t)n, PROT_READ | PROT_WRITE, MAP_SHARED, file, 0);
    if (map == MAP_FAILED)
        goto refused;
    /* A forked child would otherwise inherit a writable alias that blocks
     * F_SEAL_WRITE, or outlives it. */
    if (madvise(map, (size_t)n, MADV_DONTFORK)) {
        saved = errno;
        munmap(map, (size_t)n);
        errno = saved;
        goto refused;
    }
    *fd = file;
    *pixels = map;
    *bytes = (size_t)n;
    return SOPHIA_IPR_OK;
refused:
    saved = errno;
    close(file);
    errno = saved;
    return SOPHIA_IPR_REFUSED;
#else
    (void)width;
    (void)height;
    (void)max_bytes;
    (void)fd;
    (void)pixels;
    (void)bytes;
    errno = ENOTSUP;
    return SOPHIA_IPR_REFUSED;
#endif
}

int sophia_ip_isb_create(uint32_t width, uint32_t height, int *fd, void **pixels,
                            size_t *bytes)
{
    return sophia_ip_isb_create_bounded(width, height, UINT64_MAX, fd, pixels, bytes);
}

int sophia_ip_isb_finish(int fd, void *pixels, size_t bytes)
{
#ifdef __linux__
    int seals;
    /* F_SEAL_WRITE refuses while a writable shared mapping exists. */
    if (fd < 0 || !pixels || !bytes || munmap(pixels, bytes))
        return SOPHIA_IPR_ARGUMENT;
    if (fcntl(fd, F_ADD_SEALS, ISB_SEALS))
        return SOPHIA_IPR_REFUSED;
    seals = fcntl(fd, F_GET_SEALS);
    if (seals < 0)
        return SOPHIA_IPR_REFUSED;
    if ((seals & ISB_SEALS) != ISB_SEALS) {
        errno = EPERM;
        return SOPHIA_IPR_REFUSED;
    }
    return SOPHIA_IPR_OK;
#else
    (void)fd;
    (void)pixels;
    (void)bytes;
    errno = ENOTSUP;
    return SOPHIA_IPR_REFUSED;
#endif
}

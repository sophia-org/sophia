#define _GNU_SOURCE
#include "internal.h"
#include <errno.h>
#ifdef __linux__
#include <fcntl.h>
#include <sys/mman.h>
#include <unistd.h>
#endif

int sophia_ip_isb_create(uint32_t width, uint32_t height, int *fd, void **pixels,
                            size_t *bytes)
{
#ifdef __linux__
    uint64_t n = (uint64_t)width * height;
    void *map;
    int file, saved;
    if (!fd || !pixels || !bytes || !width || !height || n > UINT64_MAX / 4 ||
        n * 4 > (uint64_t)SIZE_MAX || n * 4 > (uint64_t)INT64_MAX)
        return SOPHIA_IPR_ARGUMENT;
    n *= 4;
    file = memfd_create("sophia-image", MFD_CLOEXEC | MFD_ALLOW_SEALING);
    if (file < 0)
        return SOPHIA_IPR_REFUSED;
    if (ftruncate(file, (off_t)n)) {
        saved = errno;
        close(file);
        errno = saved;
        return SOPHIA_IPR_REFUSED;
    }
    map = mmap(NULL, (size_t)n, PROT_READ | PROT_WRITE, MAP_SHARED, file, 0);
    if (map == MAP_FAILED) {
        saved = errno;
        close(file);
        errno = saved;
        return SOPHIA_IPR_REFUSED;
    }
    *fd = file;
    *pixels = map;
    *bytes = (size_t)n;
    return SOPHIA_IPR_OK;
#else
    (void)width;
    (void)height;
    (void)fd;
    (void)pixels;
    (void)bytes;
    errno = ENOTSUP;
    return SOPHIA_IPR_REFUSED;
#endif
}

int sophia_ip_isb_finish(int fd, void *pixels, size_t bytes)
{
#ifdef __linux__
    /* F_SEAL_WRITE refuses while a writable shared mapping exists. */
    if (fd < 0 || !pixels || !bytes || munmap(pixels, bytes))
        return SOPHIA_IPR_ARGUMENT;
    if (fcntl(fd, F_ADD_SEALS, F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL))
        return SOPHIA_IPR_REFUSED;
    return SOPHIA_IPR_OK;
#else
    (void)fd;
    (void)pixels;
    (void)bytes;
    errno = ENOTSUP;
    return SOPHIA_IPR_REFUSED;
#endif
}

/* The mapped ISB builder: bounds before allocation, exact pixels and length,
 * size seals while mapped, every seal verified after finish, refusal of
 * writes, resizes and writable remaps, fork exclusion and FD/mapping
 * cleanup on abandonment and failure. */
#define _GNU_SOURCE
#include "sophia_image_producer.h"
#include <assert.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

#define SEALS (F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL)

static unsigned fd_count(void)
{
    DIR *d = opendir("/proc/self/fd");
    unsigned n = 0;
    assert(d);
    while (readdir(d))
        n++;
    closedir(d);
    return n;
}

/* Mappings of ISB memfds in this process. */
static unsigned map_count(void)
{
    char line[512];
    unsigned n = 0;
    FILE *maps = fopen("/proc/self/maps", "r");
    assert(maps);
    while (fgets(line, sizeof line, maps))
        if (strstr(line, "memfd:sophia-image"))
            n++;
    fclose(maps);
    return n;
}

static void bounds_are_checked_before_allocation(void)
{
    unsigned fds = fd_count(), maps = map_count();
    void *pixels = (void *)&fds;
    size_t bytes = 7;
    int fd = -7;
    assert(sophia_ip_isb_create_bounded(0, 1, UINT64_MAX, &fd, &pixels, &bytes) ==
           SOPHIA_IPR_ARGUMENT);
    assert(sophia_ip_isb_create_bounded(1, 0, UINT64_MAX, &fd, &pixels, &bytes) ==
           SOPHIA_IPR_ARGUMENT);
    /* One byte under the exact size, a stride above UINT32_MAX and an
     * overflowing size are refused; outputs are untouched. */
    assert(sophia_ip_isb_create_bounded(4, 2, 31, &fd, &pixels, &bytes) == SOPHIA_IPR_ARGUMENT);
    assert(sophia_ip_isb_create_bounded(UINT32_C(1) << 30, 1, UINT64_MAX, &fd, &pixels, &bytes) ==
           SOPHIA_IPR_ARGUMENT);
    assert(sophia_ip_isb_create_bounded(UINT32_MAX / 4, UINT32_MAX, UINT64_MAX, &fd, &pixels,
                                        &bytes) == SOPHIA_IPR_ARGUMENT);
    assert(sophia_ip_isb_create_bounded(4, 2, 32, NULL, &pixels, &bytes) == SOPHIA_IPR_ARGUMENT);
    /* One row more than fits a pointer difference and a file offset. On a
     * 32-bit target a single 4 GiB row already exceeds PTRDIFF_MAX. */
    {
        uint64_t limit = (uint64_t)PTRDIFF_MAX < (uint64_t)INT64_MAX ? (uint64_t)PTRDIFF_MAX
                                                                     : (uint64_t)INT64_MAX;
        uint32_t width = (UINT32_C(1) << 30) - 1;
        uint64_t height = limit / ((uint64_t)width * 4) + 1;
        assert(height <= UINT32_MAX && (uint64_t)width * 4 * height > limit);
        assert(sophia_ip_isb_create_bounded(width, (uint32_t)height, UINT64_MAX, &fd, &pixels,
                                            &bytes) == SOPHIA_IPR_ARGUMENT);
    }
    assert(fd == -7 && pixels == (void *)&fds && bytes == 7);
    assert(fd_count() == fds && map_count() == maps);
    assert(sophia_ip_isb_create_bounded(4, 2, 32, &fd, &pixels, &bytes) == SOPHIA_IPR_OK);
    assert(bytes == 32);
    assert(sophia_ip_isb_finish(fd, pixels, bytes) == SOPHIA_IPR_OK);
    close(fd);
}

static void exact_pixels_and_length_survive_sealing(void)
{
    uint8_t pattern[24], back[25];
    struct stat st;
    void *pixels;
    size_t bytes, i;
    int fd;
    assert(sophia_ip_isb_create_bounded(3, 2, 24, &fd, &pixels, &bytes) == SOPHIA_IPR_OK);
    assert(bytes == 24);
    for (i = 0; i < bytes; i++) {
        assert(((uint8_t *)pixels)[i] == 0);
        pattern[i] = (uint8_t)(i * 7 + 1);
    }
    memcpy(pixels, pattern, sizeof pattern);
    assert(sophia_ip_isb_finish(fd, pixels, bytes) == SOPHIA_IPR_OK);
    assert(fstat(fd, &st) == 0 && st.st_size == 24);
    assert(pread(fd, back, sizeof back, 0) == 24);
    assert(memcmp(back, pattern, sizeof pattern) == 0);
    close(fd);
}

static void size_is_sealed_while_mapped(void)
{
    void *pixels;
    size_t bytes;
    int fd, seals;
    assert(sophia_ip_isb_create_bounded(2, 2, 16, &fd, &pixels, &bytes) == SOPHIA_IPR_OK);
    seals = fcntl(fd, F_GET_SEALS);
    assert(seals == (F_SEAL_GROW | F_SEAL_SHRINK));
    assert(ftruncate(fd, 8) < 0 && errno == EPERM);
    assert(ftruncate(fd, 32) < 0 && errno == EPERM);
    ((uint8_t *)pixels)[15] = 9;
    assert(sophia_ip_isb_finish(fd, pixels, bytes) == SOPHIA_IPR_OK);
    close(fd);
}

static void finish_verifies_every_seal_and_the_backing_refuses_change(void)
{
    void *pixels;
    size_t bytes;
    int fd;
    assert(sophia_ip_isb_create_bounded(2, 2, 16, &fd, &pixels, &bytes) == SOPHIA_IPR_OK);
    assert(sophia_ip_isb_finish(fd, pixels, bytes) == SOPHIA_IPR_OK);
    assert((fcntl(fd, F_GET_SEALS) & SEALS) == SEALS);
    assert(write(fd, "x", 1) < 0 && errno == EPERM);
    assert(pwrite(fd, "x", 1, 0) < 0 && errno == EPERM);
    assert(ftruncate(fd, 32) < 0 && errno == EPERM);
    assert(ftruncate(fd, 8) < 0 && errno == EPERM);
    assert(fallocate(fd, 0, 0, 32) < 0 && errno == EPERM);
    assert(mmap(NULL, bytes, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0) == MAP_FAILED &&
           errno == EPERM);
    assert(fcntl(fd, F_ADD_SEALS, F_SEAL_WRITE) < 0 && errno == EPERM);
    close(fd);
}

/* With the mapping excluded from fork, finish succeeds while a child lives
 * and the child cannot write the buffer. Re-enabling inheritance is the
 * negative control: the child's writable alias makes F_SEAL_WRITE EBUSY. */
static void fork_holds_no_writable_alias(int control)
{
    void *pixels;
    size_t bytes;
    int fd, go[2], status;
    pid_t child;
    char byte = 0;
    assert(sophia_ip_isb_create_bounded(4, 4, 64, &fd, &pixels, &bytes) == SOPHIA_IPR_OK);
    if (control)
        assert(madvise(pixels, bytes, MADV_DOFORK) == 0);
    assert(pipe(go) == 0);
    child = fork();
    assert(child >= 0);
    if (child == 0) {
        close(go[1]);
        if (read(go[0], &byte, 1) != 1)
            _exit(2);
        ((volatile uint8_t *)pixels)[0] = 1;
        _exit(control ? 0 : 3);
    }
    close(go[0]);
    if (control) {
        assert(sophia_ip_isb_finish(fd, pixels, bytes) == SOPHIA_IPR_REFUSED && errno == EBUSY);
    } else {
        assert(sophia_ip_isb_finish(fd, pixels, bytes) == SOPHIA_IPR_OK);
        assert((fcntl(fd, F_GET_SEALS) & SEALS) == SEALS);
    }
    assert(write(go[1], &byte, 1) == 1);
    close(go[1]);
    assert(waitpid(child, &status, 0) == child);
    if (control)
        assert(WIFEXITED(status) && WEXITSTATUS(status) == 0);
    else
        assert(WIFSIGNALED(status) && WTERMSIG(status) == SIGSEGV);
    close(fd);
}

static unsigned long vm_size(void)
{
    char line[256];
    unsigned long kb = 0;
    FILE *status = fopen("/proc/self/status", "r");
    assert(status);
    while (fgets(line, sizeof line, status))
        if (sscanf(line, "VmSize: %lu kB", &kb) == 1)
            break;
    fclose(status);
    assert(kb);
    return kb * 1024;
}

static void every_outcome_releases_what_it_created(void)
{
    unsigned fds = fd_count(), maps = map_count();
    struct rlimit saved, lowered;
    void *pixels;
    size_t bytes;
    int fd, result, error;
    assert(maps == 0);
    /* Abandonment: the caller unmaps and closes. */
    assert(sophia_ip_isb_create_bounded(4, 4, 64, &fd, &pixels, &bytes) == SOPHIA_IPR_OK);
    assert(fd_count() == fds + 1 && map_count() == 1);
    munmap(pixels, bytes);
    close(fd);
    assert(fd_count() == fds && map_count() == maps);
    /* Finish unmaps and leaves only the caller's descriptor. */
    assert(sophia_ip_isb_create_bounded(4, 4, 64, &fd, &pixels, &bytes) == SOPHIA_IPR_OK);
    assert(sophia_ip_isb_finish(fd, pixels, bytes) == SOPHIA_IPR_OK);
    assert(fd_count() == fds + 1 && map_count() == 0);
    close(fd);
    /* A mapping refused after the memfd was created and allocated closes
     * the memfd. An address-space limit just above current use makes only
     * the 4 MiB mapping fail. */
    assert(getrlimit(RLIMIT_AS, &saved) == 0);
    lowered = saved;
    lowered.rlim_cur = vm_size() + 1024 * 1024;
    assert(setrlimit(RLIMIT_AS, &lowered) == 0);
    result = sophia_ip_isb_create_bounded(1024, 1024, UINT64_MAX, &fd, &pixels, &bytes);
    error = errno;
    assert(setrlimit(RLIMIT_AS, &saved) == 0);
    assert(result == SOPHIA_IPR_REFUSED && error == ENOMEM);
    assert(fd_count() == fds && map_count() == maps);
}

/* The unbounded entry point shares the hardened path. */
static void unbounded_create_is_hardened(void)
{
    void *pixels;
    size_t bytes;
    int fd;
    assert(sophia_ip_isb_create(2, 1, &fd, &pixels, &bytes) == SOPHIA_IPR_OK && bytes == 8);
    assert(fcntl(fd, F_GET_SEALS) == (F_SEAL_GROW | F_SEAL_SHRINK));
    assert(sophia_ip_isb_finish(fd, pixels, bytes) == SOPHIA_IPR_OK);
    assert((fcntl(fd, F_GET_SEALS) & SEALS) == SEALS);
    close(fd);
}

int main(void)
{
    every_outcome_releases_what_it_created();
    bounds_are_checked_before_allocation();
    exact_pixels_and_length_survive_sealing();
    size_is_sealed_while_mapped();
    finish_verifies_every_seal_and_the_backing_refuses_change();
    fork_holds_no_writable_alias(0);
    fork_holds_no_writable_alias(1);
    unbounded_create_is_hardened();
    puts("isb_mapped_test: ok");
    return 0;
}

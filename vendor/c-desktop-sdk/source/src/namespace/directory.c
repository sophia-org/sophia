#define _GNU_SOURCE
#include "internal.h"
#include <errno.h>
#ifdef __linux__
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

/* Each component is opened against the previous pinned directory. */
static int traverse(int root, const char *path, int socket_leaf) {
    int directory = fcntl(root, F_DUPFD_CLOEXEC, 0), next, saved;
    const char *slash;
    char component[256];
    size_t n;
    if (directory < 0) return -1;
    while (*path) {
        slash = strchr(path, '/');
        n = slash ? (size_t)(slash - path) : strlen(path);
        if (!n || n > 255 || (n == 1 && *path == '.') || (n == 2 && !memcmp(path, "..", 2))) {
            close(directory); errno = EINVAL; return -1;
        }
        memcpy(component, path, n); component[n] = 0;
        next = openat(directory, component, O_CLOEXEC | O_NOFOLLOW |
                      ((!slash && socket_leaf) ? O_PATH : (O_RDONLY | O_DIRECTORY)));
        saved = errno; close(directory); errno = saved;
        if (next < 0) return -1;
        directory = next;
        if (!slash) break;
        path = slash + 1;
    }
    return directory;
}
int sophia_ns_open(const char *path, struct sophia_ns_discovery *out) {
    int root, directory, file, result, saved;
    struct stat st;
    unsigned char *bytes;
    size_t length = 0;
    ssize_t got;
    if (!path || *path != '/' || !out) { errno = EINVAL; return -1; }
    root = open("/", O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    if (root < 0) return -1;
    directory = traverse(root, path + 1, 0);
    saved = errno; close(root); errno = saved;
    if (directory < 0) return -1;
    file = openat(directory, SOPHIA_NS_DISCOVERY_FILE, O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK);
    if (file < 0) goto fail;
    if (fstat(file, &st) || !S_ISREG(st.st_mode) || st.st_size < 0 || (uint64_t)st.st_size > SOPHIA_NS_MAX_BYTES) {
        close(file); errno = EINVAL; goto fail;
    }
    bytes = malloc(SOPHIA_NS_MAX_BYTES + 1u);
    if (!bytes) { close(file); errno = ENOMEM; goto fail; }
    while (length <= SOPHIA_NS_MAX_BYTES) {
        got = read(file, bytes + length, SOPHIA_NS_MAX_BYTES + 1u - length);
        if (got < 0 && errno == EINTR) continue;
        if (got <= 0) break;
        length += (size_t)got;
    }
    saved = errno; close(file); errno = saved;
    result = got < 0 ? -1 : sophia_ns_parse(out, bytes, length);
    saved = errno; free(bytes); errno = saved;
    if (!result) return directory;
fail:
    saved = errno; close(directory); errno = saved;
    return -1;
}
int sophia_ns_connect(int directory, const struct sophia_ns_discovery *view, const char *path) {
    const struct sophia_ns_endpoint *endpoint = sophia_ns_find(view, path);
    struct stat st;
    struct sockaddr_un address;
    int node, stream, n, result, saved;
    if (!endpoint || !sophia_ns_path_valid((const unsigned char *)endpoint->path, strlen(endpoint->path))) { errno = EINVAL; return -1; }
    node = traverse(directory, endpoint->path, 1);
    if (node < 0) return -1;
    if (fstat(node, &st) || !S_ISSOCK(st.st_mode)) { close(node); errno = EINVAL; return -1; }
    memset(&address, 0, sizeof address); address.sun_family = AF_UNIX;
    n = snprintf(address.sun_path, sizeof address.sun_path, "/proc/self/fd/%d", node);
    if (n < 0 || (size_t)n >= sizeof address.sun_path) { close(node); errno = EINVAL; return -1; }
    stream = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    if (stream < 0) { saved = errno; close(node); errno = saved; return -1; }
    result = connect(stream, (const struct sockaddr *)&address, (socklen_t)(offsetof(struct sockaddr_un, sun_path) + (size_t)n + 1));
    saved = errno; close(node); errno = saved;
    if (result) { saved = errno; close(stream); errno = saved; return -1; }
    return stream;
}
#else
int sophia_ns_open(const char *p, struct sophia_ns_discovery *v) { (void)p; (void)v; errno = ENOTSUP; return -1; }
int sophia_ns_connect(int fd, const struct sophia_ns_discovery *v, const char *p) { (void)fd; (void)v; (void)p; errno = ENOTSUP; return -1; }
#endif

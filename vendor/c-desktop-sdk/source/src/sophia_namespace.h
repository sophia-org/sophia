#ifndef SOPHIA_NAMESPACE_H
#define SOPHIA_NAMESPACE_H
#include <stddef.h>
#include <stdint.h>

#define SOPHIA_NS_DIRECTORY_ENV "SOPHIA_NAMESPACE_DIR"
#define SOPHIA_NS_DISCOVERY_FILE "discovery"
#define SOPHIA_NS_MAX_BYTES 131072u
#define SOPHIA_NS_MAX_ENDPOINTS 64u

enum sophia_ns_protocol { SOPHIA_NS_9P2000L = 1, SOPHIA_NS_X11 = 2 };
enum sophia_ns_availability { SOPHIA_NS_AVAILABLE = 1, SOPHIA_NS_WAITING = 2, SOPHIA_NS_RECOVERING = 3 };
struct sophia_ns_endpoint {
    char path[1024], interface[256];
    uint64_t service, generation;
    uint32_t version;
    enum sophia_ns_protocol protocol;
    enum sophia_ns_availability availability;
};
struct sophia_ns_discovery {
    uint64_t revision;
    size_t count;
    struct sophia_ns_endpoint endpoints[SOPHIA_NS_MAX_ENDPOINTS];
};

/* Passive visibility records, never credentials or operation grants.
 * Parse returns 0 or -1/EINVAL; output is unchanged on failure. No heap
 * allocation. Caller owns the bounded input and output storage. */
int sophia_ns_parse(struct sophia_ns_discovery *, const void *, size_t);
const struct sophia_ns_endpoint *sophia_ns_find(const struct sophia_ns_discovery *, const char *);

/* Linux: pin an absolute directory without following symlinks, read/validate
 * discovery, and return an owned CLOEXEC directory fd. The caller closes it.
 * At most MAX_BYTES+1 temporary bytes are allocated. Output is unchanged on
 * failure. No implicit environment access or process-wide chdir. */
int sophia_ns_open(const char *directory, struct sophia_ns_discovery *);
/* Nonblocking connect through a pinned socket inode. Returns an owned CLOEXEC
 * stream fd, or -1 with errno. Caller checks protocol/interface and performs
 * its authentication. Neither availability nor a successful connect grants
 * permission. Other platforms refuse ENOTSUP. */
int sophia_ns_connect(int directory_fd, const struct sophia_ns_discovery *, const char *path);
#endif

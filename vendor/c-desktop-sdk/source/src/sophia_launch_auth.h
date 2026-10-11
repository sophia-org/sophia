#ifndef SOPHIA_LAUNCH_AUTH_H
#define SOPHIA_LAUNCH_AUTH_H
#include "sophia_9p_client.h"

/* Session-attested launch authentication, not principal selection. The caller
 * supplies distinct role and provisioned relay transports as fresh 9P clients.
 * Both clients and their disjoint storage remain exclusively borrowed until
 * ready or terminal. No allocator, clock, fd discovery, connect or close.
 * Claims are copied (at most 255 bytes each); they confer no authority.
 * Initialization marks the provisioned relay fd CLOEXEC before starting I/O;
 * a descriptor-flag failure refuses initialization. The caller must not exec
 * or delegate the inherited capability before handing it to this helper.
 * On failure/cancel the caller must retire these clients and close the role
 * transport; do not continue role traffic or reuse pending replies. */
struct sophia_la_client {
    struct sophia_9p_client *role, *relay;
    struct sophia_9p_handle request;
    uint64_t deadline, now, retry;
    uint32_t afid, root, relay_root, rpc, remote_error;
    int terminal;
    unsigned stage, pending;
    char uname[256], aname[256];
    uint8_t challenge[40];
};
int sophia_la_init(struct sophia_la_client *, struct sophia_9p_client *role,
                   struct sophia_9p_client *relay, const char *uname, const char *aname,
                   uint64_t now_msec);
/* One bounded pass, at most one reply consumed and one service call per wire.
 * 0 means no terminal error, not ready. Time is caller-supplied monotonic ms;
 * a backwards clock or the fixed five-second total deadline fails closed. */
int sophia_la_service(struct sophia_la_client *, uint64_t now_msec, size_t byte_budget);
int sophia_la_ready(const struct sophia_la_client *);
/* Root is NOFID until ready. Both wires are then quiescent. Only the role root
 * remains; the temporary afid and relay fids have been clunked. The launch
 * channel itself stays open for a later fresh conversation/Tversion. */
uint32_t sophia_la_root(const struct sophia_la_client *);
uint32_t sophia_la_remote_error(const struct sophia_la_client *);
/* Absolute wake deadline; UINT64_MAX after completion/failure. Also poll both
 * fds for input and sophia_9p_wants_write() for output while not ready. */
uint64_t sophia_la_next_wake(const struct sophia_la_client *);
void sophia_la_cancel(struct sophia_la_client *);
#endif

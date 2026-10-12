#include "../sophia_launch_auth.h"
#include <fcntl.h>
#include <string.h>

enum {
    VERSION_ROLE,
    VERSION_RELAY,
    RELAY_ATTACH,
    RELAY_WALK,
    RELAY_OPEN,
    AUTH,
    CHALLENGE,
    RELAY_WRITE,
    RELAY_READ,
    STATUS,
    ATTACH,
    AFID_CLUNK,
    RPC_CLUNK,
    ROOT_CLUNK,
    DONE
};
static int claim(char out[256], const char *value) {
    size_t n;
    if (!value)
        return SOPHIA_9P_ARGUMENT;
    for (n = 0; n < 256; n++) {
        out[n] = value[n];
        if (!value[n])
            return 0;
    }
    return SOPHIA_9P_ARGUMENT;
}
static uint32_t u32(const uint8_t *p) {
    return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
}
static int relay_stage(unsigned stage) {
    return (stage >= VERSION_RELAY && stage <= RELAY_OPEN) || stage == RELAY_WRITE ||
           stage == RELAY_READ || stage == RPC_CLUNK || stage == ROOT_CLUNK;
}
int sophia_la_init(struct sophia_la_client *c, struct sophia_9p_client *role,
                   struct sophia_9p_client *relay, const char *uname, const char *aname,
                   uint64_t now) {
    struct sophia_la_client next;
    int flags;
    if (!c || !role || !relay || role == relay || role->fd == relay->fd || role->phase ||
        relay->phase || role->terminal || relay->terminal || role->fid_limit < 2 ||
        relay->fid_limit < 2 || !role->storage || !relay->storage || now > UINT64_MAX - 5000)
        return SOPHIA_9P_ARGUMENT;
    memset(&next, 0, sizeof next);
    if (claim(next.uname, uname) || claim(next.aname, aname))
        return SOPHIA_9P_ARGUMENT;
    flags = fcntl(relay->fd, F_GETFD);
    if (flags < 0 || fcntl(relay->fd, F_SETFD, flags | FD_CLOEXEC) < 0)
        return SOPHIA_9P_IO;
    next.role = role;
    next.relay = relay;
    next.now = now;
    next.deadline = now + 5000;
    next.afid = next.root = next.relay_root = next.rpc = UINT32_MAX;
    *c = next;
    return 0;
}
int sophia_la_next_endpoint(struct sophia_la_client *c,
                            const struct sophia_la_client *completed,
                            struct sophia_9p_client *role, const char *uname,
                            const char *aname, uint64_t now) {
    struct sophia_9p_client *relay;
    char checked[256];
    size_t i, bytes;
    int result;
    if (!c || !completed || !sophia_la_ready(completed) || !role ||
        role == completed->role || !(relay = completed->relay) || role == relay ||
        role->fd == relay->fd || role->fd == completed->role->fd || role->phase || role->terminal ||
        role->fid_limit < 2 || !role->storage || now > UINT64_MAX - 5000 ||
        claim(checked, uname) || claim(checked, aname))
        return SOPHIA_9P_ARGUMENT;
    if (relay->terminal || relay->phase != 2 || relay->fid_count || relay->rx_used)
        return SOPHIA_9P_BUSY;
    for (i = 0; i < SOPHIA_9P_REQUESTS * 2; ++i)
        if (relay->slots[i].state)
            return SOPHIA_9P_BUSY;
    bytes = sophia_9p_storage_bytes(relay->offered, relay->capacity);
    result = sophia_9p_init(relay, relay->fd, relay->offered, relay->capacity,
                            relay->fid_limit, relay->storage, bytes);
    if (result)
        return result;
    return sophia_la_init(c, role, relay, uname, aname, now);
}
static int start(struct sophia_la_client *c) {
    const char *rpc = "rpc";
    switch (c->stage) {
    case VERSION_ROLE:
        return sophia_9p_version(c->role, &c->request);
    case VERSION_RELAY:
        return sophia_9p_version(c->relay, &c->request);
    case RELAY_ATTACH:
        return sophia_9p_attach(c->relay, "", "sophia-launch-relay-v1", &c->request,
                                &c->relay_root);
    case RELAY_WALK:
        return sophia_9p_walk(c->relay, c->relay_root, &rpc, 1, &c->request, &c->rpc);
    case RELAY_OPEN:
        return sophia_9p_lopen(c->relay, c->rpc, 2, &c->request);
    case AUTH:
        return sophia_9p_auth(c->role, c->uname, c->aname, &c->request, &c->afid);
    case CHALLENGE:
        return sophia_9p_read(c->role, c->afid, 0, 40, &c->request);
    case RELAY_WRITE:
        return sophia_9p_write(c->relay, c->rpc, 0, c->challenge, 40, &c->request);
    case RELAY_READ:
        return sophia_9p_read(c->relay, c->rpc, 0, 8, &c->request);
    case STATUS:
        return sophia_9p_read(c->role, c->afid, 40, 8, &c->request);
    case ATTACH:
        return sophia_9p_attach_with_afid(c->role, c->afid, c->uname, c->aname, &c->request,
                                          &c->root);
    case AFID_CLUNK:
        return sophia_9p_clunk(c->role, c->afid, &c->request);
    case RPC_CLUNK:
        return sophia_9p_clunk(c->relay, c->rpc, &c->request);
    case ROOT_CLUNK:
        return sophia_9p_clunk(c->relay, c->relay_root, &c->request);
    default:
        return SOPHIA_9P_INVALID;
    }
}
static int answer(struct sophia_la_client *c, const struct sophia_9p_reply *r) {
    if (r->type == 7) {
        if (c->stage == AUTH && r->error == 11) {
            /* Exact prior-attempt cancellation may still be draining. This
             * retry spends no new deadline and never falls back to NOFID. */
            c->retry = c->deadline - c->now <= 10 ? c->deadline : c->now + 10;
            return 0;
        }
        c->remote_error = r->error;
        return SOPHIA_9P_INVALID;
    }
    if (c->stage == RELAY_WALK && r->count != 1)
        return SOPHIA_9P_INVALID;
    if (c->stage == CHALLENGE) {
        if (r->count != 40 || u32(r->data) != 1 || u32(r->data + 4) != 1)
            return SOPHIA_9P_INVALID;
        memcpy(c->challenge, r->data, 40);
    }
    if (c->stage == RELAY_WRITE && r->count != 40)
        return SOPHIA_9P_INVALID;
    if (c->stage == RELAY_READ || c->stage == STATUS) {
        if (r->count != 8 || u32(r->data) != 1)
            return SOPHIA_9P_INVALID;
        c->remote_error = u32(r->data + 4);
        if (c->remote_error)
            return SOPHIA_9P_INVALID;
    }
    c->retry = 0;
    c->stage++;
    return 0;
}
int sophia_la_service(struct sophia_la_client *c, uint64_t now, size_t budget) {
    struct sophia_9p_client *wire;
    struct sophia_9p_reply reply;
    int result;
    if (!c || !c->role || !c->relay)
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal)
        return c->terminal;
    if (c->stage == DONE)
        return 0;
    if (now < c->now || now >= c->deadline)
        return c->terminal = SOPHIA_9P_CLOSED;
    c->now = now;
    if (!c->pending && now >= c->retry) {
        result = start(c);
        if (result)
            return c->terminal = result;
        c->pending = 1;
    }
    result = sophia_9p_service(c->role, budget);
    if (!result)
        result = sophia_9p_service(c->relay, budget);
    if (result)
        return c->terminal = result;
    if (!c->pending)
        return 0;
    wire = relay_stage(c->stage) ? c->relay : c->role;
    result = sophia_9p_peek(wire, &reply);
    if (result == SOPHIA_9P_AGAIN)
        return 0;
    if (result || reply.handle.serial != c->request.serial || reply.handle.slot != c->request.slot)
        return c->terminal = result ? result : SOPHIA_9P_INVALID;
    result = answer(c, &reply);
    if (sophia_9p_consume(wire, reply.handle))
        result = SOPHIA_9P_INVALID;
    c->pending = 0;
    if (result)
        c->terminal = result;
    return result;
}
int sophia_la_ready(const struct sophia_la_client *c) {
    return c && !c->terminal && c->stage == DONE;
}
uint32_t sophia_la_root(const struct sophia_la_client *c) {
    return sophia_la_ready(c) ? c->root : UINT32_MAX;
}
uint32_t sophia_la_remote_error(const struct sophia_la_client *c) {
    return c ? c->remote_error : 0;
}
uint64_t sophia_la_next_wake(const struct sophia_la_client *c) {
    if (!c || c->terminal || c->stage == DONE)
        return UINT64_MAX;
    if (c->pending)
        return c->deadline;
    return c->retry > c->now ? c->retry : c->now;
}
void sophia_la_cancel(struct sophia_la_client *c) {
    if (c)
        c->terminal = SOPHIA_9P_CLOSED;
}

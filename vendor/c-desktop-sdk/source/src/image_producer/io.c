#define _GNU_SOURCE
#include "internal.h"
#include <errno.h>
#include <poll.h>
#ifdef __linux__
#include <fcntl.h>
#include <stdio.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>
#endif

int sophia_ip_connect(int endpoint_dir, const char *endpoint, int *fd)
{
#ifdef __linux__
    struct sockaddr_un address;
    struct stat st;
    size_t n;
    int node, sock, length, result, saved;
    if (!endpoint || !fd || endpoint_dir < 0)
        return SOPHIA_IPR_ARGUMENT;
    n = strlen(endpoint);
    if (!n || n > SOPHIA_IC_ENDPOINT_MAX || strchr(endpoint, '/') || !strcmp(endpoint, ".") ||
        !strcmp(endpoint, ".."))
        return SOPHIA_IPR_ARGUMENT;
    node = openat(endpoint_dir, endpoint, O_PATH | O_NOFOLLOW | O_CLOEXEC);
    if (node < 0)
        return SOPHIA_IPR_REFUSED;
    if (fstat(node, &st) || !S_ISSOCK(st.st_mode)) {
        close(node);
        errno = EINVAL;
        return SOPHIA_IPR_REFUSED;
    }
    memset(&address, 0, sizeof address);
    address.sun_family = AF_UNIX;
    length = snprintf(address.sun_path, sizeof address.sun_path, "/proc/self/fd/%d", node);
    sock = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    if (sock < 0) {
        saved = errno;
        close(node);
        errno = saved;
        return SOPHIA_IPR_REFUSED;
    }
    result = connect(sock, (const struct sockaddr *)&address,
                     (socklen_t)(offsetof(struct sockaddr_un, sun_path) + (size_t)length + 1));
    saved = errno;
    close(node);
    if (result) {
        close(sock);
        errno = saved;
        /* A full listener backlog: the caller retries within its deadline. */
        return saved == EAGAIN || saved == EWOULDBLOCK ? SOPHIA_IPR_AGAIN : SOPHIA_IPR_REFUSED;
    }
    *fd = sock;
    return SOPHIA_IPR_OK;
#else
    (void)endpoint_dir;
    (void)endpoint;
    (void)fd;
    errno = ENOTSUP;
    return SOPHIA_IPR_REFUSED;
#endif
}

int sophia_ip_attach(struct sophia_ip *p, int fd, uint64_t now_ms)
{
    if (!p || fd < 0 || !ip_clock(p, now_ms))
        return SOPHIA_IPR_ARGUMENT;
    if (p->channel != SOPHIA_IPC_DISCOVERED)
        return p->channel == SOPHIA_IPC_CLOSED ? SOPHIA_IPR_CLOSED : SOPHIA_IPR_REFUSED;
    if (now_ms >= p->config.startup_deadline_ms) {
        ip_fail(p, SOPHIA_IPX_TIMEOUT);
        return SOPHIA_IPR_CLOSED;
    }
#ifdef __linux__
    {
        int type;
        socklen_t size = sizeof type;
        if (getsockopt(fd, SOL_SOCKET, SO_TYPE, &type, &size) || type != SOCK_SEQPACKET)
            return SOPHIA_IPR_REFUSED;
    }
#else
    return SOPHIA_IPR_REFUSED;
#endif
    p->fd = fd;
    p->hello_deadline_ms = now_ms + p->hello_timeout_ms;
    if (p->hello_deadline_ms < now_ms || p->hello_deadline_ms > p->config.startup_deadline_ms)
        p->hello_deadline_ms = p->config.startup_deadline_ms;
    p->channel = SOPHIA_IPC_HELLO_QUEUED;
    return SOPHIA_IPR_OK;
}

static int source_queued(const struct sophia_ip *p)
{
    unsigned i;
    if (p->channel != SOPHIA_IPC_READY)
        return 0;
    for (i = 0; i < SOPHIA_IP_MAX_OFFERS; i++)
        if (p->slots[i].state == SOPHIA_IPO_ADMITTED && !p->slots[i].cancelled)
            return 1;
    return 0;
}

int sophia_ip_poll_fd(const struct sophia_ip *p)
{
    return p ? p->fd : -1;
}

short sophia_ip_poll_events(const struct sophia_ip *p)
{
    if (!p || p->fd < 0)
        return 0;
    return (short)(POLLIN |
                   (p->channel == SOPHIA_IPC_HELLO_QUEUED || source_queued(p) ? POLLOUT : 0));
}

#ifdef __linux__
int ip_send_packet(int sock, const uint8_t *b, size_t n, int fd)
{
    struct msghdr msg;
    struct iovec iov;
    union {
        struct cmsghdr align;
        char bytes[CMSG_SPACE(sizeof(int))];
    } control;
    ssize_t sent;
    memset(&msg, 0, sizeof msg);
    iov.iov_base = (void *)b;
    iov.iov_len = n;
    msg.msg_iov = &iov;
    msg.msg_iovlen = 1;
    if (fd >= 0) {
        struct cmsghdr *c;
        memset(&control, 0, sizeof control);
        msg.msg_control = control.bytes;
        msg.msg_controllen = sizeof control.bytes;
        c = CMSG_FIRSTHDR(&msg);
        c->cmsg_level = SOL_SOCKET;
        c->cmsg_type = SCM_RIGHTS;
        c->cmsg_len = CMSG_LEN(sizeof(int));
        memcpy(CMSG_DATA(c), &fd, sizeof fd);
    }
    do
        sent = sendmsg(sock, &msg, MSG_NOSIGNAL | MSG_DONTWAIT);
    while (sent < 0 && errno == EINTR);
    if (sent == (ssize_t)n)
        return 1;
    if (sent < 0 && (errno == EAGAIN || errno == EWOULDBLOCK))
        return 0;
    if (sent >= 0)
        errno = EMSGSIZE;
    return -1;
}

/* The owner never writes on this socket: readable data is a fault, and an
 * end of stream is the peer's disconnect. */
static int hung_up(struct sophia_ip *p, short revents)
{
    uint8_t byte;
    ssize_t got;
    if (!(revents & (POLLIN | POLLHUP | POLLERR)))
        return 0;
    got = recv(p->fd, &byte, sizeof byte, MSG_DONTWAIT | MSG_PEEK);
    if (got < 0 && (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR))
        return 0;
    ip_fail(p, got > 0 ? SOPHIA_IPX_FAULT : SOPHIA_IPX_UNAVAILABLE);
    return 1;
}

static int send_hello(struct sophia_ip *p)
{
    uint8_t b[SOPHIA_IC_HELLO_BYTES];
    int r;
    if (sophia_ic_hello_encode(b, &p->expect) != SOPHIA_IC_OK) {
        ip_fail(p, SOPHIA_IPX_FAULT);
        return 1;
    }
    r = ip_send_packet(p->fd, b, sizeof b, -1);
    if (!r)
        return 0;
    if (r < 0)
        ip_fail(p, errno == EPIPE || errno == ECONNRESET ? SOPHIA_IPX_UNAVAILABLE
                                                         : SOPHIA_IPX_FAULT);
    else
        p->channel = SOPHIA_IPC_HELLO_SENT;
    return 1;
}

/* Sends admitted sources in offer order; stops at the first EAGAIN. */
static int send_sources(struct sophia_ip *p)
{
    int changed = 0;
    for (;;) {
        struct sophia_ip_slot *next = NULL;
        struct sophia_ic_plane planes[SOPHIA_IC_PLANES];
        uint8_t b[SOPHIA_IC_SOURCE_BYTES];
        unsigned i;
        int r;
        if (p->channel != SOPHIA_IPC_READY)
            return changed;
        for (i = 0; i < SOPHIA_IP_MAX_OFFERS; i++)
            if (p->slots[i].state == SOPHIA_IPO_ADMITTED && !p->slots[i].cancelled &&
                (!next || p->slots[i].metadata.offer < next->metadata.offer))
                next = &p->slots[i];
        if (!next)
            return changed;
        memset(planes, 0, sizeof planes);
        planes[0].stride = next->metadata.stride;
        if (sophia_ic_source_encode(b, &p->expect, &next->metadata, planes) != SOPHIA_IC_OK) {
            ip_fail(p, SOPHIA_IPX_FAULT);
            return 1;
        }
        r = ip_send_packet(p->fd, b, sizeof b, next->fd);
        if (!r)
            return changed;
        /* Sent or not, Sophia holds its own reference or nothing: this
         * descriptor is never sent again. */
        close(next->fd);
        next->fd = -1;
        changed = 1;
        if (r > 0) {
            next->state = SOPHIA_IPO_SENT;
            continue;
        }
        next->state = SOPHIA_IPO_AMBIGUOUS;
        ip_fail(p, errno == EPIPE || errno == ECONNRESET ? SOPHIA_IPX_UNAVAILABLE
                                                         : SOPHIA_IPX_FAULT);
        return 1;
    }
}
#else
int ip_send_packet(int sock, const uint8_t *b, size_t n, int fd)
{
    (void)sock;
    (void)b;
    (void)n;
    (void)fd;
    errno = ENOTSUP;
    return -1;
}
#endif

int sophia_ip_dispatch(struct sophia_ip *p, short revents, uint64_t now_ms)
{
    int changed = 0;
    uint8_t channel;
    if (!p || !ip_clock(p, now_ms))
        return -(int)SOPHIA_IPR_ARGUMENT;
    channel = p->channel;
    if (ip_expire(p, now_ms))
        changed |= SOPHIA_IP_CHANGED_OFFERS;
    if (p->channel != SOPHIA_IPC_READY && p->channel != SOPHIA_IPC_CLOSED &&
        (now_ms >= p->config.startup_deadline_ms ||
         ((p->channel == SOPHIA_IPC_HELLO_QUEUED || p->channel == SOPHIA_IPC_HELLO_SENT) &&
          now_ms >= p->hello_deadline_ms)))
        ip_fail(p, SOPHIA_IPX_TIMEOUT);
#ifdef __linux__
    if (p->fd >= 0 && !hung_up(p, revents)) {
        if (p->channel == SOPHIA_IPC_HELLO_QUEUED)
            send_hello(p);
        if (send_sources(p))
            changed |= SOPHIA_IP_CHANGED_OFFERS;
    }
#else
    (void)revents;
#endif
    if (p->channel != channel)
        changed |= SOPHIA_IP_CHANGED_CHANNEL;
    return changed;
}

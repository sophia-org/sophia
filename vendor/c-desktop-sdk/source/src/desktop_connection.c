#define _GNU_SOURCE
#include "sophia_desktop_connection.h"
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

static int valid_path(const char *path)
{
    struct sockaddr_un address;
    return path && path[0] == '/' && strlen(path) < sizeof(address.sun_path);
}

int sophia_desktop_select_shell(const char *files, struct sophia_desktop_endpoint *out)
{
    struct sophia_desktop_endpoint selected;
    if (!out || !valid_path(files))
        return SOPHIA_DESKTOP_CONNECT_ARGUMENT;
    selected.wire = SOPHIA_DESKTOP_FILES;
    selected.path = files;
    *out = selected;
    return SOPHIA_DESKTOP_CONNECTED;
}

int sophia_desktop_shell_environment(struct sophia_desktop_endpoint *out)
{
    if (getenv("SOPHIA_SHELL_SOCKET")) return SOPHIA_DESKTOP_CONNECT_ARGUMENT;
    return sophia_desktop_select_shell(getenv("SOPHIA_SHELL_9P_SOCKET"), out);
}

int sophia_desktop_output_environment(struct sophia_desktop_endpoint *out)
{
    if (getenv("SOPHIA_OUTPUT_SOCKET")) return SOPHIA_DESKTOP_CONNECT_ARGUMENT;
    return sophia_desktop_select_shell(getenv("SOPHIA_OUTPUT_9P_SOCKET"), out);
}

static int fail(struct sophia_desktop_connection *c, int result, int error)
{
    if (c->owns_fd && c->fd >= 0) close(c->fd);
    c->owns_fd = 0;
    c->fd = -1;
    c->system_error = error;
    c->result = result;
    return result;
}

static int authenticate(struct sophia_desktop_connection *c)
{
#if defined(__linux__)
    struct ucred peer;
    socklen_t length = sizeof(peer);
    if (getsockopt(c->fd, SOL_SOCKET, SO_PEERCRED, &peer, &length))
        return fail(c, SOPHIA_DESKTOP_CONNECT_IO, errno);
    if (length != sizeof(peer) || peer.uid != geteuid())
        return fail(c, SOPHIA_DESKTOP_CONNECT_PEER, EACCES);
#elif defined(__FreeBSD__)
    uid_t uid;
    gid_t gid;
    if (getpeereid(c->fd, &uid, &gid))
        return fail(c, SOPHIA_DESKTOP_CONNECT_IO, errno);
    if (uid != geteuid()) return fail(c, SOPHIA_DESKTOP_CONNECT_PEER, EACCES);
#else
#error "A peer credential adapter is required for this platform"
#endif
    c->result = SOPHIA_DESKTOP_CONNECTED;
    return c->result;
}

int sophia_desktop_connection_begin(struct sophia_desktop_connection *c, const char *path)
{
    struct sockaddr_un address = { .sun_family = AF_UNIX };
    struct stat metadata;
    if (!c || c->owns_fd || !valid_path(path)) return SOPHIA_DESKTOP_CONNECT_ARGUMENT;
    c->fd = -1;
    c->system_error = 0;
    if (lstat(path, &metadata)) return fail(c, SOPHIA_DESKTOP_CONNECT_IO, errno);
    if (!S_ISSOCK(metadata.st_mode) || metadata.st_uid != geteuid())
        return fail(c, SOPHIA_DESKTOP_CONNECT_PEER, EACCES);
    memcpy(address.sun_path, path, strlen(path) + 1);
#if defined(__FreeBSD__)
    address.sun_len = sizeof(address);
#endif
    c->fd = socket(AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0);
    if (c->fd < 0) return fail(c, SOPHIA_DESKTOP_CONNECT_IO, errno);
    c->owns_fd = 1;
    if (connect(c->fd, (struct sockaddr *)&address, sizeof(address))) {
        if (errno == EAGAIN || errno == EWOULDBLOCK)
            return fail(c, SOPHIA_DESKTOP_CONNECT_RETRY, errno);
        if (errno != EINPROGRESS) return fail(c, SOPHIA_DESKTOP_CONNECT_IO, errno);
        c->result = SOPHIA_DESKTOP_CONNECTING;
        return c->result;
    }
    return authenticate(c);
}

short sophia_desktop_connection_events(const struct sophia_desktop_connection *c)
{
    return c && c->owns_fd && c->fd >= 0 && c->result == SOPHIA_DESKTOP_CONNECTING ? POLLOUT : 0;
}

int sophia_desktop_connection_finish(struct sophia_desktop_connection *c, short revents)
{
    int error = 0;
    socklen_t length = sizeof(error);
    if (!c) return SOPHIA_DESKTOP_CONNECT_ARGUMENT;
    if (!c->owns_fd || c->fd < 0)
        return c->result < 0 || c->result == SOPHIA_DESKTOP_CONNECT_RETRY ?
            c->result : SOPHIA_DESKTOP_CONNECT_ARGUMENT;
    if (c->result != SOPHIA_DESKTOP_CONNECTING) return c->result;
    if (revents & POLLNVAL) return fail(c, SOPHIA_DESKTOP_CONNECT_IO, EBADF);
    if (!(revents & (POLLOUT | POLLERR | POLLHUP))) return c->result;
    if (getsockopt(c->fd, SOL_SOCKET, SO_ERROR, &error, &length))
        return fail(c, SOPHIA_DESKTOP_CONNECT_IO, errno);
    if (length != sizeof(error) || error)
        return fail(c, SOPHIA_DESKTOP_CONNECT_IO, error ? error : EIO);
    return authenticate(c);
}

int sophia_desktop_connection_take(struct sophia_desktop_connection *c)
{
    int fd;
    if (!c || !c->owns_fd || c->fd < 0 || c->result != SOPHIA_DESKTOP_CONNECTED) return -1;
    fd = c->fd;
    c->fd = -1;
    c->owns_fd = 0;
    c->result = SOPHIA_DESKTOP_CONNECT_ARGUMENT;
    return fd;
}

void sophia_desktop_connection_close(struct sophia_desktop_connection *c)
{
    if (c) (void)fail(c, SOPHIA_DESKTOP_CONNECT_ARGUMENT, 0);
}

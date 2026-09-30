#ifndef SOPHIA_DESKTOP_CONNECTION_H
#define SOPHIA_DESKTOP_CONNECTION_H
#include <poll.h>

enum sophia_desktop_wire { SOPHIA_DESKTOP_FILES = 1 };
enum sophia_desktop_connect_result {
    SOPHIA_DESKTOP_CONNECTED = 0,
    SOPHIA_DESKTOP_CONNECTING = 1,
    /* Backlog full: fd is closed; wait before begin() on the same endpoint. */
    SOPHIA_DESKTOP_CONNECT_RETRY = 2,
    SOPHIA_DESKTOP_CONNECT_ARGUMENT = -1,
    SOPHIA_DESKTOP_CONNECT_IO = -2,
    SOPHIA_DESKTOP_CONNECT_PEER = -3
};
#define SOPHIA_DESKTOP_CONNECT_RETRY_MS 5
struct sophia_desktop_endpoint {
    enum sophia_desktop_wire wire;
    /* Borrowed from the selection arguments/environment. */
    const char *path;
};
/* The file endpoint must be absolute and nonempty. Environment selection
 * requires SOPHIA_SHELL_9P_SOCKET and refuses any SOPHIA_SHELL_SOCKET value. */
int sophia_desktop_select_shell(const char *files, struct sophia_desktop_endpoint *out);
int sophia_desktop_shell_environment(struct sophia_desktop_endpoint *out);
/* The output role: requires SOPHIA_OUTPUT_9P_SOCKET and refuses any
 * SOPHIA_OUTPUT_SOCKET value, even empty. Path rules match the shell's. */
int sophia_desktop_output_environment(struct sophia_desktop_endpoint *out);

/* Initialize to { .fd = -1 } before begin. The helper owns fd until take or
 * close. No waits or endpoint discovery: caller polls and enforces a deadline.
 * Unix sockets, O_NONBLOCK, FD_CLOEXEC, same effective UID required.
 * Same-UID authentication does not prove supervisor admission. RETRY leaves
 * fd=-1; wait RETRY_MS and call begin on the same path within your deadline. */
/* system_error is host errno for IO, or EACCES for refused peer identity.
 * Private ownership state is exposed for caller allocation; do not modify it. */
struct sophia_desktop_connection { int fd, result, system_error; unsigned owns_fd; };
int sophia_desktop_connection_begin(struct sophia_desktop_connection *, const char *path);
short sophia_desktop_connection_events(const struct sophia_desktop_connection *);
int sophia_desktop_connection_finish(struct sophia_desktop_connection *, short revents);
/* Returns -1 unless connected; a successful take transfers ownership. */
int sophia_desktop_connection_take(struct sophia_desktop_connection *);
void sophia_desktop_connection_close(struct sophia_desktop_connection *);
#endif

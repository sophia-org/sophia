#define _GNU_SOURCE
#include "../sophia_desktop_connection.h"
#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

#if defined(__linux__)
static int credential_control;
int __real_getsockopt(int, int, int, void *, socklen_t *);
int __wrap_getsockopt(int fd, int level, int option, void *value, socklen_t *length)
{
    int result = __real_getsockopt(fd, level, option, value, length);
    if (!result && level == SOL_SOCKET && option == SO_PEERCRED) {
        if (credential_control == 1) ((struct ucred *)value)->uid = geteuid() + 1;
        if (credential_control == 2) --*length;
    }
    return result;
}
#endif

int main(void)
{
    struct sophia_desktop_endpoint endpoint = { SOPHIA_DESKTOP_FILES, "unchanged" };
    struct sophia_desktop_connection connection = { .fd = -1 };
    struct sophia_desktop_connection zero = {0};
    char directory[] = "/tmp/sophia-sdk-connect-XXXXXX";
    char oversized[512];
    struct sockaddr_un address = { .sun_family = AF_UNIX };
    int listener, fd, peer, result;
    int stdin_flags = fcntl(STDIN_FILENO, F_GETFD);
    sophia_desktop_connection_close(&zero);
    assert(fcntl(STDIN_FILENO, F_GETFD) == stdin_flags);
    assert(sophia_desktop_connection_finish(&connection, 0) == SOPHIA_DESKTOP_CONNECT_ARGUMENT);
    assert(sophia_desktop_select_shell(NULL, &endpoint) < 0);
    assert(sophia_desktop_select_shell("", &endpoint) < 0);
    assert(sophia_desktop_select_shell("relative", &endpoint) < 0);
    memset(oversized, 'x', sizeof(oversized));
    oversized[0] = '/'; oversized[sizeof(oversized)-1] = 0;
    assert(sophia_desktop_select_shell(oversized, &endpoint) < 0);
    assert(!strcmp(endpoint.path, "unchanged"));
    assert(!sophia_desktop_select_shell("/files", &endpoint));
    assert(endpoint.wire == SOPHIA_DESKTOP_FILES);
    assert(!unsetenv("SOPHIA_SHELL_9P_SOCKET"));
    assert(!unsetenv("SOPHIA_SHELL_SOCKET"));
    assert(sophia_desktop_shell_environment(&endpoint) < 0);
    assert(!setenv("SOPHIA_SHELL_SOCKET", "/ipc", 1));
    assert(sophia_desktop_shell_environment(&endpoint) < 0);
    assert(!setenv("SOPHIA_SHELL_9P_SOCKET", "/files", 1));
    assert(sophia_desktop_shell_environment(&endpoint) < 0);
    assert(!setenv("SOPHIA_SHELL_SOCKET", "", 1));
    assert(sophia_desktop_shell_environment(&endpoint) < 0);
    assert(!unsetenv("SOPHIA_SHELL_SOCKET"));
    assert(!sophia_desktop_shell_environment(&endpoint));
    assert(endpoint.wire == SOPHIA_DESKTOP_FILES);
    assert(!strcmp(endpoint.path, "/files"));
    assert(!unsetenv("SOPHIA_SHELL_9P_SOCKET"));
    assert(!unsetenv("SOPHIA_OUTPUT_9P_SOCKET"));
    assert(!unsetenv("SOPHIA_OUTPUT_SOCKET"));
    assert(sophia_desktop_output_environment(&endpoint) < 0);
    assert(!setenv("SOPHIA_OUTPUT_9P_SOCKET", "relative", 1));
    assert(sophia_desktop_output_environment(&endpoint) < 0);
    assert(!setenv("SOPHIA_OUTPUT_9P_SOCKET", "/output", 1));
    assert(!setenv("SOPHIA_OUTPUT_SOCKET", "", 1));
    assert(sophia_desktop_output_environment(&endpoint) < 0);
    assert(!unsetenv("SOPHIA_OUTPUT_SOCKET"));
    assert(!setenv("SOPHIA_SHELL_9P_SOCKET", "/shell", 1));
    assert(!sophia_desktop_output_environment(&endpoint));
    assert(!strcmp(endpoint.path, "/output"));
    assert(!unsetenv("SOPHIA_SHELL_9P_SOCKET"));
    assert(!unsetenv("SOPHIA_OUTPUT_9P_SOCKET"));
    assert(mkdtemp(directory));
    assert(snprintf(address.sun_path, sizeof(address.sun_path), "%s/socket", directory) > 0);
    listener = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    assert(listener >= 0);
    assert(!bind(listener, (struct sockaddr *)&address, sizeof(address)));
    assert(!listen(listener, 4));
    result = sophia_desktop_connection_begin(&connection, address.sun_path);
    if (result == SOPHIA_DESKTOP_CONNECTING) {
        struct pollfd wait = { connection.fd, sophia_desktop_connection_events(&connection), 0 };
        assert(wait.events == POLLOUT);
        assert(poll(&wait, 1, 1000) == 1);
        result = sophia_desktop_connection_finish(&connection, wait.revents);
    }
    assert(result == SOPHIA_DESKTOP_CONNECTED);
    assert(!sophia_desktop_connection_events(&connection));
    fd = sophia_desktop_connection_take(&connection);
    assert(fd >= 0 && connection.fd == -1);
    assert(sophia_desktop_connection_take(&connection) == -1);
    assert(fcntl(fd, F_GETFL) & O_NONBLOCK);
    assert(fcntl(fd, F_GETFD) & FD_CLOEXEC);
    peer = accept(listener, NULL, NULL);
    assert(peer >= 0);
    close(peer);
    close(fd);
#if defined(__linux__)
    for (credential_control = 1; credential_control <= 2; credential_control++) {
        result = sophia_desktop_connection_begin(&connection, address.sun_path);
        if (result == SOPHIA_DESKTOP_CONNECTING) {
            struct pollfd wait = { connection.fd, POLLOUT, 0 };
            assert(poll(&wait, 1, 1000) == 1);
            result = sophia_desktop_connection_finish(&connection, wait.revents);
        }
        assert(result == SOPHIA_DESKTOP_CONNECT_PEER);
        assert(connection.fd == -1);
        peer = accept(listener, NULL, NULL);
        assert(peer >= 0);
        close(peer);
    }
    credential_control = 0;
    /* Linux reports a full Unix listen backlog as EAGAIN, without an
     * asynchronously connecting descriptor. It must remain retryable. */
    assert(!listen(listener, 0));
    assert(sophia_desktop_connection_begin(&connection, address.sun_path) == SOPHIA_DESKTOP_CONNECTED);
    fd = sophia_desktop_connection_take(&connection);
    assert(fd >= 0);
    assert(sophia_desktop_connection_begin(&connection, address.sun_path) == SOPHIA_DESKTOP_CONNECT_RETRY);
    assert(connection.fd == -1);
    peer = accept(listener, NULL, NULL);
    assert(peer >= 0);
    close(peer); close(fd);
    assert(sophia_desktop_connection_begin(&connection, address.sun_path) == SOPHIA_DESKTOP_CONNECTED);
    sophia_desktop_connection_close(&connection);
    peer = accept(listener, NULL, NULL);
    assert(peer >= 0);
    close(peer);
#endif
    close(listener);
    assert(!unlink(address.sun_path));
    assert(sophia_desktop_connection_begin(&connection, directory) == SOPHIA_DESKTOP_CONNECT_PEER);
    assert(connection.fd == -1);
    assert(!symlink(directory, address.sun_path));
    assert(sophia_desktop_connection_begin(&connection, address.sun_path) == SOPHIA_DESKTOP_CONNECT_PEER);
    assert(!unlink(address.sun_path));
    assert(!rmdir(directory));
    assert(sophia_desktop_connection_begin(&connection, address.sun_path) == SOPHIA_DESKTOP_CONNECT_IO);
    assert(connection.fd == -1 && connection.system_error);
    sophia_desktop_connection_close(&connection);
    puts("desktop_connection selection=explicit peer=same_uid fd=nonblocking status=pass");
    return 0;
}

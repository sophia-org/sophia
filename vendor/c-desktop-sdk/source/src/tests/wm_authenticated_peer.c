/* Generic protected-admission peer; the supervisor bootstrap moves the
 * original launch channel onto stdin without changing process identity. */
#define _POSIX_C_SOURCE 200809L
#include "../sophia_launch_auth.h"
#include "../sophia_wm_session.h"
#include <assert.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

static uint64_t now_ms(void) {
    struct timespec t;
    assert(!clock_gettime(CLOCK_MONOTONIC, &t));
    return (uint64_t)t.tv_sec * 1000 + (uint64_t)t.tv_nsec / 1000000;
}
int main(void) {
    struct sockaddr_un address = {0};
    struct sophia_9p_client role, relay;
    struct sophia_la_client auth;
    struct sophia_ws_config cfg = {4096, {SOPHIA_WF_CAP_CONFIGURATION, 0}, 0};
    struct sophia_ws_obligations obligations;
    const char *socket_path = getenv("SOPHIA_TEST_LOCK_SOCKET");
    const char *report_path = getenv("SOPHIA_TEST_LOCK_REPORT");
    size_t wire_bytes = sophia_9p_storage_bytes(4096, 8);
    size_t wm_bytes = sophia_ws_storage_bytes(4096);
    void *role_storage = malloc(wire_bytes), *relay_storage = malloc(wire_bytes);
    void *wm_storage = malloc(wm_bytes);
    struct sophia_ws *wm = malloc(sophia_ws_state_bytes());
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    uint64_t deadline = now_ms() + 10000;
    FILE *report;
    assert(fd >= 0 && role_storage && relay_storage && wm_storage && wm);
    assert(socket_path && report_path && strlen(socket_path) < sizeof address.sun_path);
    address.sun_family = AF_UNIX;
    strcpy(address.sun_path, socket_path);
    assert(!connect(fd, (struct sockaddr *)&address, sizeof address));
    assert(!fcntl(fd, F_SETFD, FD_CLOEXEC));
    assert(!sophia_9p_init(&role, fd, 4096, 8, 16, role_storage, wire_bytes));
    assert(!sophia_9p_init(&relay, STDIN_FILENO, 4096, 8, 16, relay_storage, wire_bytes));
    assert(!sophia_la_init(&auth, &role, &relay, "wm", "spatial-policy", now_ms()));
    while (!sophia_la_ready(&auth)) {
        struct pollfd fds[2] = {
            {fd, POLLIN | (sophia_9p_wants_write(&role) ? POLLOUT : 0), 0},
            {STDIN_FILENO, POLLIN | (sophia_9p_wants_write(&relay) ? POLLOUT : 0), 0}
        };
        assert(now_ms() < deadline);
        assert(poll(fds, 2, 1) >= 0);
        assert(!sophia_la_service(&auth, now_ms(), 65536));
    }
    cfg.bootstrap_deadline_ms = deadline;
    assert(!sophia_ws_open_attached(wm, &role, sophia_la_root(&auth), &cfg,
        wm_storage, wm_bytes, now_ms()));
    do {
        struct pollfd pfd = {fd, sophia_ws_poll_events(wm), 0};
        int timeout = sophia_ws_timeout(wm, now_ms());
        if (timeout < 0 || timeout > 1) timeout = 1;
        assert(now_ms() < deadline);
        assert(poll(&pfd, 1, timeout) >= 0);
        assert(!sophia_ws_dispatch(wm, pfd.revents, 65536, now_ms()));
        assert(!sophia_ws_obligations(wm, &obligations));
    } while (sophia_ws_state(wm) != SOPHIA_WS_READY || obligations.deadline_ms);
    assert(sophia_ws_epoch(wm) == 9);
    assert(sophia_ws_capabilities(wm) == SOPHIA_WF_CAP_CONFIGURATION);
    report = fopen(report_path, "w");
    assert(report);
    assert(fputs("authenticated-and-negotiated", report) >= 0);
    assert(!fclose(report));
    for (;;) {
        struct pollfd pfd = {fd, sophia_ws_poll_events(wm), 0};
        assert(now_ms() < deadline);
        assert(poll(&pfd, 1, 1) >= 0);
        if (sophia_ws_dispatch(wm, pfd.revents, 65536, now_ms()) < 0) break;
    }
    assert(sophia_ws_state(wm) == SOPHIA_WS_CLOSED || sophia_ws_state(wm) == SOPHIA_WS_STALE);
    {
        enum sophia_ws_custody custody;
        uint32_t error;
        char *revoked = malloc(strlen(report_path) + sizeof ".revoked");
        assert(revoked);
        assert(!sophia_ws_outcome(wm, 1, &custody, &error));
        assert(custody == SOPHIA_WS_SUBMITTED);
        strcpy(revoked, report_path); strcat(revoked, ".revoked");
        report = fopen(revoked, "w");
        assert(report);
        assert(fputs("revoked-custody-retained", report) >= 0);
        assert(!fclose(report));
        free(revoked);
    }
    sophia_ws_close(wm);
    close(fd);
    close(STDIN_FILENO);
    free(wm); free(wm_storage); free(role_storage); free(relay_storage);
    return 0;
}

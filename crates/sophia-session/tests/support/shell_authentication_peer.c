/* Generic shell contract peer: authentication and readiness, no UI or pixels. */
#define _POSIX_C_SOURCE 200809L
#include "sophia_launch_auth.h"
#include "sophia_shell_session.h"
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
int main(int argc, char **argv) {
    struct sockaddr_un address = {0};
    struct sophia_9p_client wire, relay;
    struct sophia_la_client auth;
    struct sophia_ss shell;
    struct sophia_ss_config config = {{5, 6, 0x81}, SOPHIA_SF_BAR, 4096, 4, 8192, NULL, 0};
    unsigned char wire_storage[69632], relay_storage[12288];
    const char *endpoint = getenv("SOPHIA_SHELL_9P_SOCKET");
    const char *channel = getenv("SOPHIA_LAUNCH_AUTH_FD");
    char *end;
    long relay_fd;
    int fd;
    uint64_t deadline = now_ms() + 10000;
    assert(argc == 3 && endpoint && channel);
    char diagnostic[256];
    assert(snprintf(diagnostic, sizeof(diagnostic), "%s.stderr", argv[2]) > 0);
    assert(freopen(diagnostic, "w", stderr));
    relay_fd = strtol(channel, &end, 10);
    assert(*channel && !*end && relay_fd >= 3 && relay_fd <= 2147483647);
    if (!strcmp(argv[1], "launcher")) {
        config.profile = SOPHIA_SF_LAUNCHER;
        config.offer = (struct sophia_sf_negotiate){7, 7, 0x9a0};
    } else if (!strcmp(argv[1], "dock")) {
        config.profile = SOPHIA_SF_DOCK;
        config.offer = (struct sophia_sf_negotiate){8, 8, 0x11a2};
    } else if (!strcmp(argv[1], "descriptor")) {
        config.profile = SOPHIA_SF_DESCRIPTOR;
        config.offer = (struct sophia_sf_negotiate){1, 8, 1};
    } else assert(!strcmp(argv[1], "bar"));
    fd = socket(AF_UNIX, SOCK_STREAM, 0);
    assert(fd >= 0 && strlen(endpoint) < sizeof(address.sun_path));
    address.sun_family = AF_UNIX;
    strcpy(address.sun_path, endpoint);
    assert(!connect(fd, (struct sockaddr *)&address, sizeof(address)));
    assert(!sophia_9p_init(&wire, fd, 4096, 8, 32, wire_storage, sizeof(wire_storage)));
    assert(!sophia_9p_init(&relay, (int)relay_fd, 4096, 1, 4, relay_storage, sizeof(relay_storage)));
    assert(!sophia_la_init(&auth, &wire, &relay, "shell", argv[1], now_ms()));
    while (!sophia_la_ready(&auth)) {
        assert(now_ms() < deadline && !sophia_la_service(&auth, now_ms(), 65536));
        struct timespec delay = {0, 1000000};
        nanosleep(&delay, NULL);
    }
    assert(fcntl((int)relay_fd, F_GETFD) & FD_CLOEXEC);
    size_t bytes = sophia_ss_storage_bytes(config.msize, config.queue_bytes);
    void *storage = malloc(bytes);
    assert(storage && !sophia_ss_open_attached(&shell, &wire, sophia_la_root(&auth), &config, storage, bytes));
    while (sophia_ss_state(&shell) != SOPHIA_SS_READY) {
        assert(now_ms() < deadline);
        assert(!sophia_ss_dispatch(&shell, POLLIN | POLLOUT, 65536, now_ms()));
        struct timespec delay = {0, 1000000};
        nanosleep(&delay, NULL);
    }
    char temporary[256];
    assert(snprintf(temporary, sizeof(temporary), "%s.tmp", argv[2]) > 0);
    assert(strlen(argv[2]) + 5 < sizeof(temporary));
    FILE *report = fopen(temporary, "w");
    assert(report);
    assert(fprintf(report, "%s ready %llu\n", argv[1], (unsigned long long)sophia_ss_epoch(&shell)) > 0);
    assert(!fclose(report));
    assert(!rename(temporary, argv[2]));
    /* Session revokes and reaps this exact protected process. */
    while (now_ms() < deadline) {
        struct timespec delay = {0, 10000000};
        nanosleep(&delay, NULL);
    }
    free(storage);
    close(fd);
    return 0;
}

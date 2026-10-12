/* Independent descriptor-custody probe, not an authentication protocol peer. */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

static int channel = -1;

static int fail(const char *message) {
    fprintf(stderr, "launch probe: %s\n", message);
    /* Keep a failed probe alive long enough for the supervisor to record the
       spawn, so the parent can discriminate its descriptor observation. */
    if (channel >= 0) {
        (void)write(channel, "FAIL!\n", 6);
        struct pollfd pending = { .fd = channel, .events = POLLIN };
        if (poll(&pending, 1, 5000) > 0) {
            char byte;
            (void)read(channel, &byte, 1);
        }
    }
    return 1;
}

int main(int argc, char **argv) {
    if (argc == 3 && strcmp(argv[1], "--after-exec") == 0) {
        int fd = atoi(argv[2]);
        return fcntl(fd, F_GETFD) == -1 && errno == EBADF ? 0 : fail("channel survived ordinary exec");
    }
    const char *text = getenv("SOPHIA_LAUNCH_AUTH_FD");
    if (!text || !*text) return fail("missing channel");
    char *end = NULL;
    long number = strtol(text, &end, 10);
    if (*end || number < 3 || number > 0x7fffffff) return fail("invalid channel number");
    int fd = (int)number;
    int type = 0;
    socklen_t length = sizeof(type);
    if (getsockopt(fd, SOL_SOCKET, SO_TYPE, &type, &length) || type != SOCK_STREAM)
        return fail("not a stream");
    struct sockaddr address;
    length = sizeof(address);
    if (getpeername(fd, &address, &length) || address.sa_family != AF_UNIX)
        return fail("not a connected Unix channel");
    channel = fd;
    if (fcntl(fd, F_GETFD) != 0) return fail("handoff was not inheritable");
    if (argc == 2 && (strcmp(argv[1], "--host") == 0 || strcmp(argv[1], "--host-options") == 0)) {
        const char *marker = getenv("SOPHIA_HOST_PROBE");
        if (!marker || strcmp(marker, "preserved") != 0)
            return fail("host launch environment lost");
    } else if (getenv("HOME") || getenv("DISPLAY") || getenv("LD_PRELOAD")) {
        return fail("ambient environment");
    }

    if (argc == 2 && (strcmp(argv[1], "--options") == 0 || strcmp(argv[1], "--host-options") == 0)) {
        char cwd[4096];
        struct stat input, null_device;
        if (!getcwd(cwd, sizeof(cwd)) || strcmp(cwd, "/usr/bin") != 0)
            return fail("working directory lost");
        if (getenv("SOPHIA_REMOVED") || getenv("CARGO_MANIFEST_DIR"))
            return fail("removed environment survived");
        const char *restored = getenv("SOPHIA_RESTORED");
        if (!restored || strcmp(restored, "last") != 0)
            return fail("last environment selection lost");
        if (fstat(0, &input) || stat("/dev/null", &null_device) ||
            !S_ISCHR(input.st_mode) || input.st_rdev != null_device.st_rdev)
            return fail("stdin must be null");
    }

    DIR *directory = opendir("/proc/self/fd");
    if (!directory) return fail("fd inventory");
    struct dirent *entry;
    while ((entry = readdir(directory))) {
        char *tail = NULL;
        long n = strtol(entry->d_name, &tail, 10);
        if (!*entry->d_name || *tail) continue;
        if (n > 2 && n != fd && n != dirfd(directory)) {
            closedir(directory);
            return fail("unrelated inherited descriptor");
        }
    }
    closedir(directory);
    /* What SDK adoption must do; this C observation does not qualify the SDK. */
    if (fcntl(fd, F_SETFD, FD_CLOEXEC)) return fail("set CLOEXEC");
    pid_t child = fork();
    if (child < 0) return fail("fork");
    if (child == 0) {
        execl(argv[0], argv[0], "--after-exec", text, (char *)NULL);
        _exit(127);
    }
    int status;
    if (waitpid(child, &status, 0) != child || !WIFEXITED(status) || WEXITSTATUS(status))
        return fail("ordinary exec exclusion");
    if (write(fd, "READY\n", 6) != 6) return fail("write");
    struct pollfd pending = { .fd = fd, .events = POLLIN };
    if (poll(&pending, 1, 5000) <= 0) return fail("owner deadline");
    char byte;
    if (read(fd, &byte, 1) != 1 || byte != 'Q') return fail("owner answer");
    return 0;
}

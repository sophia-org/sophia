/* Test-only descriptor bootstrap, not an authentication client. Keep the
 * supervisor's process and kernel channel unchanged across exec. Rust can
 * safely duplicate stdin without adding raw-fd adoption to production code. */
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <errno.h>
#include <limits.h>
#include <stdlib.h>
#include <unistd.h>

int main(int argc, char **argv) {
    assert(argc == 4);
    const char *value = getenv("SOPHIA_LAUNCH_AUTH_FD"); assert(value);
    char *end; errno = 0; long fd = strtol(value, &end, 10);
    assert(!errno && *end == 0 && fd > 2 && fd <= INT_MAX);
    assert(dup2((int)fd, STDIN_FILENO) == STDIN_FILENO);
    assert(close((int)fd) == 0);
    assert(unsetenv("SOPHIA_LAUNCH_AUTH_FD") == 0);
    assert(setenv("SOPHIA_TEST_LOCK_SOCKET", argv[2], 1) == 0);
    assert(setenv("SOPHIA_TEST_LOCK_REPORT", argv[3], 1) == 0);
    char *args[] = {argv[1], "--ignored", "--exact",
        "lock_authenticated_transport::rust_peer::protected_rust_child",
        "--nocapture", NULL};
    execv(argv[1], args);
    return 111;
}

/* Relay a challenge delivered after this exact protected process is admitted.
 * Only the public nonce crosses the file; process identity never does. */
#define main relay_main
#include "launch_authentication_probe.c"
#undef main
#include <time.h>

int main(int argc, char **argv) {
    assert(argc == 2);
    alarm(15);
    char nonce[65] = {0};
    for (;;) {
        int fd = open(argv[1], O_RDONLY | O_CLOEXEC);
        if (fd >= 0) {
            ssize_t n = read(fd, nonce, 64);
            assert(!close(fd));
            if (n == 64) break;
        }
        struct timespec pause_time = {0, 1000000};
        nanosleep(&pause_time, NULL);
    }
    char *args[] = {argv[0], nonce, NULL};
    return relay_main(2, args);
}

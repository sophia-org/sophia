#define _GNU_SOURCE
#include "restrict.h"
#include <errno.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <unistd.h>

static unsigned number(const char *text)
{
    char *end;
    errno = 0;
    unsigned long value = strtoul(text, &end, 10);
    if (errno || !*text || *end || *text < '0' || *text > '9' ||
        value == 0 || value > INT_MAX) {
        fprintf(stderr, "development sandbox: invalid numeric identity\n");
        exit(2);
    }
    return value;
}

int main(int argc, char **argv)
{
    /* This is an ordinary, non-setuid executable in a fixed root-owned
     * bundle. Only the root worker invokes it, after bubblewrap setup. */
    if (argc < 5 || strcmp(argv[3], "--") || argv[4][0] != '/') {
        fprintf(stderr, "usage: sandbox-exec UID GID -- /absolute/auditor [args]\n");
        return 2;
    }
    uid_t uid = number(argv[1]);
    gid_t gid = number(argv[2]);
    pid_t parent = getppid();
    if (development_drop(uid, gid, parent)) {
        perror("development sandbox: credential drop refused");
        return 2;
    }
    if (development_scope() || development_socket_filter()) {
        perror("development sandbox: confinement refused");
        return 2;
    }
    execv(argv[4], &argv[4]);
    perror("development sandbox: exec failed");
    return 2;
}

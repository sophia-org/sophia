#define _GNU_SOURCE
#include <errno.h>
#include <limits.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/prctl.h>
#include <unistd.h>

/* Trusted bundle utility, never setuid. The expected parent comes from the
 * owner before fork, so parent death before this program starts also refuses.
 * No shell, credential transition, or user command expansion. */
int main(int argc, char **argv)
{
    if (argc < 4 || argv[3][0] != '/' || argv[2][0] != '-' ||
        argv[2][1] != '-' || argv[2][2] != '\0') return 2;
    char *end;
    errno = 0;
    long parent = strtol(argv[1], &end, 10);
    if (errno || *end || parent <= 1 || parent > INT_MAX) return 2;
    if (prctl(PR_SET_PDEATHSIG, SIGKILL, 0, 0, 0) || getppid() != parent) {
        fputs("development custody parent lost\n", stderr);
        return 2;
    }
    execv(argv[3], argv + 3);
    perror("development custody exec");
    return 2;
}

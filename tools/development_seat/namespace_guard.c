#define _GNU_SOURCE
#include <errno.h>
#include <poll.h>
#include <sched.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

/* A root-owned bundle utility, never setuid. custody-exec already bound this
 * monitor to the PAM worker. The namespace init uses a pidfd opened BEFORE
 * clone to close the child-start/parent-exit race, even when getppid() is 0 in
 * the new namespace. If it dies, Linux kills every descendant in that namespace,
 * including bubblewrap processes still in pre-exec setup. No PAM runs inside.
 * This executable has one thread, so raw clone has fork semantics here. */
int main(int argc, char **argv)
{
    if (argc < 3 || argv[1][0] != '-' || argv[1][1] != '-' || argv[1][2] ||
        argv[2][0] != '/' || getuid() != 0 || geteuid() != 0) return 2;
    int parent = syscall(SYS_pidfd_open, getpid(), 0);
    if (parent < 0) { perror("namespace guard pidfd"); return 2; }
    pid_t child = syscall(SYS_clone, CLONE_NEWPID | SIGCHLD, NULL, NULL, NULL, 0);
    if (child < 0) { perror("namespace guard clone"); close(parent); return 2; }
    if (child == 0) {
        struct pollfd observation = { .fd = parent, .events = POLLIN };
        if (getpid() != 1 || prctl(PR_SET_PDEATHSIG, SIGKILL, 0, 0, 0) ||
            poll(&observation, 1, 0) != 0) _exit(125);
        close(parent);
        execv(argv[2], argv + 2);
        perror("namespace guard exec");
        _exit(125);
    }
    close(parent);
    int status;
    while (waitpid(child, &status, 0) < 0) {
        if (errno != EINTR) { perror("namespace guard wait"); return 125; }
    }
    return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
}

#define _GNU_SOURCE
#include "restrict.h"
#include <errno.h>
#include <linux/audit.h>
#include <linux/filter.h>
#include <linux/landlock.h>
#include <linux/netlink.h>
#include <linux/seccomp.h>
#include <stddef.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <unistd.h>

/* Filesystem confinement is the outer mount namespace. These scopes close
 * abstract UNIX and signal access to processes outside this launch, despite
 * retaining the host network and user namespaces for authenticated udev. */
int development_scope(void)
{
    int abi = syscall(SYS_landlock_create_ruleset, NULL, 0,
                      LANDLOCK_CREATE_RULESET_VERSION);
    if (abi < 6) {
        if (abi >= 0) errno = ENOTSUP;
        return -1;
    }
    struct landlock_ruleset_attr rules = {
        .scoped = LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET | LANDLOCK_SCOPE_SIGNAL,
    };
    int fd = syscall(SYS_landlock_create_ruleset, &rules, sizeof(rules), 0);
    if (fd < 0) return -1;
    int result = syscall(SYS_landlock_restrict_self, fd, 0);
    int saved = errno;
    close(fd);
    errno = saved;
    return result;
}

#if defined(__x86_64__)
#define NATIVE_ARCH AUDIT_ARCH_X86_64
#elif defined(__aarch64__)
#define NATIVE_ARCH AUDIT_ARCH_AARCH64
#else
#error "Qualify the syscall filter for this architecture before building"
#endif
#define DENY (SECCOMP_RET_ERRNO | EPERM)
#define LOAD_NR BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr))
#define LOAD_ARG(n) BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, args[n]))
#define REFUSE(n) BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, SYS_##n, 0, 1), BPF_STMT(BPF_RET | BPF_K, DENY)

int development_socket_filter(void)
{
    /* Kernel socket arguments are ints; inspecting their low 32 bits matches
     * their syscall conversion. No architecture switch or x32 bypass. Nested
     * bubblewrap may create stricter namespaces, but cannot enter other ones.
     * clone3 gets ENOSYS so libc can use clone, as tested by a real child. */
    struct sock_filter insns[] = {
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, arch)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, NATIVE_ARCH, 1, 0),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS),
        LOAD_NR,
#if defined(__x86_64__)
        BPF_JUMP(BPF_JMP | BPF_JGE | BPF_K, 0x40000000, 0, 1),
        BPF_STMT(BPF_RET | BPF_K, DENY),
#endif
        REFUSE(io_uring_setup), REFUSE(io_uring_enter), REFUSE(io_uring_register),
        REFUSE(setns),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, SYS_clone3, 0, 1),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | ENOSYS),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, SYS_socket, 0, 8),
        LOAD_ARG(0),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, AF_UNIX, 5, 0),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, AF_NETLINK, 0, 3),
        LOAD_ARG(2),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, NETLINK_KOBJECT_UEVENT, 2, 0),
        BPF_STMT(BPF_RET | BPF_K, DENY),
        BPF_STMT(BPF_RET | BPF_K, DENY),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, SYS_socketpair, 0, 4),
        LOAD_ARG(0),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, AF_UNIX, 1, 0),
        BPF_STMT(BPF_RET | BPF_K, DENY),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
    };
    struct sock_fprog program = {
        .len = sizeof(insns) / sizeof(insns[0]), .filter = insns,
    };
    return prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &program, 0, 0);
}

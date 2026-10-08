#define _GNU_SOURCE
#include "restrict.h"
#include <errno.h>
#include <grp.h>
#include <linux/capability.h>
#include <signal.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <unistd.h>

int development_verify_drop(uid_t uid, gid_t gid)
{
    uid_t ruid, euid, suid;
    gid_t rgid, egid, sgid;
    struct __user_cap_header_struct hdr = { _LINUX_CAPABILITY_VERSION_3, 0 };
    struct __user_cap_data_struct caps[2] = {{0}};
    if (uid == 0 || gid == 0 || getresuid(&ruid, &euid, &suid) ||
        getresgid(&rgid, &egid, &sgid) || ruid != uid || euid != uid ||
        suid != uid || rgid != gid || egid != gid || sgid != gid ||
        getgroups(0, NULL) != 0 || syscall(SYS_capget, &hdr, caps)) {
        errno = EPERM;
        return -1;
    }
    for (unsigned i = 0; i < 2; i++) {
        if (caps[i].effective || caps[i].permitted || caps[i].inheritable) {
            errno = EPERM;
            return -1;
        }
    }
    for (int cap = 0; ; cap++) {
        int value = prctl(PR_CAPBSET_READ, cap, 0, 0, 0);
        if (value < 0 && errno == EINVAL) break;
        if (value != 0 || prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_IS_SET, cap, 0, 0) != 0) {
            errno = EPERM;
            return -1;
        }
    }
    if (prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) != 1) {
        errno = EPERM;
        return -1;
    }
    return 0;
}

int development_drop(uid_t uid, gid_t gid, pid_t parent)
{
    if (getuid() != 0 || geteuid() != 0 || uid == 0 || gid == 0 ||
        parent <= 0 || getppid() != parent) {
        errno = EPERM;
        return -1;
    }
    if (prctl(PR_SET_PDEATHSIG, SIGKILL, 0, 0, 0) || getppid() != parent ||
        setgroups(0, NULL) || prctl(PR_SET_KEEPCAPS, 0, 0, 0, 0)) return -1;
    /* Dropping the bounding set leaves our current SETUID/SETGID capability
     * available for the irreversible transition below. No privileged exec. */
    for (int cap = 0; ; cap++) {
        int value = prctl(PR_CAPBSET_READ, cap, 0, 0, 0);
        if (value < 0) {
            if (errno == EINVAL) break;
            return -1;
        }
        if (prctl(PR_CAPBSET_DROP, cap, 0, 0, 0)) return -1;
    }
    if (setresgid(gid, gid, gid) || setresuid(uid, uid, uid)) return -1;
    struct __user_cap_header_struct hdr = { _LINUX_CAPABILITY_VERSION_3, 0 };
    struct __user_cap_data_struct caps[2] = {{0}};
    if (syscall(SYS_capset, &hdr, caps) ||
        prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_CLEAR_ALL, 0, 0, 0) ||
        prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0)) return -1;
    /* A credential change clears PDEATHSIG. Restore it before user code and
     * check the parent again to cover death during the transition. */
    if (prctl(PR_SET_PDEATHSIG, SIGKILL, 0, 0, 0) || getppid() != parent) return -1;
    return development_verify_drop(uid, gid);
}

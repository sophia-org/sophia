/* Guest-only re-root for the session-lock-provider QEMU scenario. The kernel
 * refuses pivot_root(2) while the caller's root is the initramfs rootfs, so
 * bubblewrap protection domains cannot start there. As PID 1 this binds the
 * rootfs onto NEWROOT, moves that mount over /, enters it and execs PROGRAM:
 *
 *   sophia-qemu-reroot NEWROOT PROGRAM [ARG...]
 *
 * The bind shares the rootfs's files, so nothing is copied and nothing is
 * removed: unlike switch_root(8), there is no cleanup of the old root. It is
 * not chroot(1) either, which skips chroot(2) when NEWROOT has the same
 * device and inode as /, as a bind of / does. The bind is not recursive:
 * the caller unmounts its submounts first and mounts them again after. */
#define _GNU_SOURCE
#include <errno.h>
#include <stdio.h>
#include <sys/mount.h>
#include <sys/reboot.h>
#include <sys/stat.h>
#include <unistd.h>

/* Reports the failed step and, as PID 1, powers the guest off rather than
 * returning into a kernel panic. */
static int fail(const char *step) {
  fprintf(stderr, "sophia_qemu_reroot schema=1 status=failed step=%s errno=%d\n",
          step, errno);
  if (getpid() == 1) {
    sync();
    reboot(RB_POWER_OFF);
  }
  return 1;
}

int main(int argc, char **argv) {
  if (argc < 3 || argv[1][0] != '/' || argv[2][0] != '/') {
    errno = EINVAL;
    return fail("usage");
  }
  if (getpid() != 1) {
    errno = EPERM;
    return fail("pid");
  }
  if (mkdir(argv[1], 0700) && errno != EEXIST)
    return fail("mkdir");
  if (mount("/", argv[1], NULL, MS_BIND, NULL))
    return fail("bind");
  if (chdir(argv[1]))
    return fail("chdir_new");
  if (mount(".", "/", NULL, MS_MOVE, NULL))
    return fail("move");
  if (chroot("."))
    return fail("chroot");
  if (chdir("/"))
    return fail("chdir_root");
  execv(argv[2], argv + 2);
  return fail("exec");
}

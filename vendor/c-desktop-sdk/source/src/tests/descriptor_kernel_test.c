/* Linux prerequisites for proposed Sophia x6jp929h, not a descriptor SDK.
 * No protocol admission, journal custody or GPU completion is claimed. */
#define _GNU_SOURCE
#include <assert.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <unistd.h>

#define REQUIRED (F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL)
static int source(int seals) {
  int fd = memfd_create("t337-fixture", MFD_ALLOW_SEALING | MFD_CLOEXEC);
  assert(fd >= 0);
  assert(ftruncate(fd, 8) == 0);
  assert(write(fd, "12345678", 8) == 8);
  assert(fcntl(fd, F_ADD_SEALS, seals) == 0);
  return fd;
}
static void pair(int fds[2]) {
  assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_NONBLOCK | SOCK_CLOEXEC,
                    0, fds) == 0);
}
static ssize_t send_fds(int socket, const int *fds, unsigned count) {
  union { struct cmsghdr align; unsigned char bytes[CMSG_SPACE(4 * sizeof(int))]; } control;
  struct iovec io = {(void *)"source", 6};
  struct msghdr msg;
  struct cmsghdr *c;
  assert(count <= 4);
  memset(&msg, 0, sizeof(msg));
  memset(&control, 0, sizeof(control));
  msg.msg_iov = &io;
  msg.msg_iovlen = 1;
  if (count) {
    msg.msg_control = control.bytes;
    msg.msg_controllen = CMSG_SPACE(count * sizeof(int));
    c = CMSG_FIRSTHDR(&msg);
    c->cmsg_level = SOL_SOCKET;
    c->cmsg_type = SCM_RIGHTS;
    c->cmsg_len = CMSG_LEN(count * sizeof(int));
    memcpy(CMSG_DATA(c), fds, count * sizeof(int));
  }
  return sendmsg(socket, &msg, MSG_NOSIGNAL);
}
static unsigned receive(int socket, int no_control, int *flags, int fds[4]) {
  union { struct cmsghdr align; unsigned char bytes[CMSG_SPACE(4 * sizeof(int))]; } control;
  char data[6];
  struct iovec io = {data, sizeof(data)};
  struct msghdr msg;
  struct cmsghdr *c;
  unsigned count = 0;
  memset(&msg, 0, sizeof(msg));
  memset(&control, 0, sizeof(control));
  msg.msg_iov = &io;
  msg.msg_iovlen = 1;
  if (!no_control) {
    msg.msg_control = control.bytes;
    msg.msg_controllen = sizeof(control.bytes);
  }
  assert(recvmsg(socket, &msg, MSG_CMSG_CLOEXEC) == 6);
  assert(memcmp(data, "source", 6) == 0);
  *flags = msg.msg_flags;
  for (c = CMSG_FIRSTHDR(&msg); c; c = CMSG_NXTHDR(&msg, c)) {
    size_t n;
    assert(c->cmsg_level == SOL_SOCKET && c->cmsg_type == SCM_RIGHTS);
    assert(c->cmsg_len >= CMSG_LEN(0));
    n = c->cmsg_len - CMSG_LEN(0);
    assert(n % sizeof(int) == 0 && count + n / sizeof(int) <= 4);
    memcpy(fds + count, CMSG_DATA(c), n);
    count += (unsigned)(n / sizeof(int));
  }
  return count;
}
static unsigned fd_count(void) {
  DIR *d = opendir("/proc/self/fd");
  struct dirent *e;
  unsigned n = 0;
  assert(d);
  while ((e = readdir(d))) if (e->d_name[0] != '.') ++n;
  assert(closedir(d) == 0);
  return n;
}
static void seals_and_transfer(void) {
  const int missing[] = {F_SEAL_WRITE, F_SEAL_GROW, F_SEAL_SHRINK, F_SEAL_SEAL};
  int fd, sockets[2], fds[4], flags;
  unsigned i;
  char data[8];
  struct stat st;
  void *map;
  for (i = 0; i < 4; ++i) {
    fd = source(REQUIRED & ~missing[i]);
    assert((fcntl(fd, F_GET_SEALS) & REQUIRED) != REQUIRED);
    close(fd);
  }
  fd = source(F_SEAL_FUTURE_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL);
  assert(!(fcntl(fd, F_GET_SEALS) & F_SEAL_WRITE));
  close(fd);
  fd = source(0);
  map = mmap(NULL, 8, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  assert(map != MAP_FAILED);
  assert(fcntl(fd, F_ADD_SEALS, F_SEAL_WRITE) == -1 && errno == EBUSY);
  assert(!(fcntl(fd, F_GET_SEALS) & F_SEAL_WRITE));
  assert(munmap(map, 8) == 0);
  assert(fcntl(fd, F_ADD_SEALS, REQUIRED) == 0);
  pair(sockets);
  assert(send_fds(sockets[0], &fd, 1) == 6);
  close(fd);
  assert(receive(sockets[1], 0, &flags, fds) == 1 && !(flags & MSG_CTRUNC));
  fd = fds[0];
  assert(fcntl(fd, F_GETFD) & FD_CLOEXEC);
  assert((fcntl(fd, F_GET_SEALS) & REQUIRED) == REQUIRED);
  assert(fstat(fd, &st) == 0 && st.st_size == 8);
  assert(pread(fd, data, 8, 0) == 8 && memcmp(data, "12345678", 8) == 0);
  assert(pwrite(fd, "X", 1, 0) == -1 && errno == EPERM);
  assert(ftruncate(fd, 4) == -1 && errno == EPERM);
  assert(ftruncate(fd, 16) == -1 && errno == EPERM);
  assert(mmap(NULL, 8, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0) == MAP_FAILED
         && errno == EPERM);
  map = mmap(NULL, 8, PROT_READ, MAP_SHARED, fd, 0);
  assert(map != MAP_FAILED);
  close(fd);
  assert(memcmp(map, "12345678", 8) == 0);
  assert(munmap(map, 8) == 0);
  close(sockets[0]); close(sockets[1]);
}
static void refusal_and_backpressure(void) {
  const unsigned counts[] = {0, 2, 4};
  unsigned i, j, received, sent;
  int sockets[2], fds[4], got[4], flags;
  struct stat st;
  struct pollfd writable;
  int send_buffer = 4096;
  for (i = 0; i < 3; ++i) {
    unsigned count = counts[i];
    pair(sockets);
    for (j = 0; j < count; ++j) fds[j] = source(REQUIRED);
    assert(send_fds(sockets[0], fds, count) == 6);
    received = receive(sockets[1], count == 4, &flags, got);
    if (count == 4) assert((flags & MSG_CTRUNC) && received == 0);
    else assert(received == count && received != 1);
    for (j = 0; j < received; ++j) close(got[j]);
    for (j = 0; j < count; ++j) close(fds[j]);
    close(sockets[0]); close(sockets[1]);
  }
  pair(sockets);
  assert(setsockopt(sockets[0], SOL_SOCKET, SO_SNDBUF,
                   &send_buffer, sizeof(send_buffer)) == 0);
  fds[0] = source(REQUIRED);
  for (sent = 0; sent < 65536; ++sent) {
    ssize_t n = send_fds(sockets[0], fds, 1);
    /* In-flight rights also have a per-user limit, independent of POLLOUT. */
    if (n == -1) { assert(errno == EAGAIN || errno == ETOOMANYREFS); break; }
    assert(n == 6);
  }
  assert(sent < 65536);
  assert(fstat(fds[0], &st) == 0 && st.st_size == 8);
  /* One receive need not cross the writable low-water mark; drain our burst. */
  for (i = 0; i < sent; ++i) {
    assert(receive(sockets[1], 0, &flags, got) == 1);
    close(got[0]);
  }
  memset(&writable, 0, sizeof(writable));
  writable.fd = sockets[0];
  writable.events = POLLOUT;
  assert(poll(&writable, 1, 0) == 1 && (writable.revents & POLLOUT));
  /* Socket readiness does not reserve a global SCM_RIGHTS credit. */
  assert(send_fds(sockets[0], NULL, 0) == 6);
  close(sockets[1]); /* The final assertion observes FD-table entries only. */
  close(sockets[0]); close(fds[0]);
}
int main(void) {
  unsigned before = fd_count();
  seals_and_transfer();
  refusal_and_backpressure();
  assert(fd_count() == before);
  puts("descriptor kernel prerequisites: pass (no SDK custody claim)");
  return 0;
}

/* Guest-side line stamper for the session-lock-provider QEMU scenario. Every
 * input line passes through unchanged. Before each low-volume lock, input
 * proof or lock-provider line it writes
 *
 *   sophia_qemu_stamp schema=1 mono_ns=<CLOCK_MONOTONIC>
 *
 * The stamp is when this filter observed the line on the session's pipe: it
 * measures log observation, not the instant a cover left the screen. Lines
 * are read and written whole, so a stamp always precedes its own line.
 *
 * Diagnostic option (off unless --sysrq-on-hard-stall is given): on the first
 * native page-flip hard-stall record, after that record is written and
 * flushed, it asks the guest kernel for its blocked tasks (SysRq w) and
 * copies the kernel records that request produced, which the console's
 * loglevel would otherwise hide, as
 *
 *   sophia_qemu_sysrq schema=1 status=triggered key=w
 *   sophia_qemu_sysrq schema=1 status=failed step=<open|write> errno=<n>
 *   sophia_qemu_kmsg schema=1 seq=<n> text=<kernel record>
 *   sophia_qemu_sysrq schema=1 status=captured records=<n> lost=<n> truncated=<0|1> errno=<n>
 *
 * Only blocked-task state is requested; it never fires on a passing run, and
 * never more than once. --sysrq-trigger= and --kmsg= replace /proc/sysrq-trigger
 * and /dev/kmsg for fixtures. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static const char *const stamped[] = {
    "sophia_live_session_lock ",
    "sophia_live_lock_provider ",
    "sophia_live_session_input ",
    "sophia_qemu_lock_provider ",
};
static const char hard_stall[] =
    "sophia_live_native_page_flip_stall schema=3 status=hard_stall ";
/* Bounds on the copy: records, bytes, and read attempts of any outcome, so
 * a kernel log being overwritten (EPIPE) or interrupted reads cannot keep
 * the stamper from its pipe. */
enum { MAX_KMSG_RECORDS = 4096, MAX_KMSG_BYTES = 1 << 20, MAX_KMSG_READS = 8192 };

static int wanted(const char *line) {
  for (size_t i = 0; i < sizeof stamped / sizeof stamped[0]; ++i)
    if (!strncmp(line, stamped[i], strlen(stamped[i])))
      return 1;
  return 0;
}

/* The record itself, at the start of a line or after a space: a tracing
 * prefix is allowed, a longer record name is not. */
static int is_hard_stall(const char *line) {
  for (const char *at = strstr(line, hard_stall); at;
       at = strstr(at + 1, hard_stall))
    if (at == line || at[-1] == ' ')
      return 1;
  return 0;
}

/* Copies the message of each kernel record ("prefix;message") from fd,
 * skipping continuation lines (" KEY=value"), until none is left or the
 * bounds are reached. /dev/kmsg returns one record per read; any other
 * source may return several. */
static void copy_kmsg(int fd) {
  static char chunk[8192];
  unsigned records = 0, lost = 0, reads = 0;
  size_t bytes = 0;
  int truncated = 0, error = 0;
  for (;;) {
    if (records >= MAX_KMSG_RECORDS || bytes >= MAX_KMSG_BYTES ||
        reads >= MAX_KMSG_READS) {
      truncated = 1;
      break;
    }
    ++reads;
    ssize_t n = read(fd, chunk, sizeof chunk - 1);
    if (n < 0 && errno == EPIPE) {
      ++lost; /* overwritten before it was read; the next one follows */
      continue;
    }
    if (n < 0 && errno == EINTR)
      continue;
    if (n < 0 && errno != EAGAIN)
      error = errno;
    if (n <= 0)
      break;
    chunk[n] = '\0';
    bytes += (size_t)n;
    for (char *record = chunk, *next; record && *record; record = next) {
      next = strchr(record, '\n');
      if (next)
        *next++ = '\0';
      char *body = strchr(record, ';');
      if (record[0] == ' ' || !body || records >= MAX_KMSG_RECORDS)
        continue;
      unsigned long long seq = 0;
      (void)sscanf(record, "%*[^,],%llu", &seq);
      printf("sophia_qemu_kmsg schema=1 seq=%llu text=%s\n", seq, body + 1);
      ++records;
    }
  }
  printf("sophia_qemu_sysrq schema=1 status=captured records=%u lost=%u "
         "truncated=%d errno=%d\n",
         records, lost, truncated, error);
}

static void request_blocked_tasks(const char *trigger, const char *kmsg) {
  /* Positioned at the end first, so only what the request produces is read.
   * A pipe has no history to skip. */
  int log = open(kmsg, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
  int log_error = log < 0 ? errno : 0;
  if (log >= 0 && lseek(log, 0, SEEK_END) < 0 && errno != ESPIPE)
    log_error = errno;
  int fd = open(trigger, O_WRONLY | O_CLOEXEC);
  if (fd < 0) {
    printf("sophia_qemu_sysrq schema=1 status=failed step=open errno=%d\n", errno);
  } else if (write(fd, "w", 1) != 1) {
    printf("sophia_qemu_sysrq schema=1 status=failed step=write errno=%d\n", errno);
    close(fd);
  } else {
    close(fd);
    printf("sophia_qemu_sysrq schema=1 status=triggered key=w\n");
    if (log >= 0 && !log_error)
      copy_kmsg(log);
    else
      printf("sophia_qemu_sysrq schema=1 status=captured records=0 lost=0 "
             "truncated=0 errno=%d\n",
             log_error);
  }
  if (log >= 0)
    close(log);
  fflush(stdout);
}

int main(int argc, char **argv) {
  const char *trigger = "/proc/sysrq-trigger", *kmsg = "/dev/kmsg";
  int sysrq = 0, fired = 0;
  for (int i = 1; i < argc; ++i) {
    if (!strcmp(argv[i], "--sysrq-on-hard-stall"))
      sysrq = 1;
    else if (!strncmp(argv[i], "--sysrq-trigger=", 16))
      trigger = argv[i] + 16;
    else if (!strncmp(argv[i], "--kmsg=", 7))
      kmsg = argv[i] + 7;
    else {
      fprintf(stderr, "sophia-qemu-line-stamp: unknown argument %s\n", argv[i]);
      return 2;
    }
  }
  char *line = NULL;
  size_t capacity = 0;
  ssize_t length;
  while ((length = getline(&line, &capacity, stdin)) >= 0) {
    if (wanted(line)) {
      struct timespec t;
      clock_gettime(CLOCK_MONOTONIC, &t);
      printf("sophia_qemu_stamp schema=1 mono_ns=%llu\n",
             (unsigned long long)t.tv_sec * 1000000000ull +
                 (unsigned long long)t.tv_nsec);
    }
    fwrite(line, 1, (size_t)length, stdout);
    fflush(stdout);
    if (sysrq && !fired && is_hard_stall(line)) {
      fired = 1;
      request_blocked_tasks(trigger, kmsg);
    }
  }
  free(line);
  return 0;
}

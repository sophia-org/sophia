/* Guest-side line stamper for the session-lock-provider QEMU scenario. Every
 * input line passes through unchanged. Before each low-volume lock, input
 * proof or lock-provider line it writes
 *
 *   sophia_qemu_stamp schema=1 mono_ns=<CLOCK_MONOTONIC>
 *
 * The stamp is when this filter observed the line on the session's pipe: it
 * measures log observation, not the instant a cover left the screen. Lines
 * are read and written whole, so a stamp always precedes its own line. */
#define _GNU_SOURCE
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static const char *const stamped[] = {
    "sophia_live_session_lock ",
    "sophia_live_lock_provider ",
    "sophia_live_session_input ",
    "sophia_qemu_lock_provider ",
};

static int wanted(const char *line) {
  for (size_t i = 0; i < sizeof stamped / sizeof stamped[0]; ++i)
    if (!strncmp(line, stamped[i], strlen(stamped[i])))
      return 1;
  return 0;
}

int main(void) {
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
  }
  free(line);
  return 0;
}

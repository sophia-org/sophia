/* Generic output file performance peer for the transport gate (t253).
 *
 *   output_files_perf_peer SOCKET MODE FIXTURE
 *   MODE: connect | proposals | idle      FIXTURE: small | max
 *
 * The pinned public C desktop SDK is the only protocol client. stdin carries
 * single-byte commands: G releases the authorization gate, N starts work, X
 * exits after DONE. Samples are CLOCK_MONOTONIC nanoseconds in bounded lines;
 * no timed path writes to stdout. A connect record is written after its
 * connection closes, because the harness waits for it (and Disconnected)
 * before the next N; proposal records stay buffered until DONE or failure.
 * Gate budgets are enforced by the harness; this peer only applies a 5 s hard
 * deadline per sample.
 *
 * Exit 0 after DONE and X; 1 after an F line (prior samples preserved);
 * 2 for usage or setup failure before the gate (stderr only). */
#define _POSIX_C_SOURCE 200809L
#include "sophia_desktop_connection.h"
#include "sophia_output_session.h"
#include <errno.h>
#include <inttypes.h>
#include <poll.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define MSIZE 65536u
#define BUDGET 131072u
#define SAMPLE_NS UINT64_C(5000000000)
#define RETRY_NS UINT64_C(5000000)
#define CONNECTIONS 100u
#define WARMUP 20u
#define MEASURED 1000u
#define LINE_BYTES 192u
#define MAX_LINES 1104u /* 1,020 samples plus READY, DONE and F lines */
#define IDLE_NO_PROGRESS 64u

enum mode { CONNECT, PROPOSALS, IDLE };

static enum mode mode;
static const char *mode_name, *socket_path;
static int fixture_max;
static struct sophia_os *session;
static void *storage;
static size_t storage_bytes;
static int session_fd = -1, session_open;
static char lines[MAX_LINES][LINE_BYTES];
static unsigned line_count;

static uint64_t now_ns(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return (uint64_t)t.tv_sec * UINT64_C(1000000000) + (uint64_t)t.tv_nsec;
}
static uint64_t ms(uint64_t ns) { return ns / UINT64_C(1000000); }

static void flush_lines(void) {
  unsigned i;
  for (i = 0; i < line_count; ++i)
    fputs(lines[i], stdout);
  fflush(stdout);
  line_count = 0;
}
static void close_session(void) {
  if (session_open)
    sophia_os_close(session);
  session_open = 0;
  if (session_fd >= 0)
    close(session_fd);
  session_fd = -1;
}
/* Every emitted line fits LINE_BYTES; the buffer holds a whole workload. */
static void emit(const char *format, ...) {
  va_list args;
  int n;
  if (line_count == MAX_LINES) {
    flush_lines();
    fputs("F internal 0 line-capacity\n", stdout);
    exit(1);
  }
  va_start(args, format);
  n = vsnprintf(lines[line_count], LINE_BYTES, format, args);
  va_end(args);
  if (n < 0 || (unsigned)n >= LINE_BYTES) {
    flush_lines();
    fputs("F internal 0 line-bound\n", stdout);
    exit(1);
  }
  ++line_count;
}
/* Reasons are short and whitespace-free. Prior samples are kept. */
static void fail(unsigned index, const char *reason) {
  emit("F %s %u %s\n", mode_name, index, reason);
  flush_lines();
  close_session();
  exit(1);
}
static void session_failure(unsigned index) {
  char reason[48];
  const char *state;
  switch (sophia_os_state(session)) {
  case SOPHIA_OS_REFUSED:
    state = "session-refused";
    break;
  case SOPHIA_OS_STALE:
    state = "session-stale";
    break;
  case SOPHIA_OS_CLOSED:
    state = "session-closed";
    break;
  default:
    state = "session-failed";
  }
  snprintf(reason, sizeof(reason), "%s-errno-%" PRIu32, state,
           sophia_os_remote_error(session));
  fail(index, reason);
}

static void expect_command(char expected, unsigned index) {
  char byte;
  ssize_t n;
  do
    n = read(STDIN_FILENO, &byte, 1);
  while (n < 0 && errno == EINTR);
  if (n != 1 || byte != expected)
    fail(index, "bad-command");
}

/* One bounded wait and dispatch pass, honoring the SDK's timeout hint. */
static void step(uint64_t deadline, unsigned index) {
  struct pollfd p;
  uint64_t now = now_ns();
  int hint, wait, ready;
  if (now >= deadline)
    fail(index, "deadline");
  hint = sophia_os_timeout(session, ms(now));
  wait = (int)((deadline - now + UINT64_C(999999)) / UINT64_C(1000000));
  if (hint >= 0 && hint < wait)
    wait = hint;
  p.fd = sophia_os_poll_fd(session);
  p.events = sophia_os_poll_events(session);
  p.revents = 0;
  ready = poll(&p, 1, wait);
  if (ready < 0 && errno != EINTR)
    fail(index, "poll");
  if (sophia_os_dispatch(session, ready > 0 ? p.revents : 0, BUDGET,
                         ms(now_ns())))
    session_failure(index);
}

static void open_session(uint64_t deadline, unsigned index) {
  struct sophia_desktop_connection c = {-1, 0, 0, 0};
  struct sophia_os_config config;
  int result;
  for (;;) {
    result = sophia_desktop_connection_begin(&c, socket_path);
    while (result == SOPHIA_DESKTOP_CONNECTING) {
      struct pollfd p;
      uint64_t now = now_ns();
      int ready;
      if (now >= deadline) {
        sophia_desktop_connection_close(&c);
        fail(index, "connect-deadline");
      }
      p.fd = c.fd;
      p.events = sophia_desktop_connection_events(&c);
      p.revents = 0;
      ready = poll(&p, 1,
                   (int)((deadline - now + UINT64_C(999999)) /
                         UINT64_C(1000000)));
      if (ready < 0 && errno != EINTR) {
        sophia_desktop_connection_close(&c);
        fail(index, "connect-poll");
      }
      result = sophia_desktop_connection_finish(&c, ready > 0 ? p.revents : 0);
    }
    if (result == SOPHIA_DESKTOP_CONNECTED)
      break;
    if (result == SOPHIA_DESKTOP_CONNECT_RETRY &&
        now_ns() + RETRY_NS < deadline) {
      struct timespec pause = {0, (long)RETRY_NS};
      nanosleep(&pause, NULL);
      continue;
    }
    sophia_desktop_connection_close(&c);
    fail(index, "connect");
  }
  session_fd = sophia_desktop_connection_take(&c);
  memset(&config, 0, sizeof(config));
  config.msize = MSIZE;
  config.offer.minimum_revision = config.offer.maximum_revision = 1;
  config.offer.capabilities = SOPHIA_OF_CAP_OBSERVE | SOPHIA_OF_CAP_CONFIGURE;
  config.bootstrap_deadline_ms = ms(deadline);
  if (sophia_os_open_fd(session, session_fd, &config, storage, storage_bytes,
                        ms(now_ns())))
    fail(index, "session-open");
  session_open = 1;
}

/* The published topology must be exactly the named fixture's shape, with
 * every head connected, enabled and grouped by an existing head. */
static void check_fixture(unsigned index) {
  const struct sophia_of_topology *t = sophia_os_topology(session);
  unsigned heads = fixture_max ? 16u : 1u, modes = fixture_max ? 2048u : 2u;
  unsigned groups = fixture_max ? 16u : 1u, i, g, m;
  if (!t || t->head_count != heads || t->mode_count != modes ||
      t->group_count != groups)
    fail(index, "fixture-mismatch");
  for (i = 0; i < t->head_count; ++i)
    if ((t->heads[i].flags &
         (SOPHIA_OF_HEAD_CONNECTED | SOPHIA_OF_HEAD_ENABLED)) !=
            (SOPHIA_OF_HEAD_CONNECTED | SOPHIA_OF_HEAD_ENABLED) ||
        !t->heads[i].current_mode)
      fail(index, "fixture-mismatch");
  for (g = 0; g < t->group_count; ++g)
    for (m = 0; m < t->groups[g].member_count; ++m) {
      for (i = 0; i < t->head_count; ++i)
        if (t->heads[i].head == t->groups[g].members[m].head)
          break;
      if (i == t->head_count)
        fail(index, "fixture-mismatch");
    }
}

/* Wait for the cumulative ack covering every consumed event to be replied
 * to. Any newly presented event meanwhile is unexpected. */
static void wait_acknowledged(uint64_t deadline, unsigned index) {
  for (;;) {
    struct sophia_os_obligations o;
    const struct sophia_of_record *event;
    if (sophia_os_obligations(session, &o))
      fail(index, "obligations");
    if (o.acked >= o.consumed && o.consumed)
      return;
    step(deadline, index);
    if (!sophia_os_event(session, &event))
      fail(index, "unexpected-event");
  }
}

/* Bootstrap ends after the first ObjectPublished (the SDK has read that exact
 * object) is consumed and its cumulative ack reply observed. */
static void bootstrap(uint64_t deadline, unsigned index) {
  const struct sophia_of_record *event;
  while (sophia_os_event(session, &event))
    step(deadline, index);
  if (event->header.kind != SOPHIA_OF_OBJECT_PUBLISHED)
    fail(index, "unexpected-event");
  check_fixture(index);
  if (sophia_os_consume(session))
    session_failure(index);
  wait_acknowledged(deadline, index);
}

/* Restate the published topology exactly: current modes, published groups
 * in published order, explicit Normal transform and Disabled VRR. */
static void build_proposal(uint64_t transaction, struct sophia_of_proposal *p,
                           unsigned index) {
  const struct sophia_of_topology *t = sophia_os_topology(session);
  unsigned i, g;
  memset(p, 0, sizeof(*p));
  p->transaction = transaction;
  p->base_topology_epoch = t->topology_epoch;
  p->intent = SOPHIA_OF_VALIDATE_ONLY;
  p->head_count = t->head_count;
  p->group_count = t->group_count;
  for (i = 0; i < t->head_count; ++i) {
    p->heads[i].head = t->heads[i].head;
    p->heads[i].generation = t->heads[i].generation;
    p->heads[i].mode = t->heads[i].current_mode;
    p->heads[i].transform = SOPHIA_OF_NORMAL;
    p->heads[i].vrr = SOPHIA_OF_VRR_DISABLED;
  }
  p->primary_group_index = UINT16_MAX;
  for (g = 0; g < t->group_count; ++g) {
    const struct sophia_of_group *from = &t->groups[g];
    struct sophia_of_proposal_group *to = &p->groups[g];
    to->output = from->output;
    to->x = from->x;
    to->y = from->y;
    to->width = from->width;
    to->height = from->height;
    to->member_count = from->member_count;
    for (i = 0; i < from->member_count; ++i)
      to->members[i] = from->members[i];
    if (from->output == t->primary_output)
      p->primary_group_index = (uint16_t)g;
  }
  if (p->primary_group_index == UINT16_MAX)
    fail(index, "fixture-mismatch");
}

static void run_connect(void) {
  unsigned i;
  expect_command('G', 0);
  for (i = 0; i < CONNECTIONS; ++i) {
    uint64_t t0, t1, epoch;
    expect_command('N', i);
    t0 = now_ns();
    open_session(t0 + SAMPLE_NS, i);
    bootstrap(t0 + SAMPLE_NS, i);
    t1 = now_ns();
    epoch = sophia_os_epoch(session);
    close_session();
    emit("S connect %u %" PRIu64 " %" PRIu64 " %" PRIu64 " 0\n", i, t0, t1,
         epoch);
    flush_lines(); /* Outside timing: the harness gates the next N on it. */
  }
  emit("DONE connect %u\n", CONNECTIONS);
  flush_lines();
  expect_command('X', CONNECTIONS);
}

/* One proposal: submit, the matching Validated outcome exactly once,
 * consumed, and its cumulative ack reply observed. */
static void propose(const char *kind, unsigned index, uint64_t epoch) {
  struct sophia_of_proposal proposal;
  const struct sophia_of_record *event;
  enum sophia_os_custody custody;
  uint64_t transaction, ticket, t0, t1, deadline;
  uint32_t wire_error;
  if (sophia_os_next_transaction(session, &transaction))
    fail(index, "transaction");
  build_proposal(transaction, &proposal, index);
  t0 = now_ns();
  deadline = t0 + SAMPLE_NS;
  if (sophia_os_submit(session, &proposal, ms(deadline), &ticket))
    fail(index, "submit");
  for (;;) {
    step(deadline, index);
    if (sophia_os_outcome(session, ticket, &custody, &wire_error))
      fail(index, "custody");
    if (custody == SOPHIA_OS_REFUSED_SUBMIT)
      fail(index, "refused-submit");
    if (!sophia_os_event(session, &event))
      break;
  }
  if (event->header.kind != SOPHIA_OF_OUTCOME ||
      event->value.outcome.transaction != transaction)
    fail(index, "unexpected-event");
  if (event->value.outcome.outcome != SOPHIA_OF_VALIDATED)
    fail(index, "bad-outcome");
  if (sophia_os_consume(session))
    session_failure(index);
  wait_acknowledged(deadline, index);
  t1 = now_ns();
  if (sophia_os_outcome(session, ticket, &custody, &wire_error) ||
      custody != SOPHIA_OS_SUBMITTED)
    fail(index, "custody");
  emit("S %s %u %" PRIu64 " %" PRIu64 " %" PRIu64 " %" PRIu64 "\n", kind,
       index, t0, t1, epoch, transaction);
}

/* Complete any immediate SDK work; a presented event is unexpected. */
static void drain(uint64_t deadline, unsigned index) {
  const struct sophia_of_record *event;
  while (sophia_os_timeout(session, ms(now_ns())) == 0) {
    step(deadline, index);
    if (!sophia_os_event(session, &event))
      fail(index, "unexpected-event");
  }
}

static void run_proposals(void) {
  uint64_t start, epoch;
  unsigned i;
  expect_command('G', 0);
  start = now_ns();
  open_session(start + SAMPLE_NS, 0);
  bootstrap(start + SAMPLE_NS, 0);
  epoch = sophia_os_epoch(session);
  emit("READY proposals %" PRIu64 "\n", epoch);
  flush_lines();
  expect_command('N', 0);
  for (i = 0; i < WARMUP; ++i)
    propose("warmup", i, epoch);
  for (i = 0; i < MEASURED; ++i)
    propose("proposals", i, epoch);
  /* Already-pending SDK work must present nothing more. A later duplicate
   * delivery is the harness's to detect from owner-side counts. */
  drain(now_ns() + SAMPLE_NS, MEASURED);
  emit("DONE proposals %u\n", WARMUP + MEASURED);
  flush_lines();
  expect_command('X', MEASURED);
}

static int same_obligations(const struct sophia_os_obligations *a,
                            const struct sophia_os_obligations *b) {
  return a->consumed == b->consumed && a->acked == b->acked &&
         a->submitted_sequence == b->submitted_sequence &&
         a->deadline_ms == b->deadline_ms && a->retry_at_ms == b->retry_at_ms &&
         a->event_pending == b->event_pending &&
         a->topology_pending == b->topology_pending &&
         a->waiting_for_ack == b->waiting_for_ack;
}

/* Idle: block on the session and stdin with the SDK's own timeout. Readable
 * protocol work is dispatched; repeated readiness that changes nothing is a
 * spin and fails, as does any event. */
static void run_idle(void) {
  struct sophia_os_obligations o, before;
  const struct sophia_of_record *event;
  uint64_t start;
  unsigned stalled = 0;
  expect_command('G', 0);
  start = now_ns();
  open_session(start + SAMPLE_NS, 0);
  bootstrap(start + SAMPLE_NS, 0);
  drain(start + SAMPLE_NS, 0);
  if (sophia_os_obligations(session, &o) || o.deadline_ms || o.retry_at_ms ||
      o.event_pending || o.topology_pending || o.waiting_for_ack ||
      o.acked != o.consumed || sophia_os_timeout(session, ms(now_ns())) != -1)
    fail(0, "not-idle");
  emit("READY idle %" PRIu64 "\n", sophia_os_epoch(session));
  flush_lines();
  for (;;) {
    struct pollfd p[2];
    int hint = sophia_os_timeout(session, ms(now_ns())), ready;
    p[0].fd = sophia_os_poll_fd(session);
    p[0].events = sophia_os_poll_events(session);
    p[0].revents = 0;
    p[1].fd = STDIN_FILENO;
    p[1].events = POLLIN;
    p[1].revents = 0;
    ready = poll(p, 2, hint);
    if (ready < 0) {
      if (errno == EINTR)
        continue;
      fail(0, "poll");
    }
    if (p[1].revents) {
      expect_command('X', 0);
      break;
    }
    if (sophia_os_obligations(session, &before))
      fail(0, "obligations");
    if (sophia_os_dispatch(session, p[0].revents, BUDGET, ms(now_ns())))
      session_failure(0);
    if (!sophia_os_event(session, &event))
      fail(0, "unexpected-event");
    if (sophia_os_obligations(session, &o))
      fail(0, "obligations");
    stalled = same_obligations(&before, &o) ? stalled + 1 : 0;
    if (stalled == IDLE_NO_PROGRESS)
      fail(0, "no-progress");
  }
  emit("DONE idle 0\n");
  flush_lines();
}

int main(int argc, char **argv) {
  if (argc != 4 || argv[1][0] != '/') {
    fputs("usage: output_files_perf_peer /SOCKET connect|proposals|idle "
          "small|max\n",
          stderr);
    return 2;
  }
  socket_path = argv[1];
  mode_name = argv[2];
  if (!strcmp(mode_name, "connect"))
    mode = CONNECT;
  else if (!strcmp(mode_name, "proposals"))
    mode = PROPOSALS;
  else if (!strcmp(mode_name, "idle"))
    mode = IDLE;
  else {
    fputs("output_files_perf_peer: unknown mode\n", stderr);
    return 2;
  }
  if (!strcmp(argv[3], "max"))
    fixture_max = 1;
  else if (strcmp(argv[3], "small")) {
    fputs("output_files_perf_peer: unknown fixture\n", stderr);
    return 2;
  }
  storage_bytes = sophia_os_storage_bytes(MSIZE);
  session = calloc(1, sophia_os_state_bytes());
  storage = malloc(storage_bytes);
  if (!storage_bytes || !session || !storage) {
    fputs("output_files_perf_peer: allocation failed\n", stderr);
    return 2;
  }
  if (mode == CONNECT)
    run_connect();
  else if (mode == PROPOSALS)
    run_proposals();
  else
    run_idle();
  close_session();
  free(storage);
  free(session);
  return 0;
}

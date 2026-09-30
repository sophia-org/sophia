/* Generic output file native proof peer (t253). Session supervises it through
 * --output-process; the endpoint comes from SOPHIA_OUTPUT_9P_SOCKET. The
 * arguments are described in output_files_native_proof/arguments.h.
 *
 * Layout A is the declared baseline and B the target. Each lists the complete
 * enabled head set; a connected head that is not listed is disabled by intent.
 * Transform and VRR come only from argv: output revision 1 publishes neither,
 * so they are never inferred. Head generations are taken from the published
 * topology. The peer acts only after the published topology has exactly the
 * declared A epoch and matches A on every verifiable field.
 *
 * The pinned public C desktop SDK is the only protocol client. Every event is
 * one bounded line, written and flushed immediately, because the termination
 * stage ends by signal. There is no stdin gate and no signal handler.
 *
 * Exit: 0 pass; 1 session or protocol failure; 2 usage; 3 baseline not
 * reached; 4 unexpected event or outcome; 5 an outcome arrived before the
 * supervisor terminated the peer; 6 the peer was not terminated by the
 * deadline. */
#define _POSIX_C_SOURCE 200809L
#include "output_files_native_proof/arguments.h"
#include "sophia_desktop_connection.h"
#include "sophia_output_session.h"
#include <errno.h>
#include <inttypes.h>
#include <poll.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>

#define MSIZE 65536u
#define BUDGET 131072u
#define RECORD_BYTES 256u
#define RETRY_NS UINT64_C(5000000)

enum exit_code {
  EXIT_PASS = 0,
  EXIT_SESSION = 1,
  EXIT_USAGE = 2,
  EXIT_BASELINE = 3,
  EXIT_UNEXPECTED = 4,
  EXIT_OUTCOME_BEFORE_TERMINATION = 5,
  EXIT_NOT_TERMINATED = 6
};

static struct proof_arguments args;
static uint64_t deadline_ns;
/* Every publication's Qid names new bytes; none may repeat the last one. */
static uint64_t last_qid;
static struct sophia_os *session;
static void *storage;
static size_t storage_bytes;
static int session_fd = -1, session_open;

static uint64_t now_ns(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return (uint64_t)t.tv_sec * UINT64_C(1000000000) + (uint64_t)t.tv_nsec;
}
static uint64_t ms(uint64_t ns) { return ns / UINT64_C(1000000); }

/* One record per event, flushed at once: the process may die by signal. */
static void record(const char *event, const char *format, ...) {
  char line[RECORD_BYTES];
  va_list list;
  int head, body = 0;
  head = snprintf(line, sizeof(line),
                  "sophia_output_proof schema=1 stage=%s event=%s t=%" PRIu64,
                  args.stage_name ? args.stage_name : "none", event, now_ns());
  if (head > 0 && (unsigned)head < sizeof(line) && format && *format) {
    line[head] = ' ';
    va_start(list, format);
    body = vsnprintf(line + head + 1, sizeof(line) - (size_t)head - 1, format,
                     list);
    va_end(list);
    body = body < 0 ? 0 : body + 1;
  }
  if (head < 0 || (unsigned)(head + body) >= sizeof(line) - 1) {
    fputs("sophia_output_proof schema=1 event=fail reason=record-bound\n",
          stdout);
    fflush(stdout);
    exit(EXIT_SESSION);
  }
  line[head + body] = '\n';
  line[head + body + 1] = 0;
  fputs(line, stdout);
  fflush(stdout);
}
static void close_session(void) {
  if (session_open)
    sophia_os_close(session);
  session_open = 0;
  if (session_fd >= 0)
    close(session_fd);
  session_fd = -1;
}
/* Reasons are short and whitespace-free. */
static void finish(enum exit_code code, const char *reason) {
  if (code == EXIT_PASS)
    record("pass", "");
  else
    record("fail", "exit=%d reason=%s", (int)code, reason);
  close_session();
  exit((int)code);
}
static void session_failure(void) {
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
  record("session", "state=%s remote_errno=%" PRIu32, state,
         sophia_os_remote_error(session));
  finish(EXIT_SESSION, state);
}
static void usage(const char *reason) {
  fprintf(stderr,
          "output_files_native_proof_peer: %s%s%s%s\n"
          "usage: output_files_native_proof_peer "
          "--stage=validate|reject|commit-restore|apply-await-termination "
          "[--deadline-ms=N] --a-topology-epoch=E --a-heads=H --a-groups=G "
          "--a-primary=P [--b-heads=H --b-groups=G --b-primary=P]\n",
          args.layout_name ? "layout " : "",
          args.layout_name ? args.layout_name : "",
          args.layout_name ? ": " : "", reason);
  record("fail", "exit=%d reason=usage", (int)EXIT_USAGE);
  exit(EXIT_USAGE);
}

/* ---- session ---- */

static void step(uint64_t deadline) {
  struct pollfd p;
  uint64_t now = now_ns();
  int hint, wait, ready;
  hint = sophia_os_timeout(session, ms(now));
  wait = now >= deadline ? 0
                         : (int)((deadline - now + UINT64_C(999999)) /
                                 UINT64_C(1000000));
  if (hint >= 0 && hint < wait)
    wait = hint;
  p.fd = sophia_os_poll_fd(session);
  p.events = sophia_os_poll_events(session);
  p.revents = 0;
  ready = poll(&p, 1, wait);
  if (ready < 0 && errno != EINTR)
    finish(EXIT_SESSION, "poll");
  if (sophia_os_dispatch(session, ready > 0 ? p.revents : 0, BUDGET,
                         ms(now_ns())))
    session_failure();
}
static void open_session(void) {
  struct sophia_desktop_endpoint endpoint;
  struct sophia_desktop_connection c = {-1, 0, 0, 0};
  struct sophia_os_config config;
  uint64_t deadline = now_ns() + deadline_ns;
  int result;
  if (sophia_desktop_output_environment(&endpoint))
    finish(EXIT_SESSION, "output-endpoint-environment");
  for (;;) {
    result = sophia_desktop_connection_begin(&c, endpoint.path);
    while (result == SOPHIA_DESKTOP_CONNECTING) {
      struct pollfd p;
      uint64_t now = now_ns();
      int ready;
      if (now >= deadline) {
        sophia_desktop_connection_close(&c);
        finish(EXIT_SESSION, "connect-deadline");
      }
      p.fd = c.fd;
      p.events = sophia_desktop_connection_events(&c);
      p.revents = 0;
      ready = poll(&p, 1,
                   (int)((deadline - now + UINT64_C(999999)) /
                         UINT64_C(1000000)));
      if (ready < 0 && errno != EINTR) {
        sophia_desktop_connection_close(&c);
        finish(EXIT_SESSION, "connect-poll");
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
    finish(EXIT_SESSION, "connect");
  }
  session_fd = sophia_desktop_connection_take(&c);
  memset(&config, 0, sizeof(config));
  config.msize = MSIZE;
  config.offer.minimum_revision = config.offer.maximum_revision = 1;
  config.offer.capabilities = SOPHIA_OF_CAP_OBSERVE | SOPHIA_OF_CAP_CONFIGURE;
  config.bootstrap_deadline_ms = ms(deadline);
  if (sophia_os_open_fd(session, session_fd, &config, storage, storage_bytes,
                        ms(now_ns())))
    finish(EXIT_SESSION, "session-open");
  session_open = 1;
}
/* Wait for the ack covering every consumed event to be replied to; any
 * event presented meanwhile is unexpected. */
static void wait_acknowledged(void) {
  uint64_t deadline = now_ns() + deadline_ns;
  for (;;) {
    struct sophia_os_obligations o;
    const struct sophia_of_record *event;
    if (sophia_os_obligations(session, &o))
      finish(EXIT_SESSION, "obligations");
    if (o.acked >= o.consumed)
      return;
    if (now_ns() >= deadline)
      finish(EXIT_SESSION, "ack-deadline");
    step(deadline);
    if (!sophia_os_event(session, &event))
      finish(EXIT_UNEXPECTED, "event-while-acknowledging");
  }
}
/* Next presented event within the deadline, or NULL at the deadline. */
static const struct sophia_of_record *next_event(uint64_t deadline) {
  const struct sophia_of_record *event;
  while (sophia_os_event(session, &event)) {
    if (now_ns() >= deadline)
      return NULL;
    step(deadline);
  }
  return event;
}
static void consume(void) {
  if (sophia_os_consume(session))
    session_failure();
  wait_acknowledged();
}

/* ---- topology ---- */

static const struct sophia_of_head *published_head(
    const struct sophia_of_topology *t, uint64_t head) {
  unsigned i;
  for (i = 0; i < t->head_count; ++i)
    if (t->heads[i].head == head)
      return &t->heads[i];
  return NULL;
}
/* The published topology shows exactly this layout's verifiable fields:
 * enabled head set and current modes, outputs, geometry, membership and
 * mappings, and the primary output. */
static int matches(const struct sophia_of_topology *t, const struct layout *l) {
  unsigned i, j, enabled = 0;
  for (i = 0; i < t->head_count; ++i)
    enabled += (t->heads[i].flags & SOPHIA_OF_HEAD_ENABLED) != 0;
  if (enabled != l->head_count || t->group_count != l->group_count ||
      t->primary_output != l->groups[l->primary].output)
    return 0;
  for (i = 0; i < l->head_count; ++i) {
    const struct sophia_of_head *h = published_head(t, l->heads[i].head);
    if (!h || !(h->flags & SOPHIA_OF_HEAD_ENABLED) ||
        h->current_mode != l->heads[i].mode)
      return 0;
  }
  for (i = 0; i < l->group_count; ++i) {
    const struct layout_group *g = &l->groups[i];
    const struct sophia_of_group *p = NULL;
    for (j = 0; j < t->group_count; ++j)
      if (t->groups[j].output == g->output)
        p = &t->groups[j];
    if (!p || p->x != g->x || p->y != g->y || p->width != g->width ||
        p->height != g->height || p->member_count != g->member_count)
      return 0;
    for (j = 0; j < g->member_count; ++j) {
      unsigned k;
      for (k = 0; k < p->member_count; ++k)
        if (p->members[k].head == g->members[j].head &&
            p->members[k].mapping == g->members[j].mapping)
          break;
      if (k == p->member_count)
        return 0;
    }
  }
  return 1;
}
/* Record a presented publication and require a fresh Qid. The record comes
 * first so a refusal still shows what was published. */
static void publication(const struct sophia_of_record *event,
                        const char *matched) {
  const struct sophia_of_topology *t = sophia_os_topology(session);
  uint64_t qid = event->value.published.qid_path;
  record("topology",
         "topology_epoch=%" PRIu64 " qid=%" PRIu64 " heads=%u groups=%u "
         "match=%s",
         event->value.published.topology_epoch, qid, (unsigned)t->head_count,
         (unsigned)t->group_count, matched);
  if (last_qid && qid == last_qid)
    finish(EXIT_UNEXPECTED, "publication-qid-reused");
  last_qid = qid;
}
/* Readiness: consume publications until the exact A epoch is published and
 * matches A. A later epoch, or a mismatch at that epoch, fails. */
static void await_baseline(void) {
  uint64_t deadline = now_ns() + deadline_ns;
  for (;;) {
    const struct sophia_of_record *event = next_event(deadline);
    uint64_t epoch;
    int matched;
    if (!event)
      finish(EXIT_BASELINE, "baseline-deadline");
    if (event->header.kind != SOPHIA_OF_OBJECT_PUBLISHED)
      finish(EXIT_UNEXPECTED, "event-before-baseline");
    epoch = event->value.published.topology_epoch;
    matched = matches(sophia_os_topology(session), &args.a);
    publication(event, matched ? "a" : "none");
    consume();
    if (epoch > args.a_epoch)
      finish(EXIT_BASELINE, "baseline-epoch-passed");
    if (epoch == args.a_epoch) {
      if (!matched)
        finish(EXIT_BASELINE, "baseline-mismatch");
      record("ready", "topology_epoch=%" PRIu64, epoch);
      return;
    }
  }
}
static void build(const struct layout *l, uint16_t intent, uint64_t txn,
                  struct sophia_of_proposal *p) {
  const struct sophia_of_topology *t = sophia_os_topology(session);
  unsigned i, j;
  memset(p, 0, sizeof(*p));
  p->transaction = txn;
  p->base_topology_epoch = t->topology_epoch;
  p->intent = intent;
  p->head_count = (uint16_t)l->head_count;
  p->group_count = (uint16_t)l->group_count;
  p->primary_group_index = (uint16_t)l->primary;
  for (i = 0; i < l->head_count; ++i) {
    const struct sophia_of_head *h = published_head(t, l->heads[i].head);
    if (!h)
      finish(EXIT_BASELINE, "layout-head-not-published");
    p->heads[i].head = l->heads[i].head;
    p->heads[i].generation = h->generation;
    p->heads[i].mode = l->heads[i].mode;
    p->heads[i].transform = l->heads[i].transform;
    p->heads[i].vrr = l->heads[i].vrr;
  }
  for (i = 0; i < l->group_count; ++i) {
    struct sophia_of_proposal_group *g = &p->groups[i];
    g->output = l->groups[i].output;
    g->x = l->groups[i].x;
    g->y = l->groups[i].y;
    g->width = l->groups[i].width;
    g->height = l->groups[i].height;
    g->member_count = (uint16_t)l->groups[i].member_count;
    for (j = 0; j < l->groups[i].member_count; ++j)
      g->members[j] = l->groups[i].members[j];
  }
}
static const char *outcome_name(uint16_t outcome) {
  static const char *const names[] = {"validated", "committed",  "stale",
                                      "rejected",  "rolled-back", "failed"};
  return outcome >= 1 && outcome <= 6 ? names[outcome - 1] : "unknown";
}
static void record_outcome(const struct sophia_of_record *event) {
  record("outcome",
         "txn=%" PRIu64 " kind=%s reason=%u topology_epoch=%" PRIu64,
         event->value.outcome.transaction,
         outcome_name(event->value.outcome.outcome),
         (unsigned)event->value.outcome.reason,
         event->value.outcome.topology_epoch);
}
static uint64_t submit(const struct sophia_of_proposal *proposal,
                       const char *label) {
  uint64_t ticket;
  record("submit",
         "layout=%s txn=%" PRIu64 " intent=%s base_topology_epoch=%" PRIu64,
         label, proposal->transaction,
         proposal->intent == SOPHIA_OF_APPLY ? "apply" : "validate-only",
         proposal->base_topology_epoch);
  if (sophia_os_submit(session, proposal, ms(now_ns() + deadline_ns), &ticket))
    finish(EXIT_SESSION, "submit");
  return ticket;
}
/* Record custody once when the SDK has observed Submitted. */
static void note_custody(uint64_t ticket, int *noted) {
  enum sophia_os_custody custody;
  uint32_t wire_error;
  if (sophia_os_outcome(session, ticket, &custody, &wire_error))
    finish(EXIT_SESSION, "custody");
  if (custody == SOPHIA_OS_REFUSED_SUBMIT) {
    record("submit-refused", "errno=%" PRIu32, wire_error);
    finish(EXIT_UNEXPECTED, "submit-refused");
  }
  if (custody == SOPHIA_OS_SUBMITTED && !*noted) {
    record("submitted", "ticket=%" PRIu64, ticket);
    *noted = 1;
  }
}
/* Exactly one terminal outcome for this proposal, reported faithfully, then
 * checked: its kind, and its topology epoch, which is the proposal's base
 * for Validated and Rejected and the next epoch for Committed. It is
 * consumed and acknowledged before returning. */
static void expect_outcome(const struct sophia_of_proposal *proposal,
                           uint64_t ticket, uint16_t expected) {
  uint64_t deadline = now_ns() + deadline_ns;
  uint64_t epoch = proposal->base_topology_epoch +
                   (expected == SOPHIA_OF_COMMITTED ? 1u : 0u);
  const struct sophia_of_record *event;
  int noted = 0;
  for (;;) {
    note_custody(ticket, &noted);
    if (!sophia_os_event(session, &event))
      break;
    if (now_ns() >= deadline)
      finish(EXIT_UNEXPECTED, "outcome-deadline");
    step(deadline);
  }
  note_custody(ticket, &noted);
  if (event->header.kind != SOPHIA_OF_OUTCOME ||
      event->value.outcome.transaction != proposal->transaction) {
    record("unexpected", "kind=%u", (unsigned)event->header.kind);
    finish(EXIT_UNEXPECTED, "unexpected-event");
  }
  record_outcome(event);
  if (event->value.outcome.outcome != expected)
    finish(EXIT_UNEXPECTED, "unexpected-outcome");
  if (event->value.outcome.topology_epoch != epoch)
    finish(EXIT_UNEXPECTED, "outcome-topology-epoch");
  consume();
}
/* A commit republishes: the next event is the new topology, which must be
 * the given epoch, show the given layout and carry a fresh Qid. */
static void expect_publication(const struct layout *l, const char *label,
                               uint64_t epoch) {
  const struct sophia_of_record *event = next_event(now_ns() + deadline_ns);
  int matched;
  if (!event)
    finish(EXIT_UNEXPECTED, "publication-deadline");
  if (event->header.kind != SOPHIA_OF_OBJECT_PUBLISHED)
    finish(EXIT_UNEXPECTED, "unexpected-event");
  matched = matches(sophia_os_topology(session), l);
  publication(event, matched ? label : "none");
  if (event->value.published.topology_epoch != epoch || !matched)
    finish(EXIT_UNEXPECTED, "publication-mismatch");
  consume();
}
/* Already-pending SDK work must present nothing more before a clean exit. */
static void drain(void) {
  uint64_t deadline = now_ns() + deadline_ns;
  const struct sophia_of_record *event;
  while (sophia_os_timeout(session, ms(now_ns())) == 0) {
    if (now_ns() >= deadline)
      finish(EXIT_SESSION, "drain-deadline");
    step(deadline);
    if (!sophia_os_event(session, &event))
      finish(EXIT_UNEXPECTED, "unexpected-event");
  }
}

/* ---- stages ---- */

static uint64_t transaction(void) {
  uint64_t value;
  if (sophia_os_next_transaction(session, &value))
    finish(EXIT_SESSION, "transaction");
  return value;
}
static void run_validate(void) {
  struct sophia_of_proposal p;
  build(&args.b, SOPHIA_OF_VALIDATE_ONLY, transaction(), &p);
  expect_outcome(&p, submit(&p, "b"), SOPHIA_OF_VALIDATED);
}
/* Layout A with its first head naming a mode absent from that head's
 * published table: well-formed, semantically invalid. */
static void run_reject(void) {
  const struct sophia_of_topology *t = sophia_os_topology(session);
  const struct sophia_of_head *h = published_head(t, args.a.heads[0].head);
  struct sophia_of_proposal p;
  uint64_t highest = 0;
  unsigned i;
  if (!h)
    finish(EXIT_BASELINE, "layout-head-not-published");
  for (i = h->first_mode; i < (unsigned)h->first_mode + h->mode_count; ++i)
    if (t->modes[i].mode > highest)
      highest = t->modes[i].mode;
  if (highest == UINT64_MAX)
    finish(EXIT_USAGE, "no-absent-mode-id");
  build(&args.a, SOPHIA_OF_APPLY, transaction(), &p);
  p.heads[0].mode = highest + 1;
  record("reject-candidate", "head=%" PRIu64 " absent_mode=%" PRIu64,
         p.heads[0].head, highest + 1);
  expect_outcome(&p, submit(&p, "a-unknown-mode"), SOPHIA_OF_REJECTED);
}
static void run_commit_restore(void) {
  struct sophia_of_proposal p;
  build(&args.b, SOPHIA_OF_APPLY, transaction(), &p);
  expect_outcome(&p, submit(&p, "b"), SOPHIA_OF_COMMITTED);
  expect_publication(&args.b, "b", args.a_epoch + 1);
  build(&args.a, SOPHIA_OF_APPLY, transaction(), &p);
  expect_outcome(&p, submit(&p, "a"), SOPHIA_OF_COMMITTED);
  expect_publication(&args.a, "a", args.a_epoch + 2);
}
/* Apply B, then keep dispatching until the supervisor ends this process
 * after native apply. Any outcome first means the hold did not hold. */
static void run_apply_await_termination(void) {
  struct sophia_of_proposal p;
  const struct sophia_of_record *event;
  uint64_t ticket, deadline;
  int noted = 0;
  build(&args.b, SOPHIA_OF_APPLY, transaction(), &p);
  ticket = submit(&p, "b");
  record("awaiting-termination", "txn=%" PRIu64, p.transaction);
  deadline = now_ns() + deadline_ns;
  for (;;) {
    note_custody(ticket, &noted);
    if (!sophia_os_event(session, &event)) {
      if (event->header.kind == SOPHIA_OF_OUTCOME &&
          event->value.outcome.transaction == p.transaction) {
        record_outcome(event);
        finish(EXIT_OUTCOME_BEFORE_TERMINATION, "outcome-before-termination");
      }
      record("unexpected", "kind=%u", (unsigned)event->header.kind);
      finish(EXIT_UNEXPECTED, "unexpected-event");
    }
    if (now_ns() >= deadline)
      finish(EXIT_NOT_TERMINATED, "not-terminated");
    step(deadline);
  }
}

int main(int argc, char **argv) {
  const char *refusal = proof_parse_arguments(argc, argv, &args);
  if (refusal)
    usage(refusal);
  deadline_ns = args.deadline_ms * UINT64_C(1000000);
  storage_bytes = sophia_os_storage_bytes(MSIZE);
  session = calloc(1, sophia_os_state_bytes());
  storage = malloc(storage_bytes);
  if (!storage_bytes || !session || !storage) {
    fputs("output_files_native_proof_peer: allocation failed\n", stderr);
    return EXIT_SESSION;
  }
  record("start", "a_topology_epoch=%" PRIu64 " a_heads=%u b=%s", args.a_epoch,
         args.a.head_count, args.have_b ? "yes" : "no");
  open_session();
  {
    uint64_t deadline = now_ns() + deadline_ns;
    while (sophia_os_state(session) == SOPHIA_OS_NEGOTIATING) {
      if (now_ns() >= deadline)
        finish(EXIT_SESSION, "negotiation-deadline");
      step(deadline);
    }
    if (sophia_os_state(session) != SOPHIA_OS_READY)
      session_failure();
    record("negotiated", "epoch=%" PRIu64 " capabilities=%" PRIu64,
           sophia_os_epoch(session), sophia_os_capabilities(session));
    if (!(sophia_os_capabilities(session) & SOPHIA_OF_CAP_CONFIGURE))
      finish(EXIT_SESSION, "configure-not-granted");
  }
  await_baseline();
  if (args.stage == VALIDATE)
    run_validate();
  else if (args.stage == REJECT)
    run_reject();
  else if (args.stage == COMMIT_RESTORE)
    run_commit_restore();
  else
    run_apply_await_termination();
  drain();
  finish(EXIT_PASS, "");
  return EXIT_PASS;
}

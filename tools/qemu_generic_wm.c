/* A generic window manager for QEMU guest scenarios, on the vendored C SDK's
 * public WM session API only (sophia_wm_session.h). It is a test fixture, not
 * a policy: every surface on an output is placed at that output's work-area
 * origin at its own size, unassigned surfaces go to the active output, and
 * the newest surface on the active output is focused. All geometry and
 * identities come from Session's snapshots; nothing is product specific.
 *
 * It answers Session's profile handoff (prepare, activate, rollback) with the
 * exact server identities, submits one Configuration for the active profile
 * generation, then answers every Cycle with a complete projection over the
 * Cycle's outputs. Session operations are not requested by this policy; an
 * outcome that asks for one is a failure. Session starts it in the WM
 * protection domain with SOPHIA_WM_9P_SOCKET.
 *
 * It reports on stderr only on state changes and once per second at most.
 *
 * With the one argument --hold-shift only (the controlled-repaint
 * comparison; the protection domain clears the environment), it also requires
 * actions and registers one, "hold-shift". Each Action Cycle naming it moves
 * the placements on every output 8 pixels further right, so Session
 * recomposes a static client at a crop that names the shift: the nth shift
 * places it at x offset 8n, unless that would leave the work area. Each
 * proposal and its outcome are reported with their transaction and request
 * identities; Session's own settlement record is the authority, not these
 * lines. */
#define _GNU_SOURCE
#include "sophia_wm_session.h"
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

#define MAX_PLACEMENTS 1024u

static struct sophia_ws *session;
static uint64_t profile_generation, projections, rejected, cycles, reported;

#define HOLD_SHIFT_ACTION 1u
#define HOLD_SHIFT_PIXELS 8
static int hold_shift;     /* the controlled-repaint opt-in */
static int32_t hold_offset; /* HOLD_SHIFT_PIXELS per committed shift */

static uint64_t now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return (uint64_t)t.tv_sec * 1000u + (uint64_t)t.tv_nsec / 1000000u;
}

static void die(const char *step) {
  fprintf(stderr,
          "sophia_qemu_wm schema=1 status=failed step=%s state=%d remote=%u\n",
          step, session ? (int)sophia_ws_state(session) : -1,
          session ? sophia_ws_remote_error(session) : 0);
  exit(1);
}

/* One poll and one dispatch pass. */
static void step(void) {
  struct pollfd p = {sophia_ws_poll_fd(session), sophia_ws_poll_events(session), 0};
  int timeout = sophia_ws_timeout(session, now_ms());
  if (timeout < 0 || timeout > 100)
    timeout = 100;
  if (poll(&p, 1, timeout) < 0 && errno != EINTR)
    die("poll");
  if (sophia_ws_dispatch(session, p.revents, 65536, now_ms()))
    die("dispatch");
  switch (sophia_ws_state(session)) {
  case SOPHIA_WS_CLOSED:
  case SOPHIA_WS_STALE:
  case SOPHIA_WS_FAILED:
    die("connection");
  default:
    break;
  }
}

/* Submits one candidate and waits for its custody. */
static void submit(const struct sophia_wf_record *record, const char *what) {
  uint64_t ticket, deadline = now_ms() + 5000;
  enum sophia_ws_custody custody;
  uint32_t error;
  int status;
  while ((status = sophia_ws_submit(session, record, deadline, &ticket)) ==
         SOPHIA_9P_BUSY)
    step();
  if (status)
    die(what);
  for (;;) {
    if (sophia_ws_outcome(session, ticket, &custody, &error) || error)
      die(what);
    if (custody == SOPHIA_WS_SUBMITTED)
      return;
    if (custody != SOPHIA_WS_ADMITTED_LOCAL && custody != SOPHIA_WS_ISSUED)
      die(what);
    step();
  }
}

static uint64_t next_transaction(void) {
  uint64_t transaction;
  if (sophia_ws_next_transaction(session, &transaction))
    die("transaction");
  return transaction;
}

static void profile(const struct sophia_wf_record *event, uint16_t completion) {
  struct sophia_wf_record reply;
  memset(&reply, 0, sizeof reply);
  reply.header.kind = completion;
  reply.value.profile = event->value.profile;
  reply.value.profile.outcome = 1;
  submit(&reply, "profile");
}

static void configure(void) {
  static uint8_t action_row[SOPHIA_WF_SNAPSHOT_ACTION_BYTES];
  struct sophia_wf_record r;
  memset(&r, 0, sizeof r);
  r.header.kind = SOPHIA_WF_CONFIGURATION;
  r.value.configuration.transaction = next_transaction();
  r.value.configuration.generation = profile_generation;
  if (hold_shift) {
    struct sophia_wf_snapshot_action action;
    memset(&action, 0, sizeof action);
    action.action = HOLD_SHIFT_ACTION;
    action.name_len = sizeof("hold-shift") - 1;
    memcpy(action.name, "hold-shift", action.name_len);
    if (sophia_wf_snapshot_action_encode(action_row, sizeof action_row, &action))
      die("hold-shift action row");
    r.section_count = 1;
    r.sections[0] = (struct sophia_wf_section){3, 1, action_row, sizeof action_row};
  }
  submit(&r, "configuration");
}

static const struct sophia_wf_section *section(const struct sophia_wf_record *r,
                                               uint16_t kind) {
  for (uint16_t i = 0; i < r->section_count; ++i)
    if (r->sections[i].kind == kind)
      return &r->sections[i];
  return NULL;
}

/* Answers the head Cycle with a complete projection over its outputs. */
static void cycle(const struct sophia_wf_record *event) {
  static uint8_t output_rows[SOPHIA_WF_MAX_OUTPUTS * SOPHIA_WF_PROJECTION_OUTPUT_BYTES];
  static uint8_t placement_rows[MAX_PLACEMENTS * SOPHIA_WF_PROJECTION_PLACEMENT_BYTES];
  struct sophia_wf_cycle c = event->value.cycle;
  const struct sophia_wf_record *snap;
  const struct sophia_wf_section *outputs, *surfaces;
  struct sophia_wf_record r;
  uint32_t placed = 0;
  int status;
  int shifted = hold_shift && c.cause == SOPHIA_WF_ACTION &&
                c.value.action.action == HOLD_SHIFT_ACTION;
  if (shifted)
    hold_offset += HOLD_SHIFT_PIXELS;
  if (sophia_ws_snapshot(session, now_ms() + 5000))
    die("snapshot");
  if (sophia_ws_consume(session))
    die("consume cycle");
  while ((status = sophia_ws_snapshot_result(session, &snap)) == SOPHIA_9P_AGAIN)
    step();
  if (status)
    die("snapshot result");
  outputs = section(snap, 1);
  surfaces = section(snap, 2);
  if (!outputs)
    die("snapshot outputs");
  for (uint16_t o = 0; o < c.output_count; ++o) {
    struct sophia_wf_snapshot_output output;
    struct sophia_wf_projection_output row = {c.outputs[o], 0, 0, 0};
    int found = 0;
    for (uint32_t i = 0; i < outputs->count && !found; ++i) {
      if (sophia_wf_snapshot_output_decode(
              outputs->rows + (size_t)i * SOPHIA_WF_SNAPSHOT_OUTPUT_BYTES,
              SOPHIA_WF_SNAPSHOT_OUTPUT_BYTES, &output))
        die("snapshot output row");
      found = output.output == c.outputs[o];
    }
    if (!found)
      die("cycle output missing from snapshot");
    for (uint32_t i = 0; surfaces && i < surfaces->count; ++i) {
      struct sophia_wf_snapshot_surface s;
      struct sophia_wf_projection_placement p;
      if (sophia_wf_snapshot_surface_decode(
              surfaces->rows + (size_t)i * SOPHIA_WF_SNAPSHOT_SURFACE_BYTES,
              SOPHIA_WF_SNAPSHOT_SURFACE_BYTES, &s))
        die("snapshot surface row");
      if (!(s.current_output == output.output ||
            (s.current_output == 0 && output.output == snap->value.snapshot.active_output)))
        continue;
      if (placed == MAX_PLACEMENTS)
        die("placement capacity");
      memset(&p, 0, sizeof p);
      p.surface_index = s.surface_index;
      p.surface_generation = s.surface_generation;
      p.state_generation = s.state_generation;
      p.x = output.work_x;
      p.y = output.work_y;
      p.width = s.width > 0 ? s.width : output.work_width;
      p.height = s.height > 0 ? s.height : output.work_height;
      if (p.width > output.work_width)
        p.width = output.work_width;
      if (p.height > output.work_height)
        p.height = output.work_height;
      /* The shift never moves a placement out of its work area. */
      if (hold_offset && (int64_t)p.width + hold_offset <= (int64_t)output.work_width)
        p.x += hold_offset;
      p.requested_width = p.width;
      p.requested_height = p.height;
      p.transform = 1; /* identity */
      if (sophia_wf_projection_placement_encode(
              placement_rows + (size_t)placed * SOPHIA_WF_PROJECTION_PLACEMENT_BYTES,
              SOPHIA_WF_PROJECTION_PLACEMENT_BYTES, &p))
        die("placement row");
      ++placed;
      ++row.placement_count;
      if (output.output == snap->value.snapshot.active_output) {
        row.focus_index = s.surface_index; /* the newest, last in order */
        row.focus_generation = s.surface_generation;
      }
    }
    if (sophia_wf_projection_output_encode(
            output_rows + (size_t)o * SOPHIA_WF_PROJECTION_OUTPUT_BYTES,
            SOPHIA_WF_PROJECTION_OUTPUT_BYTES, &row))
      die("projection output row");
  }
  memset(&r, 0, sizeof r);
  r.header.kind = SOPHIA_WF_PROJECTION;
  r.value.projection.transaction = next_transaction();
  r.value.projection.request_id = c.request_id;
  r.value.projection.base_generation = c.scene_generation;
  r.value.projection.active_output = snap->value.snapshot.active_output;
  r.section_count = placed ? 2 : 1;
  r.sections[0] = (struct sophia_wf_section){
      1, c.output_count, output_rows,
      (size_t)c.output_count * SOPHIA_WF_PROJECTION_OUTPUT_BYTES};
  if (placed)
    r.sections[1] = (struct sophia_wf_section){
        2, placed, placement_rows, (size_t)placed * SOPHIA_WF_PROJECTION_PLACEMENT_BYTES};
  if (hold_shift)
    fprintf(stderr,
            "sophia_qemu_wm_hold schema=1 status=proposed transaction=%llu "
            "request_id=%llu cause=%u activation_serial=%llu shift=%d offset_x=%d "
            "placements=%u\n",
            (unsigned long long)r.value.projection.transaction,
            (unsigned long long)c.request_id, (unsigned)c.cause,
            (unsigned long long)(c.cause == SOPHIA_WF_ACTION ? c.value.action.serial : 0),
            shifted, (int)hold_offset, (unsigned)placed);
  submit(&r, "projection");
  if (sophia_ws_snapshot_release(session))
    die("snapshot release");
  ++projections;
}

static void report(const char *status) {
  fprintf(stderr,
          "sophia_qemu_wm schema=1 status=%s generation=%llu cycles=%llu "
          "projections=%llu rejected=%llu\n",
          status, (unsigned long long)profile_generation,
          (unsigned long long)cycles, (unsigned long long)projections,
          (unsigned long long)rejected);
}

int main(int argc, char **argv) {
  const char *path = getenv("SOPHIA_WM_9P_SOCKET");
  struct sockaddr_un address;
  struct sophia_ws_config config;
  void *storage;
  size_t bytes;
  int fd;
  if (argc > 2 || (argc == 2 && strcmp(argv[1], "--hold-shift")))
    die("usage: sophia-qemu-generic-wm [--hold-shift]");
  hold_shift = argc == 2;
  if (!path || strlen(path) >= sizeof address.sun_path)
    die("socket path");
  memset(&address, 0, sizeof address);
  address.sun_family = AF_UNIX;
  strcpy(address.sun_path, path);
  fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
  if (fd < 0 || connect(fd, (struct sockaddr *)&address, sizeof address) ||
      fcntl(fd, F_SETFL, O_NONBLOCK))
    die("connect");
  memset(&config, 0, sizeof config);
  config.msize = 65536;
  config.offer.required = SOPHIA_WF_CAP_PROFILE_ACTIVATION |
                          SOPHIA_WF_CAP_CONFIGURATION | SOPHIA_WF_CAP_MULTI_OUTPUT;
  if (hold_shift)
    config.offer.required |= SOPHIA_WF_CAP_ACTIONS;
  config.offer.optional = SOPHIA_WF_CAP_CHROME | SOPHIA_WF_CAP_POLICY_DIRTY;
  config.bootstrap_deadline_ms = now_ms() + 20000;
  bytes = sophia_ws_storage_bytes(config.msize);
  session = malloc(sophia_ws_state_bytes());
  storage = malloc(bytes);
  if (!session || !storage ||
      sophia_ws_open_fd(session, fd, &config, storage, bytes, now_ms()))
    die("open");
  for (;;) {
    const struct sophia_wf_record *e;
    uint64_t now;
    step();
    while (!sophia_ws_event(session, &e)) {
      switch (e->header.kind) {
      case SOPHIA_WF_PROFILE_PREPARE:
        profile(e, SOPHIA_WF_PROFILE_PREPARED);
        break;
      case SOPHIA_WF_PROFILE_ACTIVATE:
        profile_generation = e->value.profile.generation;
        profile(e, SOPHIA_WF_PROFILE_ACTIVE);
        if (sophia_ws_consume(session))
          die("consume");
        report("active");
        configure();
        continue;
      case SOPHIA_WF_PROFILE_ROLLBACK:
        profile(e, SOPHIA_WF_PROFILE_ROLLED_BACK);
        break;
      case SOPHIA_WF_CONFIGURATION_OUTCOME:
        if (e->value.configuration_outcome.outcome != 1)
          die("configuration rejected");
        report("configured");
        break;
      case SOPHIA_WF_CYCLE:
        ++cycles;
        cycle(e); /* consumes the Cycle itself */
        continue;
      case SOPHIA_WF_PROJECTION_OUTCOME:
        if (e->value.projection_outcome.expect_session_operation)
          die("session operation requested");
        if (hold_shift)
          fprintf(stderr,
                  "sophia_qemu_wm_hold schema=1 status=outcome transaction=%llu "
                  "request_id=%llu scene_generation=%llu outcome=%u\n",
                  (unsigned long long)e->value.projection_outcome.transaction,
                  (unsigned long long)e->value.projection_outcome.request_id,
                  (unsigned long long)e->value.projection_outcome.scene_generation,
                  (unsigned)e->value.projection_outcome.outcome);
        if (e->value.projection_outcome.outcome != 1)
          ++rejected;
        if (projections == 1)
          report("projecting");
        break;
      default:
        break; /* receipts and outcomes this policy has no use for */
      }
      if (sophia_ws_consume(session))
        die("consume");
    }
    now = now_ms();
    if (now - reported >= 1000 && profile_generation) {
      reported = now;
      report("serving");
    }
  }
}

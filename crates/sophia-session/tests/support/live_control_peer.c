/* Independent C SDK fixture for Session control settlement and WM replacement.
 * One empty output, one action, exact profile echo. No product policy, rendering
 * or physical acceptance: the Rust owner test deliberately withholds commit.
 */
#define _POSIX_C_SOURCE 200809L
#include "sophia_wm_session.h"
#include <assert.h>
#include <errno.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

static struct sophia_ws *session;
static uint64_t deadline;
static int recovery;

static uint64_t now_ms(void) {
  struct timespec t;
  assert(!clock_gettime(CLOCK_MONOTONIC, &t));
  return (uint64_t)t.tv_sec * 1000 + (uint64_t)t.tv_nsec / 1000000;
}

static void step(void) {
  struct pollfd p = {sophia_ws_poll_fd(session), sophia_ws_poll_events(session), 0};
  int timeout = sophia_ws_timeout(session, now_ms()), status;
  assert(now_ms() < deadline);
  if (timeout < 0 || timeout > 10)
    timeout = 10;
  status = poll(&p, 1, timeout);
  assert(status >= 0 || errno == EINTR);
  status = sophia_ws_dispatch(session, p.revents, 65536, now_ms());
  /* A controlled restart fences the old connection before its child stops. */
  if (sophia_ws_state(session) == SOPHIA_WS_CLOSED ||
      sophia_ws_state(session) == SOPHIA_WS_STALE) {
    assert(!recovery); /* A recovery proof must finish on its original epoch. */
    exit(0);
  }
  if (status)
    fprintf(stderr, "control WM SDK dispatch=%d remote=%u state=%d\n", status,
            sophia_ws_remote_error(session), (int)sophia_ws_state(session));
  assert(!status);
}

static const struct sophia_wf_record *event(uint16_t kind) {
  const struct sophia_wf_record *r;
  int status;
  while ((status = sophia_ws_event(session, &r)) == SOPHIA_9P_AGAIN)
    step();
  assert(!status && r->header.epoch == sophia_ws_epoch(session));
  if (kind)
    assert(r->header.kind == kind);
  return r;
}

static void submit(const struct sophia_wf_record *r) {
  uint64_t ticket;
  uint32_t error;
  enum sophia_ws_custody custody;
  int status;
  while ((status = sophia_ws_submit(session, r, deadline, &ticket)) == SOPHIA_9P_BUSY)
    step();
  assert(!status);
  do {
    assert(!sophia_ws_outcome(session, ticket, &custody, &error) && !error);
    if (custody == SOPHIA_WS_SUBMITTED)
      return;
    assert(custody == SOPHIA_WS_ADMITTED_LOCAL || custody == SOPHIA_WS_ISSUED);
    step();
  } while (1);
}

static void startup(void) {
  const struct sophia_wf_record *r = event(SOPHIA_WF_PROFILE_PREPARE);
  struct sophia_wf_profile identity = r->value.profile;
  struct sophia_wf_record reply = {0};
  struct sophia_wf_snapshot_action action = {0};
  uint8_t row[SOPHIA_WF_SNAPSHOT_ACTION_BYTES];
  uint64_t transaction;
  reply.header.kind = SOPHIA_WF_PROFILE_PREPARED;
  reply.value.profile = identity;
  reply.value.profile.outcome = 1;
  submit(&reply);
  assert(!sophia_ws_consume(session));
  r = event(SOPHIA_WF_PROFILE_ACTIVATE);
  assert(r->value.profile.generation == identity.generation &&
         !memcmp(r->value.profile.digest, identity.digest, sizeof(identity.digest)));
  reply.header.kind = SOPHIA_WF_PROFILE_ACTIVE;
  reply.value.profile = r->value.profile;
  reply.value.profile.outcome = 1;
  submit(&reply);
  assert(!sophia_ws_consume(session));
  memset(&reply, 0, sizeof(reply));
  assert(!sophia_ws_next_transaction(session, &transaction));
  reply.header.kind = SOPHIA_WF_CONFIGURATION;
  reply.value.configuration.transaction = transaction;
  reply.value.configuration.generation = 1;
  action.action = 1;
  action.name_len = sizeof("focus-next") - 1;
  memcpy(action.name, "focus-next", action.name_len);
  assert(!sophia_wf_snapshot_action_encode(row, sizeof(row), &action));
  reply.section_count = 1;
  reply.sections[0] = (struct sophia_wf_section){3, 1, row, sizeof(row)};
  submit(&reply);
  r = event(SOPHIA_WF_CONFIGURATION_OUTCOME);
  assert(r->value.configuration_outcome.transaction == transaction &&
         r->value.configuration_outcome.generation == 1 &&
         r->value.configuration_outcome.outcome == 1);
  assert(!sophia_ws_consume(session));
}

static void cycle(uint16_t expected_outcome) {
  static uint64_t previous_request, previous_snapshot;
  const struct sophia_wf_record *r = event(SOPHIA_WF_CYCLE);
  struct sophia_wf_cycle request = r->value.cycle;
  struct sophia_wf_record reply = {0};
  struct sophia_wf_snapshot_output snapshot_output;
  struct sophia_wf_projection_output output = {0};
  uint8_t row[SOPHIA_WF_PROJECTION_OUTPUT_BYTES];
  uint64_t transaction;
  size_t i;
  int status;
  assert(request.output_count == 1 && request.policy_generation == 1);
  assert(request.request_id > previous_request &&
         request.snapshot_transaction > previous_snapshot);
  previous_request = request.request_id;
  previous_snapshot = request.snapshot_transaction;
  if (request.cause == SOPHIA_WF_ACTION)
    assert(request.value.action.serial && request.value.action.action == 1);
  assert(!sophia_ws_snapshot(session, deadline));
  assert(!sophia_ws_consume(session));
  while ((status = sophia_ws_snapshot_result(session, &r)) == SOPHIA_9P_AGAIN)
    step();
  assert(!status && r->value.snapshot.transaction == request.snapshot_transaction &&
         r->value.snapshot.scene_generation == request.scene_generation &&
         r->section_count && r->sections[0].kind == 1 && r->sections[0].count == 1);
  assert(!sophia_wf_snapshot_output_decode(r->sections[0].rows,
                                         r->sections[0].bytes, &snapshot_output));
  assert(snapshot_output.output == request.outputs[0]);
  /* The parent supplies an empty scene; adding surfaces needs a policy peer. */
  for (i = 0; i < r->section_count; ++i)
    assert(r->sections[i].kind != 2);
  assert(!sophia_ws_next_transaction(session, &transaction));
  reply.header.kind = SOPHIA_WF_PROJECTION;
  reply.value.projection = (struct sophia_wf_projection){
      transaction, request.request_id, request.scene_generation, r->value.snapshot.active_output};
  output.output = snapshot_output.output;
  assert(!sophia_wf_projection_output_encode(row, sizeof(row), &output));
  reply.section_count = 1;
  reply.sections[0] = (struct sophia_wf_section){1, 1, row, sizeof(row)};
  assert(!sophia_ws_snapshot_release(session));
  submit(&reply);
  r = event(SOPHIA_WF_PROJECTION_OUTCOME);
  assert(r->value.projection_outcome.transaction == transaction &&
         r->value.projection_outcome.request_id == request.request_id &&
         (expected_outcome == 2
              ? r->value.projection_outcome.scene_generation > request.scene_generation
              : r->value.projection_outcome.scene_generation == request.scene_generation) &&
         r->value.projection_outcome.outcome == expected_outcome &&
         !r->value.projection_outcome.expect_session_operation);
  assert(!sophia_ws_consume(session));
}

int main(int argc, char **argv) {
  const char *socket_path = getenv("SOPHIA_WM_9P_SOCKET");
  struct sockaddr_un address = {0};
  uint64_t caps = SOPHIA_WF_CAP_CONFIGURATION | SOPHIA_WF_CAP_ACTIONS |
                  SOPHIA_WF_CAP_PROFILE_ACTIVATION | SOPHIA_WF_CAP_POINTER_FOCUS;
  struct sophia_ws_config config = {4096, {caps, 0}, 0};
  size_t bytes = sophia_ws_storage_bytes(config.msize);
  void *storage = malloc(bytes);
  int fd;
  assert(argc == 1 || (argc == 2 && !strcmp(argv[1], "--recovery")));
  recovery = argc == 2;
  assert(socket_path && !getenv("SOPHIA_WM_SOCKET") && !getenv("SOPHIA_CONTROL_SOCKET"));
  assert(strlen(socket_path) < sizeof(address.sun_path));
  address.sun_family = AF_UNIX;
  memcpy(address.sun_path, socket_path, strlen(socket_path) + 1);
  fd = socket(AF_UNIX, SOCK_STREAM, 0);
  assert(fd >= 0 && !connect(fd, (struct sockaddr *)&address, sizeof(address)));
  deadline = now_ms() + 60000;
  config.bootstrap_deadline_ms = deadline;
  session = malloc(sophia_ws_state_bytes());
  assert(session && storage &&
         !sophia_ws_open_fd(session, fd, &config, storage, bytes, now_ms()));
  startup();
  if (recovery) {
    /* Native outcome values: stale, timed out, then committed. The peer waits
     * for a fresh owner cycle instead of replaying either rejected proposal. */
    cycle(2);
    cycle(4);
    cycle(1);
    return 0;
  }
  for (;;)
    cycle(1);
}

/* Generic WM file conformance peer. The Rust fixture supplies admission and
 * policy outcomes. This exercises the public C SDK, not a WM implementation,
 * Engine policy acceptance, supervisor authentication or physical presentation.
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
static uint64_t now_ms(void) {
  struct timespec t;
  assert(!clock_gettime(CLOCK_MONOTONIC, &t));
  return (uint64_t)t.tv_sec * 1000 + (uint64_t)t.tv_nsec / 1000000;
}
static void step(void) {
  struct pollfd p = {sophia_ws_poll_fd(session), sophia_ws_poll_events(session),
                     0};
  int timeout = sophia_ws_timeout(session, now_ms()), status;
  assert(now_ms() < deadline);
  if (timeout < 0 || timeout > 10)
    timeout = 10;
  status = poll(&p, 1, timeout);
  assert(status >= 0 || errno == EINTR);
  status = sophia_ws_dispatch(session, p.revents, 65536, now_ms());
  if (status)
    fprintf(stderr, "WM SDK dispatch=%d remote=%u state=%d\n", status,
            sophia_ws_remote_error(session), (int)sophia_ws_state(session));
  assert(!status);
}
static const struct sophia_wf_record *event(uint16_t kind) {
  const struct sophia_wf_record *r;
  int status;
  while ((status = sophia_ws_event(session, &r)) == SOPHIA_9P_AGAIN)
    step();
  assert(!status && r->header.kind == kind && r->header.epoch == 9);
  return r;
}
static void submit(const struct sophia_wf_record *record) {
  uint64_t ticket;
  uint32_t error;
  enum sophia_ws_custody custody;
  int status;
  while ((status = sophia_ws_submit(session, record, deadline, &ticket)) ==
         SOPHIA_9P_BUSY)
    step();
  assert(!status);
  for (;;) {
    assert(!sophia_ws_outcome(session, ticket, &custody, &error) && !error);
    if (custody == SOPHIA_WS_SUBMITTED)
      return;
    assert(custody == SOPHIA_WS_ADMITTED_LOCAL || custody == SOPHIA_WS_ISSUED);
    step();
  }
}
static void profile(uint16_t command, uint16_t completion,
                    uint64_t transaction) {
  const struct sophia_wf_record *r = event(command);
  struct sophia_wf_record reply = {0};
  uint8_t digest[32];
  memset(digest, 7, sizeof(digest));
  assert(r->value.profile.transaction == transaction &&
         r->value.profile.generation == 3 &&
         !memcmp(r->value.profile.digest, digest, sizeof(digest)));
  reply.header.kind = completion;
  reply.value.profile = r->value.profile;
  reply.value.profile.outcome = 1;
  submit(&reply);
  assert(!sophia_ws_consume(session));
}
static void configuration(void) {
  struct sophia_wf_record r = {0};
  const struct sophia_wf_record *result;
  r.header.kind = SOPHIA_WF_CONFIGURATION;
  r.value.configuration.transaction = 10;
  r.value.configuration.generation = 3;
  submit(&r);
  result = event(SOPHIA_WF_CONFIGURATION_OUTCOME);
  assert(result->value.configuration_outcome.transaction == 10 &&
         result->value.configuration_outcome.generation == 3 &&
         result->value.configuration_outcome.outcome == 1);
  assert(!sophia_ws_consume(session));
  memset(&r, 0, sizeof(r));
  r.header.kind = SOPHIA_WF_DIRTY;
  r.value.dirty.generation = 3;
  r.value.dirty.output_count = 1;
  r.value.dirty.outputs[0] = 1;
  submit(&r);
}
static void snapshot(void) {
  const struct sophia_wf_record *r = event(SOPHIA_WF_CYCLE);
  struct sophia_wf_snapshot_output output;
  struct sophia_wf_snapshot_surface surface;
  size_t i;
  int status;
  assert(r->value.cycle.snapshot_transaction == 100 &&
         r->value.cycle.request_transaction == 101 &&
         r->value.cycle.request_id == 55 &&
         r->value.cycle.scene_generation == 7 &&
         r->value.cycle.policy_generation == 3 &&
         r->value.cycle.cause == SOPHIA_WF_SCENE_CHANGED &&
         r->value.cycle.output_count == 1 && r->value.cycle.outputs[0] == 1);
  assert(!sophia_ws_snapshot(session, deadline));
  assert(!sophia_ws_consume(session));
  while ((status = sophia_ws_snapshot_result(session, &r)) == SOPHIA_9P_AGAIN)
    step();
  assert(!status && r->header.epoch == 9 &&
         r->value.snapshot.transaction == 100 &&
         r->value.snapshot.scene_generation == 7 &&
         r->value.snapshot.active_output == 1 && r->section_count >= 2);
  assert(r->sections[0].kind == 1 && r->sections[0].count == 1 &&
         !sophia_wf_snapshot_output_decode(r->sections[0].rows,
                                           r->sections[0].bytes, &output));
  assert(output.output == 1 && output.generation == 3 && output.width == 100 &&
         output.height == 100 && output.work_width == 100 &&
         output.work_height == 100);
  assert(r->sections[1].kind == 2 && r->sections[1].count == 64 &&
         r->sections[1].bytes == 64 * SOPHIA_WF_SNAPSHOT_SURFACE_BYTES);
  for (i = 0; i < 64; ++i) {
    assert(!sophia_wf_snapshot_surface_decode(
        r->sections[1].rows + i * SOPHIA_WF_SNAPSHOT_SURFACE_BYTES,
        SOPHIA_WF_SNAPSHOT_SURFACE_BYTES, &surface));
    assert(surface.surface_index == 3 + i && surface.surface_generation == 1 &&
           surface.state_generation == 8 && surface.current_output == 1 &&
           surface.width == 100 && surface.height == 100);
  }
  assert(!sophia_ws_snapshot_release(session));
}
static void projection_and_operation(void) {
  struct sophia_wf_record r = {0};
  struct sophia_wf_projection_output output = {1, 1, 3, 1};
  struct sophia_wf_projection_placement placement = {0};
  uint8_t outputs[SOPHIA_WF_PROJECTION_OUTPUT_BYTES];
  uint8_t placements[SOPHIA_WF_PROJECTION_PLACEMENT_BYTES];
  const struct sophia_wf_record *result;
  placement.surface_index = 3;
  placement.surface_generation = 1;
  placement.state_generation = 8;
  placement.width = placement.height = 100;
  placement.transform = 1; /* Identity in the shared fixed-row contract. */
  assert(
      !sophia_wf_projection_output_encode(outputs, sizeof(outputs), &output));
  assert(!sophia_wf_projection_placement_encode(placements, sizeof(placements),
                                                &placement));
  r.header.kind = SOPHIA_WF_PROJECTION;
  r.value.projection = (struct sophia_wf_projection){11, 55, 7, 1};
  r.section_count = 2;
  r.sections[0] = (struct sophia_wf_section){1, 1, outputs, sizeof(outputs)};
  r.sections[1] =
      (struct sophia_wf_section){2, 1, placements, sizeof(placements)};
  submit(&r);
  result = event(SOPHIA_WF_PROJECTION_OUTCOME);
  assert(result->value.projection_outcome.transaction == 11 &&
         result->value.projection_outcome.request_id == 55 &&
         result->value.projection_outcome.scene_generation == 7 &&
         result->value.projection_outcome.outcome == 1 &&
         result->value.projection_outcome.expect_session_operation == 1);
  assert(!sophia_ws_consume(session));
  memset(&r, 0, sizeof(r));
  r.header.kind = SOPHIA_WF_SESSION_OPERATION;
  r.value.session_operation.transaction = 12;
  r.value.session_operation.request_id = 55;
  r.value.session_operation.operation = 11;
  submit(&r);
  result = event(SOPHIA_WF_SESSION_OPERATION_OUTCOME);
  assert(result->value.session_operation_outcome.transaction == 12 &&
         result->value.session_operation_outcome.request_id == 55 &&
         result->value.session_operation_outcome.outcome == 1);
  assert(!sophia_ws_consume(session));
  result = event(SOPHIA_WF_PRESENTATION_RECEIPT);
  assert(result->value.presentation_receipt.transaction == 102 &&
         result->value.presentation_receipt.publication_generation == 1 &&
         result->value.presentation_receipt.output == 1 &&
         result->value.presentation_receipt.output_generation == 3 &&
         result->value.presentation_receipt.presentation_epoch == 1 &&
         result->value.presentation_receipt.outcome == 1);
  assert(!sophia_ws_consume(session));
}
int main(int argc, char **argv) {
  struct sockaddr_un address = {0};
  struct sophia_ws_config config = {4096, {(UINT64_C(1) << 20) - 1, 0}, 0};
  struct sophia_ws_obligations obligations;
  void *storage;
  size_t bytes = sophia_ws_storage_bytes(config.msize);
  int fd;
  assert(argc == 2 && strlen(argv[1]) < sizeof(address.sun_path));
  address.sun_family = AF_UNIX;
  memcpy(address.sun_path, argv[1], strlen(argv[1]) + 1);
  fd = socket(AF_UNIX, SOCK_STREAM, 0);
  assert(fd >= 0 && !connect(fd, (struct sockaddr *)&address, sizeof(address)));
  deadline = now_ms() + 20000;
  config.bootstrap_deadline_ms = deadline;
  session = malloc(sophia_ws_state_bytes());
  storage = malloc(bytes);
  assert(session && storage &&
         !sophia_ws_open_fd(session, fd, &config, storage, bytes, now_ms()));
  profile(SOPHIA_WF_PROFILE_PREPARE, SOPHIA_WF_PROFILE_PREPARED, 40);
  profile(SOPHIA_WF_PROFILE_ACTIVATE, SOPHIA_WF_PROFILE_ACTIVE, 41);
  configuration();
  snapshot();
  projection_and_operation();
  do {
    step();
    assert(!sophia_ws_obligations(session, &obligations));
  } while (obligations.acked != obligations.consumed ||
           obligations.deadline_ms);
  sophia_ws_close(session);
  close(fd);
  free(storage);
  free(session);
  puts("sophia_c_wm_sdk status=pass profiles=2 snapshots=1 surfaces=64 "
       "projections=1 operations=1 receipts=1");
  return 0;
}

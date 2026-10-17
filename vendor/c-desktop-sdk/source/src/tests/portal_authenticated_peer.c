#define _POSIX_C_SOURCE 200809L
#include "sophia_launch_auth.h"
#include "sophia_namespace.h"
#include "sophia_portal_frame.h"
#include "sophia_portal_request.h"
#include "sophia_portal_session.h"
#include "sophia_portal_startup.h"
#include "sophia_portal_transfer.h"
#include "sophia_portal_watch.h"
#include <assert.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
static uint64_t now(void) {
  struct timespec t;
  assert(!clock_gettime(CLOCK_MONOTONIC, &t));
  return (uint64_t)t.tv_sec * 1000 + (uint64_t)t.tv_nsec / 1000000;
}
int main(void) {
  struct sophia_ns_discovery *names = malloc(sizeof *names);
  struct sophia_9p_client role, relay;
  struct sophia_la_client auth;
  struct sophia_po opening;
  size_t bytes = sophia_9p_storage_bytes(4096, 8);
  void *a = malloc(bytes), *b = malloc(bytes);
  int dir, fd;
  uint64_t deadline = now() + 10000;
  FILE *report;
  const char *mode = getenv("SOPHIA_TEST_PORTAL_BOUNDARY");
  int request_mode = !strncmp(mode, "sdk-request", 11);
  int busy = !strcmp(mode, "sdk-request-busy");
  int stale = !strcmp(mode, "sdk-request-stale");
  int refused = !strcmp(mode, "sdk-request-refused") || busy || stale;
  uint32_t refusal_errno = busy ? 11 : stale ? 116 : 13;
  int cancelled = !strncmp(mode, "sdk-request-cancelled", 21);
  int transfer_mode = !strncmp(mode, "sdk-transfer", 12);
  int watch_mode = !strncmp(mode, "sdk-watch", 9);
  int session_mode = !strncmp(mode, "sdk-session", 11);
  int session_capture = !strcmp(mode, "sdk-session-capture");
  int session_frame = !strcmp(mode, "sdk-session-frame");
  int frame_mode = !strncmp(mode, "sdk-frame", 9) || request_mode ||
                   session_capture || session_frame;
  int revoke = !strcmp(mode, "sdk-frame-revoke"), paused = 0;
  assert(names && a && b);
  dir = sophia_ns_open(getenv(SOPHIA_NS_DIRECTORY_ENV), names);
  assert(dir >= 0);
  fd = sophia_ns_connect(dir, names, "roles/portal");
  assert(fd >= 0);
  assert(!sophia_9p_init(&role, fd, 4096, 8, 16, a, bytes));
  assert(!sophia_9p_init(&relay, STDIN_FILENO, 4096, 8, 16, b, bytes));
  assert(!sophia_la_init(&auth, &role, &relay, "namespace-client",
                         "portal-client", now()));
  while (!sophia_la_ready(&auth)) {
    struct pollfd p[2] = {
        {fd, POLLIN | (sophia_9p_wants_write(&role) ? POLLOUT : 0), 0},
        {STDIN_FILENO, POLLIN | (sophia_9p_wants_write(&relay) ? POLLOUT : 0),
         0}};
    assert(now() < deadline && poll(p, 2, 10) >= 0);
    assert(!sophia_la_service(&auth, now(), 65536));
  }
  assert(!sophia_po_init(&opening, &role, sophia_la_root(&auth), now(),
                         now() + 2000));
  while (!sophia_po_ready(&opening)) {
    struct pollfd p = {
        fd, POLLIN | (sophia_po_wants_write(&opening) ? POLLOUT : 0), 0};
    uint64_t t = now();
    assert(t < deadline &&
           poll(&p, 1,
                (int)(opening.deadline > t ? opening.deadline - t : 0)) >= 0);
    assert(!sophia_po_dispatch(&opening, now(), 65536));
  }
  assert(opening.info.epoch &&
         opening.info.request == (frame_mode ? SOPHIA_PF_OUTPUT_CAPTURE_V1
                                             : SOPHIA_PF_REQUEST_UNAVAILABLE) &&
         role.fid_count == 1);
  if (request_mode) {
    struct sophia_pcr request;
    struct sophia_pf_capture_request capture = {1, 7};
    if (busy)
      capture.serial = 2;
    enum sophia_pf_capture_state outcome;
    uint64_t serial;
    assert(
        !sophia_pcr_init(&request, &opening, capture, 0, now(), now() + 3000));
    while (!request.error && !sophia_pcr_result(&request, &outcome)) {
      struct pollfd p = {
          fd, POLLIN | (sophia_pcr_wants_write(&request) ? POLLOUT : 0), 0};
      uint64_t t = now(), wake;
      assert(sophia_pcr_next_wake(&request, &wake));
      assert(t < deadline && poll(&p, 1, (int)(wake > t ? wake - t : 0)) >= 0);
      int dispatch = sophia_pcr_dispatch(&request, now(), 65536);
      if (!refused)
        assert(!dispatch);
    }
    if (refused) {
      assert(request.error == SOPHIA_PCR_CAPTURE &&
             request.capture.custody == SOPHIA_PC_REFUSED &&
             request.capture.remote_error == refusal_errno &&
             !sophia_pcr_result(&request, &outcome));
      goto report_result;
    }
    assert(!sophia_pcr_finish(&request, &outcome, &serial));
    assert(outcome == (cancelled ? SOPHIA_PF_CANCELLED : SOPHIA_PF_READY) &&
           serial == 1 && role.fid_count == 1 && sophia_po_ready(&opening));
    if (cancelled)
      goto report_result;
  }
  if (transfer_mode) {
    struct sophia_pt transfer;
    int absent = !strcmp(mode, "sdk-transfer-absent");
    int revoked = !strcmp(mode, "sdk-transfer-revoke");
    unsigned char *storage = malloc(65536);
    const void *result;
    size_t length, i;
    assert(storage);
    memset(storage, 0xa5, 65536);
    assert(!sophia_pt_init(&transfer, &opening, storage, 65536, now(),
                           now() + 3000));
    while (!transfer.error && !sophia_pt_result(&transfer, &result, &length)) {
      struct pollfd p = {
          fd, POLLIN | (sophia_pt_wants_write(&transfer) ? POLLOUT : 0), 0};
      uint64_t t = now();
      int dispatch;
      assert(t < deadline &&
             poll(&p, 1,
                  (int)(transfer.deadline > t ? transfer.deadline - t : 0)) >=
                 0);
      dispatch =
          sophia_pt_dispatch(&transfer, now(), revoked && !paused ? 1 : 65536);
      if (!absent && !revoked)
        assert(!dispatch);
      if (revoked && !paused && sophia_pt_received(&transfer)) {
        assert(!sophia_pt_result(&transfer, &result, &length));
        report = fopen(getenv("SOPHIA_TEST_LOCK_REPORT"), "w");
        assert(report && fputs("transfer-partial\n", report) >= 0 &&
               !fclose(report));
        for (;;) {
          char text[32] = {0};
          struct timespec delay = {0, 1000000};
          report = fopen(getenv("SOPHIA_TEST_LOCK_REPORT"), "r");
          assert(report);
          (void)fgets(text, sizeof text, report);
          assert(!fclose(report));
          if (!strcmp(text, "continue\n"))
            break;
          assert(now() < deadline);
          nanosleep(&delay, NULL);
        }
        paused = 1;
      }
    }
    if (absent || revoked) {
      assert(transfer.error == SOPHIA_PS_REMOTE &&
             transfer.fetch.remote_error == (absent ? 2u : 116u));
      assert(!sophia_pt_result(&transfer, &result, &length) &&
             (!revoked || paused));
    } else {
      assert(!sophia_pt_finish(&transfer, &result, &length));
      assert(length == 7 && result == storage && !memcmp(result, "granted", 7));
      assert(sophia_po_ready(&opening) && role.fid_count == 1);
      for (i = 7; i < 65536; ++i)
        assert(storage[i] == 0xa5);
    }
    free(storage);
    goto report_result;
  }
  if (session_mode) {
    struct sophia_pcs session;
    size_t capacity = session_frame ? 256 * 128 * 4 : 65536;
    unsigned char status_bytes[43], *storage = malloc(capacity);
    struct sophia_pf_frame_info info;
    int frame_cancelled = 0;
    const void *result;
    size_t length, i;
    enum sophia_pcs_event event;
    enum sophia_pf_capture_state outcome;
    uint64_t serial;
    uint32_t remote;
    int phase = 0, cancel = !strcmp(mode, "sdk-session-cancel");
    uint64_t expires = now() + 3000;
    assert(storage);
    memset(storage, 0xa5, capacity);
    assert(!sophia_pcs_init(&session, &opening));
    assert(!sophia_pcs_watch(&session, expires));
    assert(!sophia_pcs_snapshot(&session, SOPHIA_PS_STATUS, status_bytes,
                                sizeof status_bytes, expires));
    for (;;) {
      struct pollfd p = {
          fd, POLLIN | (sophia_pcs_wants_write(&session) ? POLLOUT : 0), 0};
      uint64_t t = now(), wake;
      assert(sophia_pcs_deadline(&session, &wake));
      assert(t < deadline && poll(&p, 1,
                                  sophia_pcs_immediate(&session)
                                      ? 0
                                      : (int)(wake > t ? wake - t : 0)) >= 0);
      assert(!sophia_pcs_dispatch(&session, now(), 65536));
      if (!phase && sophia_pcs_snapshot_result(&session, &result, &length)) {
        const char *expected =
            session_capture || session_frame
                ? "request=output-capture-v1 transfer=ready\n"
                : "request=unavailable transfer=ready\n";
        assert(length == strlen(expected) && !memcmp(result, expected, length));
        assert(sophia_pcs_watch_waiting(&session) &&
               !sophia_pcs_watch_event(&session, &event, &remote));
        assert(!sophia_pcs_release_snapshot(&session));
        report = fopen(getenv("SOPHIA_TEST_LOCK_REPORT"), "w");
        assert(report && fputs("session-ready\n", report) >= 0 &&
               !fclose(report));
        for (;;) {
          char text[32] = {0};
          struct timespec delay = {0, 1000000};
          report = fopen(getenv("SOPHIA_TEST_LOCK_REPORT"), "r");
          assert(report);
          (void)fgets(text, sizeof text, report);
          assert(!fclose(report));
          if (!strcmp(text, "continue\n"))
            break;
          assert(now() < deadline);
          nanosleep(&delay, NULL);
        }
        if (session_frame) {
          assert(sophia_pcs_frame(&session, storage, capacity, 0) ==
                 SOPHIA_PFR_DEADLINE);
          for (i = 0; i < capacity; ++i)
            assert(storage[i] == 0xa5);
          assert(!sophia_pcs_frame(&session, storage, capacity, expires));
        } else if (session_capture) {
          struct sophia_pf_capture_request request = {2, 7};
          assert(!sophia_pcs_capture(&session, request, 1, expires));
        } else {
          assert(!sophia_pcs_snapshot(&session, SOPHIA_PS_TRANSFER, storage,
                                      65536, expires));
        }
        if (cancel)
          assert(!sophia_pcs_cancel_watch(&session));
        phase = 1;
      }
      if (session_frame && !frame_cancelled &&
          sophia_pcs_frame_received(&session)) {
        assert(!sophia_pcs_frame_result(&session, &info, &result, &length));
        assert(!sophia_pcs_cancel_watch(&session));
        frame_cancelled = 1;
      }
      if (phase && sophia_pcs_watch_event(&session, &event, &remote) &&
          (session_frame
               ? sophia_pcs_frame_result(&session, &info, &result, &length)
           : session_capture
               ? sophia_pcs_capture_result(&session, &outcome, &serial)
               : sophia_pcs_snapshot_result(&session, &result, &length)))
        break;
    }
    assert(event == (cancel || session_frame ? SOPHIA_PCS_CANCELLED
                                             : SOPHIA_PCS_REVOKED));
    if (session_frame) {
      assert(frame_cancelled && result == storage && length == capacity &&
             info.image_bytes == capacity);
      for (i = 0; i < length; ++i)
        assert(storage[i] == 17);
      assert(!sophia_pcs_release_frame(&session));
    } else if (session_capture) {
      assert(outcome == SOPHIA_PF_READY && serial == 2);
      assert(!sophia_pcs_release_capture(&session));
    } else {
      assert(result == storage && length == 7 && !memcmp(result, "granted", 7));
      for (i = length; i < 65536; ++i)
        assert(storage[i] == 0xa5);
      assert(!sophia_pcs_release_snapshot(&session));
    }
    assert(!sophia_pcs_release_watch(&session));
    assert(!sophia_pcs_finish(&session) && sophia_po_ready(&opening) &&
           role.fid_count == 1);
    free(storage);
    goto report_result;
  }
  if (watch_mode) {
    struct sophia_pw watch;
    int absent = !strcmp(mode, "sdk-watch-absent");
    int cancel = !strcmp(mode, "sdk-watch-cancel");
    int expire = !strcmp(mode, "sdk-watch-expire"), announced = 0;
    assert(!sophia_pw_init(&watch, &opening, now(), now() + 3000));
    while (!watch.error && !sophia_pw_revoked(&watch)) {
      struct pollfd p = {
          fd, POLLIN | (sophia_pw_wants_write(&watch) ? POLLOUT : 0), 0};
      uint64_t t = now();
      int dispatch;
      assert(t < deadline &&
             poll(&p, 1, (int)(watch.deadline > t ? watch.deadline - t : 0)) >=
                 0);
      dispatch = sophia_pw_dispatch(&watch, now(), 65536);
      if (!absent)
        assert(!dispatch);
      if (!announced && sophia_pw_waiting(&watch) &&
          !sophia_pw_wants_write(&watch)) {
        assert(!sophia_pw_revoked(&watch));
        report = fopen(getenv("SOPHIA_TEST_LOCK_REPORT"), "w");
        assert(report && fputs("watch-ready\n", report) >= 0 &&
               !fclose(report));
        for (;;) {
          char text[32] = {0};
          struct timespec delay = {0, 1000000};
          report = fopen(getenv("SOPHIA_TEST_LOCK_REPORT"), "r");
          assert(report);
          (void)fgets(text, sizeof text, report);
          assert(!fclose(report));
          if (!strcmp(text, "continue\n"))
            break;
          assert(now() < deadline);
          nanosleep(&delay, NULL);
        }
        announced = 1;
        if (cancel)
          sophia_pw_close(&watch);
        if (expire)
          assert(sophia_pw_dispatch(&watch, watch.deadline, 65536) ==
                 SOPHIA_PS_DEADLINE);
      }
    }
    if (absent || cancel || expire) {
      assert(watch.error == (absent   ? SOPHIA_PS_REMOTE
                             : cancel ? SOPHIA_PS_CLOSED
                                      : SOPHIA_PS_DEADLINE));
      assert(!sophia_pw_revoked(&watch) &&
             (!absent || watch.remote_error == 2));
    } else {
      assert(announced && sophia_pw_revoked(&watch));
      assert(!sophia_pw_finish(&watch) && sophia_po_ready(&opening) &&
             role.fid_count == 1);
    }
    goto report_result;
  }
  if (frame_mode) {
    struct sophia_pfr frame;
    struct sophia_pf_frame_info info;
    size_t length, i;
    const void *result;
    unsigned char *image = malloc(256 * 128 * 4);
    assert(image && !sophia_pfr_init(&frame, &opening, image, 256 * 128 * 4,
                                     now(), now() + 3000));
    while (!frame.error &&
           !sophia_pfr_result(&frame, &info, &result, &length)) {
      struct pollfd p = {
          fd, POLLIN | (sophia_pfr_wants_write(&frame) ? POLLOUT : 0), 0};
      uint64_t t = now();
      assert(t < deadline &&
             poll(&p, 1, (int)(frame.deadline > t ? frame.deadline - t : 0)) >=
                 0);
      int dispatch =
          sophia_pfr_dispatch(&frame, now(), revoke && !paused ? 1 : 65536);
      if (!revoke)
        assert(!dispatch);
      if (revoke && !paused && sophia_pfr_received(&frame)) {
        assert(sophia_pfr_received(&frame) < 256 * 128 * 4 &&
               !sophia_pfr_result(&frame, &info, &result, &length));
        report = fopen(getenv("SOPHIA_TEST_LOCK_REPORT"), "w");
        assert(report && fputs("partial\n", report) >= 0 && !fclose(report));
        for (;;) {
          char text[32] = {0};
          struct timespec delay = {0, 1000000};
          report = fopen(getenv("SOPHIA_TEST_LOCK_REPORT"), "r");
          assert(report);
          (void)fgets(text, sizeof text, report);
          assert(!fclose(report));
          if (!strcmp(text, "continue\n"))
            break;
          assert(now() < deadline);
          nanosleep(&delay, NULL);
        }
        paused = 1;
      }
    }
    if (revoke) {
      assert(paused && frame.error && frame.error != SOPHIA_PFR_DEADLINE &&
             !sophia_pfr_result(&frame, &info, &result, &length));
    } else {
      assert(result == image && length == 256 * 128 * 4 && info.output == 7 &&
             info.output_generation == 9 &&
             info.presentation_generation == 11 && info.width == 256 &&
             info.height == 128 && role.fid_count == 1);
      for (i = 0; i < length; ++i)
        assert(image[i] == 17);
    }
    sophia_pfr_close(&frame);
    free(image);
  }
report_result:
  report = fopen(getenv("SOPHIA_TEST_LOCK_REPORT"), "w");
  assert(report &&
         fputs(session_mode    ? "session-done\n"
               : watch_mode    ? "watch-done\n"
               : transfer_mode ? "transfer-done\n"
               : refused       ? "request-refused\n"
               : cancelled     ? "request-cancelled\n"
               : revoke        ? "revoked\n"
               : frame_mode    ? "frame\n"
                               : "started\n",
               report) >= 0 &&
         !fclose(report));
  sophia_po_close(&opening);
  close(fd);
  close(STDIN_FILENO);
  close(dir);
  free(a);
  free(b);
  free(names);
  return 0;
}

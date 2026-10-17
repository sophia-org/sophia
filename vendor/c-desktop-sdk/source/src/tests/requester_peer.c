#define _POSIX_C_SOURCE 200809L
#include "sophia_portal_requester_session.h"
#include <assert.h>
#include <sched.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static uint64_t now(void) {
  struct timespec t;
  assert(!clock_gettime(CLOCK_MONOTONIC, &t));
  return (uint64_t)t.tv_sec * 1000 + (uint64_t)t.tv_nsec / 1000000;
}
static void pump_budget(struct sophia_pqr *s, size_t budget) {
  uint64_t time = now();
  int e;
  assert(!sophia_pqr_dispatch(s, time, budget));
  while (sophia_pqr_event(s))
    assert(!sophia_pqr_consume(s, time));
  e = sophia_pqr_acknowledge(s, time);
  assert(!e || e == SOPHIA_PQS_BUSY);
  sched_yield();
}
static void pump(struct sophia_pqr *s) { pump_budget(s, 65536); }
static void wait_reply(struct sophia_9p_client *wire,
                       struct sophia_9p_reply *reply) {
  do {
    assert(sophia_9p_service(wire, 65536) >= 0);
    sched_yield();
  } while (sophia_9p_peek(wire, reply) == SOPHIA_9P_AGAIN);
}
int main(int argc, char **argv) {
  struct sophia_9p_client wire;
  struct sophia_9p_handle handle;
  struct sophia_9p_reply reply;
  struct sophia_pqo opening;
  struct sophia_pqr session;
  struct sophia_pqr_request_id id;
  struct sophia_pq_request request = {0};
  unsigned char payload[65536];
  uint32_t root;
  uint64_t deadline = now() + 8000;
  size_t length, storage_length = sophia_9p_storage_bytes(4096, 8);
  void *storage = malloc(storage_length);
  int e, denied, cancel, completed;
  assert(argc == 3 && storage);
  alarm(9);
  length = (size_t)strtoul(argv[2], NULL, 10);
  assert(length <= sizeof payload);
  denied = !strcmp(argv[1], "denied");
  cancel = !strcmp(argv[1], "cancel") || !strcmp(argv[1], "cancel-partial");
  completed = !strcmp(argv[1], "completed");
  assert(denied || cancel || completed || !strcmp(argv[1], "revoked") ||
         !strcmp(argv[1], "disconnect") || !strcmp(argv[1], "lost-outcome"));
  /* The owner supplies and admits stdin; this peer opens no endpoint. */
  assert(!sophia_9p_init(&wire, 0, 4096, 8, 16, storage, storage_length));
  assert(!sophia_9p_version(&wire, &handle));
  wait_reply(&wire, &reply);
  assert(reply.type == 101 && !sophia_9p_consume(&wire, reply.handle));
  assert(!sophia_9p_attach(&wire, "", "", &handle, &root));
  wait_reply(&wire, &reply);
  assert(reply.type == 105 && !sophia_9p_consume(&wire, reply.handle));
  assert(!sophia_pqo_init(&opening, &wire, root, now(), deadline));
  while (!sophia_pqo_ready(&opening)) {
    assert(!sophia_pqo_dispatch(&opening, now(), 65536));
    sched_yield();
  }
  assert(!sophia_pqr_init(&session, &opening, 1, 1, deadline));
  while (!sophia_pqr_transaction_ready(&session))
    pump(&session);
  request.transfer = 1;
  request.source = 1;
  request.target = 2;
  request.kind = SOPHIA_PQ_CLIPBOARD;
  request.mime_present = 1;
  request.mime = "UTF8_STRING";
  request.mime_length = 11;
  request.byte_size = length;
  request.generation = 9;
  request.deadline_msec = 10000;
  assert(!sophia_pqr_request(&session, &request, now(), deadline, &id));
  while (!sophia_pqr_transaction_ready(&session))
    pump(&session);
  if (denied) {
    assert(sophia_pqr_transfer(&session, id)->state == SOPHIA_PQF_DENIED);
  } else {
    assert(sophia_pqr_transfer(&session, id)->state == SOPHIA_PQF_ALLOWED);
    memset(payload, 17, length);
    {
      uint64_t time = now();
      assert(sophia_pqr_begin(&session, id, payload, length, time, time));
      assert(!session.upload_present);
    }
    assert(!sophia_pqr_begin(&session, id, payload, length, now(), deadline));
    if (!strcmp(argv[1], "cancel-partial")) {
      size_t written = 0;
      do {
        pump_budget(&session, 1);
        assert(!sophia_pqr_upload_written(&session, id, &written));
      } while (!written || !sophia_pqr_transaction_ready(&session));
      assert(written > 0 && written < length);
    } else {
      while (!sophia_pqr_upload_ready(&session, id) ||
             !sophia_pqr_transaction_ready(&session))
        pump(&session);
    }
    if (!strcmp(argv[1], "disconnect")) {
      const struct sophia_pqf *transfer;
      sophia_pqr_close(&session);
      close(0);
      transfer = sophia_pqr_transfer(&session, id);
      assert(transfer && transfer->state == SOPHIA_PQF_ALLOWED &&
             transfer->closed && !sophia_pqf_grant(transfer));
      assert(!sophia_pqr_release_upload(&session, id));
      free(storage);
      return 0;
    }
    if (cancel)
      assert(!sophia_pqr_cancel(&session, id, now(), deadline));
    else
      assert(!sophia_pqr_end(&session, id, now(), deadline));
    if (!strcmp(argv[1], "lost-outcome")) {
      const struct sophia_pqf *transfer;
      while (!sophia_pqr_dispatch(&session, now(), 65536))
        sched_yield();
      transfer = sophia_pqr_transfer(&session, id);
      assert(transfer && transfer->state == SOPHIA_PQF_END_ISSUED &&
             transfer->closed && !sophia_pqf_grant(transfer));
      close(0);
      assert(!sophia_pqr_release_upload(&session, id));
      free(storage);
      return 0;
    }
    do {
      pump(&session);
      e = sophia_pqr_release_upload(&session, id);
      assert(!e || e == SOPHIA_PQS_BUSY);
    } while (e);
    assert(sophia_pqr_transfer(&session, id)->state ==
           (completed ? SOPHIA_PQF_COMPLETED : SOPHIA_PQF_REVOKED));
  }
  do {
    pump(&session);
    e = sophia_pqr_release_transfer(&session, id, now());
    assert(!e || e == SOPHIA_PQS_BUSY);
  } while (e);
  sophia_pqr_close(&session);
  close(0);
  free(storage);
  return 0;
}

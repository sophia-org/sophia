#include "sophia_broker_session.h"
#include <assert.h>
#include <errno.h>
#include <sched.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

static uint64_t get(const unsigned char *p, size_t n) {
  uint64_t v = 0;
  size_t i;
  for (i = 0; i < n; ++i)
    v |= (uint64_t)p[i] << (i * 8);
  return v;
}
static void put(unsigned char *p, uint64_t v, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    p[i] = (unsigned char)(v >> (i * 8));
}
static int exact(int fd, void *buffer, size_t n) {
  unsigned char *p = buffer;
  size_t used = 0;
  while (used < n) {
    ssize_t r = read(fd, p + used, n - used);
    if (r < 0 && errno == EINTR)
      continue;
    if (!r || (r < 0 && errno == ECONNRESET)) {
      assert(!used);
      return 0;
    }
    assert(r > 0);
    used += (size_t)r;
  }
  return 1;
}
static void send_reply(int fd, unsigned type, unsigned tag, const void *body,
                       size_t n) {
  unsigned char out[4096];
  size_t used = 0;
  put(out, n + 7, 4);
  out[4] = (unsigned char)type;
  put(out + 5, tag, 2);
  if (n)
    memcpy(out + 7, body, n);
  while (used < n + 7) {
    ssize_t r = write(fd, out + used, n + 7 - used);
    if (r < 0 && (errno == EPIPE || errno == ECONNRESET))
      return;
    assert(r > 0);
    used += (size_t)r;
  }
}
static void record(unsigned char *p, unsigned kind, size_t size, uint64_t seq) {
  memset(p, 0, size);
  put(p, size, 4);
  p[4] = 1;
  put(p + 6, kind, 2);
  p[8] = 7;
  put(p + 24, seq, 8);
}
static void peer(int fd, unsigned fault) {
  unsigned target = fault ? 1 : 2;
  unsigned char h[7], b[4096], out[4096], journal[1024], staged[448];
  unsigned names[64] = {0}, pending = 0, pending_tag = 0, requests = 0,
           responses = 0, clunks = 0, submit_walks = 0;
  size_t used = 0, staged_n = 0, offset = 0, count = 0;
  unsigned retry = 0;
  uint64_t acknowledged = 0;
  unsigned char retry_candidate[448], retry_control[24];
  while (exact(fd, h, 7)) {
    size_t n = (size_t)get(h, 4) - 7;
    unsigned tag = (unsigned)get(h + 5, 2), fid;
    assert(n <= sizeof b && exact(fd, b, n));
    memset(out, 0, sizeof out);
    switch (h[4]) {
    case 100:
      send_reply(fd, 101, tag, b, n);
      break;
    case 104:
      out[0] = 128;
      send_reply(fd, 105, tag, out, 13);
      break;
    case 110:
      fid = (unsigned)get(b + 4, 4);
      assert(fid < 64);
      if (get(b + 10, 2) == 6 && !memcmp(b + 12, "events", 6))
        names[fid] = 1;
      else if (get(b + 10, 2) == 3 && !memcmp(b + 12, "ack", 3))
        names[fid] = 2;
      else if (get(b + 10, 2) == 11 && !memcmp(b + 12, "transaction", 11)) {
        names[fid] = 3;
        staged_n = 0;
      } else {
        assert(get(b + 10, 2) == 6 && !memcmp(b + 12, "submit", 6));
        assert(++submit_walks == 1);
        names[fid] = 4;
      }
      out[0] = 1;
      send_reply(fd, 111, tag, out, 15);
      break;
    case 12:
      fid = (unsigned)get(b, 4);
      assert(fid < 64);
      put(out + 13, names[fid] == 3 ? 7 : 24, 4);
      send_reply(fd, 13, tag, out, 17);
      break;
    case 116:
      assert(!pending);
      pending = 1;
      pending_tag = tag;
      offset = (size_t)get(b + 4, 8);
      count = (size_t)get(b + 12, 4);
      break;
    case 118: {
      size_t size = (size_t)get(b + 12, 4);
      assert(size == n - 16);
      fid = (unsigned)get(b, 4);
      assert(fid < 64);
      if (names[fid] == 3) {
        assert(get(b + 4, 8) == staged_n && size <= sizeof staged - staged_n);
        memcpy(staged + staged_n, b + 16, size);
        staged_n += size;
      } else if (names[fid] == 4) {
        unsigned kind = (unsigned)get(staged + 6, 2);
        uint64_t id = get(staged + 16, 8);
        assert(get(b + 24, 8) == id);
        if (fault == 8 && kind == 257) {
          if (retry) {
            assert(staged_n == 448 && !memcmp(staged, retry_candidate, 448));
            assert(!memcmp(b + 16, retry_control, 24));
          } else {
            assert(staged_n == 448);
            memcpy(retry_candidate, staged, 448);
            memcpy(retry_control, b + 16, 24);
            retry = 1;
            put(out, 11, 4);
            send_reply(fd, 7, tag, out, 4);
            continue;
          }
        }
        if (fault == 7 && kind == 257) {
          assert(staged_n == 448);
          close(fd);
          _exit(0);
        }
        record(journal + used, 18, 48, kind == 256 ? 1 : 4 + 2 * responses);
        if (fault == 1)
          journal[used + 8] = 8;
        put(journal + used + 32, fault == 5 ? id + 1 : id, 8);
        put(journal + used + 40, kind, 2);
        used += 48;
        if (kind == 256) {
          assert(staged_n == 40);
          record(journal + used, 16, 56, 2);
          journal[used + 32] = 2;
          put(journal + used + 36, 1024, 4);
          journal[used + 40] = 7;
          journal[used + 48] = 128;
          journal[used + 50] = 2;
          if (fault == 2)
            put(journal + used + 36, 512, 4);
          if (fault == 3) {
            record(journal + used, 17, 40, 2);
            journal[used + 32] = 1;
          }
          used += fault == 3 ? 40 : 56;
        } else {
          assert(kind == 257 && staged_n == 448 && get(staged + 32, 8) == 8 + responses);
          assert(id == 2 + responses);
          if (fault == 9 || fault == 11) {
            assert(get(staged + 40, 2) == (fault == 9 ? 5u : 1u));
            assert(get(staged + 42, 2) == 2 && get(staged + 48, 2) == 1 &&
                   get(staged + 248, 2) == 2);
          } else if (fault == 12 || fault == 13) {
            assert(get(staged + 40, 2) == 3);
            assert(get(staged + 48, 2) == (fault == 12 ? 5u : 2u));
            if (fault == 13)
              assert(get(staged + 68, 2) == 2);
          } else if (fault == 10) {
            assert(get(staged + 40, 2) == 2 && staged[112] == 1);
            assert(!memcmp(staged + 120, "\0\n\xc3\xa9", 4));
          } else
            assert(get(staged + 40, 2) == 4 && get(staged + 48, 2) == 3);
          ++responses;
        }
      } else {
        assert(names[fid] == 2);
        acknowledged = get(b + 24, 8);
        if ((!fault || fault >= 6) && get(b + 24, 8) >= 2 + 2 * requests && requests < target) {
          record(journal + used, 32, 200, 3 + 2 * requests);
          journal[used + 32] = (unsigned char)(8 + requests);
          journal[used + 40] = 4;
          journal[used + 48] = 2;
          journal[used + 52] = 3;
          if (fault == 9 || fault == 11) {
            journal[used + 40] = fault == 9 ? 5 : 1;
            journal[used + 56] = 2;
          }
          if (fault == 10) {
            journal[used + 40] = 2;
            journal[used + 56] = 2;
            journal[used + 59] = 1;
            journal[used + 60] = 4;
            journal[used + 64] = 8;
            memcpy(journal + used + 72, "\0\n\xc3\xa9", 4);
          }
          if (fault == 12 || fault == 13) {
            journal[used + 40] = 3;
            journal[used + 56] = 2;
          }
          used += 200;
          ++requests;
        }
      }
      put(out, fault == 4 && names[fid] == 2 ? 15 : size, 4);
      send_reply(fd, 119, tag, out, 4);
      break;
    }
    case 120:
      assert(get(b, 4) < 64 && names[get(b, 4)] == 3);
      assert(acknowledged >= (responses ? 2 + 2 * responses : 1));
      ++clunks;
      send_reply(fd, 121, tag, NULL, 0);
      break;
    default:
      assert(0);
    }
    if (pending && offset < used) {
      if (count > used - offset)
        count = used - offset;
      put(out, count, 4);
      memcpy(out + 4, journal + offset, count);
      send_reply(fd, 117, pending_tag, out, count + 4);
      pending = 0;
    }
  }
  assert(responses == ((!fault || fault >= 8) ? target : 0u));
  if (!fault || fault >= 8)
    assert(clunks == 1 + target && submit_walks == 1);
  close(fd);
  _exit(0);
}
static void scenario(unsigned fault) {
  int target = fault ? 1 : 2;
  int pair[2], status, e, responded = 0, receipt = 0;
  pid_t child;
  struct sophia_9p_client wire;
  struct sophia_9p_handle tag;
  struct sophia_9p_reply reply;
  struct sophia_bo opening = {0};
  struct sophia_br session;
  struct sophia_bf_event event;
  struct sophia_bf_request request;
  struct sophia_bf_row rows[2] = {{0}};
  size_t size = sophia_9p_storage_bytes(4096, 8);
  void *storage = malloc(size);
  alarm(10);
  assert(storage && !socketpair(AF_UNIX, SOCK_STREAM, 0, pair));
  child = fork();
  assert(child >= 0);
  if (!child) {
    close(pair[0]);
    peer(pair[1], fault);
  }
  close(pair[1]);
  assert(!sophia_9p_init(&wire, pair[0], 4096, 8, 16, storage, size));
  assert(!sophia_9p_version(&wire, &tag));
  do {
    assert(sophia_9p_service(&wire, 4096) >= 0);
    sched_yield();
  } while (sophia_9p_peek(&wire, &reply) == SOPHIA_9P_AGAIN);
  assert(reply.type == 101 && !sophia_9p_consume(&wire, reply.handle));
  assert(!sophia_9p_attach(&wire, "", "", &tag, &opening.root));
  do {
    assert(sophia_9p_service(&wire, 4096) >= 0);
    sched_yield();
  } while (sophia_9p_peek(&wire, &reply) == SOPHIA_9P_AGAIN);
  assert(reply.type == 105 && !sophia_9p_consume(&wire, reply.handle));
  /* Startup has its own wire tests; this fixture supplies verified discovery.
   */
  opening.wire = &wire;
  opening.epoch = 7;
  opening.stage = 5;
  opening.limits.max_surfaces = 1024;
  opening.limits.max_label_bytes = 128;
  opening.limits.journal_records = 16;
  opening.limits.journal_bytes = 4096;
  opening.limits.staging_bytes = 512;
  opening.limits.response_timeout_ms = 5000;
  opening.limits.ack_progress_timeout_ms = 2000;
  opening.limits.assembly_timeout_ms = 12000;
  assert(!sophia_br_init(&session, &opening, 2, 2, 1000));
  if (fault >= 1 && fault <= 5) {
    for (;;) {
      e = sophia_br_dispatch(&session, 1, 4096);
      if (e)
        break;
      while (!sophia_br_event(&session, &event)) {
        e = sophia_br_consume(&session, 1);
        if (e)
          break;
      }
      if (e)
        break;
      e = sophia_br_acknowledge(&session, 1);
      assert(!e || e == SOPHIA_BS_BUSY);
      sched_yield();
    }
    assert(e == (fault == 3 ? SOPHIA_BR_REFUSED : SOPHIA_BS_PROTOCOL));
    assert(!sophia_br_ready(&session));
    assert(
        session.transaction.submission.custody ==
        ((fault == 1 || fault == 5) ? SOPHIA_BS_UNKNOWN : SOPHIA_BS_SUBMITTED));
    goto cleanup;
  }
  do {
    e = sophia_br_dispatch(&session, 1, 4096);
    if (e) {
      assert(fault == 7 && e == SOPHIA_BR_WIRE);
      assert(session.transaction.submission.custody == SOPHIA_BS_UNKNOWN &&
             !sophia_br_ready(&session));
      goto cleanup;
    }
    while (!(e = sophia_br_event(&session, &event))) {
      if (event.kind == SOPHIA_BF_SUBMITTED && event.candidate_kind == 257)
        ++receipt;
      assert(!sophia_br_consume(&session, 1));
    }
    assert(e == SOPHIA_BS_BUSY);
    e = sophia_br_acknowledge(&session, 1);
    assert(!e || e == SOPHIA_BS_BUSY);
    if (fault == 6 && sophia_br_transaction_ready(&session) &&
        session.has_request) {
      uint64_t due;
      if (sophia_br_deadline(&session, &due) && due == 5001) {
        assert(sophia_br_dispatch(&session, 5001, 4096) == SOPHIA_BS_DEADLINE);
        assert(!sophia_br_ready(&session));
        goto cleanup;
      }
    }
    if (fault != 6 && responded < target && sophia_br_transaction_ready(&session) &&
        !sophia_br_request(&session, &request)) {
      size_t count = 1;
      rows[0].kind = SOPHIA_BF_RETIRE_SURFACE;
      rows[0].surface_index = 2;
      rows[0].surface_generation = 3;
      if (fault == 12) {
        memset(&rows[0], 0, sizeof rows[0]);
        rows[0].kind = SOPHIA_BF_NO_CHANGE;
      } else if (fault >= 9) {
        struct sophia_bf_row descriptor = {0};
        descriptor.kind = SOPHIA_BF_EMIT_DESCRIPTOR;
        descriptor.surface_index = 2;
        descriptor.surface_generation = 3;
        descriptor.trust = 1;
        descriptor.attention = 2;
        descriptor.generation = 8;
        descriptor.grant_token = 12;
        descriptor.grant_revocation_epoch = 1;
        descriptor.grant_target_generation = 8;
        if (fault == 10 || fault == 13) {
          if (fault == 10)
            assert(request.label_present && request.label_length == 4 &&
                   !memcmp(request.label, "\0\n\xc3\xa9", 4));
          rows[0] = descriptor;
          rows[0].label_present = fault == 10;
          rows[0].label = request.label;
          rows[0].label_length = fault == 10 ? 4 : 0;
        } else {
          rows[0].kind = SOPHIA_BF_PUBLISH_RULE;
          rows[0].disclosure = 2;
          rows[0].trust = 1;
          rows[0].generation = 8;
          rows[1] = descriptor;
          count = 2;
          rows[1].grant_token = 0;
          assert(sophia_br_respond(&session, 8, rows, count, 1) ==
                 SOPHIA_BS_INVALID);
          assert(sophia_br_transaction_ready(&session));
          rows[1].grant_token = 12;
        }
      }
      if (fault == 13) {
        rows[0].attention = 1;
        assert(sophia_br_respond(&session, request.id, rows, count, 1) ==
               SOPHIA_BS_INVALID);
        assert(sophia_br_transaction_ready(&session));
        rows[0].attention = 2;
      }
      assert(request.id == 8u + (unsigned)responded &&
             !sophia_br_respond(&session, request.id, rows, count, 1));
      assert(!sophia_br_retry_ready(&session));
      ++responded;
    }
    if (fault == 8 && responded && sophia_br_retry_ready(&session)) {
      assert(session.transaction.submission.custody == SOPHIA_BS_QUEUED);
      e = sophia_br_retry(&session, 1);
      assert(!e || e == SOPHIA_BS_BUSY);
      assert(!sophia_br_retry_ready(&session));
    }
    sched_yield();
  } while (receipt < target || !sophia_br_transaction_ready(&session));
cleanup:
  sophia_br_close(&session);
  assert(!sophia_br_retry_ready(&session));
  close(pair[0]);
  free(storage);
  assert(waitpid(child, &status, 0) == child && WIFEXITED(status) &&
         !WEXITSTATUS(status));
}
int main(void) {
  unsigned fault;
  signal(SIGPIPE, SIG_IGN);
  for (fault = 0; fault <= 13; ++fault)
    scenario(fault);
  return 0;
}

#include "sophia_portal_requester_session.h"
#include <assert.h>
#include <errno.h>
#include <sched.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>
static uint64_t get(const unsigned char *b, size_t n) {
  uint64_t v = 0;
  size_t i;
  for (i = 0; i < n; ++i)
    v |= (uint64_t)b[i] << (i * 8);
  return v;
}
static void put(unsigned char *b, uint64_t v, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    b[i] = (unsigned char)(v >> (i * 8));
}
static int exact(int fd, void *buffer, size_t n) {
  unsigned char *b = buffer;
  size_t used = 0;
  while (used < n) {
    ssize_t r = read(fd, b + used, n - used);
    if (r < 0 && errno == EINTR)
      continue;
    if (!r || (r < 0 && errno == ECONNRESET))
      return 0;
    assert(r > 0);
    used += (size_t)r;
  }
  return 1;
}
static void send_reply(int fd, unsigned type, unsigned tag, const void *body,
                       size_t n) {
  unsigned char b[4096];
  size_t used = 0;
  assert(n + 7 <= sizeof b);
  put(b, n + 7, 4);
  b[4] = (unsigned char)type;
  put(b + 5, tag, 2);
  if (n)
    memcpy(b + 7, body, n);
  while (used < n + 7) {
    ssize_t r = write(fd, b + used, n + 7 - used);
    if (r < 0 && errno == EINTR)
      continue;
    if (r < 0 && (errno == EPIPE || errno == ECONNRESET))
      return;
    assert(r > 0);
    used += (size_t)r;
  }
}
static void record(unsigned char *b, size_t n, unsigned kind,
                   uint64_t sequence) {
  memset(b, 0, n);
  put(b, n, 4);
  b[4] = 1;
  put(b + 6, kind, 2);
  b[8] = 7;
  put(b + 24, sequence, 8);
}
enum { API = 1, LIMITS, EVENTS, ACK, TRANSACTION, SUBMIT, UPLOAD };
static void peer(int fd, unsigned fault) {
  unsigned char h[7], b[4096], out[4096], journal[1024], staged[344],
      limits[80];
  uint64_t sequence = 0, last_submission = 0, granted = 0;
  unsigned names[128] = {0}, versions = 0, attaches = 0, pending = 0,
           read_tag = 0, submit_pending = 0, submit_tag = 0;
  size_t journal_len = 0, staged_len = 0, read_offset = 0, read_count = 0;
  size_t payload_length = 0, payload_written = 0;
  unsigned revoked_pending = 0, revoked_tag = 0;
  unsigned end_refused = 0;
  unsigned char refused_end[48];
  unsigned char refused_control[24];
  const char *api = "sophia-portal-files version=1 role=portal-requester "
                    "epoch=7 fd_transfer=none\n";
  record(limits, 80, 1, 0);
  limits[32] = 1;
  limits[34] = 1;
  limits[44] = 255;
  put(limits + 36, 64, 4);
  put(limits + 40, 65536, 4);
  put(limits + 48, 256, 4);
  put(limits + 52, 32768, 4);
  put(limits + 56, 12000, 4);
  put(limits + 60, 2000, 4);
  put(limits + 64, 344, 4);
  put(limits + 68, 4096, 4);
  put(limits + 72, 17, 8);
  while (exact(fd, h, 7)) {
    size_t n = (size_t)get(h, 4) - 7;
    unsigned tag = (unsigned)get(h + 5, 2), fid;
    assert(n <= sizeof b && exact(fd, b, n));
    memset(out, 0, sizeof out);
    switch (h[4]) {
    case 100:
      assert(++versions == 1);
      send_reply(fd, 101, tag, b, n);
      break;
    case 104:
      assert(++attaches == 1);
      out[0] = 128;
      send_reply(fd, 105, tag, out, 13);
      break;
    case 110: {
      unsigned k;
      if (get(b + 8, 2) == 2) {
        const unsigned char path[] = {6,   0,   'u', 'p', 'l', 'o',
                                      'a', 'd', 1,   0,   '0'};
        fid = (unsigned)get(b + 4, 4);
        assert(fid < 128 && n == 21 && !memcmp(b + 10, path, sizeof path));
        names[fid] = UPLOAD;
        out[0] = 2;
        out[2] = 128;
        send_reply(fd, 111, tag, out, 28);
        break;
      }
      const char *paths[] = {"",    "api",         "limits", "events",
                             "ack", "transaction", "submit"};
      fid = (unsigned)get(b + 4, 4);
      assert(fid < 128 && get(b + 8, 2) == 1);
      for (k = API; k <= SUBMIT; ++k)
        if (get(b + 10, 2) == strlen(paths[k]) &&
            !memcmp(b + 12, paths[k], strlen(paths[k])))
          break;
      assert(k <= SUBMIT);
      names[fid] = k;
      if (k == TRANSACTION)
        staged_len = 0;
      out[0] = 1;
      send_reply(fd, 111, tag, out, 15);
      break;
    }
    case 12:
      fid = (unsigned)get(b, 4);
      assert(fid < 128 && names[fid]);
      assert(get(b + 4, 4) == (names[fid] == TRANSACTION ? 2
                               : names[fid] == ACK || names[fid] == SUBMIT ||
                                       names[fid] == UPLOAD
                                   ? 1
                                   : 0));
      out[13] = names[fid] == ACK || names[fid] == SUBMIT ? 24 : 7;
      send_reply(fd, 13, tag, out, 17);
      break;
    case 116: {
      size_t offset = (size_t)get(b + 4, 8), count = (size_t)get(b + 12, 4),
             length;
      const void *data;
      fid = (unsigned)get(b, 4);
      assert(fid < 128);
      if (names[fid] == EVENTS) {
        assert(!pending);
        pending = 1;
        read_tag = tag;
        read_offset = offset;
        read_count = count;
        break;
      }
      assert(names[fid] == API || names[fid] == LIMITS);
      data = names[fid] == API ? (const void *)api : limits;
      length = names[fid] == API ? strlen(api) : 80;
      assert(count <= 7 && offset <= length);
      if (count > length - offset)
        count = length - offset;
      put(out, count, 4);
      memcpy(out + 4, (const unsigned char *)data + offset, count);
      send_reply(fd, 117, tag, out, count + 4);
      break;
    }
    case 118: {
      size_t count = (size_t)get(b + 12, 4);
      fid = (unsigned)get(b, 4);
      assert(fid < 128 && count == n - 16);
      switch (names[fid]) {
      case UPLOAD: {
        size_t i;
        if (fault == 15 || fault == 16)
          assert(!payload_written);
        assert(get(b + 4, 8) == payload_written &&
               count <= payload_length - payload_written);
        for (i = 0; i < count; ++i)
          assert(b[16 + i] == 17);
        payload_written += count;
        if (fault == 16) {
          put(out, 116, 4);
          send_reply(fd, 7, tag, out, 4);
        } else if (fault == 15) {
          record(journal + journal_len, 48, 33, ++sequence);
          put(journal + journal_len + 32, granted, 8);
          journal[journal_len + 40] = 3;
          journal_len += 48;
          revoked_pending = 1;
          revoked_tag = tag;
        } else {
          put(out, count, 4);
          send_reply(fd, 119, tag, out, 4);
        }
        break;
      }
      case TRANSACTION:
        assert(get(b + 4, 8) == staged_len &&
               count <= sizeof staged - staged_len);
        memcpy(staged + staged_len, b + 16, count);
        staged_len += count;
        put(out, count, 4);
        send_reply(fd, 119, tag, out, 4);
        break;
      case SUBMIT: {
        unsigned kind = (unsigned)get(staged + 6, 2);
        uint64_t submission = get(staged + 16, 8);
        unsigned char *v;
        assert(count == 24 && submission == last_submission + 1 &&
               journal_len + 184 <= sizeof journal);
        if (fault >= 17 && kind == 259) {
          assert(staged_len == sizeof refused_end);
          if (!end_refused) {
            memcpy(refused_end, staged, sizeof refused_end);
            memcpy(refused_control, b + 16, sizeof refused_control);
            end_refused = 1;
            put(out, 11, 4);
            send_reply(fd, 7, tag, out, 4);
            break;
          }
          assert(!memcmp(refused_end, staged, sizeof refused_end));
          assert(!memcmp(refused_control, b + 16, sizeof refused_control));
        }
        last_submission = submission;
        v = journal + journal_len;
        record(v, 48, 18, ++sequence);
        put(v + 32, fault == 3 ? submission + 1 : submission, 8);
        put(v + 40, kind, 2);
        journal_len += 48;
        v = journal + journal_len;
        if (kind == 257) {
          uint64_t transfer = get(staged + 32, 8);
          int denied = !transfer || fault == 5 || transfer == granted;
          assert(staged_len == 344);
          record(v, 88, 32, ++sequence);
          put(v + 32, transfer, 8);
          v[40] = denied ? 1 : 2;
          if (!denied) {
            granted = transfer;
            v[41] = 1;
            memcpy(v + 42, staged + 56, 2);
            memcpy(v + 48, staged + 40, 16);
            memcpy(v + 64, staged + 72, 8);
            v[72] = fault == 6 ? 18 : 17;
            memcpy(v + 80, staged + 80, 8);
          }
          journal_len += 88;
          if (!denied && (fault == 7 || fault == 9)) {
            v = journal + journal_len;
            record(v, 48, 33, ++sequence);
            put(v + 32, transfer, 8);
            v[40] = fault == 7 ? 2 : 3;
            journal_len += 48;
          }
        } else if (kind == 258) {
          assert(staged_len == 56 && get(staged + 32, 8) == granted);
          payload_length = (size_t)get(staged + 48, 8);
          payload_written = 0;
        } else if (kind == 259) {
          unsigned i;
          assert(staged_len == 48 && get(staged + 32, 8) == granted &&
                 get(staged + 40, 8) == payload_written &&
                 payload_written == payload_length);
          for (i = 0; i < 128; ++i)
            if (names[i] == UPLOAD)
              break;
          assert(i < 128);
          record(v, 48, 33, ++sequence);
          put(v + 32, granted, 8);
          v[40] = 2;
          journal_len += 48;
        } else if (kind == 260) {
          uint64_t transfer = get(staged + 32, 8);
          assert(staged_len == 40 && transfer == granted);
          record(v, 48, 33, ++sequence);
          put(v + 32, transfer, 8);
          v[40] = 3;
          journal_len += 48;
        } else {
          assert(kind == 256 && staged_len == 40);
          record(v, fault == 1 ? 40 : 48, fault == 1 ? 17 : 16, ++sequence);
          v[32] = 1;
          if (fault != 1)
            v[40] = 7;
          journal_len += fault == 1 ? 40 : 48;
        }
        if (fault == 2) {
          submit_pending = 1;
          submit_tag = tag;
        } else {
          put(out, 24, 4);
          send_reply(fd, 119, tag, out, 4);
        }
        break;
      }
      case ACK:
        assert(count == 16 && get(b + 16, 8) == 7);
        put(out, fault == 4 ? 15 : 16, 4);
        send_reply(fd, 119, tag, out, 4);
        if (revoked_pending && get(b + 24, 8) == sequence) {
          put(out, 116, 4);
          send_reply(fd, 7, revoked_tag, out, 4);
          revoked_pending = 0;
        }
        break;
      default:
        assert(0);
      }
      break;
    }
    case 120:
      fid = (unsigned)get(b, 4);
      assert(fid < 128 && names[fid]);
      names[fid] = 0;
      send_reply(fd, 121, tag, NULL, 0);
      break;
    default:
      assert(0);
    }
    if (pending && read_offset < journal_len) {
      size_t count = journal_len - read_offset;
      if (count > read_count)
        count = read_count;
      put(out, count, 4);
      memcpy(out + 4, journal + read_offset, count);
      send_reply(fd, 117, read_tag, out, count + 4);
      pending = 0;
      if (submit_pending && read_offset + count >= 48) {
        put(out, 24, 4);
        send_reply(fd, 119, submit_tag, out, 4);
        submit_pending = 0;
      }
    }
  }
  if (fault == 15) {
    unsigned i;
    assert(!revoked_pending);
    for (i = 0; i < 128; ++i)
      assert(names[i] != UPLOAD);
  }
  close(fd);
  _exit(0);
}
static int pump(struct sophia_pqr *s) {
  int e = sophia_pqr_dispatch(s, 2, 4096);
  if (e)
    return e;
  if (sophia_pqr_event(s)) {
    e = sophia_pqr_consume(s, 2);
    if (e)
      return e;
  }
  e = sophia_pqr_acknowledge(s, 2);
  return e == SOPHIA_PQS_BUSY ? 0 : e;
}
static void requests(struct sophia_pqr *s, unsigned fault) {
  struct sophia_pq_request request = {0};
  struct sophia_pqr_request_id id, other;
  int e;
  request.transfer = 11;
  request.source = 2;
  request.target = 3;
  request.kind = SOPHIA_PQ_CLIPBOARD;
  request.mime_present = 1;
  request.mime = "text/plain";
  request.mime_length = 10;
  request.byte_size = 3;
  request.generation = 5;
  request.deadline_msec = 900;
  assert(!sophia_pqr_request(s, &request, 2, 10000, &id));
  other = id;
  other.issuer = NULL;
  assert(!sophia_pqr_transfer(s, other));
  assert(sophia_pqr_release_transfer(s, other, 2) == SOPHIA_PQS_INVALID);
  assert(sophia_pqr_ready(s));
  assert(sophia_pqr_request(s, &request, 2, 10000, &other) == SOPHIA_PQS_BUSY);
  for (;;) {
    const struct sophia_pqf *t;
    if (pump(s))
      break;
    t = sophia_pqr_transfer(s, id);
    assert(t);
    if ((fault == 8 || fault == 10) && t->state == SOPHIA_PQF_ALLOWED)
      break;
    if (t->state == SOPHIA_PQF_DENIED || t->state == SOPHIA_PQF_REVOKED) {
      e = sophia_pqr_release_transfer(s, id, 2);
      if (!e) {
        assert(!sophia_pqr_transfer(s, id));
        break;
      }
      assert(e == SOPHIA_PQS_BUSY);
    }
    sched_yield();
  }
  if (fault == 6 || fault == 7) {
    assert(s->error && !sophia_pqf_grant(sophia_pqr_transfer(s, id)));
  } else
    assert(!s->error);
  if (fault == 8) {
    struct sophia_pq_grant original =
        *sophia_pqf_grant(sophia_pqr_transfer(s, id));
    do {
      assert(!pump(s));
      e = sophia_pqr_request(s, &request, 2, 10000, &other);
      assert(!e || e == SOPHIA_PQS_BUSY);
    } while (e);
    while (sophia_pqr_transfer(s, other)->state == SOPHIA_PQF_PENDING) {
      assert(!pump(s));
      sched_yield();
    }
    assert(sophia_pqr_transfer(s, other)->state == SOPHIA_PQF_DENIED);
    assert(!memcmp(sophia_pqf_grant(sophia_pqr_transfer(s, id)), &original,
                   sizeof original));
  }
  if (fault == 10) {
    do {
      assert(!pump(s));
      e = sophia_pqr_cancel(s, id, 2, 10000);
      assert(!e || e == SOPHIA_PQS_BUSY);
    } while (e);
    assert(sophia_pqr_transfer(s, id)->state == SOPHIA_PQF_ALLOWED);
    assert(sophia_pqr_cancel(s, id, 2, 10000) == SOPHIA_PQS_BUSY);
    for (;;) {
      assert(!pump(s));
      if (sophia_pqr_transfer(s, id)->state == SOPHIA_PQF_REVOKED) {
        e = sophia_pqr_release_transfer(s, id, 2);
        if (!e)
          break;
        assert(e == SOPHIA_PQS_BUSY);
      }
      sched_yield();
    }
  }
}
static void uploads(struct sophia_pqr *s, size_t length, int cancel, int revoke,
                    int fd, int retry) {
  unsigned char bytes[65536];
  struct sophia_pq_request request = {0};
  struct sophia_pqr_request_id id;
  size_t written;
  int e;
  memset(bytes, 17, sizeof bytes);
  request.transfer = 11;
  request.source = 2;
  request.target = 3;
  request.kind = SOPHIA_PQ_CLIPBOARD;
  request.byte_size = length;
  request.generation = 5;
  request.deadline_msec = 900;
  assert(!sophia_pqr_request(s, &request, 2, 10000, &id));
  while (!sophia_pqr_transaction_ready(s)) {
    assert(!pump(s));
    sched_yield();
  }
  assert(sophia_pqr_begin(s, id, bytes, length, 2, 2));
  assert(!s->upload_present);
  if (length)
    assert(bytes[0] == 17 && bytes[length - 1] == 17);
  assert(!sophia_pqr_begin(s, id, bytes, length, 2, 10000));
  assert(sophia_pqr_begin(s, id, bytes, length, 2, 10000) == SOPHIA_PQS_BUSY);
  assert(s->upload.bytes == bytes);
  assert(sophia_pqr_end(s, id, 2, 10000) == SOPHIA_PQS_BUSY);
  if (revoke == 2) {
    const struct sophia_pqf *transfer;
    do {
      e = pump(s);
      sched_yield();
    } while (!e);
    assert(e == SOPHIA_PQS_REMOTE);
    transfer = sophia_pqr_transfer(s, id);
    assert(transfer && transfer->state == SOPHIA_PQF_ALLOWED &&
           transfer->closed && !sophia_pqf_grant(transfer));
    close(fd);
    assert(!sophia_pqr_release_upload(s, id));
    return;
  }
  while (!revoke && (!sophia_pqr_upload_ready(s, id) ||
                     !sophia_pqr_transaction_ready(s))) {
    assert(!pump(s));
    sched_yield();
  }
  if (!revoke)
    assert(!sophia_pqr_upload_written(s, id, &written) && written == length);
  assert(s->upload.bytes == bytes);
  if (revoke) {
    /* The peer revokes while the first payload write awaits its reply. */
  } else if (cancel) {
    assert(!sophia_pqr_cancel(s, id, 2, 10000));
    assert(sophia_pqr_transfer(s, id)->state == SOPHIA_PQF_ALLOWED);
  } else {
    assert(!sophia_pqr_end(s, id, 2, 10000));
  }
  if (!revoke)
    assert(sophia_pqr_cancel(s, id, 2, 10000) == SOPHIA_PQS_BUSY);
  if (retry) {
    unsigned i;
    while (s->transaction.submission.custody != SOPHIA_PQS_QUEUED ||
           sophia_pqr_transfer(s, id)->state != SOPHIA_PQF_END_ISSUED) {
      assert(!pump(s));
      sched_yield();
    }
    for (i = 0; i < 16; ++i) {
      assert(!pump(s));
      assert(s->transaction.submission.custody == SOPHIA_PQS_QUEUED);
      assert(sophia_pqr_transfer(s, id)->state == SOPHIA_PQF_END_ISSUED);
    }
    assert(!sophia_pqr_retry(s, 2));
  }
  do {
    assert(!pump(s));
    e = sophia_pqr_release_upload(s, id);
    assert(!e || e == SOPHIA_PQS_BUSY);
    sched_yield();
  } while (e);
  assert(sophia_pqr_transfer(s, id)->state ==
         (cancel || revoke ? SOPHIA_PQF_REVOKED : SOPHIA_PQF_COMPLETED));
  do {
    assert(!pump(s));
    e = sophia_pqr_release_transfer(s, id, 2);
    assert(!e || e == SOPHIA_PQS_BUSY);
    sched_yield();
  } while (e);
}
int main(void) {
  unsigned fault;
  signal(SIGPIPE, SIG_IGN);
  alarm(20);
  for (fault = 0; fault < 19; ++fault) {
    int pair[2], status, e;
    pid_t child;
    struct sophia_9p_client wire;
    struct sophia_9p_handle handle;
    struct sophia_9p_reply reply;
    struct sophia_pqo opening;
    struct sophia_pqr session;
    uint32_t root;
    uint64_t events = 0;
    size_t bytes = sophia_9p_storage_bytes(4096, fault >= 11 ? 4 : 3);
    void *storage = malloc(bytes);
    assert(storage && !socketpair(AF_UNIX, SOCK_STREAM, 0, pair));
    child = fork();
    assert(child >= 0);
    if (!child) {
      close(pair[0]);
      peer(pair[1], fault);
    }
    close(pair[1]);
    assert(!sophia_9p_init(&wire, pair[0], 4096, fault >= 11 ? 4 : 3,
                           fault >= 11 ? 6 : 5, storage, bytes));
    assert(!sophia_9p_version(&wire, &handle));
    do {
      assert(sophia_9p_service(&wire, 4096) >= 0);
      sched_yield();
    } while (sophia_9p_peek(&wire, &reply) == SOPHIA_9P_AGAIN);
    assert(reply.type == 101 && !sophia_9p_consume(&wire, reply.handle));
    assert(!sophia_9p_attach(&wire, "", "", &handle, &root));
    do {
      assert(sophia_9p_service(&wire, 4096) >= 0);
      sched_yield();
    } while (sophia_9p_peek(&wire, &reply) == SOPHIA_9P_AGAIN);
    assert(reply.type == 105 && !sophia_9p_consume(&wire, reply.handle));
    assert(!sophia_pqo_init(&opening, &wire, root, 0, 10000));
    while (!sophia_pqo_ready(&opening)) {
      assert(!sophia_pqo_dispatch(&opening, 1, 4096));
      sched_yield();
    }
    assert(!sophia_pqr_init(&session, &opening, 1, 1, 10000));
    assert(!sophia_pqo_ready(&opening));
    while (!session.error && !sophia_pqr_ready(&session)) {
      e = sophia_pqr_dispatch(&session, 1, fault ? 4096 : 1);
      if (e)
        break;
      if (sophia_pqr_event(&session)) {
        const struct sophia_pq_event *v = sophia_pqr_event(&session);
        assert(!sophia_pqr_ready(&session) && v->sequence == ++events);
        e = sophia_pqr_consume(&session, 1);
        if (e)
          break;
      }
      e = sophia_pqr_acknowledge(&session, 1);
      assert(!e || e == SOPHIA_PQS_BUSY);
      sched_yield();
    }
    if (fault == 0 || fault == 2 || fault >= 5) {
      assert(sophia_pqr_ready(&session) && events == 2 && wire.fid_count == 3);
      assert(session.transaction.submission.custody == SOPHIA_PQS_SUBMITTED);
      if (fault >= 11)
        uploads(&session, fault == 11 || fault == 13 || fault == 17 ? 0 : 65536,
                fault == 13 || fault == 14,
                fault == 15 || fault == 16 ? fault - 14 : 0, pair[0],
                fault >= 17);
      else if (fault >= 5)
        requests(&session, fault);
    } else {
      assert(session.error && !sophia_pqr_ready(&session));
      if (fault == 1)
        assert(session.error == SOPHIA_PQR_REFUSED);
    }
    sophia_pqr_close(&session);
    if (fault != 16)
      close(pair[0]);
    free(storage);
    assert(waitpid(child, &status, 0) == child && WIFEXITED(status) &&
           WEXITSTATUS(status) == 0);
  }
  return 0;
}

/* Scripted 9P2000.L peer, not a production Session. The real generic 9P
 * client, WM file codec and WM session are linked by the tests. */
#include "../sophia_wm_session.h"
#include <assert.h>
#include <errno.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#define WP_CAPS ((UINT64_C(1) << 20) - 1)
#define WP_EPOCH 41u
enum {
  WP_ROOT = 1,
  WP_API,
  WP_LIMITS,
  WP_EVENTS,
  WP_TX,
  WP_SUBMIT,
  WP_ACK,
  WP_SNAPSHOT
};
struct wm_peer {
  int fd;
  uint8_t input[16384], journal[65536], tx[SOPHIA_WF_MAX_RECORD], limits[64],
      snapshot[136];
  size_t used, journal_size, tx_size;
  unsigned files[256];
  uint64_t sequence, acked, accepted, event_offset, last_id, selected;
  unsigned opens, busy_opens, submits, acks, pins, clunks, events_pending;
  uint16_t event_tag, ack_tag;
  uint32_t event_count, short_tx, object_chunk, event_chunk, ack_hold;
  uint32_t submit_error, submit_count, event_error, ack_count, ack_error;
  unsigned object_again, object_opens, object_truncate;
  unsigned submit_silent, custody_on_error, event_first, error_once;
  unsigned bad_event_before, wrong_epoch, wrong_kind, close_after;
};
struct wm_rig {
  struct wm_peer peer;
  struct sophia_ws *session;
  void *storage;
  int client_fd;
  uint64_t now;
};
static inline uint64_t wp_get(const uint8_t *p, size_t n) {
  size_t i;
  uint64_t v = 0;
  for (i = 0; i < n; ++i)
    v |= (uint64_t)p[i] << (i * 8);
  return v;
}
static inline void wp_put(uint8_t *p, uint64_t v, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    p[i] = (uint8_t)(v >> (i * 8));
}
static inline void wp_send(struct wm_peer *p, unsigned kind, uint16_t tag,
                           const void *data, size_t n) {
  uint8_t b[8192];
  size_t at = 0;
  assert(n + 7 <= sizeof(b));
  wp_put(b, n + 7, 4);
  b[4] = (uint8_t)kind;
  wp_put(b + 5, tag, 2);
  if (n)
    memcpy(b + 7, data, n);
  while (at < n + 7) {
    ssize_t amount =
        send(p->fd, b + at, n + 7 - at, MSG_DONTWAIT | MSG_NOSIGNAL);
    assert(amount > 0);
    at += (size_t)amount;
  }
}
static inline void wp_error(struct wm_peer *p, uint16_t tag, uint32_t error) {
  uint8_t b[4];
  wp_put(b, error, 4);
  wp_send(p, 7, tag, b, 4);
}
static inline void wp_count(struct wm_peer *p, uint16_t tag, uint32_t count) {
  uint8_t b[4];
  wp_put(b, count, 4);
  wp_send(p, 119, tag, b, 4);
}
static inline void wp_data(struct wm_peer *p, uint16_t tag, const uint8_t *data,
                           size_t size, uint64_t offset, uint32_t count) {
  uint8_t b[8192];
  size_t n = offset < size ? size - (size_t)offset : 0;
  if (n > count)
    n = count;
  assert(n <= sizeof(b) - 11);
  wp_put(b, n, 4);
  if (n)
    memcpy(b + 4, data + (size_t)offset, n);
  wp_send(p, 117, tag, b, n + 4);
}
static inline void wp_events(struct wm_peer *p) {
  uint32_t count = p->event_count;
  if (!p->events_pending)
    return;
  if (p->event_error) {
    wp_error(p, p->event_tag, p->event_error);
    p->events_pending = 0;
    return;
  }
  if (p->event_offset == p->journal_size)
    return;
  assert(p->event_offset < p->journal_size);
  if (p->event_chunk && count > p->event_chunk)
    count = p->event_chunk;
  wp_data(p, p->event_tag, p->journal, p->journal_size, p->event_offset, count);
  p->events_pending = 0;
}
static inline void wp_record(struct wm_peer *p, struct sophia_wf_record *r) {
  size_t n;
  r->header.epoch = WP_EPOCH;
  r->header.sequence = ++p->sequence;
  r->header.submission = 0;
  assert(!sophia_wf_encode(p->journal + p->journal_size,
                           sizeof(p->journal) - p->journal_size, WP_CAPS, r,
                           &n));
  p->journal_size += n;
}
static inline void wp_outcome(struct wm_peer *p, uint64_t transaction) {
  struct sophia_wf_record r = {0};
  r.header.kind = SOPHIA_WF_CONFIGURATION_OUTCOME;
  r.value.configuration_outcome.transaction = transaction;
  r.value.configuration_outcome.generation = 1;
  r.value.configuration_outcome.outcome = 1;
  wp_record(p, &r);
}
static inline void wp_submitted(struct wm_peer *p, uint64_t id, uint16_t kind) {
  struct sophia_wf_record r = {0};
  size_t at;
  if (p->bad_event_before) {
    wp_outcome(p, 900);
    memset(p->journal + p->journal_size - 8, 0,
           2); /* Invalid typed outcome, valid envelope. */
  }
  r.header.kind = SOPHIA_WF_SUBMITTED;
  r.value.submitted.submission = id;
  r.value.submitted.kind = p->wrong_kind ? SOPHIA_WF_DIRTY : kind;
  at = p->journal_size;
  wp_record(p, &r);
  if (p->wrong_epoch)
    wp_put(p->journal + at + 8, WP_EPOCH + 1, 8);
  p->accepted = p->sequence;
}
static inline unsigned wp_name(const uint8_t *name, size_t n) {
  static const char *const names[] = {
      "api", "limits", "events", "transaction", "submit", "ack", "snapshot"};
  size_t i;
  for (i = 0; i < sizeof(names) / sizeof(names[0]); ++i)
    if (strlen(names[i]) == n && !memcmp(name, names[i], n))
      return (unsigned)i + WP_API;
  assert(!"unexpected file");
  return 0;
}
static inline void wp_request(struct wm_peer *p, const uint8_t *message) {
  uint8_t type = message[4], b[32] = {0};
  uint16_t tag = (uint16_t)wp_get(message + 5, 2);
  const uint8_t *q = message + 7;
  uint32_t fid = (uint32_t)wp_get(q, 4);
  if (type == 100) {
    wp_put(b, wp_get(q, 4), 4);
    wp_put(b + 4, 8, 2);
    memcpy(b + 6, "9P2000.L", 8);
    wp_send(p, 101, tag, b, 14);
    return;
  }
  assert(fid < 256);
  if (type == 104) {
    p->files[fid] = WP_ROOT;
    b[0] = 0x80;
    wp_put(b + 5, 1, 8);
    wp_send(p, 105, tag, b, 13);
  } else if (type == 110) {
    uint32_t target = (uint32_t)wp_get(q + 4, 4);
    unsigned file;
    assert(p->files[fid] == WP_ROOT && target < 256 && wp_get(q + 8, 2) == 1);
    file = wp_name(q + 12, (size_t)wp_get(q + 10, 2));
    p->files[target] = file;
    wp_put(b, 1, 2);
    wp_put(b + 7, file, 8);
    wp_send(p, 111, tag, b, 15);
  } else if (type == 12) {
    unsigned file = p->files[fid];
    if (file == WP_TX) {
      ++p->opens;
      if (p->accepted > p->acked) {
        ++p->busy_opens;
        wp_error(p, tag, 16);
        return;
      }
      p->tx_size = 0;
    }
    if (file == WP_SNAPSHOT) {
      ++p->object_opens;
      if (p->object_again) {
        --p->object_again;
        wp_error(p, tag, 11);
        return;
      }
      assert(!p->pins);
      ++p->pins;
    }
    wp_put(b + 5, file, 8);
    wp_send(p, 13, tag, b, 17);
  } else if (type == 116) {
    unsigned file = p->files[fid];
    uint64_t offset = wp_get(q + 4, 8);
    uint32_t count = (uint32_t)wp_get(q + 12, 4);
    static const uint8_t api[] =
        "sophia-wm-files version=1 output_transport=current_ipc\n";
    if (file == WP_API)
      wp_data(p, tag, api, sizeof(api) - 1, offset, count);
    else if (file == WP_LIMITS)
      wp_data(p, tag, p->limits, sizeof(p->limits), offset, count);
    else if (file == WP_SNAPSHOT) {
      if (p->object_chunk && count > p->object_chunk)
        count = p->object_chunk;
      wp_data(p, tag, p->snapshot,
              p->object_truncate ? sizeof(p->snapshot) - 1
                                 : sizeof(p->snapshot),
              offset, count);
    } else {
      assert(file == WP_EVENTS && !p->events_pending);
      p->events_pending = 1;
      p->event_tag = tag;
      p->event_offset = offset;
      p->event_count = count;
    }
  } else if (type == 118) {
    unsigned file = p->files[fid];
    uint64_t offset = wp_get(q + 4, 8);
    uint32_t count = (uint32_t)wp_get(q + 12, 4);
    const uint8_t *data = q + 16;
    if (file == WP_TX) {
      if (p->short_tx && count > p->short_tx)
        count = p->short_tx;
      assert(offset == p->tx_size && count <= sizeof(p->tx) - p->tx_size);
      memcpy(p->tx + p->tx_size, data, count);
      p->tx_size += count;
      wp_count(p, tag, count);
    } else if (file == WP_ACK) {
      assert(!offset && count == 16 && wp_get(data, 8) == WP_EPOCH);
      assert(wp_get(data + 8, 8) >= p->acked &&
             wp_get(data + 8, 8) <= p->sequence);
      p->acked = wp_get(data + 8, 8);
      ++p->acks;
      if (p->ack_error)
        wp_error(p, tag, p->ack_error);
      else if (p->ack_hold)
        p->ack_tag = tag;
      else
        wp_count(p, tag, p->ack_count);
    } else {
      struct sophia_wf_record candidate, event = {0};
      uint64_t id = wp_get(data + 8, 8);
      uint32_t error = p->submit_error;
      assert(file == WP_SUBMIT && !offset && count == 24 &&
             wp_get(data, 8) == WP_EPOCH);
      assert(wp_get(data + 16, 4) == p->tx_size && !wp_get(data + 20, 4));
      assert(!sophia_wf_decode(p->tx, p->tx_size, WP_CAPS, &candidate));
      assert(candidate.header.submission == id);
      ++p->submits;
      if (candidate.header.kind == SOPHIA_WF_NEGOTIATE) {
        event.header.kind = SOPHIA_WF_NEGOTIATED;
        event.value.selected_capabilities = p->selected;
        wp_record(p, &event);
        wp_submitted(p, id, candidate.header.kind);
        wp_count(p, tag, 24);
      } else {
        if (p->last_id && p->error_once)
          assert(id == p->last_id);
        p->last_id = id;
        if ((!error && !p->submit_silent) || p->custody_on_error)
          wp_submitted(p, id, candidate.header.kind);
        if (p->event_first)
          wp_events(p);
        if (error)
          wp_error(p, tag, error);
        else
          wp_count(p, tag, p->submit_count);
        if (p->error_once)
          p->submit_error = 0;
      }
    }
  } else if (type == 120) {
    if (p->files[fid] == WP_SNAPSHOT) {
      assert(p->pins == 1);
      --p->pins;
      ++p->clunks;
    }
    p->files[fid] = 0;
    wp_send(p, 121, tag, NULL, 0);
  } else
    assert(!"unexpected 9P request");
}
static inline void wp_pump(struct wm_peer *p) {
  for (;;) {
    ssize_t n = recv(p->fd, p->input + p->used, sizeof(p->input) - p->used,
                     MSG_DONTWAIT);
    if (n < 0) {
      assert(errno == EAGAIN || errno == EWOULDBLOCK);
      break;
    }
    if (!n)
      break;
    p->used += (size_t)n;
    while (p->used >= 4 && wp_get(p->input, 4) <= p->used) {
      size_t bytes = (size_t)wp_get(p->input, 4);
      assert(bytes >= 7);
      wp_request(p, p->input);
      p->used -= bytes;
      memmove(p->input, p->input + bytes, p->used);
    }
  }
  wp_events(p);
  if (p->close_after && p->submits >= 2) {
    assert(!shutdown(p->fd, SHUT_WR));
    p->close_after = 0;
  }
}
static inline struct wm_rig *wr_new(void) {
  struct wm_rig *r = calloc(1, sizeof(*r));
  struct sophia_ws_config cfg = {8192, {WP_CAPS, 0}, 10000};
  struct sophia_wf_record value = {0};
  size_t n;
  int sockets[2];
  assert(r && !socketpair(AF_UNIX, SOCK_STREAM, 0, sockets));
  r->client_fd = sockets[0];
  r->peer.fd = sockets[1];
  r->peer.submit_count = 24;
  r->peer.ack_count = 16;
  r->peer.selected = WP_CAPS;
  r->session = calloc(1, sophia_ws_state_bytes());
  r->storage = malloc(sophia_ws_storage_bytes(8192));
  assert(r->session && r->storage);
  value.header.kind = SOPHIA_WF_LIMITS;
  value.header.epoch = WP_EPOCH;
  value.value.limits = (struct sophia_wf_limits){WP_CAPS, 1048576, 1048576, 64,
                                                 32,      12000,   4000,    1};
  assert(!sophia_wf_encode(r->peer.limits, sizeof(r->peer.limits), 0, &value,
                           &n) &&
         n == 64);
  assert(!sophia_ws_open_fd(r->session, r->client_fd, &cfg, r->storage,
                            sophia_ws_storage_bytes(8192), 0));
  return r;
}
static inline int wr_step(struct wm_rig *r) {
  int status;
  wp_pump(&r->peer);
  status = sophia_ws_dispatch(r->session, POLLIN | POLLOUT, 65536, ++r->now);
  return status;
}
static inline void wr_ready(struct wm_rig *r) {
  unsigned i;
  for (i = 0; i < 150; ++i) {
    struct sophia_ws_obligations o;
    assert(!wr_step(r));
    assert(!sophia_ws_obligations(r->session, &o));
    if (sophia_ws_state(r->session) == SOPHIA_WS_READY && o.acked == 2 &&
        !o.deadline_ms)
      return;
  }
  assert(!"bootstrap did not finish");
}
static inline uint64_t wr_submit(struct wm_rig *r, uint64_t domain) {
  struct sophia_wf_record value = {0};
  uint64_t ticket;
  value.header.kind = SOPHIA_WF_CONFIGURATION;
  value.value.configuration.transaction = domain;
  value.value.configuration.generation = 1;
  assert(!sophia_ws_submit(r->session, &value, r->now + 1000, &ticket));
  return ticket;
}
static inline enum sophia_ws_custody wr_custody(struct wm_rig *r,
                                                uint64_t ticket) {
  enum sophia_ws_custody outcome;
  uint32_t error;
  assert(!sophia_ws_outcome(r->session, ticket, &outcome, &error));
  return outcome;
}
static inline void wr_drop(struct wm_rig *r) {
  sophia_ws_close(r->session);
  close(r->client_fd);
  close(r->peer.fd);
  free(r->storage);
  free(r->session);
  free(r);
}

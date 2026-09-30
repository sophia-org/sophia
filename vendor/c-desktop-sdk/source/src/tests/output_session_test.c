/* Output session against a scripted 9P2000.L peer, not a production Session.
 * The peer enforces the export's custody rules by default: one staged
 * candidate reopened only after its Submitted is acknowledged and its fid
 * clunked, topology EAGAIN before negotiation, one topology handle, and an
 * ack refused with EAGAIN until the announced object was read completely. */
#include "sophia_output_session.h"
#include <assert.h>
#include <errno.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#define EPOCH 41u
enum { F_ROOT = 1, F_API, F_LIMITS, F_TOPOLOGY, F_EVENTS, F_TX, F_SUBMIT, F_ACK };
static const char *const names[] = {"api",    "limits",      "topology", "events",
                                    "transaction", "submit", "ack"};

struct peer {
  int fd;
  uint8_t input[16384], journal[8192], tx[SOPHIA_OF_MAX_CANDIDATE];
  uint8_t limits[SOPHIA_OF_LIMITS_RECORD], object[SOPHIA_OF_MAX_TOPOLOGY];
  const char *api;
  size_t used, journal_size, tx_size, object_size, object_read;
  unsigned files[256];
  uint32_t tx_fid, events_tag_valid;
  uint64_t sequence, acked, accepted, event_offset, qid, announced;
  uint64_t announced_qid, published_epoch, granted, last_submit_id;
  uint16_t event_tag;
  uint32_t event_count, event_chunk, object_chunk;
  unsigned attaches, submits, acks, tx_opens, topology_open, topology_opens;
  unsigned topology_again, submit_again, events_pending, negotiated;
  uint32_t submit_error, wrong_qid, extra_grant, outcome_after;
  uint64_t refused;
  uint64_t last_bytes_id;
  uint8_t last_bytes[SOPHIA_OF_MAX_CANDIDATE];
  size_t last_size;
};
struct rig {
  struct peer peer;
  struct sophia_os *session;
  void *storage;
  int client_fd;
  uint64_t now;
};

static uint64_t get(const uint8_t *p, size_t n) {
  uint64_t v = 0;
  size_t i;
  for (i = 0; i < n; ++i)
    v |= (uint64_t)p[i] << (8 * i);
  return v;
}
static void put(uint8_t *p, uint64_t v, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    p[i] = (uint8_t)(v >> (8 * i));
}
static void send_reply(struct peer *p, unsigned kind, uint16_t tag,
                       const void *data, size_t n) {
  uint8_t b[70000];
  size_t at = 0;
  assert(n + 7 <= sizeof(b));
  put(b, n + 7, 4);
  b[4] = (uint8_t)kind;
  put(b + 5, tag, 2);
  if (n)
    memcpy(b + 7, data, n);
  while (at < n + 7) {
    ssize_t amount = send(p->fd, b + at, n + 7 - at, MSG_NOSIGNAL);
    assert(amount > 0);
    at += (size_t)amount;
  }
}
static void error(struct peer *p, uint16_t tag, uint32_t value) {
  uint8_t b[4];
  put(b, value, 4);
  send_reply(p, 7, tag, b, 4);
}
static void count(struct peer *p, uint16_t tag, uint32_t value) {
  uint8_t b[4];
  put(b, value, 4);
  send_reply(p, 119, tag, b, 4);
}
static void data(struct peer *p, uint16_t tag, const uint8_t *bytes,
                 size_t size, uint64_t offset, uint32_t n) {
  static uint8_t b[70000];
  size_t amount = offset < size ? size - (size_t)offset : 0;
  if (amount > n)
    amount = n;
  put(b, amount, 4);
  if (amount)
    memcpy(b + 4, bytes + offset, amount);
  send_reply(p, 117, tag, b, amount + 4);
}
static void qid(uint8_t *b, uint64_t path) {
  b[0] = 0;
  put(b + 1, 0, 4);
  put(b + 5, path, 8);
}
static void record(struct peer *p, struct sophia_of_record *r) {
  size_t n;
  r->header.epoch = EPOCH;
  r->header.submission = 0;
  r->header.sequence = ++p->sequence;
  assert(!sophia_of_encode(p->journal + p->journal_size,
                           sizeof(p->journal) - p->journal_size, r, &n));
  p->journal_size += n;
}
static void topology(struct sophia_of_topology *t, uint64_t epoch) {
  memset(t, 0, sizeof(*t));
  t->topology_epoch = epoch;
  t->primary_output = 2;
  t->head_count = t->group_count = 1;
  t->mode_count = 2;
  t->heads[0].head = 3;
  t->heads[0].generation = 4;
  t->heads[0].flags = SOPHIA_OF_HEAD_CONNECTED | SOPHIA_OF_HEAD_ENABLED;
  t->heads[0].transforms = 0xff;
  t->heads[0].label_bytes = 4;
  memcpy(t->heads[0].label, "DP-1", 4);
  t->heads[0].mode_count = 2;
  t->heads[0].current_mode = 5;
  t->modes[0] = (struct sophia_of_mode){5, 1920, 1080, 60000, 1};
  t->modes[1] = (struct sophia_of_mode){6, 1280, 720, 60000, 0};
  t->groups[0].output = 2;
  t->groups[0].generation = 6;
  t->groups[0].width = 1920;
  t->groups[0].height = 1080;
  t->groups[0].member_count = 1;
  t->groups[0].members[0] = (struct sophia_of_member){3, SOPHIA_OF_EXACT};
}
/* Replace the current object and announce it. The export refuses a second
 * unacknowledged announcement, so the script may only publish after ack. */
static void publish(struct peer *p, uint64_t epoch) {
  static struct sophia_of_topology t;
  struct sophia_of_record r = {0};
  assert(p->acked >= p->announced);
  topology(&t, epoch);
  assert(!sophia_of_topology_encode(p->object, sizeof(p->object), EPOCH, &t,
                                    &p->object_size));
  p->qid = 90 + epoch;
  p->object_read = 0;
  p->published_epoch = epoch;
  r.header.kind = SOPHIA_OF_OBJECT_PUBLISHED;
  r.value.published.topology_epoch = epoch;
  r.value.published.qid_path = p->qid + p->wrong_qid;
  record(p, &r);
  p->announced = p->sequence;
  p->announced_qid = p->qid;
}
static void submitted(struct peer *p, uint64_t id, uint16_t kind) {
  struct sophia_of_record r = {0};
  r.header.kind = SOPHIA_OF_SUBMITTED;
  r.value.submitted.submission = id;
  r.value.submitted.kind = kind;
  record(p, &r);
  p->accepted = p->sequence;
}
static void outcome(struct peer *p, uint64_t transaction, uint16_t kind) {
  struct sophia_of_record r = {0};
  r.header.kind = SOPHIA_OF_OUTCOME;
  r.value.outcome.transaction = transaction;
  r.value.outcome.topology_epoch = p->published_epoch;
  r.value.outcome.outcome = kind;
  record(p, &r);
}
static void events(struct peer *p) {
  uint32_t n = p->event_count;
  if (!p->events_pending || p->event_offset == p->journal_size)
    return;
  assert(p->event_offset < p->journal_size);
  if (p->event_chunk && n > p->event_chunk)
    n = p->event_chunk;
  data(p, p->event_tag, p->journal, p->journal_size, p->event_offset, n);
  p->events_pending = 0;
}
static unsigned file_name(const uint8_t *name, size_t n) {
  unsigned i;
  for (i = 0; i < sizeof(names) / sizeof(names[0]); ++i)
    if (strlen(names[i]) == n && !memcmp(name, names[i], n))
      return i + F_API;
  assert(!"unexpected file");
  return 0;
}
static uint64_t file_qid(struct peer *p, unsigned file) {
  return file == F_TOPOLOGY ? p->qid : file;
}
static void submit(struct peer *p, uint16_t tag, const uint8_t *d) {
  struct sophia_of_record c;
  uint64_t id = get(d + 8, 8);
  assert(get(d, 8) == EPOCH && get(d + 16, 4) == p->tx_size && !get(d + 20, 4));
  assert(!sophia_of_decode(p->tx, p->tx_size, &c) &&
         c.header.submission == id && c.header.epoch == EPOCH);
  ++p->submits;
  if (id == p->last_bytes_id) /* a retry must repeat the exact bytes */
    assert(p->tx_size == p->last_size && !memcmp(p->tx, p->last_bytes, p->last_size));
  else
    assert(id > p->last_submit_id);
  p->last_bytes_id = id;
  p->last_size = p->tx_size;
  memcpy(p->last_bytes, p->tx, p->tx_size);
  if (p->submit_again) {
    --p->submit_again;
    error(p, tag, 11);
    return;
  }
  if (p->submit_error) {
    error(p, tag, p->submit_error);
    p->submit_error = 0;
    p->last_submit_id = id;
    return;
  }
  p->last_submit_id = id;
  submitted(p, id, c.header.kind);
  if (c.header.kind == SOPHIA_OF_NEGOTIATE) {
    struct sophia_of_record r = {0};
    const struct sophia_of_negotiate *n = &c.value.negotiate;
    if (n->minimum_revision > 1 || n->maximum_revision < 1) {
      r.header.kind = SOPHIA_OF_REFUSED;
      r.value.refusal = SOPHIA_OF_UNSUPPORTED_REVISION;
      record(p, &r);
      p->refused = p->sequence;
    } else {
      r.header.kind = SOPHIA_OF_NEGOTIATED;
      r.value.granted_capabilities = n->capabilities & p->granted;
      record(p, &r);
      /* Faults the encoder refuses are patched into the encoded body. */
      if (p->extra_grant) {
        uint8_t *granted = p->journal + p->journal_size - 16;
        put(granted, get(granted, 8) | p->extra_grant, 8);
      }
      p->negotiated = 1;
      publish(p, 7);
    }
  } else if (p->outcome_after)
    outcome(p, c.value.proposal.transaction, (uint16_t)p->outcome_after);
  count(p, tag, 24);
}
static void request(struct peer *p, const uint8_t *m) {
  uint8_t type = m[4], b[64] = {0};
  uint16_t tag = (uint16_t)get(m + 5, 2);
  const uint8_t *q = m + 7;
  uint32_t fid = (uint32_t)get(q, 4);
  if (type == 100) {
    put(b, get(q, 4), 4);
    put(b + 4, 8, 2);
    memcpy(b + 6, "9P2000.L", 8);
    send_reply(p, 101, tag, b, 14);
    return;
  }
  assert(fid < 256);
  if (type == 104) {
    assert(!p->attaches++); /* one admitted attach per epoch */
    p->files[fid] = F_ROOT;
    b[0] = 0x80;
    put(b + 5, 1, 8);
    send_reply(p, 105, tag, b, 13);
  } else if (type == 110) {
    uint32_t target = (uint32_t)get(q + 4, 4);
    unsigned file;
    assert(p->files[fid] == F_ROOT && target < 256 && get(q + 8, 2) == 1);
    file = file_name(q + 12, (size_t)get(q + 10, 2));
    if (file == F_TOPOLOGY && !p->negotiated) {
      error(p, tag, 11);
      return;
    }
    p->files[target] = file;
    put(b, 1, 2);
    qid(b + 2, file_qid(p, file));
    send_reply(p, 111, tag, b, 15);
  } else if (type == 12) {
    unsigned file = p->files[fid];
    if (file == F_TX) {
      ++p->tx_opens;
      if (p->accepted > p->acked || p->tx_fid) {
        error(p, tag, 16);
        return;
      }
      p->tx_fid = fid;
      p->tx_size = 0;
    }
    if (file == F_TOPOLOGY) {
      ++p->topology_opens;
      if (!p->negotiated || p->topology_again) {
        if (p->topology_again)
          --p->topology_again;
        error(p, tag, 11);
        return;
      }
      if (p->topology_open) {
        error(p, tag, 16);
        return;
      }
      p->topology_open = fid;
    }
    qid(b, file_qid(p, file));
    send_reply(p, 13, tag, b, 17);
  } else if (type == 116) {
    unsigned file = p->files[fid];
    uint64_t offset = get(q + 4, 8);
    uint32_t n = (uint32_t)get(q + 12, 4);
    if (file == F_API)
      data(p, tag, (const uint8_t *)p->api, strlen(p->api), offset, n);
    else if (file == F_LIMITS)
      data(p, tag, p->limits, sizeof(p->limits), offset, n);
    else if (file == F_TOPOLOGY) {
      assert(p->topology_open == fid);
      if (p->object_chunk && n > p->object_chunk)
        n = p->object_chunk;
      if (offset == p->object_read)
        p->object_read += offset + n > p->object_size
                              ? p->object_size - offset
                              : n;
      data(p, tag, p->object, p->object_size, offset, n);
    } else {
      assert(file == F_EVENTS && !p->events_pending);
      p->events_pending = 1;
      p->event_tag = tag;
      p->event_offset = offset;
      p->event_count = n;
    }
  } else if (type == 118) {
    unsigned file = p->files[fid];
    uint64_t offset = get(q + 4, 8);
    uint32_t n = (uint32_t)get(q + 12, 4);
    const uint8_t *d = q + 16;
    if (file == F_TX) {
      assert(p->tx_fid == fid && offset == p->tx_size &&
             n <= sizeof(p->tx) - p->tx_size);
      memcpy(p->tx + p->tx_size, d, n);
      p->tx_size += n;
      count(p, tag, n);
    } else if (file == F_ACK) {
      uint64_t sequence = get(d + 8, 8);
      assert(!offset && n == 16 && get(d, 8) == EPOCH);
      assert(sequence >= p->acked && sequence <= p->sequence);
      ++p->acks;
      /* Premature acknowledgement of an announcement releases nothing. */
      if (sequence >= p->announced && p->announced > p->acked &&
          p->object_read < p->object_size) {
        error(p, tag, 11);
        return;
      }
      p->acked = sequence;
      count(p, tag, 16);
      /* The export revokes after the final ack of a refusal. */
      if (p->refused && sequence >= p->refused && p->events_pending) {
        error(p, p->event_tag, 5);
        p->events_pending = 0;
      }
    } else {
      assert(file == F_SUBMIT && !offset && n == 24);
      submit(p, tag, d);
    }
  } else if (type == 120) {
    if (p->files[fid] == F_TX && p->tx_fid == fid)
      p->tx_fid = 0;
    if (p->files[fid] == F_TOPOLOGY && p->topology_open == fid)
      p->topology_open = 0;
    p->files[fid] = 0;
    send_reply(p, 121, tag, NULL, 0);
  } else
    assert(!"unexpected 9P request");
}
static void pump(struct peer *p) {
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
    while (p->used >= 4 && get(p->input, 4) <= p->used) {
      size_t bytes = (size_t)get(p->input, 4);
      request(p, p->input);
      p->used -= bytes;
      memmove(p->input, p->input + bytes, p->used);
    }
  }
  events(p);
}

static struct rig *rig_new(uint64_t request, uint64_t granted,
                           uint16_t min_revision, uint16_t max_revision) {
  struct rig *r = calloc(1, sizeof(*r));
  struct sophia_os_config cfg;
  struct sophia_of_record limits = {0};
  size_t n;
  int sockets[2];
  assert(r && !socketpair(AF_UNIX, SOCK_STREAM, 0, sockets));
  r->client_fd = sockets[0];
  r->peer.fd = sockets[1];
  r->peer.api = "sophia-output-files version=1\n";
  r->peer.granted = granted;
  limits.header.kind = SOPHIA_OF_LIMITS;
  limits.header.epoch = EPOCH;
  limits.value.limits =
      (struct sophia_of_limits){64, 16384, 1784, 12000, 2000, 4096};
  assert(!sophia_of_encode(r->peer.limits, sizeof(r->peer.limits), &limits,
                           &n) &&
         n == SOPHIA_OF_LIMITS_RECORD);
  memset(&cfg, 0, sizeof(cfg));
  cfg.msize = 8192;
  cfg.offer.minimum_revision = min_revision;
  cfg.offer.maximum_revision = max_revision;
  cfg.offer.capabilities = request;
  cfg.bootstrap_deadline_ms = 10000;
  r->session = calloc(1, sophia_os_state_bytes());
  r->storage = malloc(sophia_os_storage_bytes(8192));
  assert(r->session && r->storage);
  assert(!sophia_os_open_fd(r->session, r->client_fd, &cfg, r->storage,
                            sophia_os_storage_bytes(8192), 0));
  return r;
}
static struct rig *rig(void) { return rig_new(3, 3, 1, 1); }
static int step(struct rig *r) {
  pump(&r->peer);
  return sophia_os_dispatch(r->session, POLLIN | POLLOUT, 65536, ++r->now);
}
/* Run until an application-visible event is ready (or the step bound). */
static const struct sophia_of_record *next_event(struct rig *r) {
  const struct sophia_of_record *e;
  unsigned i;
  for (i = 0; i < 400; ++i) {
    if (!sophia_os_event(r->session, &e))
      return e;
    assert(!step(r));
  }
  return NULL;
}
static void settle(struct rig *r, unsigned steps) {
  while (steps--)
    assert(!step(r));
}
static void ready(struct rig *r) {
  const struct sophia_of_record *e = next_event(r);
  assert(e && e->header.kind == SOPHIA_OF_OBJECT_PUBLISHED);
  assert(sophia_os_state(r->session) == SOPHIA_OS_READY);
  assert(sophia_os_epoch(r->session) == EPOCH);
  assert(sophia_os_capabilities(r->session) == 3);
  assert(sophia_os_topology(r->session) &&
         sophia_os_topology(r->session)->topology_epoch == 7);
  assert(!sophia_os_consume(r->session));
  settle(r, 10);
  assert(r->peer.acked == r->peer.sequence && r->peer.acked == 3);
}
static void drop(struct rig *r) {
  sophia_os_close(r->session);
  close(r->client_fd);
  close(r->peer.fd);
  free(r->storage);
  free(r->session);
  free(r);
}
static struct sophia_of_proposal proposal(uint64_t transaction) {
  struct sophia_of_proposal v;
  memset(&v, 0, sizeof(v));
  v.transaction = transaction;
  v.base_topology_epoch = 7;
  v.intent = SOPHIA_OF_APPLY;
  v.head_count = v.group_count = 1;
  v.heads[0] = (struct sophia_of_head_target){3, 4, 6, SOPHIA_OF_NORMAL,
                                              SOPHIA_OF_VRR_DISABLED};
  v.groups[0].output = 2;
  v.groups[0].width = 1280;
  v.groups[0].height = 720;
  v.groups[0].member_count = 1;
  v.groups[0].members[0] = (struct sophia_of_member){3, SOPHIA_OF_EXACT};
  return v;
}
static enum sophia_os_custody custody(struct rig *r, uint64_t ticket,
                                      uint32_t *error_out) {
  enum sophia_os_custody c;
  uint32_t e;
  assert(!sophia_os_outcome(r->session, ticket, &c, &e));
  if (error_out)
    *error_out = e;
  return c;
}

static void bootstrap_and_topology(void) {
  struct rig *r = rig();
  /* Small reads split both records and the object arbitrarily. */
  r->peer.event_chunk = 7;
  r->peer.object_chunk = 13;
  r->peer.topology_again = 1;
  ready(r);
  assert(r->peer.topology_opens == 2 && !r->peer.topology_open);
  assert(sophia_os_limits(r->session)->ack_timeout_ms == 2000);
  assert(!strcmp(sophia_os_topology(r->session)->heads[0].label, "DP-1"));
  drop(r);
}

static void proposal_outcome_and_republication(void) {
  struct rig *r = rig();
  const struct sophia_of_record *e;
  struct sophia_of_proposal v;
  uint64_t ticket, transaction;
  ready(r);
  assert(!sophia_os_next_transaction(r->session, &transaction) &&
         transaction == 1);
  v = proposal(transaction);
  r->peer.outcome_after = SOPHIA_OF_COMMITTED;
  assert(!sophia_os_submit(r->session, &v, r->now + 1000, &ticket));
  assert(sophia_os_submit(r->session, &v, r->now + 1000, &ticket) ==
         SOPHIA_9P_BUSY);
  e = next_event(r);
  assert(e && e->header.kind == SOPHIA_OF_OUTCOME &&
         e->value.outcome.transaction == 1 &&
         e->value.outcome.outcome == SOPHIA_OF_COMMITTED);
  assert(custody(r, ticket, NULL) == SOPHIA_OS_SUBMITTED);
  assert(!sophia_os_consume(r->session));
  settle(r, 10);
  /* The commit republishes: the SDK fetches the new object by its Qid. */
  publish(&r->peer, 8);
  e = next_event(r);
  assert(e && e->header.kind == SOPHIA_OF_OBJECT_PUBLISHED &&
         e->value.published.qid_path == 98);
  assert(sophia_os_topology(r->session)->topology_epoch == 8);
  assert(!sophia_os_consume(r->session));
  settle(r, 10);
  assert(r->peer.acked == r->peer.sequence);
  /* A second proposal reopens the transaction only after the ack. */
  r->peer.outcome_after = SOPHIA_OF_VALIDATED;
  v = proposal(5);
  v.intent = SOPHIA_OF_VALIDATE_ONLY;
  assert(!sophia_os_submit(r->session, &v, r->now + 1000, &ticket));
  e = next_event(r);
  assert(e && e->value.outcome.transaction == 5);
  assert(!sophia_os_next_transaction(r->session, &transaction) &&
         transaction == 6);
  assert(r->peer.tx_opens == 3); /* negotiate + two proposals, no EBUSY */
  drop(r);
}

static void retry_refusal_and_capacity(void) {
  struct rig *r = rig();
  struct sophia_of_proposal v = proposal(9);
  const struct sophia_of_record *e;
  uint64_t first, second;
  uint32_t wire;
  ready(r);
  r->peer.submit_again = 2; /* journal pressure: nothing transferred */
  r->peer.outcome_after = SOPHIA_OF_REJECTED;
  assert(!sophia_os_submit(r->session, &v, r->now + 5000, &first));
  e = next_event(r);
  assert(e && e->value.outcome.outcome == SOPHIA_OF_REJECTED);
  assert(r->peer.submits == 4 && custody(r, first, NULL) == SOPHIA_OS_SUBMITTED);
  assert(!sophia_os_consume(r->session));
  settle(r, 5);
  /* Domain history exhaustion refuses this ticket; the attach continues. */
  r->peer.submit_error = 28;
  v = proposal(10);
  assert(!sophia_os_submit(r->session, &v, r->now + 1000, &second));
  settle(r, 20);
  assert(custody(r, second, &wire) == SOPHIA_OS_REFUSED_SUBMIT && wire == 28);
  assert(sophia_os_state(r->session) == SOPHIA_OS_READY);
  v = proposal(11);
  r->peer.outcome_after = SOPHIA_OF_COMMITTED;
  assert(!sophia_os_submit(r->session, &v, r->now + 1000, &second));
  e = next_event(r);
  assert(e && e->value.outcome.transaction == 11);
  drop(r);
}

static void observe_only_and_refusal(void) {
  struct rig *r = rig_new(3, 1, 1, 1);
  struct sophia_of_proposal v = proposal(1);
  uint64_t ticket;
  {
    const struct sophia_of_record *e = next_event(r);
    assert(e && sophia_os_capabilities(r->session) == 1);
  }
  assert(sophia_os_submit(r->session, &v, r->now + 1000, &ticket) ==
         SOPHIA_9P_ARGUMENT);
  drop(r);
  /* An unsupported revision range is refused; the record is acknowledged
   * and the session ends without submitting again. */
  r = rig_new(3, 3, 2, 3);
  {
    unsigned i;
    for (i = 0; i < 400 && !step(r); ++i)
      ;
  }
  assert(sophia_os_state(r->session) == SOPHIA_OS_REFUSED);
  assert(sophia_os_refusal(r->session) == SOPHIA_OF_UNSUPPORTED_REVISION);
  assert(r->peer.acked == r->peer.sequence && r->peer.submits == 1);
  assert(sophia_os_remote_error(r->session) == 5); /* revoked read */
  assert(step(r) == SOPHIA_9P_CLOSED);
  assert(sophia_os_state(r->session) == SOPHIA_OS_REFUSED);
  drop(r);
}

static int fails(struct rig *r) {
  unsigned i;
  for (i = 0; i < 400; ++i)
    if (step(r))
      return sophia_os_state(r->session) == SOPHIA_OS_FAILED;
  return 0;
}

static void protocol_faults(void) {
  struct rig *r = rig();
  r->peer.wrong_qid = 1; /* announcement names another object */
  assert(fails(r) && r->peer.acked < r->peer.announced);
  drop(r);
  r = rig();
  r->peer.extra_grant = 4; /* unknown granted bit */
  assert(fails(r));
  drop(r);
  r = rig_new(1, 3, 1, 1);
  r->peer.extra_grant = 2; /* grant beyond the request */
  assert(fails(r));
  drop(r);
  r = rig();
  r->peer.api = "sophia-output-files version=2\n";
  assert(fails(r));
  drop(r);
  r = rig();
  r->peer.api = "sophia-wm-files version=1 output_transport=current_ipc\n";
  assert(fails(r));
  drop(r);
}

static void arguments(void) {
  struct sophia_os_config cfg;
  void *state = calloc(1, sophia_os_state_bytes());
  void *storage = malloc(sophia_os_storage_bytes(8192));
  memset(&cfg, 0, sizeof(cfg));
  cfg.msize = 8192;
  cfg.offer.minimum_revision = cfg.offer.maximum_revision = 1;
  cfg.offer.capabilities = SOPHIA_OF_CAP_CONFIGURE; /* observe is required */
  cfg.bootstrap_deadline_ms = 10;
  assert(sophia_os_open_fd(state, 3, &cfg, storage,
                           sophia_os_storage_bytes(8192), 0) ==
         SOPHIA_9P_ARGUMENT);
  assert(!sophia_os_storage_bytes(70000));
  free(state);
  free(storage);
}

int main(void) {
  bootstrap_and_topology();
  proposal_outcome_and_republication();
  retry_refusal_and_capacity();
  observe_only_and_refusal();
  protocol_faults();
  arguments();
  puts("output_session_test: ok");
  return 0;
}

/* Proposed x6jp929h lifecycle cases: discovery, snapshots, readiness, channel
 * joining, offer IDs and source sending. Fixture scope is stated in
 * image_lifecycle_fixture.h; no production endpoint, auth or GPU is claimed. */
#include "image_lifecycle_producer.h"

struct scripted {
  struct objects sets[4];
  unsigned count, reads;
};
static int read_scripted(void *context, struct objects *out) {
  struct scripted *s = context;
  *out = s->sets[s->reads < s->count ? s->reads : s->count - 1];
  s->reads++;
  return 0;
}

static void optional_discovery(void) {
  struct owner o;
  struct producer p;
  struct scripted s;
  int errors[3] = {EACCES, ESTALE, EIO};
  unsigned i;
  owner_init(&o, 1, 2, 16);
  memset(&s, 0, sizeof(s));
  s.count = 1;
  s.sets[0].error = ENOENT;
  producer_init(&p, 1000, 1);
  assert(producer_discover(&p, read_scripted, &s, 1, 0) == D_ABSENT);
  assert(producer_path(&p) == PATH_BYTES);
  producer_init(&p, 1000, 0);
  assert(producer_discover(&p, read_scripted, &s, 1, 0) == D_ERROR);
  assert(producer_path(&p) == PATH_ERROR);
  for (i = 0; i < 3; i++) { /* denied and stale never downgrade */
    s.reads = 0;
    s.sets[0].error = errors[i];
    producer_init(&p, 1000, 1);
    assert(producer_discover(&p, read_scripted, &s, 1, 0) == D_ERROR);
    assert(producer_path(&p) == PATH_ERROR);
  }
  owner_open(&o, &s.sets[0]);
  s.sets[0].text[s.sets[0].text_length - 2] = 'x'; /* malformed limits_generation */
  s.reads = 0;
  producer_init(&p, 1000, 1);
  assert(producer_discover(&p, read_scripted, &s, 1, 0) == D_ERROR);
  assert(producer_path(&p) == PATH_ERROR && p.channel == PC_NONE);
  assert(owner_byte_submit(&o) == 0); /* legacy bytes are unaffected */
}

static void snapshot_replacement(void) {
  struct owner o;
  struct producer p;
  struct scripted s;
  struct objects fid, again, fresh;
  int fds[2];
  owner_init(&o, 1, 2, 16);
  owner_open(&o, &fid);
  o.formats_gen = 10; /* a format-only change bumps topology and formats */
  o.topology = 10;
  owner_open(&o, &fresh);
  again = fid; /* one open fid re-read */
  assert(!memcmp(&again, &fid, sizeof(fid)) && get(fid.formats, 40, 8) == 9);
  assert(get(fresh.formats, 40, 8) == 10);
  /* Two consecutive mismatched sets: discovery from the old fid, formats from
   * the new one. One complete set per caller pass, the first included. */
  memset(&s, 0, sizeof(s));
  s.count = 2;
  s.sets[0] = fid;
  memcpy(s.sets[0].formats, fresh.formats, 64);
  s.sets[1] = s.sets[0];
  producer_init(&p, 100, 1);
  assert(producer_discover(&p, read_scripted, &s, 1, 0) == D_RETRY && s.reads == 1);
  assert(producer_discover(&p, read_scripted, &s, 1, 10) == D_RETRY && s.reads == 1);
  assert(p.channel == PC_NONE);
  assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_NONBLOCK | SOCK_CLOEXEC, 0, fds) == 0);
  assert(producer_hello(&p, fds[0]) == -EINVAL); /* mixed snapshots never Hello */
  assert(owner_accept(&o, fds[1]) == A_WAIT);
  assert(producer_discover(&p, read_scripted, &s, 2, 50) == D_RETRY && s.reads == 2);
  assert(producer_discover(&p, read_scripted, &s, 3, 100) == D_DEADLINE && s.reads == 2);
  /* A coherent fresh set on the next pass succeeds. */
  s.reads = 0;
  s.sets[1] = fresh;
  producer_init(&p, 100, 1);
  assert(producer_discover(&p, read_scripted, &s, 1, 0) == D_RETRY && s.reads == 1);
  assert(producer_discover(&p, read_scripted, &s, 2, 1) == D_READY && s.reads == 2);
  assert(p.formats_gen == 10);
  close(fds[0]);
  close(fds[1]);
}

static void readiness_race(void) {
  struct owner o;
  struct producer p;
  struct live_owner l;
  struct event stale;
  struct image img;
  uint64_t id;
  int fds[2], sock, fd = sealed_image(64, 64, REQUIRED_SEALS, 255);
  unsigned char meta[META_BYTES];
  owner_init(&o, 1, 2, 16);
  producer_init(&p, 1000, 1);
  l.owner = &o;
  l.reads = 0;
  assert(producer_discover(&p, read_live, &l, 1, 0) == D_READY);
  img = image_for(&o, 40, 64, 64);
  assert(producer_begin(&p, &img, fd, 0, &id) == -EAGAIN); /* before Hello */
  img.offer = 1;
  meta_encode(meta, &img);
  assert(owner_begin(&o, meta, 0) == B_UNAVAILABLE && o.mark == 0);
  assert(count_events(&o, EV_STATUS, 1) == 1);
  producer_pump(&p, &o);
  assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_NONBLOCK | SOCK_CLOEXEC, 0, fds) == 0);
  assert(producer_hello(&p, fds[0]) == 0);
  assert(producer_begin(&p, &img, fd, 0, &id) == -EAGAIN); /* Hello sent, not Ready */
  memset(&stale, 0, sizeof(stale));
  stale.kind = EV_READY;
  stale.id = 1; /* an older generation's Ready */
  producer_event(&p, &stale);
  assert(p.channel == PC_HELLO_SENT && p.ignored_events == 1);
  assert(owner_accept(&o, fds[1]) == A_JOINED);
  producer_pump(&p, &o);
  assert(p.channel == PC_READY);
  stale.kind = EV_CLOSED; /* an older generation's Closed cannot close this one */
  producer_event(&p, &stale);
  assert(p.channel == PC_READY && p.ignored_events == 2);
  sock = fds[0];
  assert(producer_begin(&p, &img, fd, 0, &id) == 0 && id == 1);
  assert(owner_begin(&o, producer_find(&p, id)->meta, 0) == B_ADMITTED);
  close(sock);
  close(fd);
  owner_end_epoch(&o);
}

static void duplicate_channel_and_bad_hello(void) {
  struct owner o;
  struct producer p;
  unsigned char hello[HELLO_BYTES];
  int other[2], sock;
  unsigned emitted, i;
  uint64_t bad[3][2] = {{9, 2}, {1, 7}, {1, 2}}; /* foreign epoch, stale gen, foreign peer */
  owner_init(&o, 1, 2, 16);
  /* Before any channel exists, each bad Hello is refused alone: no event, no
   * generation spent, and the valid Hello that follows still joins. */
  for (i = 0; i < 3; i++) {
    assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_NONBLOCK | SOCK_CLOEXEC, 0, other) == 0);
    header_encode(hello, HELLO_BYTES, KIND_HELLO, 0, bad[i][0], bad[i][1], 0);
    assert(send_packet(other[0], hello, HELLO_BYTES, 0, 0) == HELLO_BYTES);
    o.assignee = i == 2 ? getpid() + 1 : getpid();
    assert(owner_accept(&o, other[1]) == A_REJECTED);
    assert(o.channel == CH_NONE && o.gen == 2 && o.journal.emitted == 0);
    assert(o.journal.reserved == 0);
    close(other[0]);
  }
  o.assignee = getpid();
  producer_init(&p, 1000, 1);
  sock = connect_ready(&o, &p);
  emitted = o.journal.emitted;
  /* A valid second channel is refused, never a replacement, and closing the
   * refused socket cannot close the live one. */
  assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_NONBLOCK | SOCK_CLOEXEC, 0, other) == 0);
  header_encode(hello, HELLO_BYTES, KIND_HELLO, 0, 1, 2, 0);
  assert(send_packet(other[0], hello, HELLO_BYTES, 0, 0) == HELLO_BYTES);
  assert(owner_accept(&o, other[1]) == A_REJECTED);
  { /* only the refused socket closed; unread data reports a reset */
    ssize_t r = recv(other[0], hello, 1, MSG_DONTWAIT);
    assert(r == 0 || (r == -1 && errno == ECONNRESET));
  }
  close(other[0]);
  assert(o.channel == CH_LIVE && o.journal.emitted == emitted);
  assert(owner_receive(&o) == RX_NONE);
  /* A joined channel's protocol violation, here a second Hello, closes it
   * with reason 2 from the reserved slot. */
  header_encode(hello, HELLO_BYTES, KIND_HELLO, 0, 1, 2, 0);
  assert(send_packet(sock, hello, HELLO_BYTES, 0, 0) == HELLO_BYTES);
  assert(owner_receive(&o) == RX_FAULT && o.channel == CH_DISABLED);
  assert(count_events(&o, EV_CLOSED, 2) == 1 && o.journal.reserved == 0);
  producer_pump(&p, &o);
  assert(p.channel == PC_CLOSED && producer_path(&p) == PATH_BYTES);
  assert(owner_byte_submit(&o) == 0);
  close(sock);
}

static void offer_ids(void) {
  struct owner o;
  struct producer p;
  struct image img;
  unsigned char meta[META_BYTES];
  uint64_t a, b, c;
  int sock, fd = sealed_image(64, 64, REQUIRED_SEALS, 255);
  unsigned i;
  owner_init(&o, 1, 2, 64);
  producer_init(&p, 1000, 1);
  sock = connect_ready(&o, &p);
  a = admit(&o, &p, 40, fd, 0);
  memcpy(meta, producer_find(&p, a)->meta, META_BYTES); /* a retry keeps identity */
  assert(owner_begin(&o, meta, 0) == B_ORDER && o.mark == a);
  img = image_for(&o, 41, 64, 64);
  assert(producer_begin(&p, &img, fd, 0, &b) == 0 && b > a);
  for (i = 1; i < MAX_OFFERS; i++) o.offers[i].state = O_WAITING; /* fill the slots */
  assert(owner_begin(&o, producer_find(&p, b)->meta, 0) == B_CAPACITY && o.mark == a);
  for (i = 1; i < MAX_OFFERS; i++) o.offers[i].state = O_FREE;
  assert(owner_begin(&o, producer_find(&p, b)->meta, 0) == B_ADMITTED && o.mark == b);
  producer_pump(&p, &o);
  /* An ID below the mark that the owner never admitted is refused. */
  img = image_for(&o, 42, 64, 64);
  img.offer = a; /* a stale caller-supplied identity */
  meta_encode(meta, &img);
  assert(owner_begin(&o, meta, 0) == B_ORDER);
  /* A fresh attempt after a refusal uses a greater ID; the refused one is spent. */
  img = image_for(&o, 43, 64, 64);
  assert(producer_begin(&p, &img, fd, 0, &c) == 0 && c > b);
  producer_refused(&p, c);
  assert(producer_begin(&p, &img, fd, 0, &c) == 0 && c == b + 2);
  producer_refused(&p, c);
  /* Exhaustion requires a new connection, never wraparound. */
  p.next_id = UINT64_MAX;
  assert(producer_begin(&p, &img, fd, 0, &c) == 0 && c == UINT64_MAX);
  producer_refused(&p, c);
  assert(p.next_id == 0);
  assert(producer_begin(&p, &img, fd, 0, &c) == -ECONNRESET);
  assert(owner_byte_submit(&o) == 0);
  close(sock);
  close(fd);
  owner_end_epoch(&o);
}

static ssize_t send_eagain(int s, const void *b, size_t n, const int *f, unsigned c) {
  (void)s; (void)b; (void)n; (void)f; (void)c;
  errno = EAGAIN;
  return -1;
}

static void source_send(void) {
  struct owner o;
  struct producer p;
  struct image img;
  uint64_t id, unsent;
  int sock, fd = sealed_image(64, 64, REQUIRED_SEALS, 255);
  unsigned before = fd_count();
  owner_init(&o, 1, 2, 64);
  producer_init(&p, 10000, 1);
  sock = connect_ready(&o, &p);
  img = image_for(&o, 40, 64, 64);
  assert(producer_begin(&p, &img, fd, 0, &id) == 0);
  assert(producer_send(&p, id, 0) == -EINVAL && p.sends == 0); /* not admitted */
  assert(owner_receive(&o) == RX_NONE);
  assert(owner_begin(&o, producer_find(&p, id)->meta, 0) == B_ADMITTED);
  producer_pump(&p, &o);
  assert(producer_send(&p, id, 0) == 0);
  assert(!producer_may_reuse(&p, id) && producer_find(&p, id)->status == 0);
  assert(producer_send(&p, id, 0) == -EALREADY); /* at most one packet */
  assert(owner_receive(&o) == RX_RECEIVED);
  assert(fcntl(o.offers[0].fd, F_GETFD) & FD_CLOEXEC);
  while (owner_work(&o, 1)) {}
  producer_pump(&p, &o);
  assert(producer_find(&p, id)->status == ST_ACCEPTED && producer_may_reuse(&p, id));
  assert(producer_find(&p, id)->disposition == DISP_RETAINED);
  producer_forget(&p, id);
  /* EAGAIN keeps the unsent offer and asks for a writable wake; the deadline
   * accessor bounds the sleep; a later wake sends the same identity. */
  unsent = admit(&o, &p, 41, fd, 100);
  p.send = send_eagain;
  assert(producer_send(&p, unsent, 0) == -EAGAIN);
  assert(producer_find(&p, unsent)->state == P_ADMITTED && producer_wants_writable(&p));
  assert(producer_next_deadline(&p) == 100 + TRANSFER_MS);
  p.send = send_packet;
  assert(producer_send(&p, unsent, 0) == 0 && !producer_wants_writable(&p));
  assert(owner_receive(&o) == RX_RECEIVED && get(o.offers[0].meta, 0, 8) == unsent);
  /* A stalled unsent packet stops at the transfer deadline and never renews it. */
  id = admit(&o, &p, 42, fd, 200);
  p.send = send_eagain;
  assert(producer_send(&p, id, 0) == -EAGAIN);
  producer_tick(&p, 200 + TRANSFER_MS);
  assert(producer_find(&p, id)->state == P_EXPIRED && !producer_wants_writable(&p));
  assert(producer_send(&p, id, 0) == -EALREADY);
  owner_tick(&o, 200 + TRANSFER_MS);
  producer_pump(&p, &o);
  assert(producer_find(&p, id)->disposition == DISP_BEFORE_USE && producer_may_reuse(&p, id));
  close(sock);
  owner_end_epoch(&o);
  close(fd);
  assert(fd_count() + 1 == before);
}

static void ambiguous_send(void) {
  struct owner o;
  struct producer p;
  uint64_t id;
  int sock, fd = sealed_image(64, 64, REQUIRED_SEALS, 255);
  owner_init(&o, 1, 2, 64);
  producer_init(&p, 10000, 1);
  sock = connect_ready(&o, &p);
  /* The packet left, but the caller lost the outcome. */
  id = admit(&o, &p, 40, fd, 0);
  assert(producer_send(&p, id, 1) == -EIO);
  assert(producer_send(&p, id, 0) == -EALREADY && p.sends == 1); /* no blind replay */
  assert(!producer_may_reuse(&p, id));
  assert(owner_receive(&o) == RX_RECEIVED);
  while (owner_work(&o, 1)) {}
  producer_pump(&p, &o);
  assert(producer_may_reuse(&p, id) && producer_find(&p, id)->status == ST_ACCEPTED);
  producer_forget(&p, id);
  /* Outcome lost and the journal lost too: custody is unknown for good. */
  id = admit(&o, &p, 41, fd, 0);
  assert(producer_send(&p, id, 1) == -EIO);
  producer_journal_lost(&p);
  assert(producer_find(&p, id)->state == P_UNKNOWN && !producer_may_reuse(&p, id));
  assert(producer_send(&p, id, 0) == -EALREADY && p.sends == 2);
  assert(producer_path(&p) == PATH_ERROR); /* reconnect is the application's */
  close(sock);
  owner_end_epoch(&o);
  close(fd);
}

int main(void) {
  unsigned before = fd_count();
  for (fixture_role = ROLE_SHELL; fixture_role <= ROLE_LOCK; fixture_role++) {
    optional_discovery();
    snapshot_replacement();
    readiness_race();
    duplicate_channel_and_bad_hello();
    offer_ids();
    source_send();
    ambiguous_send();
    assert(fd_count() == before);
  }
  puts("descriptor lifecycle fixture: 7 cases passed for shell and lock roles");
  return 0;
}

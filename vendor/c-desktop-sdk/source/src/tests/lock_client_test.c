/* Lock client against a scripted 9P2000.L peer, not a production Session.
 * The peer follows the export's custody rules (spec/sophia-lock-files.md):
 * Negotiate journals Submitted, Negotiated and the lock's ObjectPublished;
 * the lock file always names the newest object; an upload writer is bound to
 * an admitted slot and writes at its cursor; a refused submit journals
 * nothing; the final ack of a refusal revokes the connection. */
#include "lock_client_peer.h"

/* Consumer link control: an application may define these generic names. The
 * library's internals must not export them, or this test would fail to link
 * (multiple definition) or call one of them below and abort. */
int lc_queue(void) { abort(); }
int lc_parse(void) { abort(); }
int lc_object_drive(void) { abort(); }
int lc_object_reply(void) { abort(); }
int lc_upload_drive(void) { abort(); }
int lc_upload_reply(void) { abort(); }
void lc_upload_status(void) { abort(); }
void lc_upload_refused(void) { abort(); }

static void bootstrap_and_lock(void) {
  struct rig *r = ready();
  const struct sophia_lf_record *e;
  const struct sophia_lf_lock *lock;
  uint64_t generation;
  publish(&r->peer, SOPHIA_LF_LOCKED, 5, 2);
  e = next(r);
  assert(e->header.kind == SOPHIA_LF_OBJECT_PUBLISHED);
  lock = sophia_lc_lock(&r->client, &generation);
  assert(generation == 2 && lock->lock_epoch == 5 && lock->allocation_count == 2);
  assert(lock->allocations[1].allocation == 42);
  /* Not acknowledged until consumed. */
  assert(r->peer.acked < r->peer.sequence);
  consume(r);
  assert(r->peer.acked == r->peer.sequence);
  rig_free(r);
}

/* An announcement whose object was already replaced is consumed unread. */
static void superseded_announcement(void) {
  struct rig *r = ready();
  const struct sophia_lf_record *e;
  uint64_t generation;
  unsigned reads = r->peer.lock_reads;
  publish(&r->peer, SOPHIA_LF_LOCKING, 5, 1);
  publish(&r->peer, SOPHIA_LF_LOCKED, 5, 1);
  e = next(r);
  assert(e->header.kind == SOPHIA_LF_OBJECT_PUBLISHED &&
         e->value.published.object_generation == 3);
  assert(sophia_lc_lock(&r->client, &generation)->phase == SOPHIA_LF_LOCKED &&
         generation == 3);
  assert(r->peer.lock_reads == reads + 2); /* one record read, one EOF probe */
  consume(r);
  rig_free(r);
}

static void rejects_invalid_lock_fetch(void) {
  unsigned fault;
  for (fault = 0; fault < 4; ++fault) {
    struct rig *r = ready();
    publish(&r->peer, SOPHIA_LF_LOCKED, 5, 1);
    switch (fault) {
    case 0: r->peer.lock[8] = EPOCH + 1; break;
    case 1: r->peer.lock[r->peer.lock_size++] = 0; break;
    case 2: r->peer.lock_size = 0; break;
    default: --r->peer.qid; break;
    }
    assert(spin(r) == SOPHIA_9P_INVALID);
    assert(sophia_lc_state(&r->client) == SOPHIA_LC_FAILED);
    rig_free(r);
  }
}

static void entry_and_chord(void) {
  struct rig *r = ready();
  struct sophia_lf_record v;
  const struct sophia_lf_record *e;
  memset(&v, 0, sizeof(v));
  v.value.entry.lock_epoch = 5;
  v.value.entry.entry = SOPHIA_LF_INSERT;
  event(&r->peer, SOPHIA_LF_ENTRY, &v);
  memset(&v, 0, sizeof(v));
  v.value.chord.lock_epoch = 5;
  v.value.chord.chord = 1;
  event(&r->peer, SOPHIA_LF_CHORD, &v);
  e = next(r);
  assert(e->header.kind == SOPHIA_LF_ENTRY && e->value.entry.entry == SOPHIA_LF_INSERT);
  consume(r);
  e = next(r);
  assert(e->header.kind == SOPHIA_LF_CHORD && e->value.chord.chord == 1);
  consume(r);
  /* A chord ID beyond the grant is a protocol violation. */
  v.value.chord.chord = 2;
  event(&r->peer, SOPHIA_LF_CHORD, &v);
  assert(spin(r) == SOPHIA_9P_INVALID &&
         sophia_lc_state(&r->client) == SOPHIA_LC_FAILED);
  rig_free(r);
}

static void demand_and_present_records(void) {
  struct rig *r = ready();
  struct sophia_lf_record v;
  const struct sophia_lf_record *e;
  /* This scripted peer tests record routing, not image acceptance. Actual
   * descriptor transfer and settlement are covered by lock_image_test. */
  publish(&r->peer, SOPHIA_LF_LOCKED, 5, 1);
  next(r);
  consume(r);
  memset(&v, 0, sizeof(v));
  v.header.kind = SOPHIA_LF_FRAME_DEMAND;
  v.value.frame_demand.transaction = 32;
  v.value.frame_demand.lock_epoch = 5;
  v.value.frame_demand.allocation = 41;
  v.value.frame_demand.allocation_generation = 4;
  v.value.frame_demand.demand = 3;
  assert(!sophia_lc_submit(&r->client, &v));
  e = next(r);
  assert(e->header.kind == SOPHIA_LF_FRAME_PERMIT &&
         e->value.frame_permit.pacing_permit == 77);
  consume(r);
  memset(&v, 0, sizeof(v));
  v.header.kind = SOPHIA_LF_CANDIDATE;
  v.value.candidate.transaction = 33;
  v.value.candidate.lock_epoch = 5;
  v.value.candidate.output = 1;
  v.value.candidate.output_generation = 3;
  v.value.candidate.allocation = 41;
  v.value.candidate.allocation_generation = 4;
  v.value.candidate.candidate_generation = 1;
  v.value.candidate.pacing_permit = 77;
  v.value.candidate.resource = (struct sophia_lf_resource){21, 2};
  assert(!sophia_lc_submit(&r->client, &v));
  e = next(r);
  assert(e->header.kind == SOPHIA_LF_CANDIDATE_OUTCOME &&
         e->value.candidate_outcome.status == SOPHIA_LF_PRESENTED);
  consume(r);
  /* Retired byte records cannot enter the session. */
  v.header.kind = SOPHIA_LF_RESOURCE_BEGIN;
  assert(sophia_lc_submit(&r->client, &v) == SOPHIA_9P_ARGUMENT);
  rig_free(r);
}

static void byte_records_are_refused(void) {
  struct rig *r = ready();
  struct sophia_lf_record v = {0};
  uint64_t next = r->client.next_submission;
  unsigned i;
  const uint16_t kinds[] = {SOPHIA_LF_RESOURCE_BEGIN, SOPHIA_LF_RESOURCE_END};
  for (i = 0; i < 2; i++) {
    v.header.kind = kinds[i];
    assert(sophia_lc_submit(&r->client, &v) == SOPHIA_9P_ARGUMENT);
    assert(r->client.next_submission == next);
  }
  rig_free(r);
}

static void negotiation_retry_with_six_requests(void) {
  struct sophia_lf_negotiate o = offer();
  struct rig *r = rig_new_capacity(&o, NULL, 6);
  unsigned submits;
  struct sophia_lc_obligations obligations;
  r->peer.submit_again = 1;
  assert(!spin(r));
  assert(!sophia_lc_ready(&r->client));
  assert(sophia_lc_obligations(NULL, &obligations) == SOPHIA_9P_ARGUMENT);
  assert(sophia_lc_obligations(&r->client, NULL) == SOPHIA_9P_ARGUMENT);
  assert(!sophia_lc_obligations(&r->client, &obligations));
  assert(obligations.waiting_for_retry);
  submits = r->peer.submits;
  assert(!spin(r) && r->peer.submits == submits);
  assert(!sophia_lc_submit_retry(&r->client));
  assert(next(r)->header.kind == SOPHIA_LF_OBJECT_PUBLISHED);
  consume(r);
  assert(sophia_lc_ready(&r->client));
  assert(!sophia_lc_obligations(&r->client, &obligations));
  assert(!obligations.waiting_for_retry && obligations.acked == obligations.consumed);
  rig_free(r);
}

static void again_then_retry(void) {
  struct rig *r = ready();
  struct sophia_lf_record v;
  enum sophia_lc_submission stage;
  uint64_t id;
  unsigned submits;
  memset(&v, 0, sizeof(v));
  v.header.kind = SOPHIA_LF_RESOURCE_RETIRE;
  v.value.resource_step.transaction = 40;
  v.value.resource_step.resource.id = 21;
  v.value.resource_step.resource.generation = 2;
  r->peer.submit_again = 1;
  assert(sophia_lc_submit_retry(&r->client) == SOPHIA_9P_ARGUMENT);
  assert(!sophia_lc_submit(&r->client, &v));
  assert(!spin(r));
  submits = r->peer.submits;
  assert(!sophia_lc_submission(&r->client, &id, &stage, NULL));
  assert(id == 2 && stage == SOPHIA_LC_SUBMISSION_STAGED);
  assert(sophia_lc_submit(&r->client, &v) == SOPHIA_9P_BUSY);
  assert(!spin(r) && r->peer.submits == submits); /* waits for the caller */
  assert(!sophia_lc_submit_retry(&r->client));
  assert(next(r)->header.kind == SOPHIA_LF_RESOURCE_RELEASED);
  assert(!sophia_lc_submission(&r->client, &id, &stage, NULL));
  assert(id == 2 && stage == SOPHIA_LC_SUBMISSION_CUSTODIED);
  consume(r);
  rig_free(r);
}

static void contradictory_retry_preserves_custody(void) {
  struct rig *r = ready();
  struct sophia_lf_record v = {0};
  enum sophia_lc_submission stage;
  uint64_t id;
  v.header.kind = SOPHIA_LF_RESOURCE_RETIRE;
  v.value.resource_step.transaction = 40;
  v.value.resource_step.resource.id = 21;
  v.value.resource_step.resource.generation = 2;
  r->peer.error_after_custody = 11;
  assert(!sophia_lc_submit(&r->client, &v));
  assert(spin(r));
  assert(sophia_lc_remote_error(&r->client) == 11);
  assert(!sophia_lc_submission(&r->client, &id, &stage, NULL));
  assert(id == 2 && stage == SOPHIA_LC_SUBMISSION_CUSTODIED);
  assert(sophia_lc_submit_retry(&r->client));
  rig_free(r);
}

static void release_ack(struct peer *p) {
  assert(p->held_sequence);
  p->hold_acks = 0;
  p->acked = p->held_sequence;
  p->held_sequence = 0;
  count(p, p->held_tag, 16);
}
/* The transaction reopens only once the ack covering its Submitted was
 * answered, even when an earlier ack is still in flight. */
static void custody_waits_for_ack(void) {
  struct rig *r = ready();
  struct sophia_lf_record v;
  enum sophia_lc_submission stage;
  uint64_t id;
  r->peer.hold_acks = 1;
  publish(&r->peer, SOPHIA_LF_LOCKED, 5, 1);
  next(r);
  consume(r);
  assert(r->peer.held_sequence && r->peer.acked < r->peer.held_sequence);
  memset(&v, 0, sizeof(v));
  v.header.kind = SOPHIA_LF_RESOURCE_RETIRE;
  v.value.resource_step.transaction = 40;
  v.value.resource_step.resource.id = 21;
  v.value.resource_step.resource.generation = 2;
  assert(!sophia_lc_submit(&r->client, &v));
  assert(next(r)->header.kind == SOPHIA_LF_RESOURCE_RELEASED);
  assert(!sophia_lc_submission(&r->client, &id, &stage, NULL));
  assert(stage == SOPHIA_LC_SUBMISSION_CUSTODIED && !r->peer.tx_fid);
  assert(sophia_lc_submit(&r->client, &v) == SOPHIA_9P_BUSY);
  release_ack(&r->peer);
  assert(!spin(r) && r->peer.tx_fid && r->peer.acked == r->peer.accepted_submitted);
  consume(r);
  rig_free(r);
}

static void refused_negotiation(void) {
  struct sophia_lf_negotiate o = offer();
  struct rig *r;
  const struct sophia_lf_record *e;
  o.minimum_revision = o.maximum_revision = 2;
  r = rig_new(&o, NULL);
  assert(spin(r) == SOPHIA_9P_INVALID);
  assert(sophia_lc_state(&r->client) == SOPHIA_LC_REFUSED);
  assert(sophia_lc_refusal(&r->client) == SOPHIA_LF_UNSUPPORTED_REVISION);
  assert(r->peer.acked == r->peer.refused); /* the refusal was acknowledged */
  assert(sophia_lc_event(&r->client, &e) == SOPHIA_9P_INVALID);
  assert(!sophia_lc_limits(&r->client));
  rig_free(r);
}

static void stale_connection(void) {
  struct rig *r = ready();
  r->peer.stale_events = 1;
  publish(&r->peer, SOPHIA_LF_LOCKED, 5, 1); /* wakes the parked read */
  assert(spin(r) == SOPHIA_9P_INVALID);
  assert(sophia_lc_state(&r->client) == SOPHIA_LC_STALE &&
         sophia_lc_remote_error(&r->client) == 116);
  rig_free(r);
}

static void refuses_bad_peers(void) {
  struct sophia_lf_negotiate o = offer();
  struct rig *r = rig_new(&o, "sophia-lock-files version=1 epoch=7\nx");
  assert(spin(r) == SOPHIA_9P_INVALID &&
         sophia_lc_state(&r->client) == SOPHIA_LC_FAILED);
  rig_free(r);
  r = rig_new(&o, "sophia-lock-files version=1 epoch=07\n");
  assert(spin(r) == SOPHIA_9P_INVALID);
  rig_free(r);
  /* A grant beyond the request. */
  o.capabilities = SOPHIA_LF_CAP_PRESENT;
  o.chord_count = 0;
  r = rig_new(&o, NULL);
  r->peer.extra_grant = SOPHIA_LF_CAP_CHORDS;
  assert(spin(r) == SOPHIA_9P_INVALID);
  rig_free(r);
}

static void refuses_bad_offers(void) {
  struct sophia_lf_negotiate o = offer();
  struct sophia_lc_client c;
  struct sophia_9p_client wire;
  size_t bytes = sophia_9p_storage_bytes(4096, 8);
  void *storage = malloc(bytes);
  int sockets[2];
  assert(storage && !socketpair(AF_UNIX, SOCK_STREAM, 0, sockets));
  assert(!sophia_9p_init(&wire, sockets[0], 4096, 8, 32, storage, bytes));
  o.capabilities = SOPHIA_LF_CAP_CHORDS; /* present is required */
  assert(sophia_lc_init(&c, &wire, &o) == SOPHIA_9P_ARGUMENT);
  o = offer();
  o.capabilities = SOPHIA_LF_CAP_PRESENT; /* chords need their capability */
  assert(sophia_lc_init(&c, &wire, &o) == SOPHIA_9P_ARGUMENT);
  o = offer();
  o.chords[0].modifiers = 0;
  assert(sophia_lc_init(&c, &wire, &o) == SOPHIA_9P_ARGUMENT);
  o = offer();
  o.minimum_revision = 2;
  o.maximum_revision = 1;
  assert(sophia_lc_init(&c, &wire, &o) == SOPHIA_9P_ARGUMENT);
  o = offer(); /* the control: the same wire takes a valid offer */
  assert(!sophia_lc_init(&c, &wire, &o));
  close(sockets[0]);
  close(sockets[1]);
  free(storage);
}


static struct sophia_9p_reply wire_reply(struct rig *r) {
  unsigned i;
  struct sophia_9p_reply reply;
  for (i=0;i<100;i++) {
    int status;
    assert(!sophia_9p_service(&r->wire,65536));
    pump(&r->peer);
    status=sophia_9p_peek(&r->wire,&reply);
    if (!status) return reply;
    assert(status==SOPHIA_9P_AGAIN);
  }
  abort();
}
static void an_admitted_root_is_not_reset_or_attached_again(void) {
  struct sophia_lf_negotiate o=offer();
  struct rig *r=rig_storage(&o,NULL,8,0);
  struct sophia_9p_handle h;
  struct sophia_9p_reply reply;
  uint32_t root;
  assert(sophia_lc_init_attached(&r->client,&r->wire,0,&o)==SOPHIA_9P_ARGUMENT);
  assert(!sophia_9p_version(&r->wire,&h));
  reply=wire_reply(r);assert(reply.type==101);
  assert(!sophia_9p_consume(&r->wire,reply.handle));
  assert(!sophia_9p_attach(&r->wire,"","",&h,&root));
  reply=wire_reply(r);assert(reply.type==105);
  assert(sophia_lc_init_attached(&r->client,&r->wire,root,&o)==SOPHIA_9P_ARGUMENT);
  assert(!sophia_9p_consume(&r->wire,reply.handle));
  assert(sophia_lc_init_attached(&r->client,&r->wire,UINT32_MAX,&o)==SOPHIA_9P_ARGUMENT);
  assert(sophia_lc_init_attached(&r->client,&r->wire,root+1,&o)==SOPHIA_9P_ARGUMENT);
  {
    const char *name = "api";
    uint32_t extra;
    assert(!sophia_9p_walk(&r->wire,root,&name,1,&h,&extra));
    reply=wire_reply(r);assert(reply.type==111);
    assert(!sophia_9p_consume(&r->wire,reply.handle));
    assert(sophia_lc_init_attached(&r->client,&r->wire,root,&o)==SOPHIA_9P_ARGUMENT);
    assert(!sophia_9p_clunk(&r->wire,extra,&h));
    reply=wire_reply(r);assert(reply.type==121);
    assert(!sophia_9p_consume(&r->wire,reply.handle));
  }
  assert(!sophia_lc_init_attached(&r->client,&r->wire,root,&o));
  assert(!spin(r));
  assert(sophia_lc_ready(&r->client) && r->peer.attaches==1);
  rig_free(r);
}

static void attached_root_rejects_partial_input(void) {
  struct sophia_lf_negotiate o=offer();
  struct rig *r=rig_storage(&o,NULL,8,0);
  struct sophia_9p_handle h;
  struct sophia_9p_reply reply;
  uint32_t root;
  unsigned spin_count;
  const uint8_t prefix=11;
  assert(!sophia_9p_version(&r->wire,&h));
  reply=wire_reply(r);assert(reply.type==101);
  assert(!sophia_9p_consume(&r->wire,reply.handle));
  assert(!sophia_9p_attach(&r->wire,"","",&h,&root));
  reply=wire_reply(r);assert(reply.type==105);
  assert(!sophia_9p_consume(&r->wire,reply.handle));
  assert(send(r->peer.fd,&prefix,1,MSG_NOSIGNAL)==1);
  for(spin_count=0; !r->wire.rx_used && spin_count<100; ++spin_count)
    assert(!sophia_9p_service(&r->wire,1));
  assert(r->wire.rx_used==1);
  assert(sophia_lc_init_attached(&r->client,&r->wire,root,&o)==SOPHIA_9P_ARGUMENT);
  rig_free(r);
}

int main(void) {
  attached_root_rejects_partial_input();
  an_admitted_root_is_not_reset_or_attached_again();
  bootstrap_and_lock();
  superseded_announcement();
  rejects_invalid_lock_fetch();
  entry_and_chord();
  demand_and_present_records();
  byte_records_are_refused();
  again_then_retry();
  negotiation_retry_with_six_requests();
  contradictory_retry_preserves_custody();
  custody_waits_for_ack();
  refused_negotiation();
  stale_connection();
  refuses_bad_peers();
  refuses_bad_offers();
  puts("lock_client_test ok");
  return 0;
}

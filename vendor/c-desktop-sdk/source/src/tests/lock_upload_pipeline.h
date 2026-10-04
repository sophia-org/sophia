/* Included by lock_client_test.c: real SDK, scripted peer with held replies. */
static struct rig *pipeline_ready(unsigned width) {
  struct sophia_lf_negotiate o = offer();
  struct sophia_lf_resource_begin begin;
  struct rig *r = rig_new_capacity(&o, NULL, 16);
  assert(next(r)->header.kind == SOPHIA_LF_OBJECT_PUBLISHED);
  consume(r);
  assert(!sophia_lc_upload_window(&r->client, width));
  r->peer.upload_iounit = 4; /* Eight messages are needed for just 32 pixels. */
  memset(&begin, 0, sizeof(begin));
  begin.transaction = 30;
  begin.resource.id = 21;
  begin.resource.generation = 2;
  begin.width_px = 16;
  begin.height_px = 2;
  assert(!sophia_lc_upload_begin(&r->client, &begin));
  assert(next(r)->value.resource_status.status == SOPHIA_LF_ADMITTED);
  consume(r);
  assert(sophia_lc_upload_ready(&r->client));
  assert(sophia_lc_upload_window(&r->client, 1) == SOPHIA_9P_BUSY);
  return r;
}
static void release_upload_reply(struct peer *p, unsigned at) {
  assert(at < p->held_uploads);
  if (p->upload_errors[at])
    error(p, p->upload_tags[at], p->upload_errors[at]);
  else
    count(p, p->upload_tags[at], p->upload_counts[at]);
}
static void pipeline_control_progress(struct rig *r) {
  struct sophia_lf_record entry;
  memset(&entry, 0, sizeof(entry));
  entry.value.entry.lock_epoch = 5;
  entry.value.entry.entry = SOPHIA_LF_INSERT;
  event(&r->peer, SOPHIA_LF_ENTRY, &entry);
  assert(next(r)->header.kind == SOPHIA_LF_ENTRY);
  consume(r);
  assert(r->peer.acked == r->peer.sequence); /* Input + ack bypass held bytes. */
}
static void pipeline_reverse_and_control(void) {
  struct rig *r = pipeline_ready(8);
  uint8_t pixels[128];
  unsigned i;
  for (i = 0; i < sizeof(pixels); ++i)
    pixels[i] = (uint8_t)(i * 7 + 1);
  r->peer.hold_upload = 1;
  assert(!sophia_lc_upload_chunk(&r->client, pixels, sizeof(pixels)));
  assert(!spin(r));
  assert(r->peer.held_uploads == 8 && r->peer.upload_cursor == 32);
  assert(sophia_lc_upload_end(&r->client) == SOPHIA_9P_BUSY);
  pipeline_control_progress(r);
  /* Free slots out of order, refilling while the earliest reply is held. */
  r->peer.hold_upload = 0;
  for (i = 8; i > 1; --i)
    release_upload_reply(&r->peer, i - 1);
  assert(!spin(r));
  assert(r->peer.upload_cursor == sizeof(pixels));
  assert(!sophia_lc_upload_ready(&r->client));
  assert(sophia_lc_upload_end(&r->client) == SOPHIA_9P_BUSY);
  release_upload_reply(&r->peer, 0);
  assert(!spin(r) && sophia_lc_upload_ready(&r->client));
  assert(!memcmp(r->peer.pixels, pixels, sizeof(pixels)));
  assert(!sophia_lc_upload_end(&r->client));
  assert(next(r)->value.resource_status.status == SOPHIA_LF_ACCEPTED);
  consume(r);
  assert(r->peer.upload_ends == 1 && !sophia_lc_upload_pending(&r->client));
  rig_free(r);
}
static void pipeline_cancel_or_failure(unsigned fault) {
  struct rig *r = pipeline_ready(8);
  uint8_t pixels[128] = {0};
  unsigned i;
  r->peer.hold_upload = 1;
  r->peer.upload_short = fault == 1;
  r->peer.upload_error = fault == 2 ? 5 : 0;
  assert(!sophia_lc_upload_chunk(&r->client, pixels, sizeof(pixels)));
  assert(!spin(r) && r->peer.held_uploads == 8);
  if (!fault)
    assert(!sophia_lc_upload_cancel(&r->client));
  /* First reply causes a failure, or acknowledges the explicit cancel's first
   * write. Storage stays borrowed and the writer stays open until all settle. */
  release_upload_reply(&r->peer, 0);
  assert(!spin(r));
  /* Only a peer Rerror may supply a remote errno. A short reply is a local
   * cancellation decision, not an invented error from the peer. */
  assert(r->client.remote_error == (fault == 2 ? 5u : 0u));
  assert(r->client.upload_data == pixels && r->peer.upload_fid);
  assert(sophia_lc_upload_pending(&r->client));
  assert(!sophia_lc_upload_ready(&r->client));
  assert(r->peer.held_uploads == 8); /* No new bytes after cancellation. */
  pipeline_control_progress(r);
  r->peer.hold_upload = 0;
  for (i = 8; i > 1; --i)
    release_upload_reply(&r->peer, i - 1);
  assert(next(r)->value.resource_status.status == SOPHIA_LF_CANCELLED);
  consume(r);
  assert(!sophia_lc_upload_pending(&r->client) && !r->peer.upload_fid);
  assert(!r->client.upload_data && !r->peer.upload_ends);
  /* A failed resource has not killed or confused the connection. */
  pipeline_control_progress(r);
  rig_free(r);
}
static void pipeline_terminal_status_drains(void) {
  struct rig *r = pipeline_ready(8);
  uint8_t pixels[128] = {0};
  unsigned i;
  r->peer.hold_upload = 1;
  assert(!sophia_lc_upload_chunk(&r->client, pixels, sizeof(pixels)));
  assert(!spin(r) && r->peer.held_uploads == 8);
  resource_status(&r->peer, r->peer.begin.transaction,
                  SOPHIA_LF_RESOURCE_REJECTED, SOPHIA_LF_REASON_BUDGET);
  assert(next(r)->value.resource_status.status == SOPHIA_LF_RESOURCE_REJECTED);
  consume(r);
  assert(r->client.upload_data == pixels && r->peer.upload_fid);
  assert(sophia_lc_upload_pending(&r->client));
  for (i = 8; i > 0; --i)
    release_upload_reply(&r->peer, i - 1);
  assert(!spin(r) && !sophia_lc_upload_pending(&r->client));
  assert(!r->peer.upload_fid && !r->client.upload_data && !r->peer.upload_ends);
  rig_free(r);
}
static void pipeline_disconnect_latches(void) {
  struct rig *r = pipeline_ready(8);
  uint8_t pixels[128] = {0};
  r->peer.hold_upload = 1;
  assert(!sophia_lc_upload_chunk(&r->client, pixels, sizeof(pixels)));
  assert(!spin(r) && r->peer.held_uploads == 8);
  close(r->peer.fd);
  r->peer.fd = -1;
  assert(sophia_lc_service(&r->client, 65536) != 0);
  assert(!sophia_lc_upload_pending(&r->client));
  assert(sophia_lc_upload_end(&r->client) != 0);
  assert(sophia_lc_upload_cancel(&r->client) != 0);
  /* Terminal wire never reads caller storage again; dispose before reuse. */
  rig_free(r);
}
static void drained_cancel_has_immediate_poll_interest(void) {
  struct rig *r = pipeline_ready(8);
  uint8_t pixels[128] = {0};
  unsigned i;
  r->peer.hold_upload = 1;
  assert(!sophia_lc_upload_chunk(&r->client, pixels, sizeof(pixels)));
  assert(!spin(r) && r->peer.held_uploads == 8);
  assert(!sophia_lc_upload_cancel(&r->client));
  assert(!sophia_9p_wants_write(&r->wire));
  for (i = 0; i < 8; ++i)
    release_upload_reply(&r->peer, i);
  /* One call consumes the last replies and creates the Cancel write. A
   * caller sleeping on IN alone would otherwise wait forever. */
  assert(!sophia_lc_service(&r->client, 65536));
  assert(r->client.submit_op.active && sophia_9p_wants_write(&r->wire));
  assert(next(r)->value.resource_status.status == SOPHIA_LF_CANCELLED);
  consume(r);
  assert(!sophia_lc_upload_pending(&r->client));
  rig_free(r);
}
static void upload_pipeline_tests(void) {
  struct rig *r = ready();
  assert(r->client.upload_window == 1);
  assert(sophia_lc_upload_window(&r->client, 0) == SOPHIA_9P_ARGUMENT);
  assert(sophia_lc_upload_window(&r->client, 9) == SOPHIA_9P_ARGUMENT);
  assert(sophia_lc_upload_window(&r->client, 8) == SOPHIA_9P_ARGUMENT);
  assert(!sophia_lc_upload_window(&r->client, 3)); /* Five slots reserved. */
  rig_free(r);
  pipeline_reverse_and_control();
  pipeline_terminal_status_drains();
  pipeline_disconnect_latches();
  drained_cancel_has_immediate_poll_interest();
  pipeline_cancel_or_failure(0);
  pipeline_cancel_or_failure(1);
  pipeline_cancel_or_failure(2);
  r = pipeline_ready(1);
  {
    uint8_t pixels[128] = {0};
    r->peer.upload_short = 1;
    assert(!sophia_lc_upload_chunk(&r->client, pixels, sizeof(pixels)));
    assert(!spin(r));
    assert(!spin(r) && sophia_lc_upload_ready(&r->client));
    assert(r->peer.upload_cursor == sizeof(pixels));
  }
  rig_free(r);
  puts("lock upload pipeline controls ok");
}

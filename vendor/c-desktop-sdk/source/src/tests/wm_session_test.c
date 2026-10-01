#include "wm_session_peer.h"

static const char *test_case;
static void steps(struct wm_rig *r, unsigned count) {
  unsigned i;
  for (i = 0; i < count; ++i) {
    int status = wr_step(r);
    if (status)
      fprintf(stderr, "%s: unexpected dispatch result %d, wire error %u\n",
              test_case, status, sophia_ws_remote_error(r->session));
    assert(!status);
  }
}
static void terminal(struct wm_rig *r) {
  unsigned i;
  for (i = 0; i < 200; ++i)
    if (wr_step(r) < 0)
      return;
  fprintf(stderr, "%s: expected a terminal session\n", test_case);
  assert(!"expected a terminal session");
}
static void bootstrap_and_partial_writes(void) {
  struct wm_rig *r = wr_new();
  struct sophia_wf_record value = {0};
  struct sophia_ws_obligations o;
  uint64_t ticket, untouched = 987, domain;
  wr_ready(r);
  assert(sophia_ws_epoch(r->session) == WP_EPOCH &&
         sophia_ws_capabilities(r->session) == WP_CAPS);
  assert(sophia_ws_limits(r->session)->max_object_bytes == 1048576);
  assert(!sophia_ws_next_transaction(r->session, &domain) && domain == 1);
  r->peer.short_tx = 7;
  ticket = wr_submit(r, 10);
  value.header.kind = SOPHIA_WF_CONFIGURATION;
  value.value.configuration.transaction = 11;
  value.value.configuration.generation = 1;
  assert(sophia_ws_submit(r->session, &value, r->now + 1000, &untouched) ==
         SOPHIA_9P_BUSY);
  assert(untouched == 987);
  steps(r, 100);
  assert(wr_custody(r, ticket) == SOPHIA_WS_SUBMITTED && r->peer.submits == 2);
  assert(!sophia_ws_obligations(r->session, &o) && o.acked == 3 &&
         !o.deadline_ms);
  assert(!r->peer.busy_opens &&
         !sophia_ws_next_transaction(r->session, &domain) && domain == 11);
  value.value.configuration.transaction = 10;
  assert(sophia_ws_submit(r->session, &value, r->now + 1000, &untouched) ==
         SOPHIA_9P_INVALID);
  assert(untouched == 987);
  sophia_ws_close(r->session);
  assert(wr_custody(r, ticket) == SOPHIA_WS_SUBMITTED);
  wr_drop(r);
}
static void submitted_and_error_both_orders(void) {
  unsigned order, error;
  for (order = 0; order < 2; ++order)
    for (error = 0; error < 2; ++error) {
      struct wm_rig *r = wr_new();
      uint64_t ticket;
      wr_ready(r);
      r->peer.submit_error = error ? 22 : 114;
      r->peer.custody_on_error = 1;
      r->peer.event_first = order;
      ticket = wr_submit(r, 1);
      if (error)
        terminal(r);
      else
        steps(r, 40);
      assert(wr_custody(r, ticket) == SOPHIA_WS_SUBMITTED);
      assert(sophia_ws_state(r->session) ==
             (error ? SOPHIA_WS_FAILED : SOPHIA_WS_READY));
      wr_drop(r);
    }
}
static void ealready_without_custody_is_unknown(void) {
  struct wm_rig *r = wr_new();
  uint64_t ticket;
  wr_ready(r);
  r->peer.submit_error = 114;
  ticket = wr_submit(r, 1);
  terminal(r);
  assert(wr_custody(r, ticket) == SOPHIA_WS_UNKNOWN_DISCONNECTED);
  wr_drop(r);
}
static void refusal_continues_without_consuming_domain(void) {
  struct wm_rig *r = wr_new();
  uint64_t first, second;
  uint32_t error;
  enum sophia_ws_custody outcome;
  wr_ready(r);
  r->peer.submit_error = 13;
  first = wr_submit(r, 15);
  steps(r, 30);
  assert(!sophia_ws_outcome(r->session, first, &outcome, &error));
  assert(outcome == SOPHIA_WS_REFUSED && error == 13);
  r->peer.submit_error = 0;
  second = wr_submit(r, 15);
  steps(r, 30);
  assert(second > first && wr_custody(r, second) == SOPHIA_WS_SUBMITTED);
  assert(wr_custody(r, first) == SOPHIA_WS_REFUSED);
  wr_drop(r);
}
static void retry_is_paced_and_keeps_identity(void) {
  struct wm_rig *r = wr_new();
  uint64_t ticket;
  unsigned i, count;
  wr_ready(r);
  r->peer.submit_error = 11;
  r->peer.error_once = 1;
  ticket = wr_submit(r, 1);
  for (i = 0; i < 100 && r->peer.submits < 2; ++i)
    assert(!wr_step(r));
  assert(r->peer.submits == 2 &&
         wr_custody(r, ticket) == SOPHIA_WS_ADMITTED_LOCAL);
  assert(sophia_ws_timeout(r->session, r->now) > 0);
  count = r->peer.submits;
  for (i = 0; i < 3; ++i) {
    assert(!sophia_ws_dispatch(r->session, POLLIN | POLLOUT, 65536, r->now));
    wp_pump(&r->peer);
    assert(r->peer.submits == count);
  }
  steps(r, 40);
  assert(r->peer.submits == 3 && r->peer.last_id == ticket &&
         wr_custody(r, ticket) == SOPHIA_WS_SUBMITTED);
  wr_drop(r);
}
static void held_event_and_ack_reply_block_next_transaction(void) {
  struct wm_rig *r = wr_new();
  const struct sophia_wf_record *event;
  struct sophia_ws_obligations o;
  uint64_t first, second;
  wr_ready(r);
  wp_outcome(&r->peer, 77);
  steps(r, 4);
  assert(!sophia_ws_event(r->session, &event) &&
         event->value.configuration_outcome.transaction == 77);
  first = wr_submit(r, 1);
  steps(r, 30);
  assert(wr_custody(r, first) == SOPHIA_WS_SUBMITTED && r->peer.opens == 2);
  assert(!sophia_ws_event(r->session, &event) &&
         event->value.configuration_outcome.transaction == 77);
  second = wr_submit(r, 2);
  steps(r, 20);
  assert(!sophia_ws_obligations(r->session, &o) && o.waiting_for_ack &&
         o.event_pending);
  assert(r->peer.opens == 2 &&
         wr_custody(r, second) == SOPHIA_WS_ADMITTED_LOCAL);
  r->peer.ack_hold = 1;
  assert(!sophia_ws_consume(r->session));
  steps(r, 10);
  assert(r->peer.acked == 4 &&
         r->peer.opens == 2); /* Server cleared; reply withheld. */
  r->peer.ack_hold = 0;
  wp_count(&r->peer, r->peer.ack_tag, 16);
  steps(r, 30);
  assert(r->peer.opens == 3 && !r->peer.busy_opens &&
         wr_custody(r, second) == SOPHIA_WS_SUBMITTED);
  wr_drop(r);
}
static void invalid_events_stop_custody_scan(void) {
  unsigned which;
  for (which = 0; which < 3; ++which) {
    struct wm_rig *r = wr_new();
    uint64_t ticket;
    wr_ready(r);
    r->peer.wrong_epoch = which == 0;
    r->peer.wrong_kind = which == 1;
    r->peer.bad_event_before = which == 2;
    ticket = wr_submit(r, 1);
    terminal(r);
    assert(wr_custody(r, ticket) == SOPHIA_WS_UNKNOWN_DISCONNECTED);
    wr_drop(r);
  }
}
static void short_submit_and_final_read_preserve_observed_custody(void) {
  unsigned which;
  for (which = 0; which < 3; ++which) {
    struct wm_rig *r = wr_new();
    uint64_t ticket;
    wr_ready(r);
    if (which < 2) {
      r->peer.submit_count = 23;
      r->peer.submit_silent = which == 1;
    } else
      r->peer.close_after = 1;
    ticket = wr_submit(r, 1);
    terminal(r);
    assert(wr_custody(r, ticket) ==
           (which == 1 ? SOPHIA_WS_UNKNOWN_DISCONNECTED : SOPHIA_WS_SUBMITTED));
    wr_drop(r);
  }
}
static void snapshot_and_cycle(struct wm_rig *r) {
  struct sophia_wf_record value = {0};
  struct sophia_wf_snapshot_output row = {0};
  uint8_t bytes[56];
  size_t n;
  row.output = 10;
  row.generation = 1;
  row.width = row.work_width = 640;
  row.height = row.work_height = 480;
  assert(!sophia_wf_snapshot_output_encode(bytes, sizeof(bytes), &row));
  value.header.kind = SOPHIA_WF_SNAPSHOT;
  value.header.epoch = WP_EPOCH;
  value.value.snapshot = (struct sophia_wf_snapshot){71, 9, 10};
  value.section_count = 1;
  value.sections[0] = (struct sophia_wf_section){1, 1, bytes, sizeof(bytes)};
  assert(!sophia_wf_encode(r->peer.snapshot, sizeof(r->peer.snapshot), WP_CAPS,
                           &value, &n) &&
         n == 136);
  memset(&value, 0, sizeof(value));
  value.header.kind = SOPHIA_WF_CYCLE;
  value.value.cycle.snapshot_transaction = 71;
  value.value.cycle.request_transaction = 81;
  value.value.cycle.request_id = 1;
  value.value.cycle.scene_generation = 9;
  value.value.cycle.policy_generation = 1;
  value.value.cycle.output_count = 1;
  value.value.cycle.outputs[0] = 10;
  wp_record(&r->peer, &value);
}
static void snapshot_is_complete_bound_and_pin_released(void) {
  struct wm_rig *r = wr_new();
  const struct sophia_wf_record *snapshot, *again;
  unsigned i;
  wr_ready(r);
  r->peer.object_chunk = 7;
  snapshot_and_cycle(r);
  steps(r, 4);
  assert(!sophia_ws_snapshot(r->session, r->now + 500));
  assert(!sophia_ws_consume(r->session));
  assert(sophia_ws_snapshot_result(r->session, &snapshot) == SOPHIA_9P_AGAIN);
  for (i = 0; i < 100; ++i) {
    assert(!wr_step(r));
    if (!sophia_ws_snapshot_result(r->session, &snapshot))
      break;
  }
  assert(i < 100 && snapshot->header.epoch == WP_EPOCH &&
         snapshot->value.snapshot.transaction == 71);
  assert(snapshot->value.snapshot.scene_generation == 9 &&
         snapshot->section_count == 1);
  assert(!r->peer.pins && r->peer.clunks == 1);
  assert(!sophia_ws_snapshot_result(r->session, &again) && again == snapshot);
  assert(sophia_ws_snapshot(r->session, r->now + 500) == SOPHIA_9P_BUSY);
  assert(!sophia_ws_snapshot_release(r->session));
  assert(sophia_ws_snapshot_result(r->session, &snapshot) == SOPHIA_9P_AGAIN);
  wr_drop(r);
}
static void mismatched_snapshot_refuses(void) {
  unsigned field;
  for (field = 0; field < 3; ++field) {
    struct wm_rig *r = wr_new();
    const struct sophia_wf_record *snapshot;
    wr_ready(r);
    snapshot_and_cycle(r);
    wp_put(r->peer.snapshot + (field == 0 ? 8 : field == 1 ? 32 : 40), 999, 8);
    steps(r, 4);
    assert(!sophia_ws_snapshot(r->session, r->now + 500));
    terminal(r);
    assert(sophia_ws_snapshot_result(r->session, &snapshot) < 0);
    wr_drop(r);
  }
}
static void idle_partial_and_clock_deadlines(void) {
  struct wm_rig *r = wr_new();
  wr_ready(r);
  assert(sophia_ws_timeout(r->session, r->now) == -1);
  r->now += 20000;
  assert(!sophia_ws_dispatch(r->session, POLLIN, 65536, r->now));
  assert(sophia_ws_dispatch(r->session, POLLIN, 65536, r->now - 1) ==
         SOPHIA_9P_ARGUMENT);
  assert(sophia_ws_state(r->session) == SOPHIA_WS_READY);
  r->peer.event_chunk = 1;
  wp_outcome(&r->peer, 90);
  steps(r, 2);
  assert(sophia_ws_timeout(r->session, r->now) > 0);
  r->now += 4000;
  assert(sophia_ws_dispatch(r->session, POLLIN, 65536, r->now) < 0);
  wr_drop(r);
}
static void complete_held_event_outlives_assembly_deadline(void) {
  struct wm_rig *r = wr_new();
  const struct sophia_wf_record *event;
  wr_ready(r);
  wp_outcome(&r->peer, 91);
  steps(r, 4);
  assert(!sophia_ws_event(r->session, &event) &&
         event->value.configuration_outcome.transaction == 91);
  assert(sophia_ws_timeout(r->session, r->now) == -1);
  r->now += 20000;
  assert(!sophia_ws_dispatch(r->session, POLLIN | POLLOUT, 65536, r->now));
  assert(sophia_ws_state(r->session) == SOPHIA_WS_READY &&
         !sophia_ws_event(r->session, &event) &&
         event->value.configuration_outcome.transaction == 91);
  assert(!sophia_ws_consume(r->session));
  steps(r, 4);
  assert(r->peer.acked == 3);
  wr_drop(r);
}
static void partial_event_deadline_runs_during_pending_ack(void) {
  struct wm_rig *r = wr_new();
  struct sophia_ws_obligations before, later;
  const struct sophia_wf_record *event;
  wr_ready(r);
  wp_outcome(&r->peer, 92);
  steps(r, 4);
  assert(!sophia_ws_event(r->session, &event));
  r->peer.ack_hold = 1;
  assert(!sophia_ws_consume(r->session));
  steps(r, 4);
  assert(!sophia_ws_obligations(r->session, &before));
  assert(r->peer.acked == 3 && before.acked == 2 && !before.deadline_ms);
  r->peer.event_chunk = 1;
  wp_outcome(&r->peer, 93);
  steps(r, 2);
  assert(!sophia_ws_obligations(r->session, &before));
  assert(before.deadline_ms > r->now && before.acked == 2);
  r->now += 1000;
  steps(r, 2); /* More partial bytes do not renew the record deadline. */
  assert(!sophia_ws_obligations(r->session, &later));
  assert(later.deadline_ms == before.deadline_ms && later.acked == 2);
  r->now = before.deadline_ms;
  assert(sophia_ws_dispatch(r->session, POLLIN | POLLOUT, 65536, r->now) ==
         SOPHIA_9P_CLOSED);
  assert(sophia_ws_state(r->session) == SOPHIA_WS_CLOSED);
  wr_drop(r);
}
static void deadline_never_sends_and_close_classifies(void) {
  struct wm_rig *r = wr_new();
  struct sophia_wf_record value = {0};
  uint64_t ticket;
  wr_ready(r);
  value.header.kind = SOPHIA_WF_CONFIGURATION;
  value.value.configuration.transaction = 1;
  value.value.configuration.generation = 1;
  assert(!sophia_ws_submit(r->session, &value, r->now + 1, &ticket));
  assert(wr_step(r) < 0 && r->peer.opens == 1 &&
         wr_custody(r, ticket) == SOPHIA_WS_DROPPED_UNSENT);
  wr_drop(r);
  r = wr_new();
  wr_ready(r);
  r->peer.submit_silent = 1;
  ticket = wr_submit(r, 1);
  steps(r, 30);
  assert(wr_custody(r, ticket) == SOPHIA_WS_ISSUED);
  sophia_ws_close(r->session);
  assert(wr_custody(r, ticket) == SOPHIA_WS_UNKNOWN_DISCONNECTED);
  wr_drop(r);
}
static void bad_limits_and_negotiation_refuse(void) {
  unsigned which;
  for (which = 0; which < 5; ++which) {
    struct wm_rig *r = wr_new();
    if (which == 0)
      wp_put(r->peer.limits + 40, SOPHIA_WF_MAX_RECORD + 1u, 4);
    else if (which == 1)
      wp_put(r->peer.limits + 24, 1, 8); /* Limits is not an event. */
    else if (which == 2)
      r->peer.selected &= ~SOPHIA_WF_CAP_PROFILE_ACTIVATION;
    else if (which == 3)
      r->peer.selected |= UINT64_C(1) << 63; /* Not offered. */
    else
      wp_put(r->peer.limits + 32, WP_CAPS & ~SOPHIA_WF_CAP_PROFILE_ACTIVATION,
             8); /* Selected beyond the ceiling. */
    terminal(r);
    assert(sophia_ws_state(r->session) == SOPHIA_WS_FAILED);
    wr_drop(r);
  }
}
static void retired_output_transport_api_refuses(void) {
  /* The api file is compared byte for byte. The retired current_ipc output
   * transport, and near-misses of the current one, never negotiate. */
  static const char *const refused[] = {
      "sophia-wm-files version=1 output_transport=current_ipc\n",
      "sophia-wm-files version=1 output_transport=9p2000.L",
      "sophia-wm-files version=1 output_transport=9P2000.L\n",
      "sophia-wm-files version=1\n",
  };
  unsigned which;
  for (which = 0; which < sizeof(refused) / sizeof(*refused); ++which) {
    struct wm_rig *r = wr_new();
    r->peer.api = refused[which];
    terminal(r);
    assert(sophia_ws_state(r->session) == SOPHIA_WS_FAILED);
    assert(!r->peer.submits);
    wr_drop(r);
  }
}
static void impossible_offer_refuses_before_submission(void) {
  unsigned profile;
  for (profile = 0; profile < 2; ++profile) {
    struct wm_rig *r = wr_new_caps(
        profile ? WP_CAPS & ~SOPHIA_WF_CAP_PROFILE_ACTIVATION : WP_CAPS);
    if (!profile)
      wp_put(r->peer.limits + 32, WP_CAPS & ~SOPHIA_WF_CAP_CONFIGURATION, 8);
    terminal(r);
    assert(sophia_ws_state(r->session) == SOPHIA_WS_FAILED);
    assert(r->peer.submits == 0 && r->peer.tx_size == 0);
    wr_drop(r);
  }
}
static void event_lengths_and_sequences_fail_immediately(void) {
  unsigned which;
  for (which = 0; which < 4; ++which) {
    struct wm_rig *r = wr_new();
    size_t at;
    wr_ready(r);
    at = r->peer.journal_size;
    wp_outcome(&r->peer, 77);
    if (which < 2) {
      wp_put(r->peer.journal + at, which ? 273 : 31, 4);
      r->peer.event_chunk = 4; /* Refuse before waiting for the body. */
    } else
      wp_put(r->peer.journal + at + 24, which == 2 ? 2 : 1, 8);
    terminal(r);
    assert(sophia_ws_state(r->session) == SOPHIA_WS_FAILED);
    wr_drop(r);
  }
}
static void ack_faults_and_revocation_keep_custody(void) {
  unsigned which;
  for (which = 0; which < 4; ++which) {
    struct wm_rig *r = wr_new();
    uint64_t ticket;
    wr_ready(r);
    if (which == 0)
      r->peer.ack_count = 15;
    else if (which == 1)
      r->peer.ack_error = 22;
    else if (which == 3)
      r->peer.submit_silent = 1;
    ticket = wr_submit(r, 1);
    if (which >= 2) {
      steps(r, 30);
      r->peer.event_error = 116;
    }
    terminal(r);
    assert(wr_custody(r, ticket) ==
           (which == 3 ? SOPHIA_WS_UNKNOWN_DISCONNECTED : SOPHIA_WS_SUBMITTED));
    assert(sophia_ws_state(r->session) ==
           (which >= 2 ? SOPHIA_WS_STALE : SOPHIA_WS_FAILED));
    wr_drop(r);
  }
}
static void snapshot_retry_is_paced_and_truncation_refuses(void) {
  struct wm_rig *r = wr_new();
  const struct sophia_wf_record *snapshot;
  unsigned i;
  wr_ready(r);
  snapshot_and_cycle(r);
  steps(r, 4);
  r->peer.object_again = 1;
  assert(!sophia_ws_snapshot(r->session, r->now + 500));
  for (i = 0; i < 50 && !r->peer.object_opens; ++i)
    assert(!wr_step(r));
  assert(i < 50 && r->peer.object_opens == 1);
  for (i = 0; i < 3; ++i) {
    assert(!sophia_ws_dispatch(r->session, POLLIN | POLLOUT, 65536, r->now));
    wp_pump(&r->peer);
    assert(r->peer.object_opens == 1 && !r->peer.pins);
  }
  steps(r, 30);
  assert(r->peer.object_opens == 2 && !r->peer.pins &&
         !sophia_ws_snapshot_result(r->session, &snapshot));
  wr_drop(r);
  r = wr_new();
  wr_ready(r);
  snapshot_and_cycle(r);
  steps(r, 4);
  r->peer.object_truncate = 1;
  assert(!sophia_ws_snapshot(r->session, r->now + 500));
  terminal(r);
  assert(sophia_ws_snapshot_result(r->session, &snapshot) == SOPHIA_9P_INVALID);
  wr_drop(r);
}
static void full_retained_journal_still_observes_submitted(void) {
  struct wm_rig *r = wr_new();
  const struct sophia_wf_record *event;
  uint64_t ticket;
  unsigned i;
  wr_ready(r);
  for (i = 0; i < 63; ++i)
    wp_outcome(&r->peer, 100 + i);
  ticket = wr_submit(r, 1);
  steps(r, 30);
  assert(wr_custody(r, ticket) == SOPHIA_WS_SUBMITTED && r->peer.acked == 2);
  for (i = 0; i < 63; ++i) {
    assert(!sophia_ws_event(r->session, &event) &&
           event->value.configuration_outcome.transaction == 100 + i);
    assert(!sophia_ws_consume(r->session));
  }
  steps(r, 10);
  assert(r->peer.acked == 66 &&
         sophia_ws_event(r->session, &event) == SOPHIA_9P_AGAIN);
  wr_drop(r);
}
static void presentation_receipt_requires_negotiated_capability(void) {
  unsigned enabled;
  for (enabled = 0; enabled < 2; ++enabled) {
    uint64_t caps = WP_CAPS;
    struct wm_rig *r;
    struct sophia_wf_record receipt = {0};
    const struct sophia_wf_record *event = NULL;
    if (!enabled)
      caps &= ~(SOPHIA_WF_CAP_SURFACE_INSTANCES |
                SOPHIA_WF_CAP_PRESENTATION_ACTIONS);
    r = wr_new_caps(caps);
    wr_ready(r);
    receipt.header.kind = SOPHIA_WF_PRESENTATION_RECEIPT;
    receipt.value.presentation_receipt =
        (struct sophia_wf_presentation_receipt){1, 2, 3, 4, 5, 1};
    /* The peer encodes a structurally valid receipt even when disclosure was
     * not negotiated. Admission must fail before an application sees it. */
    wp_record(&r->peer, &receipt);
    if (enabled) {
      steps(r, 20);
      assert(!sophia_ws_event(r->session, &event));
      assert(event->header.kind == SOPHIA_WF_PRESENTATION_RECEIPT &&
             event->value.presentation_receipt.presentation_epoch == 5);
      assert(!sophia_ws_consume(r->session));
    } else {
      terminal(r);
      assert(sophia_ws_state(r->session) == SOPHIA_WS_FAILED);
      assert(sophia_ws_event(r->session, &event) != 0 && event == NULL);
    }
    wr_drop(r);
  }
}
static void local_work_wakes_a_poll_first_caller(void) {
  unsigned kind, missed = 0;
  for (kind = 0; kind < 3; ++kind) {
    struct wm_rig *r = wr_new();
    const struct sophia_wf_record *event;
    struct pollfd fd;
    uint64_t ticket = 0;
    int wait;
    wr_ready(r);
    if (kind == 1)
      wp_outcome(&r->peer, 77);
    else if (kind == 2)
      snapshot_and_cycle(r);
    steps(r, 20);
    /* No peer bytes or queued client writes can hide a missing local wakeup. */
    fd = (struct pollfd){sophia_ws_poll_fd(r->session),
                         sophia_ws_poll_events(r->session), 0};
    assert(!(fd.events & POLLOUT) && poll(&fd, 1, 0) == 0);
    assert(sophia_ws_timeout(r->session, r->now) == -1);
    if (kind == 0)
      ticket = wr_submit(r, 1);
    else if (kind == 1) {
      assert(!sophia_ws_event(r->session, &event));
      assert(!sophia_ws_consume(r->session));
    } else
      assert(!sophia_ws_snapshot(r->session, r->now + 1000));
    wait = sophia_ws_timeout(r->session, r->now);
    if (wait != 0) {
      fprintf(stderr, "local work kind=%u: poll timeout=%d, expected 0\n",
              kind, wait);
      ++missed;
    }
    /* One dispatch queues the work; ordinary wire readiness then drives it.
     * An acknowledged idle session must return to sleeping, not spin. */
    assert(!sophia_ws_dispatch(r->session, 0, 65536, r->now));
    assert(sophia_ws_poll_events(r->session) & POLLOUT);
    steps(r, 40);
    if (kind == 0)
      assert(wr_custody(r, ticket) == SOPHIA_WS_SUBMITTED);
    else if (kind == 1)
      assert(r->peer.acked == 3);
    else
      assert(!sophia_ws_snapshot_result(r->session, &event));
    assert(sophia_ws_timeout(r->session, r->now) == -1);
    assert(!(sophia_ws_poll_events(r->session) & POLLOUT));
    wr_drop(r);
  }
  assert(!missed);
}
int main(void) {
#define RUN(test)                                                              \
  do {                                                                         \
    test_case = #test;                                                         \
    test();                                                                    \
  } while (0)
  RUN(local_work_wakes_a_poll_first_caller);
  RUN(bootstrap_and_partial_writes);
  RUN(submitted_and_error_both_orders);
  RUN(ealready_without_custody_is_unknown);
  RUN(refusal_continues_without_consuming_domain);
  RUN(retry_is_paced_and_keeps_identity);
  RUN(held_event_and_ack_reply_block_next_transaction);
  RUN(invalid_events_stop_custody_scan);
  RUN(short_submit_and_final_read_preserve_observed_custody);
  RUN(snapshot_is_complete_bound_and_pin_released);
  RUN(mismatched_snapshot_refuses);
  RUN(idle_partial_and_clock_deadlines);
  RUN(complete_held_event_outlives_assembly_deadline);
  RUN(partial_event_deadline_runs_during_pending_ack);
  RUN(deadline_never_sends_and_close_classifies);
  RUN(bad_limits_and_negotiation_refuse);
  RUN(retired_output_transport_api_refuses);
  RUN(impossible_offer_refuses_before_submission);
  RUN(event_lengths_and_sequences_fail_immediately);
  RUN(ack_faults_and_revocation_keep_custody);
  RUN(snapshot_retry_is_paced_and_truncation_refuses);
  RUN(full_retained_journal_still_observes_submitted);
  RUN(presentation_receipt_requires_negotiated_capability);
#undef RUN
  puts("wm_session_test: scripted 9P custody, snapshot and deadline controls "
       "passed");
  return 0;
}

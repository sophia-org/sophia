#ifndef SOPHIA_LOCK_CLIENT_H
#define SOPHIA_LOCK_CLIENT_H
#include "sophia_9p_client.h"
#include "sophia_lock_files.h"

/* One admitted lock provider attach over standard 9P2000.L
 * (spec/sophia-lock-files.md). The caller chooses this backend and supplies
 * the admitted fd through wire; no discovery, fallback or reconnect. A
 * replacement provider process gets a fresh connection: build a new wire and
 * client for it. Single thread, no allocator, no clock.
 *
 * The client owns fids, offsets, submission IDs, file custody and the
 * cumulative ack. Negotiated, Refused and Submitted are consumed internally;
 * every other event is presented in journal order and acknowledged after the
 * caller consumes it, so a caller that stops consuming stops acknowledging
 * and the server's ack-progress deadline (Limits) revokes the connection. */
enum sophia_lc_state {
  SOPHIA_LC_BOOTSTRAP,
  SOPHIA_LC_READY,
  SOPHIA_LC_REFUSED, /* Negotiation refused; the server revokes after the ack. */
  SOPHIA_LC_STALE,   /* ESTALE: this connection epoch was revoked or replaced. */
  SOPHIA_LC_FAILED
};
/* Progress of the most recent submission, as sophia_sf_submission. REFUSED
 * holds the server's errno (submit_error) with no further meaning attached:
 * a stale allocation, a busy slot or a full journal all refuse this way, and
 * nothing was journaled. EAGAIN is not a refusal: see submit_retry. */
enum sophia_lc_submission {
  SOPHIA_LC_SUBMISSION_NONE,
  SOPHIA_LC_SUBMISSION_STAGED,
  SOPHIA_LC_SUBMISSION_ISSUED,
  SOPHIA_LC_SUBMISSION_CUSTODIED,
  SOPHIA_LC_SUBMISSION_REFUSED
};
struct sophia_lc_operation {
  struct sophia_9p_handle handle;
  uint8_t active;
};
/* Private state exposed for caller-owned allocation. Do not modify it. */
struct sophia_lc_client {
  struct sophia_9p_client *wire;
  struct sophia_lf_negotiate offer;
  struct sophia_lf_negotiated welcome;
  struct sophia_lf_limits limits;
  struct sophia_lf_lock lock;
  struct sophia_lf_record event;
  struct sophia_lf_resource_begin upload;
  struct sophia_lc_operation boot_op, event_op, submit_op, ack_op, object_op,
      upload_op;
  uint64_t epoch, next_submission, sequence, consumed, acked, ack_pending;
  uint64_t submitted_sequence, event_offset, lock_generation;
  uint64_t object_generation, object_qid, upload_offset;
  int terminal;
  uint32_t root, fids[4], object_fid, upload_fid, remote_error, submit_error;
  uint32_t iounit[4], boot_iounit, object_iounit, upload_iounit;
  uint16_t refusal;
  uint8_t bootstrap, negotiated, have_lock, event_ready, submit_stage,
      submitted, submit_replied, submit_wait, submit_sent, object_stage,
      object_probe, upload_stage, upload_closing, stale;
  size_t tx_size, tx_offset, event_used, object_used, boot_used;
  size_t upload_size, upload_sent;
  /* An upload chunk is borrowed until its Twrite reply is processed. */
  const uint8_t *upload_data;
  uint8_t tx[SOPHIA_LF_MAX_CANDIDATE], events[1024], object[1024], boot[128];
};

/* wire must be fresh and exclusively owned by this client until disposal.
 * The offer must request present; chords need the chords capability. Starts
 * version/attach, reads api and limits, opens the streams and submits the
 * Negotiate as submission 1, all nonblocking. */
int sophia_lc_init(struct sophia_lc_client *, struct sophia_9p_client *,
                   const struct sophia_lf_negotiate *offer);
/* One bounded pass: queue requests, service the wire, process replies. A
 * terminal result latches; the caller closes the fd. */
int sophia_lc_service(struct sophia_lc_client *, size_t byte_budget);
enum sophia_lc_state sophia_lc_state(const struct sophia_lc_client *);
int sophia_lc_ready(const struct sophia_lc_client *);
uint64_t sophia_lc_epoch(const struct sophia_lc_client *);
/* Nonzero after a refused negotiation: sophia_lf_refusal. */
uint16_t sophia_lc_refusal(const struct sophia_lc_client *);
uint32_t sophia_lc_remote_error(const struct sophia_lc_client *);
/* Borrowed until disposal; NULL before READY. Chord IDs are offer indices
 * below welcome->granted_chords. */
const struct sophia_lf_limits *sophia_lc_limits(const struct sophia_lc_client *);
const struct sophia_lf_negotiated *sophia_lc_welcome(const struct sophia_lc_client *);
/* The lock object as of the most recently presented ObjectPublished, or NULL
 * before the first. An ObjectPublished is presented only after the SDK read
 * that exact object (its Qid path and generation, through EOF); an
 * announcement already superseded by a newer object is consumed internally,
 * because the newer one's announcement follows it in the journal. Copy the
 * lock to keep it past the next consume. */
const struct sophia_lf_lock *sophia_lc_lock(const struct sophia_lc_client *,
                                            uint64_t *generation);
/* Borrow one event until consume. */
int sophia_lc_event(struct sophia_lc_client *, const struct sophia_lf_record **);
int sophia_lc_event_consume(struct sophia_lc_client *);
/* Copies one Candidate, FrameDemand or ResourceRetire; epoch and submission
 * are assigned here. BUSY while the previous submission is in flight or its
 * Submitted is unacknowledged. Uploads use the calls below. */
int sophia_lc_submit(struct sophia_lc_client *, const struct sophia_lf_record *);
/* id is the latest submission (0 before any). Negotiate is submission 1. */
int sophia_lc_submission(const struct sophia_lc_client *, uint64_t *id,
                         enum sophia_lc_submission *, uint32_t *submit_error);
/* A submit answered EAGAIN transferred nothing and waits, unsent, until this
 * call; callers make it after event progress or a bounded backoff. The retry
 * reuses the epoch, ID and staged bytes. ARGUMENT when none waits. */
int sophia_lc_submit_retry(struct sophia_lc_client *);
/* One upload at a time, one transaction: End and Cancel carry the begin's
 * transaction. Begin submits ResourceBegin; on Admitted the SDK opens
 * upload/<slot>. Supply one chunk at a time once ready; the chunk is borrowed
 * until ready again (or pending ends). End is allowed after every declared
 * byte (width * height * 4) was written; End and Cancel are submissions. The
 * upload ends on the resource's Accepted, Rejected or Cancelled status, or on
 * a refused begin, end or cancel. Statuses are presented as events too. */
int sophia_lc_upload_begin(struct sophia_lc_client *,
                           const struct sophia_lf_resource_begin *);
int sophia_lc_upload_chunk(struct sophia_lc_client *, const void *, size_t);
int sophia_lc_upload_ready(const struct sophia_lc_client *);
int sophia_lc_upload_pending(const struct sophia_lc_client *);
int sophia_lc_upload_end(struct sophia_lc_client *);
int sophia_lc_upload_cancel(struct sophia_lc_client *);
#endif

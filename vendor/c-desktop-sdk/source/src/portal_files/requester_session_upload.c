#include "requester_session_internal.h"
static int disjoint(const void *a, size_t an, const void *b, size_t bn) {
  uintptr_t av = (uintptr_t)a, bv = (uintptr_t)b;
  return av <= UINTPTR_MAX - an && bv <= UINTPTR_MAX - bn &&
         (av + an <= bv || bv + bn <= av);
}
static int same_upload(const struct sophia_pqr *s,
                       struct sophia_pqr_request_id id) {
  return s && s->upload_present && id.issuer == s &&
         id.epoch == s->upload_id.epoch &&
         id.submission == s->upload_id.submission;
}
int sophia_pqr_begin(struct sophia_pqr *s, struct sophia_pqr_request_id id,
                     const void *bytes, size_t length, uint64_t now,
                     uint64_t deadline) {
  struct sophia_pqt transaction;
  struct sophia_pqu upload;
  struct sophia_pq_candidate candidate = {0};
  size_t slot, storage;
  int e = pqr_advance(s, now);
  if (e)
    return e;
  if (!sophia_pqr_transaction_ready(s) || s->upload_present)
    return SOPHIA_PQS_BUSY;
  if (s->wire->capacity < 4 || s->wire->fid_limit < 6)
    return SOPHIA_PQS_INVALID;
  slot = pqr_transfer_slot(s, id);
  if (slot == 65)
    return SOPHIA_PQS_INVALID;
  if (s->transfers[slot].begun || s->transfers[slot].cancel_pending ||
      s->transfers[slot].transfer.state != SOPHIA_PQF_ALLOWED)
    return SOPHIA_PQS_BUSY;
  storage = sophia_9p_storage_bytes(s->wire->offered, s->wire->capacity);
  if (length && (!bytes || !disjoint(bytes, length, s, sizeof *s) ||
                 !disjoint(bytes, length, s->wire, sizeof *s->wire) ||
                 !disjoint(bytes, length, s->wire->storage, storage)))
    return SOPHIA_PQS_INVALID;
  if (s->last_submission == UINT64_MAX)
    return SOPHIA_PQS_OVERFLOW;
  candidate.kind = SOPHIA_PQ_BEGIN;
  candidate.data.payload.transfer =
      s->transfers[slot].transfer.expected.transfer;
  candidate.data.payload.total_bytes = length;
  e = sophia_pqt_init(&transaction, s->journal.epoch, s->last_submission + 1,
                      &candidate, now, deadline, s->limits.assembly_timeout_ms);
  if (e)
    return e;
  e = sophia_pqu_init(&upload, bytes, length, s->limits.max_payload_bytes, now,
                      deadline);
  if (e)
    return e;
  s->transfers[slot].begun = 1;
  s->upload = upload;
  s->upload_id = id;
  s->upload_present = 1;
  s->upload_phase = PQR_UPLOAD_BEGIN;
  ++s->last_submission;
  s->transaction = transaction;
  s->command = SOPHIA_PQ_BEGIN;
  s->releasing = 0;
  e = pqr_drive(s);
  return e ? pqr_fail(s, e) : 0;
}
int sophia_pqr_upload_written(const struct sophia_pqr *s,
                              struct sophia_pqr_request_id id, size_t *out) {
  if (!out || !same_upload(s, id))
    return SOPHIA_PQS_INVALID;
  *out = s->upload.written;
  return 0;
}
int sophia_pqr_upload_ready(const struct sophia_pqr *s,
                            struct sophia_pqr_request_id id) {
  return same_upload(s, id) && !s->error &&
         s->upload_phase == PQR_UPLOAD_WRITING && sophia_pqu_held(&s->upload);
}
int sophia_pqr_end(struct sophia_pqr *s, struct sophia_pqr_request_id id,
                   uint64_t now, uint64_t deadline) {
  struct sophia_pqt transaction;
  struct sophia_pq_candidate candidate = {0};
  size_t slot;
  int e = pqr_advance(s, now);
  if (e)
    return e;
  if (!sophia_pqr_transaction_ready(s) || !sophia_pqr_upload_ready(s, id))
    return SOPHIA_PQS_BUSY;
  slot = pqr_transfer_slot(s, id);
  if (slot == 65)
    return SOPHIA_PQS_INVALID;
  if (s->transfers[slot].cancel_pending ||
      s->transfers[slot].transfer.state != SOPHIA_PQF_ALLOWED)
    return SOPHIA_PQS_BUSY;
  if (s->last_submission == UINT64_MAX)
    return SOPHIA_PQS_OVERFLOW;
  candidate.kind = SOPHIA_PQ_END;
  candidate.data.payload.transfer =
      s->transfers[slot].transfer.expected.transfer;
  candidate.data.payload.total_bytes = s->upload.length;
  e = sophia_pqt_init(&transaction, s->journal.epoch, s->last_submission + 1,
                      &candidate, now, deadline, s->limits.assembly_timeout_ms);
  if (e)
    return e;
  s->upload_phase = PQR_UPLOAD_ENDING;
  ++s->last_submission;
  s->transaction = transaction;
  s->command = SOPHIA_PQ_END;
  s->releasing = 0;
  e = pqr_drive(s);
  return e ? pqr_fail(s, e) : 0;
}
int sophia_pqr_release_upload(struct sophia_pqr *s,
                              struct sophia_pqr_request_id id) {
  if (!same_upload(s, id))
    return SOPHIA_PQS_INVALID;
  if (!s->error && (!sophia_pqu_done(&s->upload) || s->upload_active))
    return SOPHIA_PQS_BUSY;
  s->upload_present = 0;
  s->upload.bytes = NULL;
  return 0;
}
int pqr_drive_upload(struct sophia_pqr *s) {
  struct sophia_pqu_operation op;
  size_t slot;
  uint32_t fid = SOPHIA_9P_NOFID;
  int e;
  if (!s->upload_present || s->upload_active ||
      s->upload_phase == PQR_UPLOAD_BEGIN)
    return 0;
  slot = pqr_transfer_slot(s, s->upload_id);
  if (slot == 65)
    return SOPHIA_PQS_PROTOCOL;
  if (s->transfers[slot].cancel_pending &&
      s->upload_phase != PQR_UPLOAD_SETTLING)
    return 0;
  e = sophia_pqu_operation(&s->upload, s->now, s->wire->msize, &op);
  if (e)
    return e;
  switch (op.kind) {
  case SOPHIA_PQU_NONE:
    return 0;
  case SOPHIA_PQU_WALK: {
    const char *names[] = {"upload", "0"};
    e = sophia_9p_walk(s->wire, s->root, names, 2, &s->upload_tag, &fid);
    break;
  }
  case SOPHIA_PQU_OPEN:
    e = sophia_9p_lopen(s->wire, op.fid, 1, &s->upload_tag);
    break;
  case SOPHIA_PQU_WRITE:
    e = sophia_9p_write(s->wire, op.fid, op.offset, op.bytes, op.length,
                        &s->upload_tag);
    break;
  case SOPHIA_PQU_CLUNK:
    e = sophia_9p_clunk(s->wire, op.fid, &s->upload_tag);
    break;
  default:
    return SOPHIA_PQS_PROTOCOL;
  }
  if (e) {
    s->wire_error = e;
    return SOPHIA_PQR_WIRE;
  }
  e = sophia_pqu_issued(&s->upload, s->now, s->wire->msize, fid);
  if (e)
    return e;
  s->upload_active = 1;
  return 0;
}

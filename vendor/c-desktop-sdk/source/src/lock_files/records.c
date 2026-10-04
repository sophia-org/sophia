#include "../sophia_lock_files.h"
#include <string.h>

static uint64_t get(const uint8_t *p, size_t n) {
  uint64_t v = 0;
  size_t i;
  for (i = 0; i < n; ++i)
    v |= (uint64_t)p[i] << (i * 8);
  return v;
}
static void put(uint8_t *p, uint64_t v, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    p[i] = (uint8_t)(v >> (i * 8));
}
static int zero(const uint8_t *p, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    if (p[i])
      return 0;
  return 1;
}
static int in(uint64_t v, uint64_t low, uint64_t high) {
  return v >= low && v <= high;
}

/* Objects have neither identity; candidates a submission; events a
 * sequence. */
static int header_valid(const struct sophia_lf_header *v) {
  if (!v->epoch)
    return 0;
  switch (v->kind) {
  case SOPHIA_LF_LIMITS:
  case SOPHIA_LF_LOCK:
    return !v->submission && !v->sequence;
  case SOPHIA_LF_NEGOTIATED:
  case SOPHIA_LF_REFUSED:
  case SOPHIA_LF_SUBMITTED:
  case SOPHIA_LF_OBJECT_PUBLISHED:
  case SOPHIA_LF_RESOURCE_STATUS:
  case SOPHIA_LF_RESOURCE_RELEASED:
  case SOPHIA_LF_CANDIDATE_OUTCOME:
  case SOPHIA_LF_FRAME_PERMIT:
  case SOPHIA_LF_ENTRY:
  case SOPHIA_LF_CHORD:
    return !v->submission && v->sequence;
  case SOPHIA_LF_NEGOTIATE:
  case SOPHIA_LF_RESOURCE_BEGIN:
  case SOPHIA_LF_RESOURCE_END:
  case SOPHIA_LF_RESOURCE_CANCEL:
  case SOPHIA_LF_RESOURCE_RETIRE:
  case SOPHIA_LF_CANDIDATE:
  case SOPHIA_LF_FRAME_DEMAND:
    return v->submission && !v->sequence;
  default:
    return 0;
  }
}
static int candidate_kind(uint64_t kind) {
  return kind == SOPHIA_LF_NEGOTIATE ||
         in(kind, SOPHIA_LF_RESOURCE_BEGIN, SOPHIA_LF_FRAME_DEMAND);
}

static int limits_valid(const struct sophia_lf_limits *v) {
  return in(v->max_outputs, 1, SOPHIA_LF_MAX_OUTPUTS) &&
         in(v->upload_slots, 1, 4) && v->max_chords <= SOPHIA_LF_MAX_CHORDS &&
         in(v->max_width_px, 1, SOPHIA_LF_MAX_SIDE) &&
         in(v->max_height_px, 1, SOPHIA_LF_MAX_SIDE) &&
         in(v->max_resource_bytes, 4, SOPHIA_LF_MAX_RESOURCE_BYTES) &&
         in(v->max_live_resources, 2, 32) &&
         in(v->journal_records, 8, 256) &&
         in(v->journal_bytes, 2048, 65536) &&
         in(v->assembly_timeout_ms, 1, 12000) &&
         in(v->ack_timeout_ms, 1, 2000);
}
static int allocation_valid(const struct sophia_lf_allocation *v) {
  return v->output && v->output_generation && v->allocation &&
         v->allocation_generation && in(v->pixel_width, 1, SOPHIA_LF_MAX_SIDE) &&
         in(v->pixel_height, 1, SOPHIA_LF_MAX_SIDE) &&
         in(v->scale_numerator, 1, 32) && in(v->scale_denominator, 1, 4);
}
static int lock_valid(const struct sophia_lf_lock *v) {
  uint16_t i;
  int covers = v->phase == SOPHIA_LF_LOCKING || v->phase == SOPHIA_LF_LOCKED;
  if (!v->topology_generation || !in(v->phase, 1, 4) ||
      v->allocation_count > SOPHIA_LF_MAX_OUTPUTS ||
      (v->phase == SOPHIA_LF_UNLOCKED) != (v->lock_epoch == 0) ||
      (!covers && v->allocation_count))
    return 0;
  for (i = 0; i < v->allocation_count; ++i)
    if (!allocation_valid(&v->allocations[i]))
      return 0;
  return 1;
}
static int chord_valid(const struct sophia_lf_chord_request *v) {
  return v->keysym && v->modifiers && !(v->modifiers & ~SOPHIA_LF_MOD_MASK);
}
static int negotiate_valid(const struct sophia_lf_negotiate *v) {
  uint16_t i;
  if (v->chord_count > SOPHIA_LF_MAX_CHORDS)
    return 0;
  for (i = 0; i < v->chord_count; ++i)
    if (!chord_valid(&v->chords[i]))
      return 0;
  return 1;
}
static int negotiated_valid(const struct sophia_lf_negotiated *v) {
  return v->granted_chords <= SOPHIA_LF_MAX_CHORDS &&
         (v->granted_capabilities & SOPHIA_LF_CAP_PRESENT) &&
         !(v->granted_capabilities &
           ~(uint64_t)(SOPHIA_LF_CAP_PRESENT | SOPHIA_LF_CAP_CHORDS));
}
static int resource_valid(const struct sophia_lf_resource *v) {
  return v->id && v->generation;
}

/* Reads the bytes a kind's body holds. A field list keeps each layout in
 * one place for both directions. */
static void read_resource(const uint8_t *p, struct sophia_lf_resource *v) {
  v->id = get(p, 8);
  v->generation = get(p + 8, 8);
}
static void write_resource(uint8_t *p, const struct sophia_lf_resource *v) {
  put(p, v->id, 8);
  put(p + 8, v->generation, 8);
}

static int body_read(const uint8_t *p, size_t n, struct sophia_lf_record *r) {
  uint16_t i;
  switch (r->header.kind) {
  case SOPHIA_LF_LIMITS: {
    struct sophia_lf_limits *v = &r->value.limits;
    if (n != 48 || get(p, 2) != SOPHIA_LF_REVISION || get(p + 26, 2) != 1 ||
        !zero(p + 44, 4))
      return -1;
    v->max_outputs = (uint16_t)get(p + 2, 2);
    v->upload_slots = (uint16_t)get(p + 4, 2);
    v->max_chords = (uint16_t)get(p + 6, 2);
    v->max_width_px = (uint32_t)get(p + 8, 4);
    v->max_height_px = (uint32_t)get(p + 12, 4);
    v->max_resource_bytes = get(p + 16, 8);
    v->max_live_resources = (uint16_t)get(p + 24, 2);
    v->journal_records = (uint32_t)get(p + 28, 4);
    v->journal_bytes = (uint32_t)get(p + 32, 4);
    v->assembly_timeout_ms = (uint32_t)get(p + 36, 4);
    v->ack_timeout_ms = (uint32_t)get(p + 40, 4);
    return limits_valid(v) ? 0 : -1;
  }
  case SOPHIA_LF_LOCK: {
    struct sophia_lf_lock *v = &r->value.lock;
    if (n < 32 || !zero(p + 20, 12))
      return -1;
    v->lock_epoch = get(p, 8);
    v->topology_generation = get(p + 8, 8);
    v->phase = (uint16_t)get(p + 16, 2);
    v->allocation_count = (uint16_t)get(p + 18, 2);
    if (v->allocation_count > SOPHIA_LF_MAX_OUTPUTS ||
        n != 32 + (size_t)v->allocation_count * 56)
      return -1;
    for (i = 0; i < v->allocation_count; ++i) {
      const uint8_t *row = p + 32 + (size_t)i * 56;
      struct sophia_lf_allocation *a = &v->allocations[i];
      if (!zero(row + 48, 8))
        return -1;
      a->output = get(row, 8);
      a->output_generation = get(row + 8, 8);
      a->allocation = get(row + 16, 8);
      a->allocation_generation = get(row + 24, 8);
      a->pixel_width = (uint32_t)get(row + 32, 4);
      a->pixel_height = (uint32_t)get(row + 36, 4);
      a->scale_numerator = (uint32_t)get(row + 40, 4);
      a->scale_denominator = (uint32_t)get(row + 44, 4);
    }
    return lock_valid(v) ? 0 : -1;
  }
  case SOPHIA_LF_NEGOTIATE: {
    struct sophia_lf_negotiate *v = &r->value.negotiate;
    if (n < 16 || !zero(p + 6, 2))
      return -1;
    v->minimum_revision = (uint16_t)get(p, 2);
    v->maximum_revision = (uint16_t)get(p + 2, 2);
    v->chord_count = (uint16_t)get(p + 4, 2);
    v->capabilities = get(p + 8, 8);
    if (v->chord_count > SOPHIA_LF_MAX_CHORDS ||
        n != 16 + (size_t)v->chord_count * 8)
      return -1;
    for (i = 0; i < v->chord_count; ++i) {
      const uint8_t *row = p + 16 + (size_t)i * 8;
      if (!zero(row + 6, 2))
        return -1;
      v->chords[i].keysym = (uint32_t)get(row, 4);
      v->chords[i].modifiers = (uint16_t)get(row + 4, 2);
    }
    return negotiate_valid(v) ? 0 : -1;
  }
  case SOPHIA_LF_NEGOTIATED:
    if (n != 16 || get(p, 2) != SOPHIA_LF_REVISION || !zero(p + 4, 4))
      return -1;
    r->value.negotiated.granted_chords = (uint16_t)get(p + 2, 2);
    r->value.negotiated.granted_capabilities = get(p + 8, 8);
    return negotiated_valid(&r->value.negotiated) ? 0 : -1;
  case SOPHIA_LF_REFUSED:
    if (n != 8 || !zero(p + 2, 6))
      return -1;
    r->value.refusal = (uint16_t)get(p, 2);
    return in(r->value.refusal, 1, 3) ? 0 : -1;
  case SOPHIA_LF_SUBMITTED:
    if (n != 16 || !zero(p + 10, 6))
      return -1;
    r->value.submitted.submission = get(p, 8);
    r->value.submitted.kind = (uint16_t)get(p + 8, 2);
    return r->value.submitted.submission &&
                   candidate_kind(r->value.submitted.kind)
               ? 0
               : -1;
  case SOPHIA_LF_OBJECT_PUBLISHED:
    if (n != 24 || get(p, 2) != SOPHIA_LF_LOCK || !zero(p + 2, 6))
      return -1;
    r->value.published.object_generation = get(p + 8, 8);
    r->value.published.qid_path = get(p + 16, 8);
    return r->value.published.object_generation && r->value.published.qid_path
               ? 0
               : -1;
  case SOPHIA_LF_RESOURCE_BEGIN: {
    struct sophia_lf_resource_begin *v = &r->value.resource_begin;
    if (n != 40 || get(p + 34, 2) != 1 || !zero(p + 36, 4))
      return -1;
    v->transaction = get(p, 8);
    read_resource(p + 8, &v->resource);
    v->width_px = (uint32_t)get(p + 24, 4);
    v->height_px = (uint32_t)get(p + 28, 4);
    v->slot = (uint16_t)get(p + 32, 2);
    return v->transaction && resource_valid(&v->resource) &&
                   in(v->width_px, 1, SOPHIA_LF_MAX_SIDE) &&
                   in(v->height_px, 1, SOPHIA_LF_MAX_SIDE) && v->slot <= 3
               ? 0
               : -1;
  }
  case SOPHIA_LF_RESOURCE_END:
  case SOPHIA_LF_RESOURCE_CANCEL:
  case SOPHIA_LF_RESOURCE_RETIRE: {
    struct sophia_lf_resource_step *v = &r->value.resource_step;
    int end = r->header.kind == SOPHIA_LF_RESOURCE_END;
    if (n != (end ? 32u : 24u))
      return -1;
    v->transaction = get(p, 8);
    read_resource(p + 8, &v->resource);
    v->total_bytes = end ? get(p + 24, 8) : 0;
    return v->transaction && resource_valid(&v->resource) &&
                   (!end || v->total_bytes)
               ? 0
               : -1;
  }
  case SOPHIA_LF_RESOURCE_STATUS: {
    struct sophia_lf_resource_status *v = &r->value.resource_status;
    if (n != 40 || !zero(p + 28, 4))
      return -1;
    v->transaction = get(p, 8);
    read_resource(p + 8, &v->resource);
    v->status = (uint16_t)get(p + 24, 2);
    v->reason = (uint16_t)get(p + 26, 2);
    v->admitted_bytes = get(p + 32, 8);
    return v->transaction && resource_valid(&v->resource) &&
                   in(v->status, 1, 4)
               ? 0
               : -1;
  }
  case SOPHIA_LF_RESOURCE_RELEASED: {
    struct sophia_lf_resource_released *v = &r->value.resource_released;
    if (n != 32 || !zero(p + 26, 6))
      return -1;
    v->transaction = get(p, 8);
    read_resource(p + 8, &v->resource);
    v->reason = (uint16_t)get(p + 24, 2);
    return v->transaction && resource_valid(&v->resource) ? 0 : -1;
  }
  case SOPHIA_LF_CANDIDATE: {
    struct sophia_lf_candidate *v = &r->value.candidate;
    if (n != 96 || !zero(p + 80, 16))
      return -1;
    v->transaction = get(p, 8);
    v->lock_epoch = get(p + 8, 8);
    v->output = get(p + 16, 8);
    v->output_generation = get(p + 24, 8);
    v->allocation = get(p + 32, 8);
    v->allocation_generation = get(p + 40, 8);
    v->candidate_generation = get(p + 48, 8);
    v->pacing_permit = get(p + 56, 8);
    read_resource(p + 64, &v->resource);
    return v->transaction && v->lock_epoch && v->output &&
                   v->output_generation && v->allocation &&
                   v->allocation_generation && v->candidate_generation &&
                   v->pacing_permit && resource_valid(&v->resource)
               ? 0
               : -1;
  }
  case SOPHIA_LF_CANDIDATE_OUTCOME: {
    struct sophia_lf_candidate_outcome *v = &r->value.candidate_outcome;
    if (n != 56 || !zero(p + 44, 12))
      return -1;
    v->transaction = get(p, 8);
    v->lock_epoch = get(p + 8, 8);
    v->output = get(p + 16, 8);
    v->allocation = get(p + 24, 8);
    v->candidate_generation = get(p + 32, 8);
    v->status = (uint16_t)get(p + 40, 2);
    v->reason = (uint16_t)get(p + 42, 2);
    return v->transaction && v->lock_epoch && v->output && v->allocation &&
                   v->candidate_generation && in(v->status, 1, 5)
               ? 0
               : -1;
  }
  case SOPHIA_LF_FRAME_DEMAND: {
    struct sophia_lf_frame_demand *v = &r->value.frame_demand;
    if (n != 40)
      return -1;
    v->transaction = get(p, 8);
    v->lock_epoch = get(p + 8, 8);
    v->allocation = get(p + 16, 8);
    v->allocation_generation = get(p + 24, 8);
    v->demand = get(p + 32, 8);
    return v->transaction && v->lock_epoch && v->allocation &&
                   v->allocation_generation && v->demand
               ? 0
               : -1;
  }
  case SOPHIA_LF_FRAME_PERMIT: {
    struct sophia_lf_frame_permit *v = &r->value.frame_permit;
    if (n != 48 || !zero(p + 44, 4))
      return -1;
    v->lock_epoch = get(p, 8);
    v->allocation = get(p + 8, 8);
    v->allocation_generation = get(p + 16, 8);
    v->demand = get(p + 24, 8);
    v->pacing_permit = get(p + 32, 8);
    v->expires_after_ms = (uint32_t)get(p + 40, 4);
    return v->lock_epoch && v->allocation && v->allocation_generation &&
                   v->demand && v->pacing_permit &&
                   in(v->expires_after_ms, 1, 250)
               ? 0
               : -1;
  }
  case SOPHIA_LF_ENTRY:
    if (n != 16 || !zero(p + 12, 4))
      return -1;
    r->value.entry.lock_epoch = get(p, 8);
    r->value.entry.entry = (uint16_t)get(p + 8, 2);
    r->value.entry.empty_after = (uint16_t)get(p + 10, 2);
    return r->value.entry.lock_epoch && in(r->value.entry.entry, 1, 7) &&
                   r->value.entry.empty_after <= 1
               ? 0
               : -1;
  case SOPHIA_LF_CHORD:
    if (n != 16 || !zero(p + 10, 6))
      return -1;
    r->value.chord.lock_epoch = get(p, 8);
    r->value.chord.chord = (uint16_t)get(p + 8, 2);
    return r->value.chord.lock_epoch &&
                   r->value.chord.chord < SOPHIA_LF_MAX_CHORDS
               ? 0
               : -1;
  default:
    return -1;
  }
}

int sophia_lf_decode(const void *src, size_t bytes,
                     struct sophia_lf_record *out) {
  const uint8_t *p = src;
  struct sophia_lf_record r;
  if (!src || !out)
    return -4;
  if (bytes < SOPHIA_LF_HEADER_BYTES || bytes > SOPHIA_LF_MAX_RECORD ||
      get(p, 4) != bytes || get(p + 4, 2) != 1)
    return -1;
  memset(&r, 0, sizeof r);
  r.header.kind = (uint16_t)get(p + 6, 2);
  r.header.epoch = get(p + 8, 8);
  r.header.submission = get(p + 16, 8);
  r.header.sequence = get(p + 24, 8);
  if (!header_valid(&r.header) ||
      (candidate_kind(r.header.kind) && bytes > SOPHIA_LF_MAX_CANDIDATE) ||
      body_read(p + SOPHIA_LF_HEADER_BYTES, bytes - SOPHIA_LF_HEADER_BYTES,
                &r))
    return -1;
  *out = r;
  return 0;
}

/* Writes a valid body and its size; -1 when the record breaks a rule. */
static int body_write(uint8_t *p, size_t capacity,
                      const struct sophia_lf_record *r, size_t *n) {
  uint16_t i;
  uint8_t body[32 + SOPHIA_LF_MAX_OUTPUTS * 56];
  size_t size;
  memset(body, 0, sizeof body);
  switch (r->header.kind) {
  case SOPHIA_LF_LIMITS: {
    const struct sophia_lf_limits *v = &r->value.limits;
    if (!limits_valid(v))
      return -1;
    put(body, SOPHIA_LF_REVISION, 2);
    put(body + 2, v->max_outputs, 2);
    put(body + 4, v->upload_slots, 2);
    put(body + 6, v->max_chords, 2);
    put(body + 8, v->max_width_px, 4);
    put(body + 12, v->max_height_px, 4);
    put(body + 16, v->max_resource_bytes, 8);
    put(body + 24, v->max_live_resources, 2);
    put(body + 26, 1, 2);
    put(body + 28, v->journal_records, 4);
    put(body + 32, v->journal_bytes, 4);
    put(body + 36, v->assembly_timeout_ms, 4);
    put(body + 40, v->ack_timeout_ms, 4);
    size = 48;
    break;
  }
  case SOPHIA_LF_LOCK: {
    const struct sophia_lf_lock *v = &r->value.lock;
    if (!lock_valid(v))
      return -1;
    put(body, v->lock_epoch, 8);
    put(body + 8, v->topology_generation, 8);
    put(body + 16, v->phase, 2);
    put(body + 18, v->allocation_count, 2);
    for (i = 0; i < v->allocation_count; ++i) {
      uint8_t *row = body + 32 + (size_t)i * 56;
      const struct sophia_lf_allocation *a = &v->allocations[i];
      put(row, a->output, 8);
      put(row + 8, a->output_generation, 8);
      put(row + 16, a->allocation, 8);
      put(row + 24, a->allocation_generation, 8);
      put(row + 32, a->pixel_width, 4);
      put(row + 36, a->pixel_height, 4);
      put(row + 40, a->scale_numerator, 4);
      put(row + 44, a->scale_denominator, 4);
    }
    size = 32 + (size_t)v->allocation_count * 56;
    break;
  }
  case SOPHIA_LF_NEGOTIATE: {
    const struct sophia_lf_negotiate *v = &r->value.negotiate;
    if (!negotiate_valid(v))
      return -1;
    put(body, v->minimum_revision, 2);
    put(body + 2, v->maximum_revision, 2);
    put(body + 4, v->chord_count, 2);
    put(body + 8, v->capabilities, 8);
    for (i = 0; i < v->chord_count; ++i) {
      put(body + 16 + (size_t)i * 8, v->chords[i].keysym, 4);
      put(body + 20 + (size_t)i * 8, v->chords[i].modifiers, 2);
    }
    size = 16 + (size_t)v->chord_count * 8;
    break;
  }
  case SOPHIA_LF_NEGOTIATED:
    if (!negotiated_valid(&r->value.negotiated))
      return -1;
    put(body, SOPHIA_LF_REVISION, 2);
    put(body + 2, r->value.negotiated.granted_chords, 2);
    put(body + 8, r->value.negotiated.granted_capabilities, 8);
    size = 16;
    break;
  case SOPHIA_LF_REFUSED:
    if (!in(r->value.refusal, 1, 3))
      return -1;
    put(body, r->value.refusal, 2);
    size = 8;
    break;
  case SOPHIA_LF_SUBMITTED:
    if (!r->value.submitted.submission ||
        !candidate_kind(r->value.submitted.kind))
      return -1;
    put(body, r->value.submitted.submission, 8);
    put(body + 8, r->value.submitted.kind, 2);
    size = 16;
    break;
  case SOPHIA_LF_OBJECT_PUBLISHED:
    if (!r->value.published.object_generation || !r->value.published.qid_path)
      return -1;
    put(body, SOPHIA_LF_LOCK, 2);
    put(body + 8, r->value.published.object_generation, 8);
    put(body + 16, r->value.published.qid_path, 8);
    size = 24;
    break;
  case SOPHIA_LF_RESOURCE_BEGIN: {
    const struct sophia_lf_resource_begin *v = &r->value.resource_begin;
    if (!v->transaction || !resource_valid(&v->resource) ||
        !in(v->width_px, 1, SOPHIA_LF_MAX_SIDE) ||
        !in(v->height_px, 1, SOPHIA_LF_MAX_SIDE) || v->slot > 3)
      return -1;
    put(body, v->transaction, 8);
    write_resource(body + 8, &v->resource);
    put(body + 24, v->width_px, 4);
    put(body + 28, v->height_px, 4);
    put(body + 32, v->slot, 2);
    put(body + 34, 1, 2);
    size = 40;
    break;
  }
  case SOPHIA_LF_RESOURCE_END:
  case SOPHIA_LF_RESOURCE_CANCEL:
  case SOPHIA_LF_RESOURCE_RETIRE: {
    const struct sophia_lf_resource_step *v = &r->value.resource_step;
    int end = r->header.kind == SOPHIA_LF_RESOURCE_END;
    if (!v->transaction || !resource_valid(&v->resource) ||
        (end ? !v->total_bytes : v->total_bytes != 0))
      return -1;
    put(body, v->transaction, 8);
    write_resource(body + 8, &v->resource);
    if (end)
      put(body + 24, v->total_bytes, 8);
    size = end ? 32 : 24;
    break;
  }
  case SOPHIA_LF_RESOURCE_STATUS: {
    const struct sophia_lf_resource_status *v = &r->value.resource_status;
    if (!v->transaction || !resource_valid(&v->resource) ||
        !in(v->status, 1, 4))
      return -1;
    put(body, v->transaction, 8);
    write_resource(body + 8, &v->resource);
    put(body + 24, v->status, 2);
    put(body + 26, v->reason, 2);
    put(body + 32, v->admitted_bytes, 8);
    size = 40;
    break;
  }
  case SOPHIA_LF_RESOURCE_RELEASED: {
    const struct sophia_lf_resource_released *v = &r->value.resource_released;
    if (!v->transaction || !resource_valid(&v->resource))
      return -1;
    put(body, v->transaction, 8);
    write_resource(body + 8, &v->resource);
    put(body + 24, v->reason, 2);
    size = 32;
    break;
  }
  case SOPHIA_LF_CANDIDATE: {
    const struct sophia_lf_candidate *v = &r->value.candidate;
    if (!v->transaction || !v->lock_epoch || !v->output ||
        !v->output_generation || !v->allocation || !v->allocation_generation ||
        !v->candidate_generation || !v->pacing_permit ||
        !resource_valid(&v->resource))
      return -1;
    put(body, v->transaction, 8);
    put(body + 8, v->lock_epoch, 8);
    put(body + 16, v->output, 8);
    put(body + 24, v->output_generation, 8);
    put(body + 32, v->allocation, 8);
    put(body + 40, v->allocation_generation, 8);
    put(body + 48, v->candidate_generation, 8);
    put(body + 56, v->pacing_permit, 8);
    write_resource(body + 64, &v->resource);
    size = 96;
    break;
  }
  case SOPHIA_LF_CANDIDATE_OUTCOME: {
    const struct sophia_lf_candidate_outcome *v = &r->value.candidate_outcome;
    if (!v->transaction || !v->lock_epoch || !v->output || !v->allocation ||
        !v->candidate_generation || !in(v->status, 1, 5))
      return -1;
    put(body, v->transaction, 8);
    put(body + 8, v->lock_epoch, 8);
    put(body + 16, v->output, 8);
    put(body + 24, v->allocation, 8);
    put(body + 32, v->candidate_generation, 8);
    put(body + 40, v->status, 2);
    put(body + 42, v->reason, 2);
    size = 56;
    break;
  }
  case SOPHIA_LF_FRAME_DEMAND: {
    const struct sophia_lf_frame_demand *v = &r->value.frame_demand;
    if (!v->transaction || !v->lock_epoch || !v->allocation ||
        !v->allocation_generation || !v->demand)
      return -1;
    put(body, v->transaction, 8);
    put(body + 8, v->lock_epoch, 8);
    put(body + 16, v->allocation, 8);
    put(body + 24, v->allocation_generation, 8);
    put(body + 32, v->demand, 8);
    size = 40;
    break;
  }
  case SOPHIA_LF_FRAME_PERMIT: {
    const struct sophia_lf_frame_permit *v = &r->value.frame_permit;
    if (!v->lock_epoch || !v->allocation || !v->allocation_generation ||
        !v->demand || !v->pacing_permit || !in(v->expires_after_ms, 1, 250))
      return -1;
    put(body, v->lock_epoch, 8);
    put(body + 8, v->allocation, 8);
    put(body + 16, v->allocation_generation, 8);
    put(body + 24, v->demand, 8);
    put(body + 32, v->pacing_permit, 8);
    put(body + 40, v->expires_after_ms, 4);
    size = 48;
    break;
  }
  case SOPHIA_LF_ENTRY:
    if (!r->value.entry.lock_epoch || !in(r->value.entry.entry, 1, 7) ||
        r->value.entry.empty_after > 1)
      return -1;
    put(body, r->value.entry.lock_epoch, 8);
    put(body + 8, r->value.entry.entry, 2);
    put(body + 10, r->value.entry.empty_after, 2);
    size = 16;
    break;
  case SOPHIA_LF_CHORD:
    if (!r->value.chord.lock_epoch ||
        r->value.chord.chord >= SOPHIA_LF_MAX_CHORDS)
      return -1;
    put(body, r->value.chord.lock_epoch, 8);
    put(body + 8, r->value.chord.chord, 2);
    size = 16;
    break;
  default:
    return -1;
  }
  if (size > capacity)
    return -4;
  memcpy(p, body, size);
  *n = size;
  return 0;
}

int sophia_lf_encode(void *dst, size_t capacity,
                     const struct sophia_lf_record *record, size_t *bytes) {
  uint8_t *p = dst;
  size_t body = 0, size;
  int status;
  if (!dst || !record || !bytes || capacity < SOPHIA_LF_HEADER_BYTES)
    return -4;
  if (!header_valid(&record->header))
    return -1;
  status = body_write(p + SOPHIA_LF_HEADER_BYTES,
                      capacity - SOPHIA_LF_HEADER_BYTES, record, &body);
  if (status)
    return status;
  size = SOPHIA_LF_HEADER_BYTES + body;
  if (candidate_kind(record->header.kind) && size > SOPHIA_LF_MAX_CANDIDATE)
    return -1;
  put(p, size, 4);
  put(p + 4, 1, 2);
  put(p + 6, record->header.kind, 2);
  put(p + 8, record->header.epoch, 8);
  put(p + 16, record->header.submission, 8);
  put(p + 24, record->header.sequence, 8);
  *bytes = size;
  return 0;
}

int sophia_lf_submit_encode(uint8_t dst[24], uint64_t epoch,
                            uint64_t submission, uint32_t bytes) {
  if (!dst)
    return -4;
  if (!epoch || !submission ||
      !in(bytes, SOPHIA_LF_MIN_CANDIDATE, SOPHIA_LF_MAX_CANDIDATE))
    return -1;
  memset(dst, 0, 24);
  put(dst, epoch, 8);
  put(dst + 8, submission, 8);
  put(dst + 16, bytes, 4);
  return 0;
}

int sophia_lf_ack_encode(uint8_t dst[16], uint64_t epoch, uint64_t sequence) {
  if (!dst)
    return -4;
  if (!epoch || !sequence)
    return -1;
  put(dst, epoch, 8);
  put(dst + 8, sequence, 8);
  return 0;
}

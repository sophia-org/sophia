#include "internal.h"

static const uint16_t fixed_limits[7] = {
    SOPHIA_OF_REVISION,        SOPHIA_OF_MAX_HEADS, SOPHIA_OF_MAX_GROUPS,
    SOPHIA_OF_MAX_MODES_PER_HEAD, SOPHIA_OF_MAX_MEMBERS, SOPHIA_OF_MAX_LABEL,
    SOPHIA_OF_MAX_MODES};

int of_header(const struct sophia_of_header *v) {
  if (!v->epoch)
    return -1;
  switch (v->kind) {
  case SOPHIA_OF_LIMITS:
  case SOPHIA_OF_TOPOLOGY:
    return !v->submission && !v->sequence ? 0 : -1;
  case SOPHIA_OF_NEGOTIATED:
  case SOPHIA_OF_REFUSED:
  case SOPHIA_OF_SUBMITTED:
  case SOPHIA_OF_OBJECT_PUBLISHED:
  case SOPHIA_OF_OUTCOME:
    return !v->submission && v->sequence ? 0 : -1;
  case SOPHIA_OF_NEGOTIATE:
  case SOPHIA_OF_PROPOSAL:
    return v->submission && !v->sequence ? 0 : -1;
  default:
    return -1;
  }
}
void of_header_write(uint8_t *p, const struct sophia_of_header *v,
                     size_t size) {
  of_put(p, size, 4);
  of_put(p + 4, 1, 2);
  of_put(p + 6, v->kind, 2);
  of_put(p + 8, v->epoch, 8);
  of_put(p + 16, v->submission, 8);
  of_put(p + 24, v->sequence, 8);
}
static int limits_valid(const struct sophia_of_limits *v) {
  return v->journal_records >= 8 && v->journal_records <= 64 &&
         v->journal_bytes >= 2048 && v->journal_bytes <= 16384 &&
         v->staging_bytes == SOPHIA_OF_MAX_CANDIDATE &&
         v->assembly_timeout_ms >= 1 && v->assembly_timeout_ms <= 12000 &&
         v->ack_timeout_ms >= 1 && v->ack_timeout_ms <= 2000 &&
         v->max_domain_transactions >= 1 &&
         v->max_domain_transactions <= 4096;
}
static int limits_read(const uint8_t *p, size_t n, struct sophia_of_limits *v) {
  unsigned i;
  if (n != 40)
    return -1;
  for (i = 0; i < 7; ++i)
    if (of_get(p + 2 * i, 2) != fixed_limits[i])
      return -1;
  if (!of_zero(p + 14, 2))
    return -1;
  v->journal_records = (uint32_t)of_get(p + 16, 4);
  v->journal_bytes = (uint32_t)of_get(p + 20, 4);
  v->staging_bytes = (uint32_t)of_get(p + 24, 4);
  v->assembly_timeout_ms = (uint32_t)of_get(p + 28, 4);
  v->ack_timeout_ms = (uint32_t)of_get(p + 32, 4);
  v->max_domain_transactions = (uint32_t)of_get(p + 36, 4);
  return limits_valid(v) ? 0 : -1;
}
static int granted_valid(uint64_t caps) {
  return (caps & SOPHIA_OF_CAP_OBSERVE) &&
         !(caps & ~(uint64_t)(SOPHIA_OF_CAP_OBSERVE | SOPHIA_OF_CAP_CONFIGURE));
}
/* Body bytes for every fixed-size kind; the proposal is variable. */
static int body_read(const uint8_t *p, size_t n, struct sophia_of_record *r) {
  switch (r->header.kind) {
  case SOPHIA_OF_LIMITS:
    return limits_read(p, n, &r->value.limits);
  case SOPHIA_OF_NEGOTIATE:
    if (n != 16 || !of_zero(p + 4, 4))
      return -1;
    r->value.negotiate.minimum_revision = (uint16_t)of_get(p, 2);
    r->value.negotiate.maximum_revision = (uint16_t)of_get(p + 2, 2);
    r->value.negotiate.capabilities = of_get(p + 8, 8);
    return 0;
  case SOPHIA_OF_NEGOTIATED:
    if (n != 24 || of_get(p, 2) != SOPHIA_OF_REVISION || !of_zero(p + 2, 6) ||
        of_get(p + 16, 2) != SOPHIA_OF_MAX_HEADS ||
        of_get(p + 18, 2) != SOPHIA_OF_MAX_GROUPS ||
        of_get(p + 20, 2) != SOPHIA_OF_MAX_MODES_PER_HEAD ||
        of_get(p + 22, 2) != SOPHIA_OF_MAX_MEMBERS)
      return -1;
    r->value.granted_capabilities = of_get(p + 8, 8);
    return granted_valid(r->value.granted_capabilities) ? 0 : -1;
  case SOPHIA_OF_REFUSED:
    if (n != 8 || !of_zero(p + 2, 6))
      return -1;
    r->value.refusal = (uint16_t)of_get(p, 2);
    return r->value.refusal == SOPHIA_OF_UNSUPPORTED_REVISION ||
                   r->value.refusal == SOPHIA_OF_OBSERVATION_REQUIRED
               ? 0
               : -1;
  case SOPHIA_OF_SUBMITTED:
    if (n != 16 || !of_zero(p + 10, 6))
      return -1;
    r->value.submitted.submission = of_get(p, 8);
    r->value.submitted.kind = (uint16_t)of_get(p + 8, 2);
    return r->value.submitted.submission &&
                   (r->value.submitted.kind == SOPHIA_OF_NEGOTIATE ||
                    r->value.submitted.kind == SOPHIA_OF_PROPOSAL)
               ? 0
               : -1;
  case SOPHIA_OF_OBJECT_PUBLISHED:
    if (n != 24 || of_get(p, 2) != SOPHIA_OF_TOPOLOGY || !of_zero(p + 2, 6))
      return -1;
    r->value.published.topology_epoch = of_get(p + 8, 8);
    r->value.published.qid_path = of_get(p + 16, 8);
    return r->value.published.topology_epoch && r->value.published.qid_path
               ? 0
               : -1;
  case SOPHIA_OF_OUTCOME:
    if (n != 24 || !of_zero(p + 20, 4))
      return -1;
    r->value.outcome.transaction = of_get(p, 8);
    r->value.outcome.topology_epoch = of_get(p + 8, 8);
    r->value.outcome.outcome = (uint16_t)of_get(p + 16, 2);
    r->value.outcome.reason = (uint16_t)of_get(p + 18, 2);
    return r->value.outcome.transaction && r->value.outcome.topology_epoch &&
                   r->value.outcome.outcome >= SOPHIA_OF_VALIDATED &&
                   r->value.outcome.outcome <= SOPHIA_OF_FAILED
               ? 0
               : -1;
  case SOPHIA_OF_PROPOSAL:
    return of_proposal_read(p, n, &r->value.proposal);
  default:
    return -1;
  }
}
/* Writes the body into p (capacity n); the result is checked by decoding. */
static int body_write(uint8_t *p, size_t n, const struct sophia_of_record *r,
                      size_t *size) {
  unsigned i;
  if (r->header.kind == SOPHIA_OF_PROPOSAL)
    return of_proposal_write(p, n, &r->value.proposal, size);
  if (n < 40)
    return -4;
  memset(p, 0, 40);
  switch (r->header.kind) {
  case SOPHIA_OF_LIMITS: {
    const struct sophia_of_limits *v = &r->value.limits;
    for (i = 0; i < 7; ++i)
      of_put(p + 2 * i, fixed_limits[i], 2);
    of_put(p + 16, v->journal_records, 4);
    of_put(p + 20, v->journal_bytes, 4);
    of_put(p + 24, v->staging_bytes, 4);
    of_put(p + 28, v->assembly_timeout_ms, 4);
    of_put(p + 32, v->ack_timeout_ms, 4);
    of_put(p + 36, v->max_domain_transactions, 4);
    *size = 40;
    return 0;
  }
  case SOPHIA_OF_NEGOTIATE:
    of_put(p, r->value.negotiate.minimum_revision, 2);
    of_put(p + 2, r->value.negotiate.maximum_revision, 2);
    of_put(p + 8, r->value.negotiate.capabilities, 8);
    *size = 16;
    return 0;
  case SOPHIA_OF_NEGOTIATED:
    of_put(p, SOPHIA_OF_REVISION, 2);
    of_put(p + 8, r->value.granted_capabilities, 8);
    of_put(p + 16, SOPHIA_OF_MAX_HEADS, 2);
    of_put(p + 18, SOPHIA_OF_MAX_GROUPS, 2);
    of_put(p + 20, SOPHIA_OF_MAX_MODES_PER_HEAD, 2);
    of_put(p + 22, SOPHIA_OF_MAX_MEMBERS, 2);
    *size = 24;
    return 0;
  case SOPHIA_OF_REFUSED:
    of_put(p, r->value.refusal, 2);
    *size = 8;
    return 0;
  case SOPHIA_OF_SUBMITTED:
    of_put(p, r->value.submitted.submission, 8);
    of_put(p + 8, r->value.submitted.kind, 2);
    *size = 16;
    return 0;
  case SOPHIA_OF_OBJECT_PUBLISHED:
    of_put(p, SOPHIA_OF_TOPOLOGY, 2);
    of_put(p + 8, r->value.published.topology_epoch, 8);
    of_put(p + 16, r->value.published.qid_path, 8);
    *size = 24;
    return 0;
  case SOPHIA_OF_OUTCOME:
    of_put(p, r->value.outcome.transaction, 8);
    of_put(p + 8, r->value.outcome.topology_epoch, 8);
    of_put(p + 16, r->value.outcome.outcome, 2);
    of_put(p + 18, r->value.outcome.reason, 2);
    *size = 24;
    return 0;
  default:
    return -1;
  }
}
int sophia_of_decode(const void *src, size_t size,
                     struct sophia_of_record *out) {
  const uint8_t *p = src;
  struct sophia_of_record r;
  if (!src || !out)
    return -4;
  if (size < SOPHIA_OF_HEADER_BYTES || size > SOPHIA_OF_MAX_RECORD ||
      of_get(p, 4) != size || of_get(p + 4, 2) != 1)
    return -1;
  memset(&r, 0, sizeof(r));
  r.header.kind = (uint16_t)of_get(p + 6, 2);
  r.header.epoch = of_get(p + 8, 8);
  r.header.submission = of_get(p + 16, 8);
  r.header.sequence = of_get(p + 24, 8);
  if (r.header.kind == SOPHIA_OF_TOPOLOGY || of_header(&r.header) ||
      (r.header.kind >= SOPHIA_OF_NEGOTIATE && size > SOPHIA_OF_MAX_CANDIDATE) ||
      body_read(p + 32, size - 32, &r))
    return -1;
  *out = r;
  return 0;
}
int sophia_of_encode(void *dst, size_t capacity,
                     const struct sophia_of_record *r, size_t *bytes) {
  uint8_t body[SOPHIA_OF_MAX_CANDIDATE];
  struct sophia_of_record check;
  size_t size = 0;
  int result;
  if (!dst || !r || !bytes)
    return -4;
  if (r->header.kind == SOPHIA_OF_TOPOLOGY || of_header(&r->header))
    return -1;
  result = body_write(body, sizeof(body) - 32, r, &size);
  if (result)
    return result;
  /* Decoding the encoded body applies exactly the same wire rules. */
  check.header = r->header;
  if (body_read(body, size, &check))
    return -1;
  if (capacity < 32 + size)
    return -4;
  of_header_write(dst, &r->header, 32 + size);
  memcpy((uint8_t *)dst + 32, body, size);
  *bytes = 32 + size;
  return 0;
}
int sophia_of_submit_encode(uint8_t dst[24], uint64_t epoch,
                            uint64_t submission, uint32_t bytes) {
  if (!dst)
    return -4;
  if (!epoch || !submission || bytes < SOPHIA_OF_MIN_CANDIDATE ||
      bytes > SOPHIA_OF_MAX_CANDIDATE)
    return -1;
  of_put(dst, epoch, 8);
  of_put(dst + 8, submission, 8);
  of_put(dst + 16, bytes, 4);
  of_put(dst + 20, 0, 4);
  return 0;
}
int sophia_of_ack_encode(uint8_t dst[16], uint64_t epoch, uint64_t sequence) {
  if (!dst)
    return -4;
  if (!epoch || !sequence)
    return -1;
  of_put(dst, epoch, 8);
  of_put(dst + 8, sequence, 8);
  return 0;
}

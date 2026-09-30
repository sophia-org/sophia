#ifndef SOPHIA_OUTPUT_FILES_INTERNAL_H
#define SOPHIA_OUTPUT_FILES_INTERNAL_H
#include "../sophia_output_files.h"
#include <limits.h>
#include <string.h>
static inline uint64_t of_get(const uint8_t *p, size_t n) {
  uint64_t v = 0;
  size_t i;
  for (i = 0; i < n; ++i)
    v |= (uint64_t)p[i] << (i * 8);
  return v;
}
static inline int32_t of_signed(const uint8_t *p) {
  uint64_t v = of_get(p, 4);
  return v > INT32_MAX ? -(int32_t)(UINT32_MAX - v) - 1 : (int32_t)v;
}
static inline void of_put(uint8_t *p, uint64_t v, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    p[i] = (uint8_t)(v >> (i * 8));
}
static inline int of_zero(const uint8_t *p, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    if (p[i])
      return 0;
  return 1;
}
/* Kind classes and complete header identity rules. */
int of_header(const struct sophia_of_header *);
void of_header_write(uint8_t *, const struct sophia_of_header *, size_t size);
/* Member rows: 12 bytes each, four slots, unused slots all zero. */
int of_members_read(const uint8_t *, uint16_t count,
                    struct sophia_of_member *);
void of_members_write(uint8_t *, uint16_t count,
                      const struct sophia_of_member *);
int of_proposal_read(const uint8_t *, size_t, struct sophia_of_proposal *);
int of_proposal_write(uint8_t *, size_t, const struct sophia_of_proposal *,
                      size_t *);
#endif

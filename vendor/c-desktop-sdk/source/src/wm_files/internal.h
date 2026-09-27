#ifndef SOPHIA_WM_FILES_INTERNAL_H
#define SOPHIA_WM_FILES_INTERNAL_H
#include "../sophia_wm_files.h"
#include <limits.h>
#include <string.h>
static inline uint64_t wf_get(const uint8_t *p, size_t n) {
  uint64_t v = 0;
  size_t i;
  for (i = 0; i < n; ++i)
    v |= (uint64_t)p[i] << (i * 8);
  return v;
}
static inline int32_t wf_signed(const uint8_t *p, size_t n) {
  uint64_t v = wf_get(p, n);
  return v > INT32_MAX ? -(int32_t)(UINT32_MAX - v) - 1 : (int32_t)v;
}
static inline void wf_put(uint8_t *p, uint64_t v, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    p[i] = (uint8_t)(v >> (i * 8));
}
static inline int wf_zero(const uint8_t *p, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    if (p[i])
      return 0;
  return 1;
}
static inline int wf_surface(struct sophia_wf_surface v, int optional) {
  return (optional && !v.index && !v.generation) ||
         (v.index != UINT32_MAX && v.generation);
}
int wf_row_layout(uint16_t family, uint16_t kind, size_t *width,
                  uint32_t *maximum, uint64_t *capabilities);
int wf_row_reserved(uint16_t family, uint16_t kind, const uint8_t *row);
int wf_sections(const struct sophia_wf_record *, uint64_t, size_t *);
int wf_prefix_read(const uint8_t *, size_t, uint64_t, struct sophia_wf_record *,
                   size_t *);
int wf_prefix_write(uint8_t *, const struct sophia_wf_record *, size_t *);
#endif

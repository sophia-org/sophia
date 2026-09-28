#ifndef SOPHIA_SF_DESCRIPTORS_INTERNAL_H
#define SOPHIA_SF_DESCRIPTORS_INTERNAL_H
#include "roles_internal.h"
static inline int sf_descriptor_kind(unsigned k)
{
    return (k >= 5 && k <= 7) || (k >= 46 && k <= 53) || (k >= 273 && k <= 278);
}
static inline int sf_descriptor_object_kind(unsigned k) { return k >= 5 && k <= 7; }
static inline int sf_descriptor_candidate_kind(unsigned k)
{
    return k == 273 || k == 275 || k == 276 || k == 277;
}
size_t sf_descriptor_size(const struct sophia_sf_record *);
int sf_descriptor_check(const struct sophia_sf_record *);
void sf_descriptor_put(uint8_t *, const struct sophia_sf_record *);
int sf_descriptor_take(const uint8_t *, size_t, struct sophia_sf_record *);
size_t sf_descriptor_candidate_size(const struct sophia_sf_record *);
int sf_descriptor_candidate_check(const struct sophia_sf_record *);
void sf_descriptor_candidate_put(uint8_t *, const struct sophia_sf_record *);
int sf_descriptor_candidate_take(const uint8_t *, size_t, struct sophia_sf_record *);
int sf_reference_candidate_check(const struct sophia_sf_reference_candidate *, uint64_t);
void sf_reference_candidate_put(uint8_t *, const struct sophia_sf_reference_candidate *);
int sf_reference_candidate_take(const uint8_t *, size_t, struct sophia_sf_reference_candidate *);
size_t sf_descriptor_object_size(const struct sophia_sf_record *);
int sf_descriptor_object_check(const struct sophia_sf_record *);
void sf_descriptor_object_put(uint8_t *, const struct sophia_sf_record *);
int sf_descriptor_object_take(const uint8_t *, size_t, struct sophia_sf_record *);
int sf_descriptor_group_order_unique(const uint8_t *, size_t, size_t);
#endif

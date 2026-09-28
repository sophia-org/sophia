#ifndef SOPHIA_SF_ROLES_INTERNAL_H
#define SOPHIA_SF_ROLES_INTERNAL_H
#include "internal.h"
static inline int sf_role_kind(unsigned k)
{
    return k == 3 || k == 4 || (k >= 38 && k <= 45) || (k >= 266 && k <= 272);
}
int sf_text_check(struct sophia_sf_text, size_t);
int sf_launcher_text_check(struct sophia_sf_text, size_t);
int sf_descriptor_text_check(struct sophia_sf_text, size_t);
int sf_text_take(const uint8_t *, size_t, struct sophia_sf_text *);
void sf_text_put(uint8_t *, size_t, struct sophia_sf_text);
size_t sf_role_size(const struct sophia_sf_record *);
int sf_role_check(const struct sophia_sf_record *);
void sf_role_put(uint8_t *, const struct sophia_sf_record *);
int sf_role_take(const uint8_t *, size_t, struct sophia_sf_record *);
int sf_catalog_check(const struct sophia_sf_catalog *);
void sf_catalog_put(uint8_t *, const struct sophia_sf_catalog *);
int sf_catalog_take(const uint8_t *, size_t, struct sophia_sf_catalog *);
int sf_indicators_check(const struct sophia_sf_indicators *);
void sf_indicators_put(uint8_t *, const struct sophia_sf_indicators *);
int sf_indicators_take(const uint8_t *, size_t, struct sophia_sf_indicators *);
int sf_native_check(const struct sophia_sf_record *);
void sf_native_put(uint8_t *, const struct sophia_sf_record *);
int sf_native_take(const uint8_t *, size_t, struct sophia_sf_record *);
int sf_role_candidate_check(const struct sophia_sf_role_candidate *, int);
void sf_role_candidate_put(uint8_t *, const struct sophia_sf_role_candidate *, int);
int sf_role_candidate_take(const uint8_t *, size_t, struct sophia_sf_role_candidate *, int);
#endif

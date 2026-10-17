#ifndef SOPHIA_ADMIN_FILES_INTERNAL_H
#define SOPHIA_ADMIN_FILES_INTERNAL_H
#include "../sophia_admin_files.h"
int sophia_af_name_valid(const char *name, size_t n);
int sophia_af_number(const unsigned char *p, size_t length, size_t *pos,
                     unsigned char delimiter, uint64_t *out);
#endif

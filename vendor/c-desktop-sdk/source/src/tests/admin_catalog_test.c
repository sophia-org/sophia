#include "sophia_admin_files.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

int main(void) {
  const char *wire = "v1 catalog 12 2\nv1 command policy 8:next tab\nv1 command session 10:restart-wm\n";
  const char *bad[] = {
    "v1 catalog 0 0\n", "v1 catalog 01 0\n", "v1 catalog 1 00\n", "v1 catalog 1 260\n",
    "v1 catalog 1 18446744073709551616\n", "v1 catalog 1 0 \n", "v1 catalog 1 1\n",
    "v1 catalog 1 0\nv1 command policy 1:a\n",
    "v1 catalog 1 2\nv1 command policy 1:a\nv1 command policy 1:a\n",
    "v1 catalog 1 2\nv1 command policy 1:b\nv1 command policy 1:a\n",
    "v1 catalog 1 2\nv1 command session 6:logout\nv1 command policy 1:a\n",
    "v1 catalog 1 1\nv1 command session 6:launch\n",
    "v1 catalog 1 1\nv1 command other 1:a\n", "v1 catalog 1 1\nv1 command policy 0:\n",
    "v1 catalog 1 1\nv1 command policy 1:ab\n", "v1 catalog 1 1\nv1 command policy 2: a\n",
    "v1 catalog 1 1\nv1 command policy 2:\xc3\xa9\n", "v1 catalog 1 0\n\n"
  };
  struct sophia_af_catalog catalog;
  struct sophia_af_command command = {SOPHIA_AF_POLICY, "next tab", 8};
  char full[10000], name[130], oversized[SOPHIA_AF_CATALOG_MAX_BYTES + 1];
  size_t i, pos;
  assert(!sophia_af_decode_catalog(wire, strlen(wire), &catalog));
  assert(catalog.generation == 12 && catalog.count == 2);
  assert(catalog.rows[1].name == wire + strlen(wire) - 11);
  assert(sophia_af_catalog_contains(&catalog, &command));
  command.owner = SOPHIA_AF_SESSION;
  assert(!sophia_af_catalog_contains(&catalog, &command));
  for (i = 0; i < strlen(wire); ++i) {
    assert(sophia_af_decode_catalog(wire, i, &catalog) == -1);
    assert(catalog.generation == 12 && catalog.count == 2 && catalog.rows[1].name == wire + strlen(wire) - 11);
  }
  for (i = 0; i < sizeof bad / sizeof bad[0]; ++i)
    assert(sophia_af_decode_catalog(bad[i], strlen(bad[i]), &catalog) == -1);
  pos = (size_t)snprintf(full, sizeof full, "v1 catalog 1 259\n");
  for (i = 0; i < 256; ++i) pos += (size_t)snprintf(full+pos, sizeof full-pos, "v1 command policy 4:p%03zu\n", i);
  pos += (size_t)snprintf(full+pos, sizeof full-pos, "v1 command session 6:logout\nv1 command session 14:reload-profile\nv1 command session 10:restart-wm\n");
  assert(!sophia_af_decode_catalog(full, pos, &catalog) && catalog.count == 259);
  command.owner = SOPHIA_AF_POLICY; command.name = "p255"; command.name_length = 4;
  assert(sophia_af_catalog_contains(&catalog, &command));
  pos = (size_t)snprintf(full, sizeof full, "v1 catalog 1 257\n");
  for (i = 0; i < 257; ++i) pos += (size_t)snprintf(full+pos, sizeof full-pos, "v1 command policy 4:p%03zu\n", i);
  assert(sophia_af_decode_catalog(full, pos, &catalog) == -1);
  for (i = 128; i <= 129; ++i) {
    memset(name, 'a', i); name[i] = 0;
    pos = (size_t)snprintf(full, sizeof full, "v1 catalog 1 1\nv1 command policy %zu:%s\n", i, name);
    assert((sophia_af_decode_catalog(full, pos, &catalog) == 0) == (i == 128));
  }
  wire = "v1 catalog 18446744073709551615 0\n";
  assert(!sophia_af_decode_catalog(wire, strlen(wire), &catalog));
  assert(catalog.generation == UINT64_MAX && !catalog.count);
  assert(!sophia_af_catalog_contains(&catalog, &command));
  memset(oversized, 'x', sizeof oversized);
  assert(sophia_af_decode_catalog(oversized, sizeof oversized, &catalog) == -1);
  assert(sophia_af_decode_catalog(NULL, 0, &catalog) == -1);
  assert(sophia_af_decode_catalog(wire, strlen(wire), NULL) == -1);
  return 0;
}

#include "sophia_admin_files.h"
#include <assert.h>
#include <string.h>

int main(void) {
  const char *wire = "v1 invoke 7 9 12 policy 8:next tab\n";
  const char *bad[] = {
    "v1 invoke 0 9 12 policy 8:next tab\n", "v1 invoke 7 0 12 policy 8:next tab\n",
    "v1 invoke 7 9 0 policy 8:next tab\n", "v1 invoke 07 9 12 policy 8:next tab\n",
    "v1 invoke 18446744073709551616 9 12 policy 8:next tab\n",
    "v1 invoke 7 9 12 policy 08:next tab\n", "v1 invoke 7 9 12 other 8:next tab\n",
    "v1 invoke 7 9 12 policy 7:next tab\n", "v1 invoke 7 9 12 policy 0:\n",
    "v1 invoke 7 9 12 policy 2: a\n", "v1 invoke 7 9 12 policy 2:a \n",
    "v1 invoke 7 9 12 policy 2:a/\n", "v1 invoke 7 9 12 policy 2:\xc3\xa9\n",
    "v1 invoke 7 9 12 policy 1:a\n\n"
  };
  struct sophia_af_invocation v, decoded;
  unsigned char out[512];
  char name[129];
  size_t i, j, n = 999;
  assert(!sophia_af_decode_invocation(wire, strlen(wire), &v));
  assert(v.connection == 7 && v.request == 9 && v.generation == 12 && v.owner == SOPHIA_AF_POLICY);
  assert(v.name == wire + 26 && v.name_length == 8);
  memset(out, 0xaa, sizeof out);
  assert(!sophia_af_encode_invocation(&v, out, sizeof out, &n));
  assert(n == strlen(wire) && !memcmp(out, wire, n) && out[n] == 0xaa);
  for (i = 0; i < strlen(wire); ++i) {
    memset(out, 0xaa, sizeof out); n = 999;
    assert(sophia_af_encode_invocation(&v, out, i, &n) == -1 && n == 999);
    for (j = 0; j < sizeof out; ++j) assert(out[j] == 0xaa);
    assert(sophia_af_decode_invocation(wire, i, &v) == -1);
    assert(v.connection == 7 && v.name == wire + 26);
  }
  for (i = 0; i < sizeof bad / sizeof bad[0]; ++i)
    assert(sophia_af_decode_invocation(bad[i], strlen(bad[i]), &v) == -1);
  memset(name, 'a', sizeof name);
  v.connection = v.request = v.generation = UINT64_MAX;
  v.owner = SOPHIA_AF_SESSION; v.name = name; v.name_length = 128;
  assert(!sophia_af_encode_invocation(&v, out, sizeof out, &n));
  assert(!sophia_af_decode_invocation(out, n, &decoded));
  assert(decoded.connection == UINT64_MAX && decoded.request == UINT64_MAX && decoded.generation == UINT64_MAX);
  assert(decoded.owner == SOPHIA_AF_SESSION && decoded.name_length == 128);
  /* The borrowed name can safely be encoded over its own backing record. */
  assert(!sophia_af_encode_invocation(&decoded, out, sizeof out, &n));
  v.name_length = 129;
  assert(sophia_af_encode_invocation(&v, out, sizeof out, &n) == -1);
  v.name_length = 128; v.owner = (enum sophia_af_owner)0;
  assert(sophia_af_encode_invocation(&v, out, sizeof out, &n) == -1);
  v.owner = SOPHIA_AF_POLICY; name[1] = '\0';
  assert(sophia_af_encode_invocation(&v, out, sizeof out, &n) == -1);
  assert(sophia_af_decode_invocation(NULL, 0, &v) == -1);
  assert(sophia_af_decode_invocation(wire, strlen(wire), NULL) == -1);
  assert(sophia_af_encode_invocation(NULL, out, sizeof out, &n) == -1);
  return 0;
}

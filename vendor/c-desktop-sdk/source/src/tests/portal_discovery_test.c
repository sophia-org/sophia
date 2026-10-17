#include "sophia_portal_files.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
int main(void) {
  const char *caps[] = {"unavailable", "output-capture-v1"};
  const char *states[] = {"absent", "ready", "revoked"};
  const char *bad_epochs[] = {"0", "01", "+1", "18446744073709551616",
                              "",  "-1"};
  struct sophia_pf_api api, before;
  struct sophia_pf_status status, prior;
  char text[160];
  size_t i, j, n, length;
  for (i = 0; i < 2; ++i) {
    snprintf(text, sizeof text,
             "sophia-portal-client version=1 epoch=18446744073709551615 "
             "request=%s fd_transfer=none\n",
             caps[i]);
    length = strlen(text);
    assert(!sophia_pf_decode_api(text, length, &api));
    assert(api.epoch == UINT64_MAX && (unsigned)api.request == i);
    if (i)
      assert(length == SOPHIA_PF_API_MAX_BYTES);
    memcpy(&before, &api, sizeof api);
    for (n = 0; n < length; ++n) {
      assert(sophia_pf_decode_api(text, n, &api) == -1);
      assert(!memcmp(&before, &api, sizeof api));
    }
    text[length] = 'x';
    assert(sophia_pf_decode_api(text, length + 1, &api) == -1);
    for (j = 0; j < sizeof bad_epochs / sizeof bad_epochs[0]; ++j) {
      snprintf(text, sizeof text,
               "sophia-portal-client version=1 epoch=%s request=%s "
               "fd_transfer=none\n",
               bad_epochs[j], caps[i]);
      assert(sophia_pf_decode_api(text, strlen(text), &api) == -1);
      assert(!memcmp(&before, &api, sizeof api));
    }
    for (j = 0; j < 3; ++j) {
      snprintf(text, sizeof text, "request=%s transfer=%s\n", caps[i],
               states[j]);
      length = strlen(text);
      assert(!sophia_pf_decode_status(text, length, &status));
      assert((unsigned)status.request == i && (unsigned)status.transfer == j);
      memcpy(&prior, &status, sizeof status);
      for (n = 0; n < length; ++n) {
        assert(sophia_pf_decode_status(text, n, &status) == -1);
        assert(!memcmp(&prior, &status, sizeof status));
      }
      text[length] = '\n';
      assert(sophia_pf_decode_status(text, length + 1, &status) == -1);
    }
  }
  {
    const char *bad[] = {"sophia-portal-client version=2 epoch=1 "
                         "request=unavailable fd_transfer=none\n",
                         "sophia-portal-client version=1 epoch=1 "
                         "request=unknown fd_transfer=none\n",
                         "sophia-portal-client version=1 epoch=1 "
                         "request=unavailable fd_transfer=scm-rights\n",
                         "sophia-portal-client version=1 epoch=1 "
                         "request=unavailable fd_transfer=none\r\n"};
    for (i = 0; i < sizeof bad / sizeof bad[0]; ++i)
      assert(sophia_pf_decode_api(bad[i], strlen(bad[i]), &api) == -1);
  }
  {
    const char *bad[] = {"request=unknown transfer=ready\n",
                         "request=unavailable transfer=unknown\n", "\xff"};
    for (i = 0; i < sizeof bad / sizeof bad[0]; ++i)
      assert(sophia_pf_decode_status(bad[i], strlen(bad[i]), &status) == -1);
  }
  return 0;
}

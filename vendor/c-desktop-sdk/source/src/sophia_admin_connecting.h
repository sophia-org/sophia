#ifndef SOPHIA_ADMIN_CONNECTING_H
#define SOPHIA_ADMIN_CONNECTING_H
#include "sophia_admin_session.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Caller-allocated, immovable state. Wire must be freshly initialized on a
 * caller-connected HostDomain socket. Session storage is initialized here;
 * do not use it until take_session succeeds. Caller owns and closes the fd. */
struct sophia_ad_connecting {
  struct sophia_9p_client *wire;
  struct sophia_ad *session;
  struct sophia_ao opening;
  struct sophia_9p_handle version;
  uint64_t now, deadline;
  unsigned phase;
  int error, component_error;
};
int sophia_ad_connect_init(struct sophia_ad_connecting *,
                           struct sophia_9p_client *, struct sophia_ad *,
                           uint64_t now, uint64_t deadline);
int sophia_ad_connect_dispatch(struct sophia_ad_connecting *, uint64_t now,
                               size_t budget);
int sophia_ad_connect_ready(const struct sophia_ad_connecting *);
int sophia_ad_connect_next_wake(const struct sophia_ad_connecting *,
                                uint64_t *time);
int sophia_ad_connect_wants_write(const struct sophia_ad_connecting *);
struct sophia_ad *sophia_ad_connect_take_session(struct sophia_ad_connecting *);
void sophia_ad_connect_close(struct sophia_ad_connecting *);
#ifdef __cplusplus
}
#endif
#endif

#ifndef SOPHIA_ADMIN_SESSION_H
#define SOPHIA_ADMIN_SESSION_H
#include "sophia_admin_command.h"
#include "sophia_admin_startup.h"
#ifdef __cplusplus
extern "C" {
#endif
enum sophia_ad_error {
  SOPHIA_AD_INVALID = -100,
  SOPHIA_AD_BUSY = -101,
  SOPHIA_AD_MISSING_CATALOG = -102,
  SOPHIA_AD_UNADVERTISED = -103,
  SOPHIA_AD_EXHAUSTED = -104,
  SOPHIA_AD_CLOCK_REGRESSION = -105,
  SOPHIA_AD_CLOSED = -106,
  SOPHIA_AD_WIRE = -107,
  SOPHIA_AD_SNAPSHOT = -108,
  SOPHIA_AD_COMMAND = -109
};
/* Caller-owned state. Do not move, copy or modify after initialization: catalog
 * names and snapshot storage point inside this object. Wire/fd storage stays
 * caller-owned, exclusively borrowed; caller closes fd on terminal failure. */
struct sophia_ad {
  struct sophia_9p_client *wire;
  struct sophia_af_info info;
  struct sophia_af_catalog catalog;
  struct sophia_as_fetch fetch;
  struct sophia_ac command;
  struct sophia_9p_handle pending;
  uint32_t root, ctl;
  uint64_t now, next_request;
  unsigned active, fetching, valid, have_command;
  enum sophia_as_file file;
  int error, component_error;
  unsigned char catalog_bytes[SOPHIA_AF_CATALOG_MAX_BYTES], scratch[512];
};
/* Consumes a ready startup handoff and starts one catalog fetch. No I/O. */
int sophia_ad_init(struct sophia_ad *, struct sophia_ao *);
int sophia_ad_dispatch(struct sophia_ad *, uint64_t now, size_t byte_budget);
int sophia_ad_refresh_catalog(struct sophia_ad *, uint64_t now);
int sophia_ad_invoke(struct sophia_ad *, const struct sophia_af_command *,
                     uint64_t now, uint64_t *request);
/* Borrowed views remain valid until the next refresh/invocation respectively.
 * Catalog NULL means unavailable/stale. Result inspection survives close. */
const struct sophia_af_catalog *sophia_ad_catalog(const struct sophia_ad *);
const struct sophia_ac *sophia_ad_latest(const struct sophia_ad *);
int sophia_ad_next_wake(const struct sophia_ad *, uint64_t *time);
int sophia_ad_wants_write(const struct sophia_ad *);
void sophia_ad_close(struct sophia_ad *);
#ifdef __cplusplus
}
#endif
#endif

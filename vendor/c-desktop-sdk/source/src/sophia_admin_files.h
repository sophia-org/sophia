#ifndef SOPHIA_ADMIN_FILES_H
#define SOPHIA_ADMIN_FILES_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#define SOPHIA_AF_LINE_MAX_BYTES 512u
struct sophia_af_info {
  uint64_t session_low, session_high, connection;
  uint32_t command_ms, frame_ms, idle_ms;
};
/* Parse a complete LF-terminated info snapshot without allocation. Callers
 * establish EOF separately. Returns 0 on success, -1 on invalid input;
 * failure leaves out unchanged. Parsing grants no authority. */
int sophia_af_decode_info(const void *bytes, size_t length,
                          struct sophia_af_info *out);
enum sophia_af_owner { SOPHIA_AF_POLICY = 1, SOPHIA_AF_SESSION = 2 };
struct sophia_af_invocation {
  uint64_t connection, request, generation;
  enum sophia_af_owner owner;
  const char *name;
  size_t name_length;
};
/* The decoded name borrows the input bytes and is not NUL-terminated.
 * Parsing does not establish catalog membership. Failure leaves out unchanged. */
int sophia_af_decode_invocation(const void *bytes, size_t length,
                                struct sophia_af_invocation *out);
/* One complete ctl record, no NUL terminator. No allocation; failure leaves
 * bytes and written unchanged. Input/name may overlap output storage. */
int sophia_af_encode_invocation(const struct sophia_af_invocation *value,
                                void *bytes, size_t capacity, size_t *written);
enum sophia_af_status_kind { SOPHIA_AF_READY = 1, SOPHIA_AF_PENDING = 2, SOPHIA_AF_OUTCOME = 3 };
enum sophia_af_outcome {
  SOPHIA_AF_COMMITTED = 1, SOPHIA_AF_COMPLETED, SOPHIA_AF_UNCHANGED,
  SOPHIA_AF_REJECTED, SOPHIA_AF_STALE, SOPHIA_AF_DENIED, SOPHIA_AF_UNAVAILABLE,
  SOPHIA_AF_OVERLOADED, SOPHIA_AF_TIMED_OUT, SOPHIA_AF_INDETERMINATE
};
struct sophia_af_status {
  enum sophia_af_status_kind kind;
  uint64_t connection, request, generation;
  enum sophia_af_outcome outcome;
  const char *detail;
  size_t detail_length;
};
/* No allocation. Detail borrows input and is not NUL-terminated. Unused fields
 * are zero. Failure leaves out unchanged; identity/owner checks belong to client. */
int sophia_af_decode_status(const void *bytes, size_t length,
                            struct sophia_af_status *out);
#define SOPHIA_AF_CATALOG_MAX_COMMANDS 259u
#define SOPHIA_AF_CATALOG_MAX_BYTES 39432u
struct sophia_af_command {
  enum sophia_af_owner owner;
  const char *name;
  size_t name_length;
};
struct sophia_af_catalog {
  uint64_t generation;
  size_t count;
  struct sophia_af_command rows[SOPHIA_AF_CATALOG_MAX_COMMANDS];
};
/* Bounded inline rows, names borrowed from input. Keep input alive/unchanged.
 * Validates the entire snapshot without allocation; failure leaves out unchanged.
 * Caller establishes file EOF. Discovery supplies names, never permission. */
int sophia_af_decode_catalog(const void *bytes, size_t length,
                             struct sophia_af_catalog *out);
/* Use a successfully decoded, unchanged catalog. No allocation. */
int sophia_af_catalog_contains(const struct sophia_af_catalog *catalog,
                               const struct sophia_af_command *command);
#ifdef __cplusplus
}
#endif
#endif

#ifndef SOPHIA_PORTAL_FILES_H
#define SOPHIA_PORTAL_FILES_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#define SOPHIA_PF_FRAME_INFO_BYTES 64u
#define SOPHIA_PF_API_MAX_BYTES 101u
#define SOPHIA_PF_STATUS_MAX_BYTES 43u
enum sophia_pf_request_capability {
  SOPHIA_PF_REQUEST_UNAVAILABLE,
  SOPHIA_PF_OUTPUT_CAPTURE_V1
};
enum sophia_pf_transfer_state {
  SOPHIA_PF_TRANSFER_ABSENT,
  SOPHIA_PF_TRANSFER_READY,
  SOPHIA_PF_TRANSFER_REVOKED
};
struct sophia_pf_api {
  uint64_t epoch;
  enum sophia_pf_request_capability request;
};
struct sophia_pf_status {
  enum sophia_pf_request_capability request;
  enum sophia_pf_transfer_state transfer;
};
/* Exact version-1 snapshots, no allocation. Caller establishes EOF. No grant
 * is inferred; failure returns -1 and leaves out unchanged. */
int sophia_pf_decode_api(const void *, size_t, struct sophia_pf_api *out);
int sophia_pf_decode_status(const void *, size_t, struct sophia_pf_status *out);
#define SOPHIA_PF_CAPTURE_BYTES 24u
#define SOPHIA_PF_CAPTURE_STATUS_BYTES 32u
struct sophia_pf_capture_request {
  uint64_t serial, output;
};
enum sophia_pf_capture_state {
  SOPHIA_PF_IDLE,
  SOPHIA_PF_QUEUED,
  SOPHIA_PF_CAPTURING,
  SOPHIA_PF_READY,
  SOPHIA_PF_DENIED,
  SOPHIA_PF_UNAVAILABLE,
  SOPHIA_PF_PRESSURE,
  SOPHIA_PF_CANCELLED
};
struct sophia_pf_capture_status {
  uint64_t serial, output;
  enum sophia_pf_capture_state state;
};
/* No allocation. Failure returns -1 and leaves output unchanged. Encoding
 * writes exactly 24 bytes, leaving any remaining capacity untouched. Ordering
 * and admission belong to the client; status conveys no frame authority. */
int sophia_pf_encode_capture(struct sophia_pf_capture_request request,
                             void *bytes, size_t capacity);
int sophia_pf_decode_capture_status(const void *bytes, size_t length,
                                    struct sophia_pf_capture_status *out);
#define SOPHIA_PF_IMAGE_MAX_BYTES UINT64_C(67108864)
/* Passive RGBA8 metadata; no pixel storage or authority is retained. */
struct sophia_pf_frame_info {
  uint64_t output, output_generation, presentation_generation;
  uint32_t width, height;
  uint64_t stride, image_bytes;
};
/* Exact complete record; caller establishes EOF and same-object frame reads.
 * No allocation. Returns -1 on invalid input and leaves out unchanged. */
int sophia_pf_decode_frame_info(const void *bytes, size_t length,
                                struct sophia_pf_frame_info *out);
#ifdef __cplusplus
}
#endif
#endif

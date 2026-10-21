#ifndef SOPHIA_LOCK_IMAGE_H
#define SOPHIA_LOCK_IMAGE_H
#include "sophia_image_producer.h"
#include "sophia_lock_client.h"

/* Optional sealed-image adapter for a lock client at revision 2.
 * The caller owns this struct, attaches it to one client after
 * sophia_lc_init or sophia_lc_init_attached and before Negotiated arrives,
 * and keeps both alive and unmoved until the client is terminal and the
 * adapter is closed. The client reads no clock; neither does this adapter:
 * every call takes CLOCK_MONOTONIC milliseconds.
 *
 * Once Negotiated selects revision 2 and grants sealed images (bit 2), the
 * adapter reads the image-channel, image-limits and image-formats files
 * through the client's own 9P wire, one complete set per pass, and hands the
 * set to the producer. It then connects the endpoint beneath endpoint_dir and
 * attaches the socket. A selection of 1, or no sealed grant, closes the
 * producer cleanly as unavailable. Image journal events are consumed and
 * acknowledged by the client and routed to the producer; they never reach
 * sophia_lc_event. ResourceStatus for a live image offer is routed too and
 * still presented. ImageBegin is the client's one submission.
 *
 * DMA-BUF is never requested. Default clients never touch this file. */
#define SOPHIA_LC_IMAGE_TEXT_BYTES 2048u
#define SOPHIA_LC_IMAGE_LIMITS_BYTES 136u
/* Header plus one row: the smallest useful formats buffer. */
#define SOPHIA_LC_IMAGE_FORMATS_MIN 64u
/* ImageBegin: 32-byte header and the producer's 96-byte body. */
#define SOPHIA_LC_IMAGE_BEGIN_BYTES 128u

struct sophia_lc_image_config {
  /* role, revision, connection_epoch and bounds are set from the client;
   * granted_kinds from the negotiated capabilities. The caller supplies
   * transfer timeout and startup deadline. producer.bounds is ignored;
   * discovery must match the role's Limits, not caller estimates. */
  struct sophia_ip_config producer;
  /* Borrowed directory fd holding the lock provider's 9P socket. */
  int endpoint_dir;
  /* Caller storage for the formats object, at least FORMATS_MIN bytes.
   * Reads never exceed the contract's formats budget. */
  void *formats;
  size_t formats_capacity;
};

/* Private state exposed for caller-owned allocation. Do not modify it. */
struct sophia_lc_image {
  struct sophia_lc_image_config config;
  struct sophia_ip producer;
  struct sophia_lc_client *client;
  uint64_t pass, retry_at, offer;
  uint32_t retry_delay, read_error;
  size_t text_used, limits_used, formats_used;
  uint8_t stage, file, started;
  uint8_t text[SOPHIA_LC_IMAGE_TEXT_BYTES];
  uint8_t limits[SOPHIA_LC_IMAGE_LIMITS_BYTES];
};

/* Validates and copies config; touches no client. */
int sophia_lc_image_init(struct sophia_lc_image *,
                         const struct sophia_lc_image_config *);
/* The client's offer must span revision 2 and request bit 2. One adapter
 * per client. ARGUMENT or BUSY (already negotiated or terminal) changes
 * nothing. */
int sophia_lc_image_attach(struct sophia_lc_client *, struct sophia_lc_image *);
/* The producer's socket, or -1. Poll it beside the client's fd. */
int sophia_lc_image_poll_fd(const struct sophia_lc_image *);
short sophia_lc_image_poll_events(const struct sophia_lc_image *);
/* -1, or milliseconds until the producer or a discovery/connect retry is
 * due. On expiry call sophia_lc_image_dispatch. */
int64_t sophia_lc_image_timeout(const struct sophia_lc_image *, uint64_t now_ms);
/* Call after each sophia_lc_service and whenever the producer's fd is ready
 * or the timeout expires: producer socket service, then start, discovery,
 * connect and loss progress. New 9P requests are queued on the client's wire
 * before it returns. Returns the producer's changed mask
 * (SOPHIA_IP_CHANGED_*), or -ARGUMENT. */
int sophia_lc_image_dispatch(struct sophia_lc_image *, short revents,
                             uint64_t now_ms);
/* Submits one sealed source as an ImageBegin. READY clients only. On OK the
 * producer owns source_fd and *offer names it. BUSY (a submission in flight
 * or awaiting its ack), AGAIN (channel not ready) and every refusal leave
 * source_fd with the caller. A refused submit refuses the offer; an EAGAIN
 * submit waits for sophia_lc_submit_retry as any submission does. */
int sophia_lc_image_begin(struct sophia_lc_image *, struct sophia_ic_metadata *,
                          int source_fd, uint64_t now_ms, uint64_t *offer);
/* Submits ResourceCancel naming offer (transaction = offer ID, the offer's
 * resource) as the client's one submission. READY clients only. BUSY while
 * any submission is in flight or awaits its ack, including the offer's own
 * ImageBegin. Room is checked first; then the producer marks the offer
 * (sophia_ip_cancel), so its source is never sent afterward. A refused
 * cancel changes nothing else and the mark stays; a cancel possibly sent
 * when the client ends is journal loss like any offer. REFUSED (a status, a
 * mark, or no live custody) and ARGUMENT (unknown offer) change nothing.
 * Acceptance may still win; settlement is only SourceReleased. */
int sophia_lc_image_cancel(struct sophia_lc_image *, uint64_t offer,
                           uint64_t now_ms);
/* The producer once started, for offer, path, cause and forget queries. */
struct sophia_ip *sophia_lc_image_producer(struct sophia_lc_image *);
#endif

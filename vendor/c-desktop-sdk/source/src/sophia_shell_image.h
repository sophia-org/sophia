#ifndef SOPHIA_SHELL_IMAGE_H
#define SOPHIA_SHELL_IMAGE_H
#include "sophia_image_producer.h"
#include "sophia_shell_session.h"

/* ISB image adapter for a content-granted shell session at revision 9.
 * The caller owns this struct, attaches it to one live session before image
 * use, and keeps both alive and unmoved until the session
 * is final and the adapter is closed. Nothing here reads a clock: every call
 * takes CLOCK_MONOTONIC milliseconds, as the session does.
 *
 * Once r9 is negotiated with sealed images (bit 13), the adapter reads the
 * image-channel, image-limits and image-formats files through the session's
 * own 9P wire, one complete set per pass, and hands the set to the producer.
 * After discovery it connects the endpoint beneath endpoint_dir and attaches
 * the socket. Image journal events are consumed by the session and routed to
 * the producer; they never reach sophia_ss_event. ResourceStatus for a live
 * image offer is routed too and still delivered. ImageBegin is queued as an
 * ordinary session candidate with its own ticket, and so is a ResourceCancel
 * naming an offer.
 *
 * DMA-BUF is never requested. Default sessions never touch this file. */
#define SOPHIA_SS_IMAGE_TEXT_BYTES 2048u
#define SOPHIA_SS_IMAGE_LIMITS_BYTES 136u
/* Header plus one row: the smallest useful formats buffer. */
#define SOPHIA_SS_IMAGE_FORMATS_MIN 64u
/* ImageBegin: 32-byte header and the producer's 96-byte body. */
#define SOPHIA_SS_IMAGE_BEGIN_BYTES 128u

struct sophia_ss_image_config {
    /* role, revision and connection_epoch are set from the session;
     * granted_kinds from the negotiated capabilities. The rest is the
     * caller's: bounds, transfer timeout and startup deadline. */
    struct sophia_ip_config producer;
    /* Borrowed directory fd holding the role's 9P socket. */
    int endpoint_dir;
    /* Caller storage for the formats object, at least FORMATS_MIN bytes. */
    void *formats;
    size_t formats_capacity;
};

/* Private state exposed for caller-owned allocation. Do not modify it. */
struct sophia_ss_image_ticket {
    uint64_t ticket, offer;
};
struct sophia_ss_image {
    struct sophia_ss_image_config config;
    struct sophia_ip producer;
    struct sophia_ss *session;
    struct sophia_ss_image_ticket tickets[SOPHIA_IP_MAX_OFFERS];
    uint64_t pass, retry_at, status_seen;
    uint32_t retry_delay, read_error;
    size_t text_used, limits_used, formats_used;
    uint8_t stage, file, started, lost;
    uint8_t text[SOPHIA_SS_IMAGE_TEXT_BYTES];
    uint8_t limits[SOPHIA_SS_IMAGE_LIMITS_BYTES];
};

/* Validates and copies config; touches no session. */
int sophia_ss_image_init(struct sophia_ss_image *, const struct sophia_ss_image_config *);
/* After sophia_ss_open_*, during negotiation or in READY. Attaching after
 * negotiation lets callers derive bounds and transfer timeout from Limits.
 * The offer must span revision 9, require content and bit 13 but not bit 14;
 * a completed negotiation must grant r9, content and bit 13 without bit 14.
 * Role/grant checks remain with the
 * session. One adapter per session, never replaced after closure. ARGUMENT or
 * BUSY changes nothing. ChannelReady still gates every image offer. */
int sophia_ss_image_attach(struct sophia_ss *, struct sophia_ss_image *);
/* The producer's socket, or -1. Poll it beside the session's fd. */
int sophia_ss_image_poll_fd(const struct sophia_ss_image *);
short sophia_ss_image_poll_events(const struct sophia_ss_image *);
/* -1, or milliseconds until the producer or a discovery/connect retry is
 * due. On expiry call sophia_ss_image_dispatch, then sophia_ss_dispatch. */
int64_t sophia_ss_image_timeout(const struct sophia_ss_image *, uint64_t now_ms);
/* Producer socket service plus discovery and connect progress. Returns the
 * producer's changed mask (SOPHIA_IP_CHANGED_*), or -ARGUMENT. */
int sophia_ss_image_dispatch(struct sophia_ss_image *, short revents, uint64_t now_ms);
/* Queues one sealed source as an ImageBegin candidate. READY sessions only.
 * On OK the producer owns source_fd, *offer names it and *ticket is the
 * session ticket for its ImageBegin. BUSY (no queue room), AGAIN (channel
 * not ready) and every refusal leave source_fd with the caller. */
int sophia_ss_image_begin(struct sophia_ss_image *, struct sophia_ic_metadata *, int source_fd,
                          uint64_t now_ms, uint64_t *offer, uint64_t *ticket);
/* Queues ResourceCancel naming offer: transaction is the offer ID, the
 * grant's content epoch is the metadata's admission epoch. READY sessions
 * only. Queue room is checked first; then the producer marks the offer
 * (sophia_ip_cancel), so its source is never sent afterward. The cancel may
 * queue behind its ImageBegin. On OK *ticket is the session ticket, for the
 * caller only: whatever its outcome, the mark stays and custody is unchanged.
 * Session end is journal loss as for any offer. BUSY (no room),
 * REFUSED (a status, a mark, or no live custody) and ARGUMENT (unknown
 * offer) change nothing. Acceptance may still win; settlement is only
 * SourceReleased. */
int sophia_ss_image_cancel(struct sophia_ss_image *, uint64_t offer, uint64_t now_ms,
                           uint64_t *ticket);
/* The producer, for offer, path, cause and forget queries. */
struct sophia_ip *sophia_ss_image_producer(struct sophia_ss_image *);
#endif

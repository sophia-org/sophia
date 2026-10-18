#ifndef SOPHIA_IMAGE_PRODUCER_H
#define SOPHIA_IMAGE_PRODUCER_H
/* Image-channel producer for one role connection: discovery, Hello and the
 * ChannelReady barrier, up to four offers, Immutable Shared Buffers (ISB)
 * and their custody.
 * Single thread, no allocator, no clock. Callers pass CLOCK_MONOTONIC
 * milliseconds; a now_ms smaller than the previous call is ARGUMENT.
 *
 * The producer never uses 9P. The role client reads the discovery set,
 * submits the ImageBegin body this producer returns under its own header
 * and submission identity, and forwards image records and ResourceStatus
 * outcomes. The producer owns the side socket after a successful attach,
 * offer IDs, slots, source FDs and deadlines. It never replays a send and
 * never invents a release: only SourceReleased, a definitive refusal before
 * admission, or journal loss ends an offer's custody.
 *
 * Fields of struct sophia_ip are private; the layout is public so callers
 * can allocate it, as with the session structs. */
#include "sophia_image_channel.h"
#include <stddef.h>
#include <stdint.h>

#define SOPHIA_IP_MAX_OFFERS 4u

/* Call results. */
enum sophia_ip_result {
    SOPHIA_IPR_OK = 0,
    SOPHIA_IPR_AGAIN,     /* not now: retry after the next dispatch or pass */
    SOPHIA_IPR_BUSY,      /* every offer slot is live */
    SOPHIA_IPR_CLOSED,    /* channel not usable; see sophia_ip_path */
    SOPHIA_IPR_EXHAUSTED, /* offer IDs exhausted: a new 9P connection is needed */
    SOPHIA_IPR_REFUSED,   /* value, source or state fails a check; nothing changed */
    SOPHIA_IPR_ARGUMENT
};

/* Channel barrier: DISCOVERED -> attach -> HELLO_QUEUED -> HELLO_SENT ->
 * ChannelReady for this generation -> READY. CLOSED is final for the 9P
 * epoch. Offers start only in READY. */
enum sophia_ip_channel {
    SOPHIA_IPC_IDLE,
    SOPHIA_IPC_DISCOVERED,
    SOPHIA_IPC_HELLO_QUEUED,
    SOPHIA_IPC_HELLO_SENT,
    SOPHIA_IPC_READY,
    SOPHIA_IPC_CLOSED
};

/* Why the channel closed. Every closure ends image admission for this epoch.
 * Recovery needs a new authenticated connection, never a byte fallback. */
enum sophia_ip_cause {
    SOPHIA_IPX_NONE,
    SOPHIA_IPX_ABSENT,       /* discovery object absent (ENOENT) */
    SOPHIA_IPX_UNAVAILABLE,  /* reason 13, peer disconnect, revocation, no sealed grant */
    SOPHIA_IPX_EXHAUSTED,    /* offer IDs exhausted */
    SOPHIA_IPX_DISCOVERY,    /* discovery malformed, denied or incoherent */
    SOPHIA_IPX_TIMEOUT,      /* startup or Hello deadline passed */
    SOPHIA_IPX_FAULT,        /* protocol fault, contradiction or failed send */
    SOPHIA_IPX_JOURNAL_LOST, /* the 9P journal ended */
    SOPHIA_IPX_CALLER        /* sophia_ip_close */
};

/* Offer custody. FREE slots only; REFUSED, SETTLED and UNKNOWN may be
 * forgotten. No state ever sends twice. */
enum sophia_ip_offer_state {
    SOPHIA_IPO_FREE,
    SOPHIA_IPO_BEGUN,     /* body returned; awaiting admission or refusal */
    SOPHIA_IPO_REFUSED,   /* definitively refused before admission; ID spent */
    SOPHIA_IPO_ADMITTED,  /* exact ImageAdmitted seen; source send pending */
    SOPHIA_IPO_SENT,      /* the full packet was sent; awaiting release */
    SOPHIA_IPO_AMBIGUOUS, /* send outcome unknown; never replayed */
    SOPHIA_IPO_EXPIRED,   /* admitted, deadline passed unsent; awaiting release */
    SOPHIA_IPO_SETTLED,   /* SourceReleased seen */
    SOPHIA_IPO_UNKNOWN    /* journal lost before release */
};

/* Which path the caller should use for its next image. */
enum sophia_ip_path {
    SOPHIA_IPP_DESCRIPTOR,
    SOPHIA_IPP_WAIT,  /* the barrier is still in progress */
    /* Value 2 was the retired byte fallback. */
    SOPHIA_IPP_ERROR = 3
};

/* ResourceStatus outcomes as the role client reports them. */
enum sophia_ip_status {
    SOPHIA_IPS_ACCEPTED = 1,
    SOPHIA_IPS_REJECTED,
    SOPHIA_IPS_CANCELLED
};

struct sophia_ip_config {
    uint16_t role;                       /* SOPHIA_IC_ROLE_SHELL or _LOCK */
    uint16_t revision;                   /* negotiated; shell >= 9, lock >= 2 */
    uint64_t connection_epoch;           /* the 9P connection epoch */
    uint32_t granted_kinds;              /* source-kind bits from negotiated capabilities */
    struct sophia_ic_role_bounds bounds; /* the role's negotiated limits */
    uint32_t transfer_timeout_ms;        /* the role's transfer deadline, nonzero */
    uint64_t startup_deadline_ms;        /* absolute: discovery through ChannelReady */
};

struct sophia_ip_offer_info {
    uint64_t offer, resource_id, resource_generation;
    enum sophia_ip_offer_state state;
    int deadline_passed;           /* local expiry; custody unchanged */
    enum sophia_ip_status status;  /* 0 until a ResourceStatus */
    int unavailable;               /* that status carried reason image_channel_unavailable */
    uint32_t disposition;          /* SourceReleased disposition, 0 until then */
    int cancel_requested;          /* sophia_ip_cancel marked it; never cleared */
};

struct sophia_ip_slot {
    struct sophia_ic_metadata metadata;
    uint64_t deadline_ms;
    int fd;
    uint8_t state, deadline_passed, unavailable, status, cancelled;
    uint32_t disposition;
};

struct sophia_ip {
    struct sophia_ip_config config;
    struct sophia_ic_expect expect;
    struct sophia_ip_slot slots[SOPHIA_IP_MAX_OFFERS];
    uint64_t last_now, last_pass, next_offer, hello_deadline_ms;
    uint32_t hello_timeout_ms, max_offers, ignored;
    int fd, have_pass;
    uint8_t channel, cause;
};

int sophia_ip_init(struct sophia_ip *, const struct sophia_ip_config *, uint64_t now_ms);

/* One discovery set per caller pass: the image-channel text and the limits
 * and formats records, read by the role client in one pass. read_errno is 0
 * or the read failure; only ENOENT means absent. A repeated pass number is
 * AGAIN without reading. Mixed generations are AGAIN (read a fresh set on a
 * later pass, within the startup deadline). Malformed, incoherent or denied
 * sets close the channel with DISCOVERY, as does a set naming a source kind
 * outside granted_kinds. A set without the sealed kind is clean
 * UNAVAILABLE. OK means DISCOVERED. */
int sophia_ip_discover(struct sophia_ip *, const void *text, size_t text_bytes,
                       const void *limits, size_t limits_bytes, const void *formats,
                       size_t formats_bytes, int read_errno, uint64_t pass, uint64_t now_ms);

/* Connects a nonblocking close-on-exec SEQPACKET socket to the endpoint, a
 * single name inside endpoint_dir. OK fills *fd; AGAIN means the listener's
 * backlog is full (retry before the startup deadline); REFUSED leaves errno.
 * It never waits. */
int sophia_ip_connect(int endpoint_dir, const char *endpoint, int *fd);
/* In DISCOVERED, verifies SOCK_SEQPACKET, takes ownership of the connected fd
 * and queues Hello. On any other result the caller still owns fd. */
int sophia_ip_attach(struct sophia_ip *, int fd, uint64_t now_ms);

/* -1 when not attached. POLLIN while attached (hangup detection; the owner
 * never writes), plus POLLOUT while Hello or an admitted source is queued. */
int sophia_ip_poll_fd(const struct sophia_ip *);
short sophia_ip_poll_events(const struct sophia_ip *);
/* -1, or milliseconds until the earliest live deadline: startup, Hello,
 * or an offer's local deadline. A passed deadline reads 0 until the next
 * dispatch expires it, and is then dropped; its offer keeps custody. */
int64_t sophia_ip_timeout(const struct sophia_ip *, uint64_t now_ms);
/* One bounded nonblocking pass: expire deadlines, detect hangup, send a
 * queued Hello and then admitted sources in offer order. A send failure
 * other than EAGAIN marks that offer AMBIGUOUS and closes the channel.
 * Returns a mask of SOPHIA_IP_CHANGED_*, or -(int)SOPHIA_IPR_ARGUMENT. */
#define SOPHIA_IP_CHANGED_CHANNEL 1
#define SOPHIA_IP_CHANGED_OFFERS 2
int sophia_ip_dispatch(struct sophia_ip *, short revents, uint64_t now_ms);

/* An image record from the role client's journal, already decoded at the
 * negotiated revision. Records of another connection epoch are counted and
 * ignored. In this epoch, ChannelReady, ChannelClosed, ImageAdmitted and
 * SourceReleased must agree with the channel generation and live offers;
 * a contradiction closes the channel with FAULT and keeps every offer's
 * custody. An event for an offer that was already forgotten is ignored. */
int sophia_ip_record(struct sophia_ip *, const struct sophia_ic_record *, uint64_t now_ms);
/* A ResourceStatus for an offer's resource. Rejected or Cancelled before
 * admission refuses the offer. After admission the status is recorded and
 * the offer still waits for SourceReleased. unavailable (reason
 * image_channel_unavailable, Rejected only) also closes the channel with
 * UNAVAILABLE. Statuses for other resources are ignored. */
int sophia_ip_resource_status(struct sophia_ip *, uint64_t resource_id, uint64_t generation,
                              enum sophia_ip_status, int unavailable, uint64_t now_ms);
/* The role refused the ImageBegin transaction itself: a BEGUN offer only. */
int sophia_ip_begin_refused(struct sophia_ip *, uint64_t offer, uint64_t now_ms);
/* The 9P journal ended: unsettled offers become UNKNOWN; the channel closes. */
void sophia_ip_journal_lost(struct sophia_ip *);
/* Closes the socket (CALLER). Offers keep their custody states. */
void sophia_ip_close(struct sophia_ip *);

/* Starts one offer: READY, a free slot, a sealed source of a granted kind,
 * and metadata admissible under the discovered limits. metadata->offer is
 * assigned here. source_fd must carry WRITE, GROW, SHRINK and SEAL seals and
 * exactly the declared bytes. On OK the producer owns source_fd and *offer
 * and body are filled; on any other result the caller still owns it. */
int sophia_ip_begin(struct sophia_ip *, struct sophia_ic_metadata *metadata, int source_fd,
                    uint64_t now_ms, uint8_t body[SOPHIA_IC_METADATA_BYTES], uint64_t *offer);

int sophia_ip_offer(const struct sophia_ip *, uint64_t offer, struct sophia_ip_offer_info *);
/* Live slots in slot order; *cursor starts at 0. Returns 0 when done. */
int sophia_ip_offer_next(const struct sophia_ip *, unsigned *cursor, struct sophia_ip_offer_info *);
/* Marks an offer cancelled before its role client queues ResourceCancel
 * naming it; check queue room first. The mark is permanent, even if the
 * cancel is refused: an admitted source is never sent afterward and the fd
 * stays held. Settlement is still only SourceReleased (or UNKNOWN on journal
 * loss); acceptance may win the race and is kept. REFUSED once the offer has
 * a status or a mark, or is refused, settled or unknown. */
int sophia_ip_cancel(struct sophia_ip *, uint64_t offer, uint64_t now_ms);
/* Frees a REFUSED, SETTLED or UNKNOWN slot. */
int sophia_ip_forget(struct sophia_ip *, uint64_t offer);
enum sophia_ip_channel sophia_ip_channel(const struct sophia_ip *);
enum sophia_ip_cause sophia_ip_cause(const struct sophia_ip *);
enum sophia_ip_path sophia_ip_path(const struct sophia_ip *);

/* A fresh sealable memfd of width*height*4 bytes, mapped writable for the
 * caller to fill in place. ARGUMENT, before anything is allocated, for a zero
 * dimension, a stride above UINT32_MAX, a size above SIZE_MAX, PTRDIFF_MAX or
 * INT64_MAX, or a size above max_bytes (normally the role's granted
 * max_resource_bytes). The backing is
 * allocated up front, so ordinary memory exhaustion is REFUSED here rather
 * than a fault while filling; this narrows SIGBUS exposure but does not rule
 * out hardware memory errors. GROW and SHRINK are sealed at once, so the
 * backing cannot shrink beneath the mapping, and the mapping is not inherited
 * by fork, so a child holds no writable alias. A forked child therefore has no
 * mapping at *pixels: it must not access it or munmap it, and it inherits the
 * unsealed fd until exec closes it (CLOEXEC), so it must close fd or exec
 * without writing to it. Do not resize or remap fd. On
 * OK the caller owns *fd and the mapping: finish it, or abandon it with
 * munmap(*pixels, *bytes) and close(*fd); never offer an unfinished fd.
 * Otherwise nothing was created and the outputs are unchanged. */
int sophia_ip_isb_create_bounded(uint32_t width, uint32_t height, uint64_t max_bytes, int *fd,
                                 void **pixels, size_t *bytes);
/* sophia_ip_isb_create_bounded with no resource bound. */
int sophia_ip_isb_create(uint32_t width, uint32_t height, int *fd, void **pixels,
                         size_t *bytes);
/* Unmaps pixels, then adds WRITE, GROW, SHRINK and SEAL seals and reads them
 * back. The caller keeps fd on every result. ARGUMENT leaves the mapping in
 * place; REFUSED means the mapping is gone but the seals are missing, so fd
 * must not be offered. */
int sophia_ip_isb_finish(int fd, void *pixels, size_t bytes);
#endif

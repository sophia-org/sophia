/* The ISB image adapter: discovery through the session's 9P
 * wire, producer attach, journal routing and ImageBegin tickets. */
#include "internal.h"
#include "../shell_files/session_internal.h"
#include "../sophia_shell_image.h"
#include "../image_producer/internal.h"
#include <errno.h>
#include <unistd.h>

#define SEALED_BIT (UINT64_C(1) << 13)
#define DMA_BIT (UINT64_C(1) << 14)
#define CONTENT_BIT (UINT64_C(1) << 7)

enum { STAGE_NEGOTIATING, STAGE_DISCOVERING, STAGE_CONNECTING, STAGE_RUNNING };

int sophia_ss_image_init(struct sophia_ss_image *img, const struct sophia_ss_image_config *c)
{
    if (!img || !c || c->endpoint_dir < 0 || !c->formats ||
        c->formats_capacity < SOPHIA_SS_IMAGE_FORMATS_MIN)
        return SOPHIA_IPR_ARGUMENT;
    memset(img, 0, sizeof(*img));
    img->config = *c;
    img->producer.fd = -1;
    return SOPHIA_IPR_OK;
}

int sophia_ss_image_attach(struct sophia_ss *s, struct sophia_ss_image *img)
{
    const struct sophia_sf_negotiate *o;
    if (!s || !img || img->session)
        return SOPHIA_IPR_ARGUMENT;
    o = &s->files.offer;
    if (o->minimum_revision > 9 || o->maximum_revision < 9 ||
        (o->required_capabilities & (SEALED_BIT | CONTENT_BIT)) != (SEALED_BIT | CONTENT_BIT) ||
        (o->required_capabilities & DMA_BIT))
        return SOPHIA_IPR_ARGUMENT;
    if ((s->state != SOPHIA_SS_NEGOTIATING && s->state != SOPHIA_SS_READY) || s->image)
        return SOPHIA_IPR_BUSY;
    if (s->files.negotiated &&
        (s->files.welcome.selected_revision != SOPHIA_SF_REVISION_IMAGE_CHANNEL ||
         (s->files.welcome.capabilities & (SEALED_BIT | CONTENT_BIT)) !=
             (SEALED_BIT | CONTENT_BIT) ||
         (s->files.welcome.capabilities & DMA_BIT)))
        return SOPHIA_IPR_ARGUMENT;
    img->session = s;
    s->image = img;
    s->files.image_mode = 1;
    return SOPHIA_IPR_OK;
}

struct sophia_ip *sophia_ss_image_producer(struct sophia_ss_image *img)
{
    return img && img->started ? &img->producer : NULL;
}

int sophia_ss_image_poll_fd(const struct sophia_ss_image *img)
{
    return img && img->started ? sophia_ip_poll_fd(&img->producer) : -1;
}

short sophia_ss_image_poll_events(const struct sophia_ss_image *img)
{
    return img && img->started ? sophia_ip_poll_events(&img->producer) : 0;
}

int64_t sophia_ss_image_timeout(const struct sophia_ss_image *img, uint64_t now_ms)
{
    int64_t t;
    if (!img || !img->started)
        return -1;
    t = sophia_ip_timeout(&img->producer, now_ms);
    if ((img->stage == STAGE_DISCOVERING || img->stage == STAGE_CONNECTING) && !img->file) {
        int64_t retry = img->retry_at <= now_ms ? 0
                        : img->retry_at - now_ms > INT64_MAX ? INT64_MAX
                                                            : (int64_t)(img->retry_at - now_ms);
        if (t < 0 || retry < t)
            t = retry;
    }
    return t;
}

/* The session's EAGAIN policy: 4 ms doubling to 256 ms, saturating. */
static void back_off(struct sophia_ss_image *img, uint64_t now)
{
    img->retry_delay = !img->retry_delay ? 4u : img->retry_delay < 256u ? 2u * img->retry_delay
                                                                        : 256u;
    img->retry_at = now > UINT64_MAX - img->retry_delay ? UINT64_MAX : now + img->retry_delay;
}

static int start_producer(struct sophia_ss_image *img, uint64_t now)
{
    const struct sophia_sf_negotiated *w = &img->session->files.welcome;
    struct sophia_ip_config c = img->config.producer;
    c.role = SOPHIA_IC_ROLE_SHELL;
    c.revision = SOPHIA_SF_REVISION_IMAGE_CHANNEL;
    c.connection_epoch = img->session->files.epoch;
    c.granted_kinds = ((w->capabilities & SEALED_BIT) ? SOPHIA_IC_KIND_BIT_SEALED : 0u) |
                      ((w->capabilities & DMA_BIT) ? SOPHIA_IC_KIND_BIT_DMABUF : 0u);
    if (sophia_ip_init(&img->producer, &c, now) != SOPHIA_IPR_OK) {
        /* A configuration that cannot serve this epoch: close as the caller. */
        memset(&img->producer, 0, sizeof(img->producer));
        img->producer.fd = -1;
        img->producer.channel = SOPHIA_IPC_CLOSED;
        img->producer.cause = SOPHIA_IPX_CALLER;
        return 0;
    }
    return 1;
}

static const char *const files[] = {"image-channel", "image-limits", "image-formats"};

static int read_next(struct sophia_ss_image *img)
{
    struct sophia_sf_client *c = &img->session->files;
    void *buf = img->file == 1 ? (void *)img->text
                : img->file == 2 ? (void *)img->limits
                                 : img->config.formats;
    /* Larger caller storage never widens the formats object's budget. */
    size_t cap = img->file == 1 ? sizeof(img->text)
                 : img->file == 2 ? sizeof(img->limits)
                 : img->config.formats_capacity < SOPHIA_IC_MAX_FORMATS_BYTES
                     ? img->config.formats_capacity
                     : SOPHIA_IC_MAX_FORMATS_BYTES;
    return sf_session_file_read(c, files[img->file - 1], buf, cap);
}

/* One set per pass. ENOENT on image-channel alone means absent; on a
 * companion, any read failure or an over-long file is a discovery error. */
static void discovery(struct sophia_ss_image *img, uint64_t now)
{
    struct sophia_sf_client *c = &img->session->files;
    size_t n;
    uint32_t error;
    int r, read_errno = 0;
    if (!img->file) {
        if (now < img->retry_at)
            return;
        img->file = 1;
        img->read_error = 0;
        img->pass++;
        if (read_next(img))
            img->file = 0;
        return;
    }
    r = sf_session_file_result(c, &n, &error);
    if (r == SOPHIA_9P_BUSY)
        return;
    if (r == SOPHIA_9P_AGAIN) {
        img->file = 0;
        back_off(img, now);
        return;
    }
    if (r) {
        read_errno = !error || (error == ENOENT && img->file != 1) ? EPROTO : (int)error;
    } else {
        if (img->file == 1)
            img->text_used = n;
        else if (img->file == 2)
            img->limits_used = n;
        else
            img->formats_used = n;
        if (img->file < 3) {
            img->file++;
            if (read_next(img)) {
                img->file = 0;
                back_off(img, now);
            }
            return;
        }
    }
    img->file = 0;
    r = sophia_ip_discover(&img->producer, img->text, img->text_used, img->limits,
                           img->limits_used, img->config.formats, img->formats_used, read_errno,
                           img->pass, now);
    if (r == SOPHIA_IPR_OK) {
        img->stage = STAGE_CONNECTING;
        img->retry_delay = 0;
        img->retry_at = now;
    } else if (r == SOPHIA_IPR_AGAIN)
        back_off(img, now);
    else
        img->stage = STAGE_RUNNING;
}

static void connecting(struct sophia_ss_image *img, uint64_t now)
{
    struct sophia_ic_discovery d;
    int fd = -1, r;
    if (now < img->retry_at)
        return;
    if (sophia_ic_discovery_decode(img->text, img->text_used, &d) != SOPHIA_IC_OK) {
        sophia_ip_close(&img->producer);
        img->stage = STAGE_RUNNING;
        return;
    }
    r = sophia_ip_connect(img->config.endpoint_dir, d.endpoint, &fd);
    if (r == SOPHIA_IPR_AGAIN) {
        back_off(img, now);
        return;
    }
    if (r != SOPHIA_IPR_OK || sophia_ip_attach(&img->producer, fd, now) != SOPHIA_IPR_OK) {
        if (fd >= 0)
            close(fd);
        sophia_ip_close(&img->producer);
    }
    img->stage = STAGE_RUNNING;
}

/* Ticket outcomes: an ImageBegin refused or never sent is a definitive
 * pre-admission refusal; one whose fate is unknown loses the journal. */
static void tickets(struct sophia_ss_image *img)
{
    unsigned i;
    for (i = 0; i < SOPHIA_IP_MAX_OFFERS; i++) {
        struct sophia_ss_image_ticket *t = &img->tickets[i];
        enum sophia_ss_outcome outcome;
        if (!t->ticket)
            continue;
        if (sophia_ss_outcome(img->session, t->ticket, &outcome, NULL))
            outcome = SOPHIA_SS_UNAVAILABLE;
        switch (outcome) {
        case SOPHIA_SS_ADMITTED_LOCAL:
        case SOPHIA_SS_IN_FLIGHT:
            continue;
        case SOPHIA_SS_REFUSED:
        case SOPHIA_SS_DROPPED_UNSENT:
            (void)sophia_ip_begin_refused(&img->producer, t->offer, img->producer.last_now);
            break;
        case SOPHIA_SS_SUBMITTED:
            break;
        default:
            img->lost = 1;
            break;
        }
        t->ticket = t->offer = 0;
    }
}

void ss_image_drive(struct sophia_ss *s, uint64_t now)
{
    struct sophia_ss_image *img = s->image;
    if (!img)
        return;
    if (!img->started) {
        if (!s->files.negotiated || now < img->producer.last_now)
            return;
        img->started = 1;
        img->stage = start_producer(img, now) ? STAGE_DISCOVERING : STAGE_RUNNING;
    }
    if (now < img->producer.last_now)
        return;
    tickets(img);
    if (img->stage == STAGE_DISCOVERING) {
        if (sophia_ip_channel(&img->producer) != SOPHIA_IPC_IDLE)
            img->stage = STAGE_RUNNING;
        else if (!ss_final(s))
            discovery(img, now);
    }
    if (img->stage == STAGE_CONNECTING) {
        if (sophia_ip_channel(&img->producer) != SOPHIA_IPC_DISCOVERED)
            img->stage = STAGE_RUNNING;
        else
            connecting(img, now);
    }
    if ((ss_final(s) || img->lost) && img->started)
        sophia_ip_journal_lost(&img->producer);
}

/* Image events at the head of the journal: decode and route, then the
 * session consumes them. A contradiction closes the channel, not the 9P
 * session; the producer keeps custody. */
int ss_image_event(struct sophia_ss *s, const struct sophia_sf_record *e)
{
    struct sophia_ss_image *img = s->image;
    struct sophia_ic_record v;
    if (!img || !sf_session_image_kind(e->header.kind))
        return 0;
    if (img->started && sophia_ic_record_decode(s->files.event_bytes,
                                                (size_t)sf_get(s->files.event_bytes, 4),
                                                &v) == SOPHIA_IC_OK)
        (void)sophia_ip_record(&img->producer, &v, s->now_ms);
    return 1;
}

/* ResourceStatus for a live image offer is the producer's too; the
 * application still receives it. Status 1 (admitted) is not final. */
void ss_image_status(struct sophia_ss *s)
{
    struct sophia_ss_image *img = s->image;
    const struct sophia_sf_record *e;
    const struct sophia_sf_resource_status *v;
    struct sophia_ip_offer_info info;
    enum sophia_ip_status status;
    if (!img || !img->started || sophia_sf_client_event(&s->files, &e) ||
        e->header.kind != SOPHIA_SF_RESOURCE_STATUS || e->header.sequence <= img->status_seen)
        return;
    img->status_seen = e->header.sequence;
    v = &e->value.resource_status;
    if (v->status < 2 || v->status > 4 ||
        sophia_ip_offer(&img->producer, v->transaction, &info) != SOPHIA_IPR_OK ||
        info.resource_id != v->resource_id || info.resource_generation != v->resource_generation)
        return;
    status = v->status == 2 ? SOPHIA_IPS_ACCEPTED
             : v->status == 3 ? SOPHIA_IPS_REJECTED
                              : SOPHIA_IPS_CANCELLED;
    (void)sophia_ip_resource_status(&img->producer, v->resource_id, v->resource_generation, status,
                                    v->status == 3 && v->reason == 13, s->now_ms);
}

int sophia_ss_image_dispatch(struct sophia_ss_image *img, short revents, uint64_t now_ms)
{
    int r;
    if (!img || !img->session)
        return -(int)SOPHIA_IPR_ARGUMENT;
    if (!img->started)
        return 0;
    r = sophia_ip_dispatch(&img->producer, revents, now_ms);
    if (r >= 0)
        ss_image_drive(img->session, now_ms);
    return r;
}

int sophia_ss_image_begin(struct sophia_ss_image *img, struct sophia_ic_metadata *metadata,
                          int source_fd, uint64_t now_ms, uint64_t *offer, uint64_t *ticket)
{
    struct sophia_ss *s;
    struct sophia_ic_record record;
    uint8_t bytes[SOPHIA_SS_IMAGE_BEGIN_BYTES];
    uint8_t body[SOPHIA_IC_METADATA_BYTES];
    size_t n;
    unsigned i, free_ticket = SOPHIA_IP_MAX_OFFERS;
    int r;
    if (!img || !img->session || !metadata || !offer || !ticket || source_fd < 0)
        return SOPHIA_IPR_ARGUMENT;
    s = img->session;
    if (!img->started || ss_final(s))
        return SOPHIA_IPR_CLOSED;
    if (s->state != SOPHIA_SS_READY)
        return SOPHIA_IPR_AGAIN;
    for (i = 0; i < SOPHIA_IP_MAX_OFFERS; i++)
        if (!img->tickets[i].ticket) {
            free_ticket = i;
            break;
        }
    /* Room first: once the producer owns the fd, admission cannot fail. */
    if (free_ticket == SOPHIA_IP_MAX_OFFERS || !ss_room(s, SOPHIA_SS_IMAGE_BEGIN_BYTES))
        return SOPHIA_IPR_BUSY;
    r = sophia_ip_begin(&img->producer, metadata, source_fd, now_ms, body, offer);
    if (r != SOPHIA_IPR_OK)
        return r;
    memset(&record, 0, sizeof(record));
    record.header.kind = SOPHIA_IC_IMAGE_BEGIN;
    record.header.epoch = s->files.epoch;
    record.header.submission = 1;
    record.value.image = *metadata;
    if (sophia_ic_record_encode(bytes, sizeof(bytes), &record, &n) != SOPHIA_IC_OK ||
        n != sizeof(bytes) || memcmp(bytes + 32, body, sizeof(body)) ||
        ss_admit_encoded(s, bytes, n, ticket)) {
        /* Unreachable after the checks above; keep custody honest anyway. */
        (void)sophia_ip_begin_refused(&img->producer, *offer, now_ms);
        return SOPHIA_IPR_REFUSED;
    }
    img->tickets[free_ticket].ticket = *ticket;
    img->tickets[free_ticket].offer = *offer;
    (void)ss_pump(s, 0);
    return SOPHIA_IPR_OK;
}

int sophia_ss_image_cancel(struct sophia_ss_image *img, uint64_t offer, uint64_t now_ms,
                           uint64_t *ticket)
{
    struct sophia_ss *s;
    struct sophia_ip_slot *slot;
    struct sophia_sf_record record;
    uint8_t bytes[SOPHIA_SS_IMAGE_BEGIN_BYTES];
    size_t n;
    int r;
    if (!img || !img->session || !ticket)
        return SOPHIA_IPR_ARGUMENT;
    s = img->session;
    if (!img->started || ss_final(s))
        return SOPHIA_IPR_CLOSED;
    if (s->state != SOPHIA_SS_READY)
        return SOPHIA_IPR_AGAIN;
    slot = ip_slot(&img->producer, offer);
    if (!slot)
        return SOPHIA_IPR_ARGUMENT;
    memset(&record, 0, sizeof(record));
    record.header.kind = SOPHIA_SF_RESOURCE_CANCEL;
    record.header.epoch = s->files.epoch;
    record.header.submission = 1;
    record.value.resource_cancel.transaction = offer;
    record.value.resource_cancel.grant_connection_epoch = s->files.epoch;
    record.value.resource_cancel.grant_content_epoch = slot->metadata.admission_epoch;
    record.value.resource_cancel.resource_id = slot->metadata.resource_id;
    record.value.resource_cancel.resource_generation = slot->metadata.resource_generation;
    if (sophia_sf_encode(bytes, sizeof(bytes), &record, &n))
        return SOPHIA_IPR_REFUSED;
    /* Room first: once the producer marks the offer, the cancel must queue. */
    if (!ss_room(s, n))
        return SOPHIA_IPR_BUSY;
    r = sophia_ip_cancel(&img->producer, offer, now_ms);
    if (r != SOPHIA_IPR_OK)
        return r;
    if (ss_admit_encoded(s, bytes, n, ticket))
        return SOPHIA_IPR_REFUSED; /* unreachable after the room check */
    (void)ss_pump(s, 0);
    return SOPHIA_IPR_OK;
}

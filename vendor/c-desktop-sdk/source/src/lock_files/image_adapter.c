/* The optional lock sealed-image adapter: discovery through the client's 9P
 * wire, producer attach, journal routing and the ImageBegin submission. */
#include "client_internal.h"
#include "../sophia_lock_image.h"
#include "../image_producer/internal.h"
#include <errno.h>
#include <unistd.h>

void sophia_lc_internal_image_record(struct sophia_lc_image *,
                                     const struct sophia_ic_record *);
void sophia_lc_internal_image_resource(struct sophia_lc_image *,
                                       const struct sophia_lf_resource_status *);
void sophia_lc_internal_image_refused(struct sophia_lc_image *);

enum { STAGE_NEGOTIATING, STAGE_DISCOVERING, STAGE_CONNECTING, STAGE_RUNNING };

int sophia_lc_image_init(struct sophia_lc_image *img,
                         const struct sophia_lc_image_config *c) {
  if (!img || !c || c->endpoint_dir < 0 || !c->formats ||
      c->formats_capacity < SOPHIA_LC_IMAGE_FORMATS_MIN)
    return SOPHIA_IPR_ARGUMENT;
  memset(img, 0, sizeof(*img));
  img->config = *c;
  img->producer.fd = -1;
  return SOPHIA_IPR_OK;
}

int sophia_lc_image_attach(struct sophia_lc_client *c,
                           struct sophia_lc_image *img) {
  const struct sophia_lf_negotiate *o;
  if (!c || !img || img->client)
    return SOPHIA_IPR_ARGUMENT;
  o = &c->offer;
  if (o->minimum_revision > SOPHIA_LF_REVISION_IMAGE_CHANNEL ||
      o->maximum_revision < SOPHIA_LF_REVISION_IMAGE_CHANNEL ||
      !(o->capabilities & SOPHIA_LF_CAP_IMAGE_SEALED) ||
      (o->capabilities & SOPHIA_LF_CAP_IMAGE_DMABUF))
    return SOPHIA_IPR_ARGUMENT;
  if (c->negotiated || c->refusal || c->terminal || c->image)
    return SOPHIA_IPR_BUSY;
  img->client = c;
  c->image = img;
  return SOPHIA_IPR_OK;
}

struct sophia_ip *sophia_lc_image_producer(struct sophia_lc_image *img) {
  return img && img->started ? &img->producer : NULL;
}
int sophia_lc_image_poll_fd(const struct sophia_lc_image *img) {
  return img && img->started ? sophia_ip_poll_fd(&img->producer) : -1;
}
short sophia_lc_image_poll_events(const struct sophia_lc_image *img) {
  return img && img->started ? sophia_ip_poll_events(&img->producer) : 0;
}
int64_t sophia_lc_image_timeout(const struct sophia_lc_image *img,
                                uint64_t now_ms) {
  int64_t t;
  if (!img || !img->started)
    return -1;
  t = sophia_ip_timeout(&img->producer, now_ms);
  if ((img->stage == STAGE_DISCOVERING || img->stage == STAGE_CONNECTING) &&
      !img->file) {
    int64_t retry = img->retry_at <= now_ms ? 0
                    : img->retry_at - now_ms > INT64_MAX
                        ? INT64_MAX
                        : (int64_t)(img->retry_at - now_ms);
    if (t < 0 || retry < t)
      t = retry;
  }
  return t;
}

/* 4 ms doubling to 256 ms, saturating, as the shell session's EAGAIN. */
static void back_off(struct sophia_lc_image *img, uint64_t now) {
  img->retry_delay = !img->retry_delay       ? 4u
                     : img->retry_delay < 256u ? 2u * img->retry_delay
                                               : 256u;
  img->retry_at =
      now > UINT64_MAX - img->retry_delay ? UINT64_MAX : now + img->retry_delay;
}

static void closed(struct sophia_lc_image *img, enum sophia_ip_cause cause) {
  memset(&img->producer, 0, sizeof(img->producer));
  img->producer.fd = -1;
  img->producer.channel = SOPHIA_IPC_CLOSED;
  img->producer.cause = (uint8_t)cause;
}

/* A selection of 1 or no sealed grant is a clean absence of the channel. */
static int start_producer(struct sophia_lc_image *img, uint64_t now) {
  const struct sophia_lc_client *c = img->client;
  const struct sophia_lf_negotiated *w = &c->welcome;
  struct sophia_ip_config p = img->config.producer;
  if (w->selected_revision != SOPHIA_LF_REVISION_IMAGE_CHANNEL ||
      !(w->granted_capabilities & SOPHIA_LF_CAP_IMAGE_SEALED)) {
    closed(img, SOPHIA_IPX_UNAVAILABLE);
    return 0;
  }
  p.role = SOPHIA_IC_ROLE_LOCK;
  p.revision = SOPHIA_LF_REVISION_IMAGE_CHANNEL;
  p.connection_epoch = c->epoch;
  p.granted_kinds = SOPHIA_IC_KIND_BIT_SEALED |
                    ((w->granted_capabilities & SOPHIA_LF_CAP_IMAGE_DMABUF)
                         ? SOPHIA_IC_KIND_BIT_DMABUF
                         : 0u);
  if (sophia_ip_init(&img->producer, &p, now) != SOPHIA_IPR_OK) {
    /* A configuration that cannot serve this epoch: close as the caller. */
    closed(img, SOPHIA_IPX_CALLER);
    return 0;
  }
  return 1;
}

static const char *const files[] = {"image-channel", "image-limits",
                                    "image-formats"};

static int read_next(struct sophia_lc_image *img) {
  void *buf = img->file == 1   ? (void *)img->text
              : img->file == 2 ? (void *)img->limits
                               : img->config.formats;
  /* Larger caller storage never widens the formats object's budget. */
  size_t cap = img->file == 1   ? sizeof(img->text)
               : img->file == 2 ? sizeof(img->limits)
               : img->config.formats_capacity < SOPHIA_IC_MAX_FORMATS_BYTES
                   ? img->config.formats_capacity
                   : SOPHIA_IC_MAX_FORMATS_BYTES;
  return sophia_lc_internal_file_read(img->client, files[img->file - 1], buf,
                                      cap);
}

/* One set per pass. ENOENT on image-channel alone means absent; on a
 * companion, any read failure or an over-long file is a discovery error. */
static void discovery(struct sophia_lc_image *img, uint64_t now) {
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
  r = sophia_lc_internal_file_result(img->client, &n, &error);
  if (r == SOPHIA_9P_BUSY)
    return;
  if (r == SOPHIA_9P_AGAIN) {
    img->file = 0;
    back_off(img, now);
    return;
  }
  if (r) {
    read_errno =
        !error || (error == ENOENT && img->file != 1) ? EPROTO : (int)error;
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
  r = sophia_ip_discover(&img->producer, img->text, img->text_used,
                         img->limits, img->limits_used, img->config.formats,
                         img->formats_used, read_errno, img->pass, now);
  if (r == SOPHIA_IPR_OK) {
    img->stage = STAGE_CONNECTING;
    img->retry_delay = 0;
    img->retry_at = now;
  } else if (r == SOPHIA_IPR_AGAIN)
    back_off(img, now);
  else
    img->stage = STAGE_RUNNING;
}

static void connecting(struct sophia_lc_image *img, uint64_t now) {
  struct sophia_ic_discovery d;
  int fd = -1, r;
  if (now < img->retry_at)
    return;
  if (sophia_ic_discovery_decode(img->text, img->text_used, &d) !=
      SOPHIA_IC_OK) {
    sophia_ip_close(&img->producer);
    img->stage = STAGE_RUNNING;
    return;
  }
  r = sophia_ip_connect(img->config.endpoint_dir, d.endpoint, &fd);
  if (r == SOPHIA_IPR_AGAIN) {
    back_off(img, now);
    return;
  }
  if (r != SOPHIA_IPR_OK ||
      sophia_ip_attach(&img->producer, fd, now) != SOPHIA_IPR_OK) {
    if (fd >= 0)
      close(fd);
    sophia_ip_close(&img->producer);
  }
  img->stage = STAGE_RUNNING;
}

/* A terminal client ends the journal. An ImageBegin whose submit was never
 * queued cannot have been journaled: that is an authoritative refusal.
 * Anything possibly sent stays with the producer as journal loss. */
static void lost(struct sophia_lc_image *img) {
  struct sophia_lc_client *c = img->client;
  if (c->image_submission && c->image_submission == c->next_submission - 1 &&
      !c->submitted && !c->submit_sent &&
      (c->submit_stage == LC_TX_WRITE || c->submit_stage == LC_TX_SUBMIT)) {
    c->image_submission = 0;
    sophia_lc_internal_image_refused(img);
  }
  sophia_ip_journal_lost(&img->producer);
}

static void drive(struct sophia_lc_image *img, uint64_t now) {
  struct sophia_lc_client *c = img->client;
  int r;
  if (!img->started) {
    if (!c->negotiated || c->terminal || now < img->producer.last_now)
      return;
    img->started = 1;
    img->stage = start_producer(img, now) ? STAGE_DISCOVERING : STAGE_RUNNING;
  }
  if (now < img->producer.last_now)
    return;
  if (c->terminal) {
    lost(img);
    return;
  }
  if (img->stage == STAGE_DISCOVERING) {
    if (sophia_ip_channel(&img->producer) != SOPHIA_IPC_IDLE)
      img->stage = STAGE_RUNNING;
    else
      discovery(img, now);
  }
  if (img->stage == STAGE_CONNECTING) {
    if (sophia_ip_channel(&img->producer) != SOPHIA_IPC_DISCOVERED)
      img->stage = STAGE_RUNNING;
    else
      connecting(img, now);
  }
  /* Queue any new request now, so the caller's poll sees it. */
  if ((r = sophia_lc_internal_drive(c)))
    c->terminal = r;
}

int sophia_lc_image_dispatch(struct sophia_lc_image *img, short revents,
                             uint64_t now_ms) {
  int r = 0;
  if (!img || !img->client)
    return -(int)SOPHIA_IPR_ARGUMENT;
  if (img->started)
    r = sophia_ip_dispatch(&img->producer, revents, now_ms);
  if (r >= 0)
    drive(img, now_ms);
  return r;
}

/* Journal records arrive inside sophia_lc_service, which has no clock; the
 * producer's last time is the latest one the caller gave. */
void sophia_lc_internal_image_record(struct sophia_lc_image *img,
                                     const struct sophia_ic_record *v) {
  if (img->started)
    (void)sophia_ip_record(&img->producer, v, img->producer.last_now);
}
/* Status 1 (admitted) is not final; Rejected reason 9 is unavailable. */
void sophia_lc_internal_image_resource(
    struct sophia_lc_image *img, const struct sophia_lf_resource_status *v) {
  struct sophia_ip_offer_info info;
  enum sophia_ip_status status;
  if (!img->started || v->status < SOPHIA_LF_ACCEPTED ||
      v->status > SOPHIA_LF_CANCELLED ||
      sophia_ip_offer(&img->producer, v->transaction, &info) != SOPHIA_IPR_OK ||
      info.resource_id != v->resource.id ||
      info.resource_generation != v->resource.generation)
    return;
  status = v->status == SOPHIA_LF_ACCEPTED          ? SOPHIA_IPS_ACCEPTED
           : v->status == SOPHIA_LF_RESOURCE_REJECTED ? SOPHIA_IPS_REJECTED
                                                      : SOPHIA_IPS_CANCELLED;
  (void)sophia_ip_resource_status(
      &img->producer, v->resource.id, v->resource.generation, status,
      v->status == SOPHIA_LF_RESOURCE_REJECTED &&
          v->reason == SOPHIA_LF_REASON_IMAGE_CHANNEL_UNAVAILABLE,
      img->producer.last_now);
}
void sophia_lc_internal_image_refused(struct sophia_lc_image *img) {
  uint64_t offer = img->offer;
  img->offer = 0;
  if (img->started && offer)
    (void)sophia_ip_begin_refused(&img->producer, offer,
                                  img->producer.last_now);
}

int sophia_lc_image_begin(struct sophia_lc_image *img,
                          struct sophia_ic_metadata *metadata, int source_fd,
                          uint64_t now_ms, uint64_t *offer) {
  struct sophia_lc_client *c;
  struct sophia_ic_record record;
  uint8_t bytes[SOPHIA_LC_IMAGE_BEGIN_BYTES];
  uint8_t body[SOPHIA_IC_METADATA_BYTES];
  size_t n;
  int r;
  if (!img || !img->client || !metadata || !offer || source_fd < 0)
    return SOPHIA_IPR_ARGUMENT;
  c = img->client;
  if (!img->started || c->terminal ||
      sophia_ip_channel(&img->producer) == SOPHIA_IPC_CLOSED)
    return SOPHIA_IPR_CLOSED;
  if (!sophia_lc_ready(c))
    return SOPHIA_IPR_AGAIN;
  /* Room first: once the producer owns the fd, submission cannot fail. */
  if (c->submit_stage || c->next_submission == UINT64_MAX)
    return SOPHIA_IPR_BUSY;
  r = sophia_ip_begin(&img->producer, metadata, source_fd, now_ms, body, offer);
  if (r != SOPHIA_IPR_OK)
    return r;
  memset(&record, 0, sizeof(record));
  record.header.kind = SOPHIA_IC_IMAGE_BEGIN;
  record.header.epoch = c->epoch;
  record.header.submission = c->next_submission;
  record.value.image = *metadata;
  img->offer = *offer;
  if (sophia_ic_record_encode(bytes, sizeof(bytes), &record, &n) !=
          SOPHIA_IC_OK ||
      n != sizeof(bytes) || memcmp(bytes + 32, body, sizeof(body)) ||
      sophia_lc_internal_queue_image(c, bytes, n)) {
    /* Unreachable after the checks above; keep custody honest anyway. */
    sophia_lc_internal_image_refused(img);
    return SOPHIA_IPR_REFUSED;
  }
  if ((r = sophia_lc_internal_drive(c)))
    c->terminal = r;
  return SOPHIA_IPR_OK;
}

int sophia_lc_image_cancel(struct sophia_lc_image *img, uint64_t offer,
                           uint64_t now_ms) {
  struct sophia_lc_client *c;
  struct sophia_ip_slot *slot;
  struct sophia_lf_record record;
  int r;
  if (!img || !img->client)
    return SOPHIA_IPR_ARGUMENT;
  c = img->client;
  if (!img->started || c->terminal)
    return SOPHIA_IPR_CLOSED;
  if (!sophia_lc_ready(c))
    return SOPHIA_IPR_AGAIN;
  slot = ip_slot(&img->producer, offer);
  if (!slot)
    return SOPHIA_IPR_ARGUMENT;
  /* Room first: once the producer marks the offer, the cancel must queue. */
  if (c->submit_stage || c->next_submission == UINT64_MAX)
    return SOPHIA_IPR_BUSY;
  memset(&record, 0, sizeof(record));
  record.header.kind = SOPHIA_LF_RESOURCE_CANCEL;
  record.value.resource_step.transaction = offer;
  record.value.resource_step.resource.id = slot->metadata.resource_id;
  record.value.resource_step.resource.generation =
      slot->metadata.resource_generation;
  r = sophia_ip_cancel(&img->producer, offer, now_ms);
  if (r != SOPHIA_IPR_OK)
    return r;
  if (sophia_lc_internal_queue(c, &record))
    return SOPHIA_IPR_REFUSED; /* unreachable after the room check */
  if ((r = sophia_lc_internal_drive(c)))
    c->terminal = r;
  return SOPHIA_IPR_OK;
}

#define _GNU_SOURCE
#include "internal.h"
#include <errno.h>
#ifdef __linux__
#include <fcntl.h>
#include <sys/stat.h>
#endif

#define REQUIRED_SEALS (F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL)

/* The source is a regular file of exactly the declared bytes, sealed
 * against every change. A read-only mapping alone would prove nothing. */
static int sealed_source(int fd, uint64_t bytes)
{
#ifdef __linux__
    struct stat st;
    int seals = fcntl(fd, F_GET_SEALS);
    return seals >= 0 && (seals & REQUIRED_SEALS) == REQUIRED_SEALS && !fstat(fd, &st) &&
           S_ISREG(st.st_mode) && st.st_size >= 0 && (uint64_t)st.st_size == bytes;
#else
    (void)fd;
    (void)bytes;
    return 0;
#endif
}

int sophia_ip_begin(struct sophia_ip *p, struct sophia_ic_metadata *metadata, int source_fd,
                    uint64_t now_ms, uint8_t body[SOPHIA_IC_METADATA_BYTES], uint64_t *offer)
{
    struct sophia_ic_metadata m;
    struct sophia_ip_slot *free_slot = NULL;
    unsigned i, live = 0;
    if (!p || !metadata || !body || !offer || source_fd < 0 || !ip_clock(p, now_ms))
        return SOPHIA_IPR_ARGUMENT;
    if (p->channel != SOPHIA_IPC_READY)
        return p->channel == SOPHIA_IPC_CLOSED ? SOPHIA_IPR_CLOSED : SOPHIA_IPR_AGAIN;
    if (!p->next_offer) {
        ip_fail(p, SOPHIA_IPX_EXHAUSTED);
        return SOPHIA_IPR_EXHAUSTED;
    }
    for (i = 0; i < SOPHIA_IP_MAX_OFFERS; i++) {
        struct sophia_ip_slot *s = &p->slots[i];
        if (s->state == SOPHIA_IPO_FREE) {
            if (!free_slot)
                free_slot = s;
            continue;
        }
        live++;
        /* One live offer per resource generation. */
        if (s->state != SOPHIA_IPO_REFUSED && s->state != SOPHIA_IPO_SETTLED &&
            s->state != SOPHIA_IPO_UNKNOWN && s->metadata.resource_id == metadata->resource_id &&
            s->metadata.resource_generation == metadata->resource_generation)
            return SOPHIA_IPR_REFUSED;
    }
    if (!free_slot || live >= p->max_offers)
        return SOPHIA_IPR_BUSY;
    m = *metadata;
    m.offer = p->next_offer;
    if (m.source_kind != SOPHIA_IC_SOURCE_SEALED ||
        sophia_ic_metadata_admissible(&m, &p->expect) != SOPHIA_IC_OK ||
        !sealed_source(source_fd, m.logical_bytes))
        return SOPHIA_IPR_REFUSED;
    memset(free_slot, 0, sizeof *free_slot);
    free_slot->metadata = m;
    free_slot->fd = source_fd;
    free_slot->state = SOPHIA_IPO_BEGUN;
    free_slot->deadline_ms = now_ms + p->config.transfer_timeout_ms;
    if (free_slot->deadline_ms < now_ms)
        free_slot->deadline_ms = UINT64_MAX;
    p->next_offer++;
    ic_put_metadata(body, &m);
    *metadata = m;
    *offer = m.offer;
    return SOPHIA_IPR_OK;
}

int ip_expire(struct sophia_ip *p, uint64_t now_ms)
{
    unsigned i;
    int changed = 0;
    for (i = 0; i < SOPHIA_IP_MAX_OFFERS; i++) {
        struct sophia_ip_slot *s = &p->slots[i];
        if ((s->state != SOPHIA_IPO_BEGUN && s->state != SOPHIA_IPO_ADMITTED) ||
            s->deadline_passed || now_ms < s->deadline_ms)
            continue;
        /* Local expiry only: the owner's release or refusal still ends it. */
        s->deadline_passed = 1;
        if (s->state == SOPHIA_IPO_ADMITTED)
            s->state = SOPHIA_IPO_EXPIRED;
        changed = 1;
    }
    return changed;
}

static uint64_t earliest(uint64_t a, uint64_t b)
{
    return a < b ? a : b;
}

int64_t sophia_ip_timeout(const struct sophia_ip *p, uint64_t now_ms)
{
    uint64_t due = UINT64_MAX;
    unsigned i;
    if (!p)
        return -1;
    if (p->channel != SOPHIA_IPC_READY && p->channel != SOPHIA_IPC_CLOSED)
        due = p->config.startup_deadline_ms;
    if (p->channel == SOPHIA_IPC_HELLO_QUEUED || p->channel == SOPHIA_IPC_HELLO_SENT)
        due = earliest(due, p->hello_deadline_ms);
    for (i = 0; i < SOPHIA_IP_MAX_OFFERS; i++) {
        const struct sophia_ip_slot *s = &p->slots[i];
        if ((s->state == SOPHIA_IPO_BEGUN || s->state == SOPHIA_IPO_ADMITTED) &&
            !s->deadline_passed)
            due = earliest(due, s->deadline_ms);
    }
    if (due == UINT64_MAX)
        return -1;
    if (due <= now_ms)
        return 0;
    return due - now_ms > (uint64_t)INT64_MAX ? INT64_MAX : (int64_t)(due - now_ms);
}

static void info(const struct sophia_ip_slot *s, struct sophia_ip_offer_info *out)
{
    memset(out, 0, sizeof *out);
    out->offer = s->metadata.offer;
    out->resource_id = s->metadata.resource_id;
    out->resource_generation = s->metadata.resource_generation;
    out->state = (enum sophia_ip_offer_state)s->state;
    out->deadline_passed = s->deadline_passed;
    out->status = (enum sophia_ip_status)s->status;
    out->unavailable = s->unavailable;
    out->disposition = s->disposition;
    out->cancel_requested = s->cancelled;
}

int sophia_ip_offer(const struct sophia_ip *p, uint64_t offer, struct sophia_ip_offer_info *out)
{
    const struct sophia_ip_slot *s;
    if (!p || !out)
        return SOPHIA_IPR_ARGUMENT;
    s = ip_slot((struct sophia_ip *)p, offer);
    if (!s)
        return SOPHIA_IPR_REFUSED;
    info(s, out);
    return SOPHIA_IPR_OK;
}

int sophia_ip_offer_next(const struct sophia_ip *p, unsigned *cursor, struct sophia_ip_offer_info *out)
{
    if (!p || !cursor || !out)
        return 0;
    while (*cursor < SOPHIA_IP_MAX_OFFERS) {
        const struct sophia_ip_slot *s = &p->slots[(*cursor)++];
        if (s->state != SOPHIA_IPO_FREE) {
            info(s, out);
            return 1;
        }
    }
    return 0;
}

int sophia_ip_cancel(struct sophia_ip *p, uint64_t offer, uint64_t now_ms)
{
    struct sophia_ip_slot *s;
    if (!p || !ip_clock(p, now_ms))
        return SOPHIA_IPR_ARGUMENT;
    s = ip_slot(p, offer);
    if (!s)
        return SOPHIA_IPR_ARGUMENT;
    if (s->cancelled || s->status ||
        (s->state != SOPHIA_IPO_BEGUN && s->state != SOPHIA_IPO_ADMITTED &&
         s->state != SOPHIA_IPO_SENT && s->state != SOPHIA_IPO_AMBIGUOUS &&
         s->state != SOPHIA_IPO_EXPIRED))
        return SOPHIA_IPR_REFUSED;
    s->cancelled = 1;
    return SOPHIA_IPR_OK;
}

int sophia_ip_forget(struct sophia_ip *p, uint64_t offer)
{
    struct sophia_ip_slot *s;
    if (!p)
        return SOPHIA_IPR_ARGUMENT;
    s = ip_slot(p, offer);
    if (!s || (s->state != SOPHIA_IPO_REFUSED && s->state != SOPHIA_IPO_SETTLED &&
               s->state != SOPHIA_IPO_UNKNOWN))
        return SOPHIA_IPR_REFUSED;
    ip_free_slot(s);
    return SOPHIA_IPR_OK;
}

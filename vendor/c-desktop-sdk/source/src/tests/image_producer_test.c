/* The image-channel producer against a scripted owner over a real
 * SEQPACKET socketpair: the discovery barrier, offers, sealed sources,
 * deadlines, refusals, contradictions, closure causes and FD custody. */
#define _GNU_SOURCE
#include "sophia_image_producer.h"
#undef NDEBUG
#include <assert.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

#define EPOCH 21u
#define GEN 22u
#define LIMITS_GEN 6u
#define FORMATS_GEN 5u
#define START 1000u
#define STARTUP (START + 5000u)
#define TRANSFER 2000u
#define HELLO_MS 500u
#define SEALS (F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL)

static unsigned fd_count(void)
{
    DIR *d = opendir("/proc/self/fd");
    unsigned n = 0;
    assert(d);
    while (readdir(d))
        n++;
    closedir(d);
    return n;
}

/* ---- discovery set ------------------------------------------------------ */

struct set {
    char text[1024];
    size_t text_bytes, limits_bytes, formats_bytes;
    uint8_t limits[SOPHIA_IC_LIMITS_BYTES], formats[SOPHIA_IC_FORMATS_HEADER_BYTES];
};

/* Offer bound and channel kinds for ready(); tests restore the defaults. */
static unsigned offers_bound = 4;
static uint32_t channel_kinds = SOPHIA_IC_KIND_BIT_SEALED;

static void make_set(struct set *s, uint16_t role, uint64_t limits_generation, uint32_t kinds)
{
    struct sophia_ic_record r;
    struct sophia_ic_header h = {SOPHIA_IC_IMAGE_FORMATS, EPOCH, 0, 0};
    struct sophia_ic_formats f;
    memset(s, 0, sizeof *s);
    s->text_bytes = (size_t)snprintf(
        s->text, sizeof s->text,
        "sophia-image-channel version=1\nrole=%u\nconnection_epoch=%u\nchannel_generation=%u\n"
        "endpoint=image.sock\nsource_kinds=%u\nmax_packet_bytes=200\nmax_fds=4\nmax_offers=%u\n"
        "max_packets_per_turn=8\nhello_timeout_ms=%u\nvalidation_timeout_ms=100\n"
        "validation_chunk_bytes=65536\ncapture_interest_timeout_ms=200\nformats_generation=%u\n"
        "limits_generation=%u\n",
        role, EPOCH, GEN, kinds, offers_bound, HELLO_MS, FORMATS_GEN, LIMITS_GEN);
    memset(&r, 0, sizeof r);
    r.header.kind = SOPHIA_IC_IMAGE_LIMITS;
    r.header.epoch = EPOCH;
    r.value.limits.channel_generation = GEN;
    r.value.limits.limits_generation = limits_generation;
    r.value.limits.max_resource_bytes = 1u << 20;
    r.value.limits.max_sealed_bytes = 1u << 21;
    r.value.limits.max_offers = offers_bound;
    r.value.limits.max_validation_jobs = 2;
    r.value.limits.max_outputs = 2;
    r.value.limits.max_rows_per_output = 1;
    r.value.limits.max_width = 512;
    r.value.limits.max_height = 512;
    r.value.limits.validation_timeout_ms = 100;
    r.value.limits.capture_interest_timeout_ms = 200;
    r.value.limits.validation_chunk_bytes = 65536;
    r.value.limits.max_packets_per_turn = 8;
    if (kinds & SOPHIA_IC_KIND_BIT_DMABUF) {
        r.value.limits.max_dmabuf_source_bytes = 1u << 20;
        r.value.limits.max_renderer_bytes = 1u << 20;
        r.value.limits.max_renderer_entries = 2;
        r.value.limits.max_capture_jobs = 1;
    }
    if (!(kinds & SOPHIA_IC_KIND_BIT_SEALED))
        r.value.limits.max_validation_jobs = 0, r.value.limits.max_sealed_bytes = 0;
    assert(sophia_ic_record_encode(s->limits, sizeof s->limits, &r, &s->limits_bytes) ==
           SOPHIA_IC_OK);
    memset(&f, 0, sizeof f);
    f.channel_generation = GEN;
    f.formats_generation = FORMATS_GEN;
    f.limits_generation = LIMITS_GEN;
    assert(sophia_ic_formats_encode(s->formats, sizeof s->formats, &h, &f, NULL,
                                    &s->formats_bytes) == SOPHIA_IC_OK);
}

static struct sophia_ip_config config(uint16_t role)
{
    struct sophia_ip_config c;
    memset(&c, 0, sizeof c);
    c.role = role;
    c.revision = role == SOPHIA_IC_ROLE_SHELL ? 9 : 2;
    c.connection_epoch = EPOCH;
    c.granted_kinds = channel_kinds;
    c.bounds.max_resource_bytes = 1u << 20;
    c.bounds.max_width = c.bounds.max_height = 512;
    c.bounds.max_outputs = 2;
    c.bounds.transfer_slots = offers_bound;
    c.transfer_timeout_ms = TRANSFER;
    c.startup_deadline_ms = STARTUP;
    return c;
}

static int discover(struct sophia_ip *p, const struct set *s, uint64_t pass, uint64_t now)
{
    return sophia_ip_discover(p, s->text, s->text_bytes, s->limits, s->limits_bytes,
                              s->formats, s->formats_bytes, 0, pass, now);
}

/* ---- scripted owner ----------------------------------------------------- */

struct owner {
    int sock;
};

static struct sophia_ic_record event(uint16_t kind, uint64_t epoch)
{
    struct sophia_ic_record r;
    memset(&r, 0, sizeof r);
    r.header.kind = kind;
    r.header.epoch = epoch;
    r.header.sequence = 1;
    return r;
}

static struct sophia_ic_record channel_event(uint16_t kind, uint64_t generation, uint32_t value)
{
    struct sophia_ic_record r = event(kind, EPOCH);
    r.value.channel.channel_generation = generation;
    r.value.channel.value = value;
    return r;
}

static struct sophia_ic_record released(const struct sophia_ic_metadata *m, uint32_t disposition)
{
    struct sophia_ic_record r = event(SOPHIA_IC_SOURCE_RELEASED, EPOCH);
    r.value.released.offer = m->offer;
    r.value.released.resource_id = m->resource_id;
    r.value.released.resource_generation = m->resource_generation;
    r.value.released.disposition = disposition;
    return r;
}

static struct sophia_ic_record admitted(const struct sophia_ic_metadata *m)
{
    struct sophia_ic_record r = event(SOPHIA_IC_IMAGE_ADMITTED, EPOCH);
    r.value.image = *m;
    return r;
}

/* Receives one packet; returns its length and the received fd (or -1). */
static ssize_t owner_receive(struct owner *o, uint8_t *b, size_t n, int *fd)
{
    struct msghdr msg;
    struct iovec iov = {b, n};
    union {
        struct cmsghdr align;
        char bytes[CMSG_SPACE(4 * sizeof(int))];
    } control;
    struct cmsghdr *c;
    ssize_t got;
    memset(&msg, 0, sizeof msg);
    msg.msg_iov = &iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.bytes;
    msg.msg_controllen = sizeof control.bytes;
    *fd = -1;
    got = recvmsg(o->sock, &msg, MSG_DONTWAIT | MSG_CMSG_CLOEXEC);
    for (c = CMSG_FIRSTHDR(&msg); got >= 0 && c; c = CMSG_NXTHDR(&msg, c)) {
        assert(c->cmsg_level == SOL_SOCKET && c->cmsg_type == SCM_RIGHTS &&
               c->cmsg_len == CMSG_LEN(sizeof(int)) && *fd < 0);
        memcpy(fd, CMSG_DATA(c), sizeof *fd);
    }
    return got;
}

static struct sophia_ic_expect expect(uint16_t role)
{
    struct sophia_ic_expect e;
    memset(&e, 0, sizeof e);
    e.role = role;
    e.connection_epoch = EPOCH;
    e.channel_generation = GEN;
    e.source_kinds = channel_kinds;
    e.max_width = e.max_height = 512;
    e.max_resource_bytes = 1u << 20;
    return e;
}

/* ---- producer helpers --------------------------------------------------- */

static uint64_t now;

/* A READY producer on role, with the owner's end in o. */
static void ready(struct sophia_ip *p, struct owner *o, uint16_t role)
{
    struct sophia_ip_config c = config(role);
    struct set s;
    struct sophia_ic_record r;
    struct sophia_ic_packet packet;
    struct sophia_ic_expect e = expect(role);
    uint8_t b[256];
    int sv[2], fd;
    now = START;
    assert(sophia_ip_init(p, &c, now) == SOPHIA_IPR_OK);
    make_set(&s, role, LIMITS_GEN, channel_kinds);
    assert(discover(p, &s, 1, now) == SOPHIA_IPR_OK);
    assert(sophia_ip_channel(p) == SOPHIA_IPC_DISCOVERED);
    assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC | SOCK_NONBLOCK, 0, sv) == 0);
    o->sock = sv[1];
    assert(sophia_ip_attach(p, sv[0], now) == SOPHIA_IPR_OK);
    assert(sophia_ip_poll_fd(p) == sv[0]);
    assert(sophia_ip_poll_events(p) == (POLLIN | POLLOUT));
    assert(sophia_ip_path(p) == SOPHIA_IPP_WAIT);
    assert(sophia_ip_dispatch(p, POLLOUT, ++now) == SOPHIA_IP_CHANGED_CHANNEL);
    assert(sophia_ip_channel(p) == SOPHIA_IPC_HELLO_SENT);
    assert(sophia_ip_poll_events(p) == POLLIN);
    assert(owner_receive(o, b, sizeof b, &fd) == SOPHIA_IC_HELLO_BYTES && fd < 0);
    assert(sophia_ic_packet_decode(b, SOPHIA_IC_HELLO_BYTES, 0, &e, &packet) == SOPHIA_IC_OK);
    assert(packet.kind == SOPHIA_IC_HELLO);
    r = channel_event(SOPHIA_IC_CHANNEL_READY, GEN, channel_kinds);
    assert(sophia_ip_record(p, &r, ++now) == SOPHIA_IPR_OK);
    assert(sophia_ip_channel(p) == SOPHIA_IPC_READY);
    assert(sophia_ip_path(p) == SOPHIA_IPP_DESCRIPTOR);
}

static struct sophia_ic_metadata image(uint64_t resource, uint32_t w, uint32_t h)
{
    struct sophia_ic_metadata m;
    memset(&m, 0, sizeof m);
    m.resource_id = resource;
    m.resource_generation = 1;
    m.admission_epoch = 3;
    m.output = 7;
    m.allocation = 8;
    m.topology_generation = 9;
    m.source_kind = SOPHIA_IC_SOURCE_SEALED;
    m.source_format = SOPHIA_IC_SEALED_BGRA8_PREMULTIPLIED;
    m.width = w;
    m.height = h;
    m.stride = w * 4;
    m.plane_count = m.descriptor_count = 1;
    m.logical_bytes = (uint64_t)w * h * 4;
    return m;
}

static int sealed(uint32_t w, uint32_t h)
{
    void *pixels;
    size_t bytes;
    int fd;
    assert(sophia_ip_isb_create(w, h, &fd, &pixels, &bytes) == SOPHIA_IPR_OK);
    assert(bytes == (size_t)w * h * 4);
    memset(pixels, 0x40, bytes);
    assert(sophia_ip_isb_finish(fd, pixels, bytes) == SOPHIA_IPR_OK);
    return fd;
}

static uint64_t begin(struct sophia_ip *p, struct sophia_ic_metadata *m)
{
    uint8_t body[SOPHIA_IC_METADATA_BYTES], expected[SOPHIA_IC_METADATA_BYTES + 32];
    struct sophia_ic_record r = event(SOPHIA_IC_IMAGE_BEGIN, EPOCH);
    size_t n;
    uint64_t offer = 0;
    int fd = sealed(m->width, m->height);
    assert(sophia_ip_begin(p, m, fd, now, body, &offer) == SOPHIA_IPR_OK);
    assert(offer && m->offer == offer);
    /* The body is exactly the metadata an ImageBegin record carries. */
    r.header.submission = 1, r.header.sequence = 0;
    r.value.image = *m;
    assert(sophia_ic_record_encode(expected, sizeof expected, &r, &n) == SOPHIA_IC_OK);
    assert(!memcmp(body, expected + 32, sizeof body));
    return offer;
}

static enum sophia_ip_offer_state state(const struct sophia_ip *p, uint64_t offer)
{
    struct sophia_ip_offer_info i;
    assert(sophia_ip_offer(p, offer, &i) == SOPHIA_IPR_OK);
    return i.state;
}

/* Ends a test producer: custody is declared lost and every offer, with its
 * source fd, is forgotten. */
static void drop(struct sophia_ip *p, struct owner *o)
{
    struct sophia_ip_offer_info i;
    unsigned cursor = 0;
    sophia_ip_journal_lost(p);
    while (sophia_ip_offer_next(p, &cursor, &i))
        assert(sophia_ip_forget(p, i.offer) == SOPHIA_IPR_OK);
    close(o->sock);
}

/* ---- cases -------------------------------------------------------------- */

static void init_arguments(void)
{
    struct sophia_ip p;
    struct sophia_ip_config c = config(SOPHIA_IC_ROLE_SHELL);
    c.revision = 8;
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_ARGUMENT);
    c.revision = 10;
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_ARGUMENT);
    c = config(SOPHIA_IC_ROLE_LOCK);
    c.revision = 1;
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_ARGUMENT);
    c.revision = 3;
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_ARGUMENT);
    c = config(SOPHIA_IC_ROLE_SHELL);
    c.granted_kinds = 4;
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_ARGUMENT);
    c = config(SOPHIA_IC_ROLE_SHELL);
    assert(sophia_ip_init(&p, &c, STARTUP) == SOPHIA_IPR_ARGUMENT);
    c.transfer_timeout_ms = 0;
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_ARGUMENT);
    c = config(3);
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_ARGUMENT);
}

static void discovery_outcomes(void)
{
    struct sophia_ip p;
    struct sophia_ip_config c = config(SOPHIA_IC_ROLE_SHELL);
    struct set s, mixed;
    make_set(&s, SOPHIA_IC_ROLE_SHELL, LIMITS_GEN, SOPHIA_IC_KIND_BIT_SEALED);
    /* Absent: image admission fails for this epoch. */
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_OK);
    assert(sophia_ip_discover(&p, NULL, 0, NULL, 0, NULL, 0, ENOENT, 1, START) == SOPHIA_IPR_CLOSED);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_ABSENT && sophia_ip_path(&p) == SOPHIA_IPP_ERROR);
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_OK);
    assert(sophia_ip_discover(&p, NULL, 0, NULL, 0, NULL, 0, ENOENT, 1, START) == SOPHIA_IPR_CLOSED);
    assert(sophia_ip_path(&p) == SOPHIA_IPP_ERROR);
    /* Denied or malformed discovery also ends image admission. */
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_OK);
    assert(sophia_ip_discover(&p, NULL, 0, NULL, 0, NULL, 0, EACCES, 1, START) == SOPHIA_IPR_CLOSED);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_DISCOVERY && sophia_ip_path(&p) == SOPHIA_IPP_ERROR);
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_OK);
    assert(sophia_ip_discover(&p, s.text, s.text_bytes - 1, s.limits, s.limits_bytes, s.formats,
                              s.formats_bytes, 0, 1, START) == SOPHIA_IPR_CLOSED);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_DISCOVERY);
    /* Mixed generations: retry on a later pass, once per pass. */
    make_set(&mixed, SOPHIA_IC_ROLE_SHELL, LIMITS_GEN + 1, SOPHIA_IC_KIND_BIT_SEALED);
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_OK);
    assert(discover(&p, &mixed, 1, START) == SOPHIA_IPR_AGAIN);
    assert(sophia_ip_path(&p) == SOPHIA_IPP_WAIT);
    assert(discover(&p, &s, 1, START) == SOPHIA_IPR_AGAIN); /* same pass reads nothing */
    assert(sophia_ip_channel(&p) == SOPHIA_IPC_IDLE);
    assert(discover(&p, &s, 2, START + 1) == SOPHIA_IPR_OK);
    assert(discover(&p, &s, 3, START + 2) == SOPHIA_IPR_REFUSED);
    /* Another role's or another epoch's discovery is an error. */
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_OK);
    make_set(&mixed, SOPHIA_IC_ROLE_LOCK, LIMITS_GEN, SOPHIA_IC_KIND_BIT_SEALED);
    assert(discover(&p, &mixed, 1, START) == SOPHIA_IPR_CLOSED);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_DISCOVERY);
    c.connection_epoch = EPOCH + 1;
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_OK);
    assert(discover(&p, &s, 1, START) == SOPHIA_IPR_CLOSED);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_DISCOVERY);
    /* Role bounds below the limits object: incoherent. */
    c = config(SOPHIA_IC_ROLE_SHELL);
    c.bounds.max_width = 256;
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_OK);
    assert(discover(&p, &s, 1, START) == SOPHIA_IPR_CLOSED);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_DISCOVERY);
    /* No sealed source available: clean unavailability. */
    c = config(SOPHIA_IC_ROLE_SHELL);
    make_set(&mixed, SOPHIA_IC_ROLE_SHELL, LIMITS_GEN, SOPHIA_IC_KIND_BIT_DMABUF);
    c.granted_kinds = 3;
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_OK);
    assert(discover(&p, &mixed, 1, START) == SOPHIA_IPR_CLOSED);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_UNAVAILABLE && sophia_ip_path(&p) == SOPHIA_IPP_ERROR);
    /* Discovery naming a kind negotiation did not grant is incoherent,
     * not silently narrowed. */
    c.granted_kinds = SOPHIA_IC_KIND_BIT_DMABUF;
    make_set(&mixed, SOPHIA_IC_ROLE_SHELL, LIMITS_GEN, 3);
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_OK);
    assert(discover(&p, &mixed, 1, START) == SOPHIA_IPR_CLOSED);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_DISCOVERY && sophia_ip_path(&p) == SOPHIA_IPP_ERROR);
    c.granted_kinds = SOPHIA_IC_KIND_BIT_SEALED;
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_OK);
    assert(discover(&p, &mixed, 1, START) == SOPHIA_IPR_CLOSED);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_DISCOVERY && sophia_ip_path(&p) == SOPHIA_IPP_ERROR);
    /* A granted kind discovery withholds is availability: sealed granted
     * and dmabuf-only discovery is clean. */
    c.granted_kinds = 3;
    make_set(&mixed, SOPHIA_IC_ROLE_SHELL, LIMITS_GEN, SOPHIA_IC_KIND_BIT_DMABUF);
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_OK);
    assert(discover(&p, &mixed, 1, START) == SOPHIA_IPR_CLOSED);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_UNAVAILABLE);
    /* The startup deadline is an error, as is a clock that goes back. */
    c = config(SOPHIA_IC_ROLE_SHELL);
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_OK);
    assert(discover(&p, &s, 1, START - 1) == SOPHIA_IPR_ARGUMENT);
    assert(discover(&p, &s, 1, STARTUP) == SOPHIA_IPR_CLOSED);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_TIMEOUT && sophia_ip_path(&p) == SOPHIA_IPP_ERROR);
}

static void barrier(void)
{
    struct sophia_ip p;
    struct owner o;
    struct sophia_ic_record r;
    struct sophia_ic_metadata m = image(40, 16, 16);
    uint8_t body[SOPHIA_IC_METADATA_BYTES];
    uint64_t offer;
    unsigned base = fd_count();
    int fd;
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    /* A second Ready contradicts the barrier. */
    r = channel_event(SOPHIA_IC_CHANNEL_READY, GEN, 1);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_CLOSED);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_FAULT && sophia_ip_path(&p) == SOPHIA_IPP_ERROR);
    assert(sophia_ip_poll_fd(&p) == -1 && sophia_ip_poll_events(&p) == 0);
    fd = sealed(16, 16);
    assert(sophia_ip_begin(&p, &m, fd, now, body, &offer) == SOPHIA_IPR_CLOSED);
    close(fd);
    drop(&p, &o);
    assert(fd_count() == base);
    /* Ready for another generation, or before Hello is sent. */
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    drop(&p, &o);
    {
        struct sophia_ip q;
        struct sophia_ip_config c = config(SOPHIA_IC_ROLE_SHELL);
        struct set s;
        int sv[2];
        make_set(&s, SOPHIA_IC_ROLE_SHELL, LIMITS_GEN, 1);
        assert(sophia_ip_init(&q, &c, START) == SOPHIA_IPR_OK);
        assert(discover(&q, &s, 1, START) == SOPHIA_IPR_OK);
        assert(socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, sv) == 0);
        assert(sophia_ip_attach(&q, sv[0], START) == SOPHIA_IPR_REFUSED);
        assert(sophia_ip_channel(&q) == SOPHIA_IPC_DISCOVERED);
        assert(fcntl(sv[0], F_GETFD) >= 0); /* Refusal retains caller custody. */
        close(sv[0]);
        close(sv[1]);
        fd = sealed(16, 16);
        assert(sophia_ip_begin(&q, &m, fd, START, body, &offer) == SOPHIA_IPR_AGAIN);
        close(fd);
        r = channel_event(SOPHIA_IC_CHANNEL_READY, GEN, 1);
        assert(sophia_ip_record(&q, &r, START) == SOPHIA_IPR_CLOSED); /* not attached */
        assert(sophia_ip_cause(&q) == SOPHIA_IPX_FAULT);
        assert(sophia_ip_init(&q, &c, START) == SOPHIA_IPR_OK);
        assert(discover(&q, &s, 1, START) == SOPHIA_IPR_OK);
        assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC | SOCK_NONBLOCK, 0, sv) == 0);
        assert(sophia_ip_attach(&q, sv[0], START) == SOPHIA_IPR_OK);
        assert(sophia_ip_attach(&q, sv[0], START) == SOPHIA_IPR_REFUSED);
        assert(sophia_ip_record(&q, &r, START) == SOPHIA_IPR_CLOSED); /* Hello still queued */
        close(sv[1]);
        assert(sophia_ip_init(&q, &c, START) == SOPHIA_IPR_OK);
        assert(discover(&q, &s, 1, START) == SOPHIA_IPR_OK);
        assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC | SOCK_NONBLOCK, 0, sv) == 0);
        assert(sophia_ip_attach(&q, sv[0], START) == SOPHIA_IPR_OK);
        assert(sophia_ip_dispatch(&q, POLLOUT, START) == SOPHIA_IP_CHANGED_CHANNEL);
        r = channel_event(SOPHIA_IC_CHANNEL_READY, GEN + 1, 1);
        assert(sophia_ip_record(&q, &r, START) == SOPHIA_IPR_CLOSED);
        assert(sophia_ip_cause(&q) == SOPHIA_IPX_FAULT);
        /* A grant beyond discovery is a contradiction as well. */
        assert(sophia_ip_init(&q, &c, START) == SOPHIA_IPR_OK);
        close(sv[1]);
        assert(discover(&q, &s, 1, START) == SOPHIA_IPR_OK);
        assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC | SOCK_NONBLOCK, 0, sv) == 0);
        assert(sophia_ip_attach(&q, sv[0], START) == SOPHIA_IPR_OK);
        assert(sophia_ip_dispatch(&q, POLLOUT, START) == SOPHIA_IP_CHANGED_CHANNEL);
        r = channel_event(SOPHIA_IC_CHANNEL_READY, GEN, 3);
        assert(sophia_ip_record(&q, &r, START) == SOPHIA_IPR_CLOSED);
        /* Records of another epoch are ignored, not trusted. */
        assert(sophia_ip_init(&q, &c, START) == SOPHIA_IPR_OK);
        close(sv[1]);
        assert(discover(&q, &s, 1, START) == SOPHIA_IPR_OK);
        assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC | SOCK_NONBLOCK, 0, sv) == 0);
        assert(sophia_ip_attach(&q, sv[0], START) == SOPHIA_IPR_OK);
        assert(sophia_ip_dispatch(&q, POLLOUT, START) == SOPHIA_IP_CHANGED_CHANNEL);
        r = channel_event(SOPHIA_IC_CHANNEL_READY, GEN + 1, 3);
        r.header.epoch = EPOCH + 1;
        assert(sophia_ip_record(&q, &r, START) == SOPHIA_IPR_OK);
        assert(sophia_ip_channel(&q) == SOPHIA_IPC_HELLO_SENT && q.ignored == 1);
        /* No Ready by the Hello deadline: an error, not a fallback. */
        assert(sophia_ip_timeout(&q, START) == HELLO_MS);
        assert(sophia_ip_dispatch(&q, 0, START + HELLO_MS) == SOPHIA_IP_CHANGED_CHANNEL);
        assert(sophia_ip_cause(&q) == SOPHIA_IPX_TIMEOUT && sophia_ip_path(&q) == SOPHIA_IPP_ERROR);
        assert(sophia_ip_timeout(&q, START + HELLO_MS) == -1);
        close(sv[1]);
    }
    assert(fd_count() == base);
}

static void offer_lifecycle(uint16_t role)
{
    struct sophia_ip p;
    struct owner o;
    struct sophia_ic_metadata m = image(41, 16, 16), late;
    struct sophia_ic_record r;
    struct sophia_ic_packet packet;
    struct sophia_ic_expect e = expect(role);
    struct sophia_ip_offer_info i;
    uint8_t b[256];
    uint64_t offer, second;
    unsigned base = fd_count(), cursor = 0;
    int fd;
    ready(&p, &o, role);
    offer = begin(&p, &m);
    assert(offer == 1 && state(&p, offer) == SOPHIA_IPO_BEGUN);
    assert(sophia_ip_timeout(&p, now) == TRANSFER);
    /* Admission before send: nothing is queued until ImageAdmitted. */
    assert(sophia_ip_poll_events(&p) == POLLIN);
    assert(sophia_ip_dispatch(&p, POLLOUT, ++now) == 0);
    assert(owner_receive(&o, b, sizeof b, &fd) < 0 && errno == EAGAIN);
    r = admitted(&m);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(state(&p, offer) == SOPHIA_IPO_ADMITTED);
    assert(sophia_ip_poll_events(&p) == (POLLIN | POLLOUT));
    assert(sophia_ip_dispatch(&p, POLLOUT, ++now) == SOPHIA_IP_CHANGED_OFFERS);
    assert(state(&p, offer) == SOPHIA_IPO_SENT);
    assert(owner_receive(&o, b, sizeof b, &fd) == SOPHIA_IC_SOURCE_BYTES && fd >= 0);
    assert(sophia_ic_packet_decode(b, SOPHIA_IC_SOURCE_BYTES, 1, &e, &packet) == SOPHIA_IC_OK);
    assert(sophia_ic_metadata_equal(&packet.metadata, &m));
    assert((fcntl(fd, F_GET_SEALS) & SEALS) == SEALS);
    assert(lseek(fd, 0, SEEK_END) == 16 * 16 * 4);
    close(fd);
    /* The producer's copy was closed after the send. */
    assert(fd_count() == base + 2);
    assert(sophia_ip_forget(&p, offer) == SOPHIA_IPR_REFUSED);
    r = released(&m, SOPHIA_IC_RETAINED);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(sophia_ip_offer(&p, offer, &i) == SOPHIA_IPR_OK);
    assert(i.state == SOPHIA_IPO_SETTLED && i.disposition == SOPHIA_IC_RETAINED);
    assert(sophia_ip_offer_next(&p, &cursor, &i) && i.offer == offer);
    assert(!sophia_ip_offer_next(&p, &cursor, &i));
    /* A second release for a still-held offer is a contradiction. */
    {
        struct sophia_ip copy = p;
        copy.fd = -1; /* failing would close p's socket */
        assert(sophia_ip_record(&copy, &r, now) == SOPHIA_IPR_CLOSED);
        assert(sophia_ip_cause(&copy) == SOPHIA_IPX_FAULT);
    }
    assert(sophia_ip_forget(&p, offer) == SOPHIA_IPR_OK);
    /* A late event for a forgotten offer is ignored. */
    assert(sophia_ip_record(&p, &r, now) == SOPHIA_IPR_OK && p.ignored == 1);
    /* IDs keep increasing; a resource generation may return as a new offer. */
    late = image(41, 16, 16);
    second = begin(&p, &late);
    assert(second == 2);
    /* An event above every issued ID was never begun. */
    m.offer = 9;
    r = admitted(&m);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_CLOSED);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_FAULT);
    /* Closure keeps custody: the BEGUN offer is not forgettable. */
    assert(state(&p, second) == SOPHIA_IPO_BEGUN);
    assert(sophia_ip_forget(&p, second) == SOPHIA_IPR_REFUSED);
    sophia_ip_journal_lost(&p);
    assert(state(&p, second) == SOPHIA_IPO_UNKNOWN);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_FAULT); /* a fault is never downgraded */
    assert(sophia_ip_forget(&p, second) == SOPHIA_IPR_OK);
    drop(&p, &o);
    assert(fd_count() == base);
}

static void begin_refusals(void)
{
    struct sophia_ip p;
    struct owner o;
    struct sophia_ic_metadata m = image(50, 16, 16), big = image(51, 513, 1), dma;
    uint8_t body[SOPHIA_IC_METADATA_BYTES];
    uint64_t offer = 0;
    unsigned base, k;
    int fd, raw;
    void *pixels;
    size_t bytes;
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    base = fd_count();
    /* The caller keeps its fd on every refusal. */
    assert(sophia_ip_isb_create(16, 16, &raw, &pixels, &bytes) == SOPHIA_IPR_OK);
    assert(sophia_ip_begin(&p, &m, raw, now, body, &offer) == SOPHIA_IPR_REFUSED); /* unsealed */
    assert(fcntl(raw, F_GETFD) >= 0 && m.offer == 0 && offer == 0);
    assert(fcntl(raw, F_ADD_SEALS, F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL) == 0);
    assert(sophia_ip_begin(&p, &m, raw, now, body, &offer) == SOPHIA_IPR_REFUSED); /* no WRITE seal */
    munmap(pixels, bytes);
    close(raw);
    fd = sealed(16, 8); /* wrong size for 16x16 */
    assert(sophia_ip_begin(&p, &m, fd, now, body, &offer) == SOPHIA_IPR_REFUSED);
    close(fd);
    fd = sealed(513, 1);
    assert(sophia_ip_begin(&p, &big, fd, now, body, &offer) == SOPHIA_IPR_REFUSED); /* over max_width */
    close(fd);
    dma = image(52, 16, 16);
    dma.source_kind = SOPHIA_IC_SOURCE_DMABUF;
    dma.source_format = 0x34325241;
    dma.stride = 0;
    fd = sealed(16, 16);
    assert(sophia_ip_begin(&p, &dma, fd, now, body, &offer) == SOPHIA_IPR_REFUSED);
    close(fd);
    m.width = 0;
    fd = sealed(16, 16);
    assert(sophia_ip_begin(&p, &m, fd, now, body, &offer) == SOPHIA_IPR_REFUSED);
    assert(sophia_ip_begin(&p, &m, -1, now, body, &offer) == SOPHIA_IPR_ARGUMENT);
    close(fd);
    assert(fd_count() == base);
    /* One live offer per resource generation; four slots in all. */
    m = image(53, 16, 16);
    begin(&p, &m);
    m = image(53, 16, 16);
    fd = sealed(16, 16);
    assert(sophia_ip_begin(&p, &m, fd, now, body, &offer) == SOPHIA_IPR_REFUSED);
    m.resource_generation = 2;
    assert(sophia_ip_begin(&p, &m, fd, now, body, &offer) == SOPHIA_IPR_OK);
    for (k = 0; k < 2; k++) {
        m = image(60 + k, 16, 16);
        begin(&p, &m);
    }
    m = image(70, 16, 16);
    fd = sealed(16, 16);
    assert(sophia_ip_begin(&p, &m, fd, now, body, &offer) == SOPHIA_IPR_BUSY);
    close(fd);
    assert(sophia_ip_begin(&p, &m, 3, now - 1, body, &offer) == SOPHIA_IPR_ARGUMENT);
    sophia_ip_journal_lost(&p);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_JOURNAL_LOST && sophia_ip_path(&p) == SOPHIA_IPP_ERROR);
    for (k = 1; k <= 4; k++)
        assert(sophia_ip_forget(&p, k) == SOPHIA_IPR_OK);
    drop(&p, &o);
    assert(fd_count() == base - 2);
}

static void statuses(void)
{
    struct sophia_ip p;
    struct owner o;
    struct sophia_ic_metadata a = image(80, 16, 16), b = image(81, 16, 16), c = image(82, 16, 16);
    struct sophia_ic_record r;
    struct sophia_ip_offer_info i;
    unsigned base;
    uint8_t buf[256];
    int fd;
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    base = fd_count();
    begin(&p, &a);
    begin(&p, &b);
    begin(&p, &c);
    /* Rejected before admission: definitive, and the source is closed. */
    assert(sophia_ip_resource_status(&p, 80, 1, SOPHIA_IPS_REJECTED, 0, now) == SOPHIA_IPR_OK);
    assert(state(&p, a.offer) == SOPHIA_IPO_REFUSED);
    assert(fd_count() == base + 2);
    assert(sophia_ip_forget(&p, a.offer) == SOPHIA_IPR_OK);
    /* Statuses for other resources are not this producer's. */
    assert(sophia_ip_resource_status(&p, 999, 1, SOPHIA_IPS_ACCEPTED, 0, now) == SOPHIA_IPR_OK);
    assert(sophia_ip_resource_status(&p, 81, 1, SOPHIA_IPS_ACCEPTED, 1, now) == SOPHIA_IPR_ARGUMENT);
    /* Rejected after admission still waits for SourceReleased. */
    r = admitted(&b);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(sophia_ip_resource_status(&p, 81, 1, SOPHIA_IPS_REJECTED, 0, now) == SOPHIA_IPR_OK);
    assert(sophia_ip_offer(&p, b.offer, &i) == SOPHIA_IPR_OK);
    assert(i.state == SOPHIA_IPO_ADMITTED && i.status == SOPHIA_IPS_REJECTED);
    assert(sophia_ip_forget(&p, b.offer) == SOPHIA_IPR_REFUSED);
    r = released(&b, SOPHIA_IC_CANCELLED);
    assert(sophia_ip_dispatch(&p, POLLOUT, ++now) == SOPHIA_IP_CHANGED_OFFERS); /* sent anyway */
    assert(owner_receive(&o, buf, sizeof buf, &fd) == SOPHIA_IC_SOURCE_BYTES);
    close(fd);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(state(&p, b.offer) == SOPHIA_IPO_SETTLED);
    /* Reason 13: the offer is refused and the channel cleanly unavailable. */
    assert(sophia_ip_resource_status(&p, 82, 1, SOPHIA_IPS_REJECTED, 1, now) == SOPHIA_IPR_OK);
    assert(state(&p, c.offer) == SOPHIA_IPO_REFUSED);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_UNAVAILABLE && sophia_ip_path(&p) == SOPHIA_IPP_ERROR);
    assert(sophia_ip_path(&p) == SOPHIA_IPP_ERROR);
    drop(&p, &o);
    /* Accepted before a source was sent contradicts custody. */
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    a = image(90, 16, 16);
    begin(&p, &a);
    assert(sophia_ip_resource_status(&p, 90, 1, SOPHIA_IPS_ACCEPTED, 0, now) == SOPHIA_IPR_CLOSED);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_FAULT && state(&p, a.offer) == SOPHIA_IPO_BEGUN);
    drop(&p, &o);
    /* A second status, a release before admission, or a resource mismatch. */
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    a = image(91, 16, 16);
    begin(&p, &a);
    r = released(&a, SOPHIA_IC_BEFORE_USE);
    assert(sophia_ip_record(&p, &r, now) == SOPHIA_IPR_CLOSED);
    drop(&p, &o);
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    a = image(92, 16, 16);
    begin(&p, &a);
    r = admitted(&a);
    assert(sophia_ip_record(&p, &r, now) == SOPHIA_IPR_OK);
    r = released(&a, SOPHIA_IC_CANCELLED);
    r.value.released.resource_generation = 2;
    assert(sophia_ip_record(&p, &r, now) == SOPHIA_IPR_CLOSED);
    drop(&p, &o);
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    a = image(93, 16, 16);
    begin(&p, &a);
    r = admitted(&a);
    r.value.image.width = 8; /* not the offered metadata */
    assert(sophia_ip_record(&p, &r, now) == SOPHIA_IPR_CLOSED);
    assert(state(&p, a.offer) == SOPHIA_IPO_BEGUN);
    drop(&p, &o);
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    a = image(94, 16, 16);
    begin(&p, &a);
    r = admitted(&a);
    assert(sophia_ip_record(&p, &r, now) == SOPHIA_IPR_OK);
    assert(sophia_ip_resource_status(&p, 94, 1, SOPHIA_IPS_CANCELLED, 0, now) == SOPHIA_IPR_OK);
    assert(sophia_ip_resource_status(&p, 94, 1, SOPHIA_IPS_CANCELLED, 0, now) == SOPHIA_IPR_CLOSED);
    assert(sophia_ip_record(&p, &r, now) == SOPHIA_IPR_CLOSED); /* second admission */
    drop(&p, &o);
    /* The role refused the ImageBegin transaction. */
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    a = image(95, 16, 16);
    begin(&p, &a);
    assert(sophia_ip_begin_refused(&p, a.offer + 1, now) == SOPHIA_IPR_REFUSED);
    assert(sophia_ip_begin_refused(&p, a.offer, now) == SOPHIA_IPR_OK);
    assert(sophia_ip_begin_refused(&p, a.offer, now) == SOPHIA_IPR_REFUSED);
    assert(sophia_ip_forget(&p, a.offer) == SOPHIA_IPR_OK);
    assert(sophia_ip_path(&p) == SOPHIA_IPP_DESCRIPTOR);
    sophia_ip_close(&p);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_CALLER && sophia_ip_path(&p) == SOPHIA_IPP_ERROR);
    drop(&p, &o);
    assert(fd_count() == base - 2);
}

static void deadlines(void)
{
    struct sophia_ip p;
    struct owner o;
    struct sophia_ic_metadata a = image(100, 16, 16), b = image(101, 16, 16);
    struct sophia_ic_record r;
    struct sophia_ip_offer_info i;
    uint64_t t0;
    uint8_t buf[256];
    int fd;
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    assert(sophia_ip_timeout(&p, now) == -1);
    t0 = now;
    begin(&p, &a);
    now += 500;
    begin(&p, &b);
    assert(sophia_ip_timeout(&p, now) == TRANSFER - 500);
    r = admitted(&b);
    assert(sophia_ip_record(&p, &r, now) == SOPHIA_IPR_OK);
    /* Fill the owner's queue so b cannot be sent yet. */
    {
        uint8_t junk = 0;
        int dup_fd = dup(sophia_ip_poll_fd(&p)), n = 0;
        while (send(dup_fd, &junk, 1, MSG_DONTWAIT) == 1)
            n++;
        assert(errno == EAGAIN && n > 0);
        close(dup_fd);
        assert(sophia_ip_dispatch(&p, POLLOUT, now) == 0);
        assert(state(&p, b.offer) == SOPHIA_IPO_ADMITTED);
        assert(sophia_ip_poll_events(&p) == (POLLIN | POLLOUT));
        while (n--)
            assert(recv(o.sock, &junk, 1, MSG_DONTWAIT) == 1);
    }
    /* a's deadline passes before admission: kept BEGUN, reported once. */
    now = t0 + TRANSFER;
    assert(sophia_ip_timeout(&p, now) == 0);
    {
        /* Without the queue drained, b would still be ADMITTED; the dispatch
         * expires a first and then sends b. */
        assert(sophia_ip_dispatch(&p, POLLOUT, now) == SOPHIA_IP_CHANGED_OFFERS);
    }
    assert(sophia_ip_offer(&p, a.offer, &i) == SOPHIA_IPR_OK);
    assert(i.state == SOPHIA_IPO_BEGUN && i.deadline_passed);
    assert(state(&p, b.offer) == SOPHIA_IPO_SENT);
    assert(owner_receive(&o, buf, sizeof buf, &fd) == SOPHIA_IC_SOURCE_BYTES);
    close(fd);
    assert(sophia_ip_timeout(&p, now) == -1);
    assert(sophia_ip_forget(&p, a.offer) == SOPHIA_IPR_REFUSED);
    /* A late admission of an expired offer never sends. */
    r = admitted(&a);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(state(&p, a.offer) == SOPHIA_IPO_EXPIRED);
    assert(sophia_ip_poll_events(&p) == POLLIN);
    assert(sophia_ip_dispatch(&p, POLLOUT, ++now) == 0);
    assert(owner_receive(&o, buf, sizeof buf, &fd) < 0 && errno == EAGAIN);
    r = released(&a, SOPHIA_IC_CANCELLED);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(state(&p, a.offer) == SOPHIA_IPO_SETTLED);
    /* An admitted offer whose deadline passes unsent becomes EXPIRED. */
    a = image(102, 16, 16);
    begin(&p, &a);
    r = admitted(&a);
    assert(sophia_ip_record(&p, &r, now) == SOPHIA_IPR_OK);
    assert(sophia_ip_dispatch(&p, 0, now + TRANSFER) == SOPHIA_IP_CHANGED_OFFERS);
    assert(state(&p, a.offer) == SOPHIA_IPO_EXPIRED);
    now += TRANSFER;
    drop(&p, &o);
    sophia_ip_close(&p);
}

static void send_failures(void)
{
    struct sophia_ip p;
    struct owner o;
    struct sophia_ic_metadata a = image(110, 16, 16);
    struct sophia_ic_record r;
    unsigned base;
    uint8_t junk = 1;
    /* The owner hangs up: the next send is ambiguous, never replayed. */
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    base = fd_count();
    begin(&p, &a);
    r = admitted(&a);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    close(o.sock);
    assert(sophia_ip_dispatch(&p, POLLOUT, ++now) ==
           (SOPHIA_IP_CHANGED_CHANNEL | SOPHIA_IP_CHANGED_OFFERS));
    assert(state(&p, a.offer) == SOPHIA_IPO_AMBIGUOUS);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_UNAVAILABLE && sophia_ip_path(&p) == SOPHIA_IPP_ERROR);
    assert(fd_count() == base - 2); /* socket and source both closed */
    assert(sophia_ip_dispatch(&p, POLLOUT, ++now) == 0);
    r = released(&a, SOPHIA_IC_CANCELLED);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(state(&p, a.offer) == SOPHIA_IPO_SETTLED);
    /* Hangup seen by poll before any send. */
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    close(o.sock);
    assert(sophia_ip_dispatch(&p, POLLIN | POLLHUP, ++now) == SOPHIA_IP_CHANGED_CHANNEL);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_UNAVAILABLE);
    /* The owner never writes on the channel: data is a fault. */
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    assert(send(o.sock, &junk, 1, 0) == 1);
    assert(sophia_ip_dispatch(&p, POLLIN, ++now) == SOPHIA_IP_CHANGED_CHANNEL);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_FAULT && sophia_ip_path(&p) == SOPHIA_IPP_ERROR);
    drop(&p, &o);
    /* ChannelClosed: peer and revoked are clean, protocol is a fault, and a
     * later clean cause never replaces a fault. */
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    r = channel_event(SOPHIA_IC_CHANNEL_CLOSED, GEN, SOPHIA_IC_CLOSED_REVOKED);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_UNAVAILABLE);
    r = channel_event(SOPHIA_IC_CHANNEL_READY, GEN, 1);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK); /* never reopens */
    assert(sophia_ip_channel(&p) == SOPHIA_IPC_CLOSED);
    r = channel_event(SOPHIA_IC_CHANNEL_CLOSED, GEN, SOPHIA_IC_CLOSED_PROTOCOL);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_FAULT);
    r = channel_event(SOPHIA_IC_CHANNEL_CLOSED, GEN, SOPHIA_IC_CLOSED_PEER);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_FAULT);
    drop(&p, &o);
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    r = channel_event(SOPHIA_IC_CHANNEL_CLOSED, GEN, SOPHIA_IC_CLOSED_HELLO_TIMEOUT);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_FAULT);
    drop(&p, &o);
    assert(sophia_ip_dispatch(&p, 0, now - 1) == -(int)SOPHIA_IPR_ARGUMENT);
}

static void hello_backpressure(void)
{
    struct sophia_ip p;
    struct sophia_ip_config c = config(SOPHIA_IC_ROLE_LOCK);
    struct set s;
    uint8_t junk = 0, b[256];
    int sv[2], dup_fd, n = 0, fd;
    make_set(&s, SOPHIA_IC_ROLE_LOCK, LIMITS_GEN, 1);
    assert(sophia_ip_init(&p, &c, START) == SOPHIA_IPR_OK);
    assert(discover(&p, &s, 1, START) == SOPHIA_IPR_OK);
    assert(socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC | SOCK_NONBLOCK, 0, sv) == 0);
    dup_fd = dup(sv[0]);
    while (send(dup_fd, &junk, 1, MSG_DONTWAIT) == 1)
        n++;
    close(dup_fd);
    assert(sophia_ip_attach(&p, sv[0], START) == SOPHIA_IPR_OK);
    assert(sophia_ip_dispatch(&p, POLLOUT, START) == 0);
    assert(sophia_ip_channel(&p) == SOPHIA_IPC_HELLO_QUEUED);
    assert(sophia_ip_poll_events(&p) == (POLLIN | POLLOUT));
    while (n--)
        assert(recv(sv[1], &junk, 1, MSG_DONTWAIT) == 1);
    assert(sophia_ip_dispatch(&p, POLLOUT, START + 1) == SOPHIA_IP_CHANGED_CHANNEL);
    assert(owner_receive(&(struct owner){sv[1]}, b, sizeof b, &fd) == SOPHIA_IC_HELLO_BYTES);
    assert(b[8] == SOPHIA_IC_ROLE_LOCK);
    /* Startup expiry while waiting for Ready is an error. */
    assert(sophia_ip_dispatch(&p, 0, STARTUP) == SOPHIA_IP_CHANGED_CHANNEL);
    assert(sophia_ip_cause(&p) == SOPHIA_IPX_TIMEOUT);
    close(sv[1]);
}

static void connect_helper(void)
{
    char dir[] = "/tmp/sophia-ip-XXXXXX", path[128];
    struct sockaddr_un a;
    int d, listener, fd, clients[64], n = 0, i;
    assert(mkdtemp(dir));
    d = open(dir, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    assert(d >= 0);
    listener = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0);
    memset(&a, 0, sizeof a);
    a.sun_family = AF_UNIX;
    snprintf(a.sun_path, sizeof a.sun_path, "%s/image.sock", dir);
    assert(bind(listener, (struct sockaddr *)&a, sizeof a) == 0 && listen(listener, 0) == 0);
    assert(sophia_ip_connect(d, "image.sock", &fd) == SOPHIA_IPR_OK);
    assert(fcntl(fd, F_GETFL) & O_NONBLOCK);
    assert(fcntl(fd, F_GETFD) & FD_CLOEXEC);
    clients[n++] = fd;
    /* A full backlog is AGAIN, never a wait. */
    for (;;) {
        int r = sophia_ip_connect(d, "image.sock", &fd);
        if (r == SOPHIA_IPR_AGAIN)
            break;
        assert(r == SOPHIA_IPR_OK && n < 64);
        clients[n++] = fd;
    }
    for (i = 0; i < n; i++)
        close(clients[i]);
    assert(sophia_ip_connect(d, "a/b", &fd) == SOPHIA_IPR_ARGUMENT);
    assert(sophia_ip_connect(d, "..", &fd) == SOPHIA_IPR_ARGUMENT);
    assert(sophia_ip_connect(d, "", &fd) == SOPHIA_IPR_ARGUMENT);
    assert(sophia_ip_connect(d, "missing", &fd) == SOPHIA_IPR_REFUSED && errno == ENOENT);
    snprintf(path, sizeof path, "%s/file", dir);
    close(open(path, O_CREAT | O_WRONLY | O_CLOEXEC, 0600));
    assert(sophia_ip_connect(d, "file", &fd) == SOPHIA_IPR_REFUSED && errno == EINVAL);
    assert(symlinkat("image.sock", d, "link") == 0);
    assert(sophia_ip_connect(d, "link", &fd) == SOPHIA_IPR_REFUSED);
    unlinkat(d, "link", 0);
    unlinkat(d, "file", 0);
    unlinkat(d, "image.sock", 0);
    close(listener);
    close(d);
    rmdir(dir);
}

static void sealed_helpers(void)
{
    void *pixels;
    size_t bytes;
    int fd;
    assert(sophia_ip_isb_create(0, 1, &fd, &pixels, &bytes) == SOPHIA_IPR_ARGUMENT);
    assert(sophia_ip_isb_create(UINT32_MAX, UINT32_MAX, &fd, &pixels, &bytes) ==
           SOPHIA_IPR_ARGUMENT);
    assert(sophia_ip_isb_create(4, 2, &fd, &pixels, &bytes) == SOPHIA_IPR_OK && bytes == 32);
    assert(sophia_ip_isb_finish(fd, NULL, bytes) == SOPHIA_IPR_ARGUMENT);
    assert(sophia_ip_isb_finish(fd, pixels, bytes) == SOPHIA_IPR_OK);
    assert((fcntl(fd, F_GET_SEALS) & SEALS) == SEALS);
    assert(write(fd, "x", 1) < 0 && errno == EPERM);
    assert(ftruncate(fd, 64) < 0);
    close(fd);
}

/* Survivor-driven cases: the bound below four slots, ordered multi-send,
 * Accepted after a send, channel events before attach, settled offers
 * across journal loss, repeated expiry and non-sealed metadata. */
static void bounds_and_order(void)
{
    struct sophia_ip p;
    struct owner o;
    struct sophia_ic_metadata a, b, c, dma;
    struct sophia_ic_record r;
    struct sophia_ic_packet packet;
    struct sophia_ic_expect e;
    struct sophia_ip_offer_info i;
    uint8_t body[SOPHIA_IC_METADATA_BYTES], buf[256];
    uint64_t offer;
    int fd;
    /* Two transfer slots: the third live offer is BUSY. */
    offers_bound = 2;
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    a = image(120, 16, 16);
    b = image(121, 16, 16);
    begin(&p, &a);
    begin(&p, &b);
    c = image(122, 16, 16);
    fd = sealed(16, 16);
    assert(sophia_ip_begin(&p, &c, fd, now, body, &offer) == SOPHIA_IPR_BUSY);
    /* Refuse offer 1 and reuse its slot, so offer 3 sits before offer 2. */
    assert(sophia_ip_begin_refused(&p, a.offer, now) == SOPHIA_IPR_OK);
    assert(sophia_ip_forget(&p, a.offer) == SOPHIA_IPR_OK);
    assert(sophia_ip_begin(&p, &c, fd, now, body, &offer) == SOPHIA_IPR_OK && offer == 3);
    assert(p.slots[0].metadata.offer == 3);
    r = admitted(&c);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    r = admitted(&b);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    /* One dispatch sends both, in offer order. */
    assert(sophia_ip_dispatch(&p, POLLOUT, ++now) == SOPHIA_IP_CHANGED_OFFERS);
    assert(state(&p, b.offer) == SOPHIA_IPO_SENT && state(&p, c.offer) == SOPHIA_IPO_SENT);
    e = expect(SOPHIA_IC_ROLE_SHELL);
    assert(owner_receive(&o, buf, sizeof buf, &fd) == SOPHIA_IC_SOURCE_BYTES);
    close(fd);
    assert(sophia_ic_packet_decode(buf, SOPHIA_IC_SOURCE_BYTES, 1, &e, &packet) == SOPHIA_IC_OK);
    assert(packet.metadata.offer == b.offer);
    assert(owner_receive(&o, buf, sizeof buf, &fd) == SOPHIA_IC_SOURCE_BYTES);
    close(fd);
    assert(sophia_ic_packet_decode(buf, SOPHIA_IC_SOURCE_BYTES, 1, &e, &packet) == SOPHIA_IC_OK);
    assert(packet.metadata.offer == c.offer);
    /* Accepted after the send is the ordinary outcome. */
    assert(sophia_ip_resource_status(&p, 121, 1, SOPHIA_IPS_ACCEPTED, 0, now) == SOPHIA_IPR_OK);
    assert(sophia_ip_offer(&p, b.offer, &i) == SOPHIA_IPR_OK);
    assert(i.state == SOPHIA_IPO_SENT && i.status == SOPHIA_IPS_ACCEPTED);
    r = released(&b, SOPHIA_IC_RETAINED);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    /* Journal loss leaves a settled offer settled. */
    sophia_ip_journal_lost(&p);
    assert(sophia_ip_offer(&p, b.offer, &i) == SOPHIA_IPR_OK);
    assert(i.state == SOPHIA_IPO_SETTLED && i.disposition == SOPHIA_IC_RETAINED);
    assert(state(&p, c.offer) == SOPHIA_IPO_UNKNOWN);
    drop(&p, &o);
    offers_bound = 4;
    /* An expired BEGUN offer is reported changed once, not every dispatch. */
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    a = image(123, 16, 16);
    begin(&p, &a);
    assert(sophia_ip_dispatch(&p, 0, now + TRANSFER) == SOPHIA_IP_CHANGED_OFFERS);
    assert(sophia_ip_dispatch(&p, 0, now + TRANSFER + 1) == 0);
    now += TRANSFER + 1;
    drop(&p, &o);
    /* A channel granting both kinds still takes sealed sources only. */
    channel_kinds = 3;
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    dma = image(124, 16, 16);
    dma.source_kind = SOPHIA_IC_SOURCE_DMABUF;
    dma.source_format = 0x34325241;
    dma.modifier = 0;
    fd = sealed(16, 16);
    assert(sophia_ip_begin(&p, &dma, fd, now, body, &offer) == SOPHIA_IPR_REFUSED);
    close(fd);
    drop(&p, &o);
    channel_kinds = SOPHIA_IC_KIND_BIT_SEALED;
    /* Channel events before attach are faults, Closed included. */
    {
        struct sophia_ip_config cfg = config(SOPHIA_IC_ROLE_SHELL);
        struct set s;
        make_set(&s, SOPHIA_IC_ROLE_SHELL, LIMITS_GEN, 1);
        assert(sophia_ip_init(&p, &cfg, START) == SOPHIA_IPR_OK);
        assert(discover(&p, &s, 1, START) == SOPHIA_IPR_OK);
        r = channel_event(SOPHIA_IC_CHANNEL_CLOSED, GEN, SOPHIA_IC_CLOSED_PEER);
        assert(sophia_ip_record(&p, &r, START) == SOPHIA_IPR_CLOSED);
        assert(sophia_ip_cause(&p) == SOPHIA_IPX_FAULT);
    }
}

static int cancel_requested(const struct sophia_ip *p, uint64_t offer)
{
    struct sophia_ip_offer_info i;
    assert(sophia_ip_offer(p, offer, &i) == SOPHIA_IPR_OK);
    return i.cancel_requested;
}

static void cancellation(void)
{
    struct sophia_ip p;
    struct owner o;
    struct sophia_ic_metadata a = image(110, 16, 16), b = image(111, 16, 16), c = image(112, 16, 16);
    struct sophia_ic_record r;
    struct sophia_ip_offer_info i;
    uint8_t buf[256];
    unsigned base;
    int fd;
    ready(&p, &o, SOPHIA_IC_ROLE_SHELL);
    base = fd_count();
    begin(&p, &a);
    /* Arguments: unknown offers and a clock that went backward. */
    assert(sophia_ip_cancel(NULL, a.offer, now) == SOPHIA_IPR_ARGUMENT);
    assert(sophia_ip_cancel(&p, a.offer + 5, now) == SOPHIA_IPR_ARGUMENT);
    assert(sophia_ip_cancel(&p, a.offer, now - 1) == SOPHIA_IPR_ARGUMENT);
    assert(!cancel_requested(&p, a.offer));
    /* Marked before admission; a second mark is refused. */
    assert(sophia_ip_cancel(&p, a.offer, ++now) == SOPHIA_IPR_OK);
    assert(cancel_requested(&p, a.offer) && state(&p, a.offer) == SOPHIA_IPO_BEGUN);
    assert(sophia_ip_cancel(&p, a.offer, now) == SOPHIA_IPR_REFUSED);
    /* Admission after the mark: the source is never sent, the fd stays held. */
    r = admitted(&a);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(state(&p, a.offer) == SOPHIA_IPO_ADMITTED);
    assert(sophia_ip_poll_events(&p) == POLLIN);
    assert(sophia_ip_dispatch(&p, POLLOUT, ++now) == 0);
    assert(owner_receive(&o, buf, sizeof buf, &fd) < 0 && errno == EAGAIN);
    assert(fd_count() == base + 1);
    assert(sophia_ip_forget(&p, a.offer) == SOPHIA_IPR_REFUSED);
    /* An admitted offer marked cancelled still expires locally. */
    now += TRANSFER;
    assert(sophia_ip_dispatch(&p, 0, now) == SOPHIA_IP_CHANGED_OFFERS);
    assert(state(&p, a.offer) == SOPHIA_IPO_EXPIRED && fd_count() == base + 1);
    assert(sophia_ip_cancel(&p, a.offer, now) == SOPHIA_IPR_REFUSED);
    /* Accepted is impossible without a source; Cancelled then release. */
    assert(sophia_ip_resource_status(&p, 110, 1, SOPHIA_IPS_CANCELLED, 0, now) == SOPHIA_IPR_OK);
    assert(fd_count() == base + 1); /* no early release on the status */
    r = released(&a, SOPHIA_IC_CANCELLED);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(sophia_ip_offer(&p, a.offer, &i) == SOPHIA_IPR_OK);
    assert(i.state == SOPHIA_IPO_SETTLED && i.status == SOPHIA_IPS_CANCELLED && i.cancel_requested);
    assert(fd_count() == base);
    {
        struct sophia_ip copy = p; /* exactly once */
        copy.fd = -1;              /* p keeps its socket */
        assert(sophia_ip_record(&copy, &r, now) == SOPHIA_IPR_CLOSED);
        assert(sophia_ip_cause(&copy) == SOPHIA_IPX_FAULT);
    }
    assert(sophia_ip_cancel(&p, a.offer, now) == SOPHIA_IPR_REFUSED); /* settled */
    assert(sophia_ip_forget(&p, a.offer) == SOPHIA_IPR_OK);
    /* Acceptance wins the race: kept, then the release settles it. */
    begin(&p, &b);
    r = admitted(&b);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(sophia_ip_dispatch(&p, POLLOUT, ++now) == SOPHIA_IP_CHANGED_OFFERS);
    assert(owner_receive(&o, buf, sizeof buf, &fd) == SOPHIA_IC_SOURCE_BYTES && fd >= 0);
    close(fd);
    assert(sophia_ip_cancel(&p, b.offer, ++now) == SOPHIA_IPR_OK);
    assert(state(&p, b.offer) == SOPHIA_IPO_SENT);
    assert(sophia_ip_resource_status(&p, 111, 1, SOPHIA_IPS_ACCEPTED, 0, now) == SOPHIA_IPR_OK);
    assert(sophia_ip_offer(&p, b.offer, &i) == SOPHIA_IPR_OK);
    assert(i.status == SOPHIA_IPS_ACCEPTED && i.cancel_requested);
    assert(sophia_ip_cancel(&p, b.offer, now) == SOPHIA_IPR_REFUSED);
    r = released(&b, SOPHIA_IC_RETAINED);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(state(&p, b.offer) == SOPHIA_IPO_SETTLED);
    assert(sophia_ip_forget(&p, b.offer) == SOPHIA_IPR_OK);
    /* Cancelled before admission is a refusal: no source, fd closed. A
     * status already received refuses a later mark. */
    begin(&p, &c);
    assert(sophia_ip_cancel(&p, c.offer, now) == SOPHIA_IPR_OK);
    assert(sophia_ip_resource_status(&p, 112, 1, SOPHIA_IPS_CANCELLED, 0, now) == SOPHIA_IPR_OK);
    assert(state(&p, c.offer) == SOPHIA_IPO_REFUSED && fd_count() == base);
    assert(sophia_ip_cancel(&p, c.offer, now) == SOPHIA_IPR_REFUSED);
    assert(sophia_ip_forget(&p, c.offer) == SOPHIA_IPR_OK);
    c = image(113, 16, 16);
    begin(&p, &c);
    r = admitted(&c);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(sophia_ip_resource_status(&p, 113, 1, SOPHIA_IPS_REJECTED, 0, now) == SOPHIA_IPR_OK);
    assert(sophia_ip_cancel(&p, c.offer, now) == SOPHIA_IPR_REFUSED);
    assert(!cancel_requested(&p, c.offer));
    /* Settled or refused without any status: nothing left to cancel. */
    b = image(115, 16, 16);
    begin(&p, &b);
    r = admitted(&b);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(sophia_ip_dispatch(&p, POLLOUT, ++now) == SOPHIA_IP_CHANGED_OFFERS);
    assert(owner_receive(&o, buf, sizeof buf, &fd) == SOPHIA_IC_SOURCE_BYTES && fd >= 0);
    close(fd);
    r = released(&b, SOPHIA_IC_BEFORE_USE);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(state(&p, b.offer) == SOPHIA_IPO_SETTLED);
    assert(sophia_ip_cancel(&p, b.offer, now) == SOPHIA_IPR_REFUSED);
    assert(sophia_ip_forget(&p, b.offer) == SOPHIA_IPR_OK);
    b = image(116, 16, 16);
    begin(&p, &b);
    assert(sophia_ip_begin_refused(&p, b.offer, now) == SOPHIA_IPR_OK);
    assert(sophia_ip_cancel(&p, b.offer, now) == SOPHIA_IPR_REFUSED);
    assert(sophia_ip_forget(&p, b.offer) == SOPHIA_IPR_OK);
    /* Journal loss ends a marked, held offer as UNKNOWN. */
    a = image(114, 16, 16);
    begin(&p, &a);
    r = admitted(&a);
    assert(sophia_ip_record(&p, &r, ++now) == SOPHIA_IPR_OK);
    assert(sophia_ip_cancel(&p, a.offer, now) == SOPHIA_IPR_OK);
    sophia_ip_journal_lost(&p);
    assert(state(&p, a.offer) == SOPHIA_IPO_UNKNOWN);
    assert(sophia_ip_cancel(&p, a.offer, now) == SOPHIA_IPR_REFUSED);
    drop(&p, &o);
    assert(fd_count() == base - 2);
}

int main(void)
{
    unsigned base = fd_count();
    init_arguments();
    discovery_outcomes();
    barrier();
    offer_lifecycle(SOPHIA_IC_ROLE_SHELL);
    offer_lifecycle(SOPHIA_IC_ROLE_LOCK);
    begin_refusals();
    statuses();
    deadlines();
    send_failures();
    hello_backpressure();
    connect_helper();
    sealed_helpers();
    bounds_and_order();
    cancellation();
    assert(fd_count() == base);
    puts("image_producer_test: ok");
    return 0;
}

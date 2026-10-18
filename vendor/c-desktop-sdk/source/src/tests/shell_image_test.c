/* The shell image adapter over a scripted 9P shell peer and a real
 * SEQPACKET image endpoint: r9 negotiation, discovery through the session
 * wire, Hello, ImageBegin tickets, image journal routing and closed-channel refusal. */
#define _GNU_SOURCE
#include "shell_session_peer.h"
#include "../sophia_shell_image.h"
#undef NDEBUG
#include <assert.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <sys/un.h>

#define SEALED (UINT64_C(1) << 13)
#define CONTENT (UINT64_C(1) << 7)
#define GEN 22u
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

/* ---- discovery files ------------------------------------------------------ */

struct set {
    char text[1024];
    uint8_t limits[SOPHIA_IC_LIMITS_BYTES], formats[SOPHIA_IC_FORMATS_HEADER_BYTES];
    size_t text_bytes, limits_bytes, formats_bytes;
};

static void make_set(struct set *s, uint32_t kinds)
{
    struct sophia_ic_record r;
    struct sophia_ic_header h = {SOPHIA_IC_IMAGE_FORMATS, PEER_EPOCH, 0, 0};
    struct sophia_ic_formats f;
    memset(s, 0, sizeof(*s));
    s->text_bytes = (size_t)snprintf(
        s->text, sizeof(s->text),
        "sophia-image-channel version=1\nrole=1\nconnection_epoch=%u\nchannel_generation=%u\n"
        "endpoint=image.sock\nsource_kinds=%u\nmax_packet_bytes=200\nmax_fds=4\nmax_offers=4\n"
        "max_packets_per_turn=8\nhello_timeout_ms=500\nvalidation_timeout_ms=100\n"
        "validation_chunk_bytes=65536\ncapture_interest_timeout_ms=200\nformats_generation=5\n"
        "limits_generation=6\n",
        PEER_EPOCH, GEN, kinds);
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_IC_IMAGE_LIMITS;
    r.header.epoch = PEER_EPOCH;
    r.value.limits.channel_generation = GEN;
    r.value.limits.limits_generation = 6;
    r.value.limits.max_resource_bytes = 1u << 20;
    r.value.limits.max_sealed_bytes = 1u << 21;
    r.value.limits.max_offers = 4;
    r.value.limits.max_validation_jobs = 2;
    r.value.limits.max_outputs = 2;
    r.value.limits.max_rows_per_output = 1;
    r.value.limits.max_width = r.value.limits.max_height = 512;
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
    assert(sophia_ic_record_encode(s->limits, sizeof(s->limits), &r, &s->limits_bytes) ==
           SOPHIA_IC_OK);
    memset(&f, 0, sizeof(f));
    f.channel_generation = GEN;
    f.formats_generation = 5;
    f.limits_generation = 6;
    assert(sophia_ic_formats_encode(s->formats, sizeof(s->formats), &h, &f, NULL,
                                    &s->formats_bytes) == SOPHIA_IC_OK);
}

/* ---- rig ------------------------------------------------------------------ */

struct image_rig {
    struct rig r;
    struct set set;
    struct sophia_ss_image img;
    struct sophia_sf_negotiated welcome;
    uint8_t formats[256];
    uint8_t content_limits[512];
    char dir[64];
    int dirfd, listener, owner;
};

static const struct sophia_ss_config image_config = {{5, 9, SEALED | CONTENT}, SOPHIA_SF_BAR, 4096, 4, 8192,
                                                     NULL, 0};

static struct sophia_ss_image_config adapter_config(struct image_rig *t)
{
    struct sophia_ss_image_config c;
    memset(&c, 0, sizeof(c));
    c.producer.bounds.max_resource_bytes = 1u << 20;
    c.producer.bounds.max_width = c.producer.bounds.max_height = 512;
    c.producer.bounds.max_outputs = 2;
    c.producer.bounds.transfer_slots = 4;
    c.producer.transfer_timeout_ms = 2000;
    c.producer.startup_deadline_ms = 1000 + 5000;
    c.endpoint_dir = t->dirfd;
    c.formats = t->formats;
    c.formats_capacity = sizeof(t->formats);
    return c;
}

static void listen_dir(struct image_rig *t)
{
    struct sockaddr_un a;
    snprintf(t->dir, sizeof(t->dir), "/tmp/sophia-ss-image-XXXXXX");
    assert(mkdtemp(t->dir));
    t->dirfd = open(t->dir, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    assert(t->dirfd >= 0);
    t->listener = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    memset(&a, 0, sizeof(a));
    a.sun_family = AF_UNIX;
    snprintf(a.sun_path, sizeof(a.sun_path), "%s/image.sock", t->dir);
    assert(!bind(t->listener, (struct sockaddr *)&a, sizeof(a)) && !listen(t->listener, 4));
    t->owner = -1;
}

static void unlisten(struct image_rig *t)
{
    if (t->owner >= 0)
        close(t->owner);
    close(t->listener);
    unlinkat(t->dirfd, "image.sock", 0);
    close(t->dirfd);
    rmdir(t->dir);
}

/* Opens the session with the adapter attached and serves the files. */
static void image_start_role(struct image_rig *t, uint32_t kinds,
                             enum sophia_sf_profile profile, int deferred)
{
    struct sophia_ss_image_config c;
    struct sophia_ss_config config = image_config;
    config.profile = profile;
    if (profile == SOPHIA_SF_DOCK || profile == SOPHIA_SF_LAUNCHER)
        config.offer.required_capabilities = SEALED | (profile == SOPHIA_SF_DOCK ? 0x11a2 : 0x9a0);
    memset(t, 0, sizeof(*t));
    listen_dir(t);
    make_set(&t->set, kinds);
    rig_start(&t->r, &config);
    if (profile == SOPHIA_SF_DOCK)
        t->r.p.api_override = "sophia-shell-files version=1 role=dock epoch=17 fd_transfer=none\n";
    else if (profile == SOPHIA_SF_LAUNCHER)
        t->r.p.api_override = "sophia-shell-files version=1 role=launcher epoch=17 fd_transfer=none\n";
    t->welcome.selected_revision = 9;
    t->welcome.connection_epoch = PEER_EPOCH;
    t->welcome.capabilities = config.offer.required_capabilities;
    t->welcome.max_descriptors = 16;
    t->welcome.max_label_bytes = 128;
    t->welcome.max_pending_activations = 16;
    t->welcome.limits_published = 1;
    t->r.p.welcome_override = &t->welcome;
    t->r.p.revision = 9;
    t->r.p.image_data[0] = (const uint8_t *)t->set.text;
    t->r.p.image_bytes[0] = t->set.text_bytes;
    t->r.p.image_data[1] = t->set.limits;
    t->r.p.image_bytes[1] = t->set.limits_bytes;
    t->r.p.image_data[2] = t->set.formats;
    t->r.p.image_bytes[2] = t->set.formats_bytes;
    c = adapter_config(t);
    if (deferred) {
        const struct sophia_sf_limits *limits;
        struct sophia_sf_record record;
        size_t bytes;
        assert(!sophia_sf_decode(vector_Limits, sizeof(vector_Limits), &record));
        record.header.epoch = PEER_EPOCH;
        record.value.limits.grant_connection_epoch = PEER_EPOCH;
        record.value.limits.grant_content_epoch = 3;
        record.value.limits.max_resource_bytes = 1u << 20;
        record.value.limits.max_staging_bytes = 1u << 20;
        record.value.limits.max_resident_bytes = 1u << 20;
        record.value.limits.max_retiring_bytes = 1u << 20;
        record.value.limits.max_session_retiring_bytes = 3u << 20;
        record.value.limits.max_width_px = record.value.limits.max_height_px = 512;
        record.value.limits.max_outputs = 2;
        record.value.limits.max_open_transfers = 4;
        record.value.limits.max_chunk_bytes = 2048;
        record.value.limits.max_frame_payload = 4096;
        record.value.limits.max_input_queue_bytes = 8192;
        record.value.limits.transfer_timeout_ms = 2000;
        assert(!sophia_sf_encode(t->content_limits, sizeof(t->content_limits), &record, &bytes));
        t->r.p.object_data[SOPHIA_SF_LIMITS] = t->content_limits;
        t->r.p.object_bytes[SOPHIA_SF_LIMITS] = bytes;
        rig_settle(&t->r);
        assert(sophia_ss_state(&t->r.s) == SOPHIA_SS_READY);
        limits = sophia_ss_limits(&t->r.s);
        assert(limits);
        c.producer.bounds.max_resource_bytes = limits->max_resource_bytes;
        c.producer.bounds.max_width = limits->max_width_px;
        c.producer.bounds.max_height = limits->max_height_px;
        c.producer.bounds.max_outputs = limits->max_outputs;
        c.producer.bounds.transfer_slots = limits->max_open_transfers;
        c.producer.transfer_timeout_ms = limits->transfer_timeout_ms;
    }
    assert(sophia_ss_image_init(&t->img, &c) == SOPHIA_IPR_OK);
    assert(sophia_ss_image_attach(&t->r.s, &t->img) == SOPHIA_IPR_OK);
    assert(sophia_ss_image_attach(&t->r.s, &t->img) == SOPHIA_IPR_ARGUMENT);
}

static void image_start(struct image_rig *t, uint32_t kinds)
{
    image_start_role(t, kinds, SOPHIA_SF_BAR, 0);
}

static void image_step(struct image_rig *t)
{
    rig_step(&t->r);
    if (sophia_ss_image_poll_fd(&t->img) >= 0)
        (void)sophia_ss_image_dispatch(&t->img, sophia_ss_image_poll_events(&t->img), t->r.now);
    rig_step(&t->r);
}

static void image_close(struct image_rig *t)
{
    struct sophia_ip *p = sophia_ss_image_producer(&t->img);
    struct sophia_ip_offer_info i;
    unsigned cursor = 0;
    sophia_ss_close(&t->r.s);
    if (p) {
        sophia_ip_close(p);
        while (sophia_ip_offer_next(p, &cursor, &i))
            assert(sophia_ip_forget(p, i.offer) == SOPHIA_IPR_OK);
    }
    rig_close(&t->r);
    unlisten(t);
}

/* Journals one image event at the next sequence. */
static void peer_image(struct peer *p, struct sophia_ic_record *r)
{
    uint8_t b[256];
    size_t n;
    r->header.epoch = PEER_EPOCH;
    r->header.submission = 0;
    r->header.sequence = ++p->sequence;
    assert(sophia_ic_record_encode(b, sizeof(b), r, &n) == SOPHIA_IC_OK);
    peer_raw(p, b, n);
}

static void peer_channel(struct peer *p, uint16_t kind, uint32_t value)
{
    struct sophia_ic_record r;
    memset(&r, 0, sizeof(r));
    r.header.kind = kind;
    r.value.channel.channel_generation = GEN;
    r.value.channel.value = value;
    peer_image(p, &r);
}

static void peer_status(struct peer *p, uint64_t offer, uint64_t resource, uint16_t status,
                        uint16_t reason)
{
    struct sophia_sf_record r;
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_RESOURCE_STATUS;
    r.value.resource_status.transaction = offer;
    r.value.resource_status.grant_connection_epoch = PEER_EPOCH;
    r.value.resource_status.grant_content_epoch = 3;
    r.value.resource_status.resource_id = resource;
    r.value.resource_status.resource_generation = 1;
    r.value.resource_status.status = status;
    r.value.resource_status.reason = reason;
    peer_record(p, &r);
}

/* Reads the owner's next packet; returns its length and any fd. */
static ssize_t owner_receive(struct image_rig *t, uint8_t *b, size_t n, int *fd)
{
    struct msghdr msg;
    struct iovec iov = {b, n};
    union {
        struct cmsghdr align;
        char bytes[CMSG_SPACE(4 * sizeof(int))];
    } control;
    struct cmsghdr *c;
    ssize_t got;
    if (t->owner < 0)
        t->owner = accept4(t->listener, NULL, NULL, SOCK_CLOEXEC | SOCK_NONBLOCK);
    assert(t->owner >= 0);
    memset(&msg, 0, sizeof(msg));
    msg.msg_iov = &iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.bytes;
    msg.msg_controllen = sizeof(control.bytes);
    *fd = -1;
    got = recvmsg(t->owner, &msg, MSG_DONTWAIT | MSG_CMSG_CLOEXEC);
    for (c = CMSG_FIRSTHDR(&msg); got >= 0 && c; c = CMSG_NXTHDR(&msg, c))
        memcpy(fd, CMSG_DATA(c), sizeof(*fd));
    return got;
}

/* Every pending application event: image kinds never appear. Returns the
 * kinds seen, in order, up to n. */
static unsigned app_events(struct image_rig *t, uint16_t *kinds, unsigned n)
{
    const struct sophia_sf_record *e;
    unsigned seen = 0;
    while (!sophia_ss_event(&t->r.s, &e)) {
        assert(e->header.kind < 256);
        if (seen < n)
            kinds[seen] = e->header.kind;
        seen++;
        assert(!sophia_ss_consume(&t->r.s));
    }
    return seen;
}

static int sealed(uint32_t w, uint32_t h)
{
    void *pixels;
    size_t bytes;
    int fd;
    assert(sophia_ip_isb_create(w, h, &fd, &pixels, &bytes) == SOPHIA_IPR_OK);
    memset(pixels, 0x40, bytes);
    assert(sophia_ip_isb_finish(fd, pixels, bytes) == SOPHIA_IPR_OK);
    return fd;
}

static struct sophia_ic_metadata image(uint64_t resource)
{
    struct sophia_ic_metadata m;
    memset(&m, 0, sizeof(m));
    m.resource_id = resource;
    m.resource_generation = 1;
    m.admission_epoch = 3;
    m.output = 7;
    m.allocation = 8;
    m.topology_generation = 9;
    m.source_kind = SOPHIA_IC_SOURCE_SEALED;
    m.source_format = SOPHIA_IC_SEALED_BGRA8_PREMULTIPLIED;
    m.width = m.height = 16;
    m.stride = 64;
    m.plane_count = m.descriptor_count = 1;
    m.logical_bytes = 16 * 16 * 4;
    return m;
}

/* Through discovery, connect, Hello and ChannelReady. */
static void image_ready(struct image_rig *t)
{
    uint8_t b[256];
    unsigned i;
    int fd;
    struct sophia_ip *p;
    rig_settle(&t->r);
    assert(sophia_ss_state(&t->r.s) == SOPHIA_SS_READY);
    assert(sophia_ss_welcome(&t->r.s)->selected_revision == 9);
    for (i = 0; i < 32 && sophia_ss_image_poll_fd(&t->img) < 0; i++)
        image_step(t);
    p = sophia_ss_image_producer(&t->img);
    assert(p && sophia_ip_channel(p) >= SOPHIA_IPC_HELLO_QUEUED);
    for (i = 0; i < 4 && sophia_ip_channel(p) != SOPHIA_IPC_HELLO_SENT; i++)
        image_step(t);
    if (sophia_ip_channel(p) != SOPHIA_IPC_HELLO_SENT)
        fprintf(stderr, "profile %u channel %u cause %u; role bounds %llu %u %u %u %u\n",
                t->r.s.files.profile, sophia_ip_channel(p), sophia_ip_cause(p),
                (unsigned long long)p->config.bounds.max_resource_bytes,
                p->config.bounds.max_width, p->config.bounds.max_height,
                p->config.bounds.max_outputs, p->config.bounds.transfer_slots);
    assert(sophia_ip_channel(p) == SOPHIA_IPC_HELLO_SENT);
    assert(t->r.p.image_walks[0] == 1 && t->r.p.image_walks[1] == 1 && t->r.p.image_walks[2] == 1);
    assert(owner_receive(t, b, sizeof(b), &fd) == SOPHIA_IC_HELLO_BYTES && fd < 0);
    peer_channel(&t->r.p, SOPHIA_IC_CHANNEL_READY, SOPHIA_IC_KIND_BIT_SEALED);
    for (i = 0; i < 8 && sophia_ip_channel(p) != SOPHIA_IPC_READY; i++)
        image_step(t);
    assert(sophia_ip_channel(p) == SOPHIA_IPC_READY && sophia_ip_path(p) == SOPHIA_IPP_DESCRIPTOR);
}

static void ack_all(struct image_rig *t)
{
    unsigned i;
    (void)sophia_ss_ack(&t->r.s);
    for (i = 0; i < 6; i++)
        image_step(t);
}

/* ---- cases ---------------------------------------------------------------- */

static void attach_arguments(void)
{
    struct image_rig t;
    struct sophia_ss_image_config c;
    struct sophia_ss_config bad = image_config;
    memset(&t, 0, sizeof(t));
    listen_dir(&t);
    c = adapter_config(&t);
    assert(sophia_ss_image_init(&t.img, &c) == SOPHIA_IPR_OK);
    c.formats_capacity = SOPHIA_SS_IMAGE_FORMATS_MIN - 1;
    assert(sophia_ss_image_init(&t.img, &c) == SOPHIA_IPR_ARGUMENT);
    c = adapter_config(&t);
    assert(sophia_ss_image_init(&t.img, &c) == SOPHIA_IPR_OK);
    /* Bit 13 must be required, bit 14 never, and revision 9 offered. */
    bad.offer.required_capabilities = 0;
    rig_start(&t.r, &bad);
    assert(sophia_ss_image_attach(&t.r.s, &t.img) == SOPHIA_IPR_ARGUMENT);
    assert(!t.r.s.image && !t.r.s.files.image_mode);
    rig_close(&t.r);
    bad.offer.required_capabilities = SEALED;
    rig_start(&t.r, &bad);
    assert(sophia_ss_image_attach(&t.r.s, &t.img) == SOPHIA_IPR_ARGUMENT);
    rig_close(&t.r);
    bad = image_config;
    bad.offer.required_capabilities |= UINT64_C(1) << 14;
    rig_start(&t.r, &bad);
    assert(sophia_ss_image_attach(&t.r.s, &t.img) == SOPHIA_IPR_ARGUMENT);
    rig_close(&t.r);
    bad = image_config;
    bad.offer.maximum_revision = 8;
    rig_start(&t.r, &bad);
    assert(sophia_ss_image_attach(&t.r.s, &t.img) == SOPHIA_IPR_ARGUMENT);
    rig_close(&t.r);
    /* A completed legacy negotiation cannot acquire the image grant. */
    rig_ready(&t.r, &rig_config);
    t.r.s.files.offer.required_capabilities |= SEALED;
    t.r.s.files.offer.maximum_revision = 9;
    assert(sophia_ss_image_attach(&t.r.s, &t.img) == SOPHIA_IPR_ARGUMENT);
    rig_close(&t.r);
    unlisten(&t);
}

static void role_grants_remain_exact(void)
{
    const enum sophia_sf_profile profiles[] = {SOPHIA_SF_DOCK, SOPHIA_SF_LAUNCHER};
    unsigned role, mutation;
    for (role = 0; role < 2; role++) {
        for (mutation = 0; mutation < 3; mutation++) {
            struct image_rig t;
            image_start_role(&t, SOPHIA_IC_KIND_BIT_SEALED, profiles[role], 0);
            if (mutation == 0)
                t.welcome.selected_revision = profiles[role] == SOPHIA_SF_DOCK ? 8 : 7;
            else if (mutation == 1)
                t.welcome.capabilities |= UINT64_C(1) << 9;
            else
                t.welcome.capabilities &= ~SEALED;
            rig_settle(&t.r);
            rig_run(&t.r, 4);
            assert(sophia_ss_state(&t.r.s) == SOPHIA_SS_FAILED);
            assert(t.r.p.image_walks[0] == 0);
            image_close(&t);
        }
    }
}

static void sealed_offer_end_to_end(enum sophia_sf_profile profile, int deferred)
{
    struct image_rig t;
    struct sophia_ic_metadata m = image(40);
    struct sophia_ic_record r, staged;
    struct sophia_ip *p;
    struct sophia_ip_offer_info info;
    uint8_t b[256];
    uint16_t kinds[8];
    uint64_t offer = 0, ticket = 0;
    unsigned i, base = fd_count();
    int fd, src;
    image_start_role(&t, SOPHIA_IC_KIND_BIT_SEALED, profile, deferred);
    image_ready(&t);
    p = sophia_ss_image_producer(&t.img);
    /* Negotiation records were consumed by the session. */
    assert(app_events(&t, kinds, 8) == 0);
    ack_all(&t);
    src = sealed(16, 16);
    assert(sophia_ss_image_begin(&t.img, &m, src, t.r.now, &offer, &ticket) == SOPHIA_IPR_OK);
    assert(offer == 1 && ticket);
    for (i = 0; i < 16 && rig_outcome(&t.r, ticket) != SOPHIA_SS_SUBMITTED; i++)
        image_step(&t);
    assert(rig_outcome(&t.r, ticket) == SOPHIA_SS_SUBMITTED);
    /* The candidate is ImageBegin with the producer's metadata. */
    assert(sophia_ic_record_decode(t.r.p.staged, t.r.p.staged_used, &staged) == SOPHIA_IC_OK);
    assert(staged.header.kind == SOPHIA_IC_IMAGE_BEGIN && staged.header.epoch == PEER_EPOCH &&
           staged.header.submission == rig_last_id(&t.r));
    assert(sophia_ic_metadata_equal(&staged.value.image, &m));
    assert(app_events(&t, kinds, 8) == 0);
    ack_all(&t);
    /* Admission routes to the producer, which sends the source. */
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_IC_IMAGE_ADMITTED;
    r.value.image = m;
    peer_image(&t.r.p, &r);
    for (i = 0; i < 8 && owner_receive(&t, b, sizeof(b), &fd) < 0; i++)
        image_step(&t);
    assert(fd >= 0 && (fcntl(fd, F_GET_SEALS) & SEALS) == SEALS);
    close(fd);
    assert(sophia_ip_offer(p, offer, &info) == SOPHIA_IPR_OK && info.state == SOPHIA_IPO_SENT);
    /* Accepted reaches both the producer and the application. */
    peer_status(&t.r.p, offer, 40, 2, 0);
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_IC_SOURCE_RELEASED;
    r.value.released.offer = offer;
    r.value.released.resource_id = 40;
    r.value.released.resource_generation = 1;
    r.value.released.disposition = SOPHIA_IC_RETAINED;
    peer_image(&t.r.p, &r);
    for (i = 0; i < 6; i++)
        image_step(&t);
    assert(app_events(&t, kinds, 8) == 1 && kinds[0] == SOPHIA_SF_RESOURCE_STATUS);
    for (i = 0; i < 6; i++)
        image_step(&t);
    assert(sophia_ip_offer(p, offer, &info) == SOPHIA_IPR_OK);
    assert(info.state == SOPHIA_IPO_SETTLED && info.status == SOPHIA_IPS_ACCEPTED &&
           info.disposition == SOPHIA_IC_RETAINED);
    /* One routed status, however many passes saw it at the head. */
    assert(sophia_ip_channel(p) == SOPHIA_IPC_READY);
    assert(sophia_ip_forget(p, offer) == SOPHIA_IPR_OK);
    ack_all(&t);
    assert(sophia_ss_state(&t.r.s) == SOPHIA_SS_READY);
    image_close(&t);
    assert(fd_count() == base);
}

static void refusals_without_fallback(void)
{
    struct image_rig t;
    struct sophia_ic_metadata m = image(50);
    struct sophia_ip *p;
    struct sophia_ip_offer_info info;
    uint16_t kinds[8];
    uint64_t offer, ticket;
    unsigned i, base = fd_count();
    int src;
    /* A refused ImageBegin is a definitive pre-admission refusal. */
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    image_ready(&t);
    p = sophia_ss_image_producer(&t.img);
    ack_all(&t);
    t.r.p.policy = P_ERROR;
    t.r.p.error = 22;
    t.r.p.once = 1;
    src = sealed(16, 16);
    assert(sophia_ss_image_begin(&t.img, &m, src, t.r.now, &offer, &ticket) == SOPHIA_IPR_OK);
    for (i = 0; i < 16 && rig_outcome(&t.r, ticket) != SOPHIA_SS_REFUSED; i++)
        image_step(&t);
    assert(rig_outcome(&t.r, ticket) == SOPHIA_SS_REFUSED);
    image_step(&t);
    assert(sophia_ip_offer(p, offer, &info) == SOPHIA_IPR_OK && info.state == SOPHIA_IPO_REFUSED);
    assert(sophia_ip_forget(p, offer) == SOPHIA_IPR_OK);
    /* Rejected reason 13 before admission: refused, channel cleanly unavailable. */
    m = image(51);
    src = sealed(16, 16);
    assert(sophia_ss_image_begin(&t.img, &m, src, t.r.now, &offer, &ticket) == SOPHIA_IPR_OK);
    for (i = 0; i < 16 && rig_outcome(&t.r, ticket) != SOPHIA_SS_SUBMITTED; i++)
        image_step(&t);
    assert(rig_outcome(&t.r, ticket) == SOPHIA_SS_SUBMITTED);
    peer_status(&t.r.p, offer, 51, 3, 13);
    for (i = 0; i < 6; i++)
        image_step(&t);
    assert(app_events(&t, kinds, 8) >= 1);
    assert(sophia_ip_offer(p, offer, &info) == SOPHIA_IPR_OK && info.state == SOPHIA_IPO_REFUSED);
    assert(sophia_ip_cause(p) == SOPHIA_IPX_UNAVAILABLE && sophia_ip_path(p) == SOPHIA_IPP_ERROR);
    image_close(&t);
    assert(fd_count() == base);
}

static void absent_and_denied_discovery(void)
{
    struct image_rig t;
    struct sophia_ip *p;
    unsigned i, base = fd_count();
    /* ENOENT on image-channel: image admission ends for the epoch. */
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    t.r.p.image_error[0] = ENOENT;
    rig_settle(&t.r);
    for (i = 0; i < 32; i++)
        image_step(&t);
    p = sophia_ss_image_producer(&t.img);
    assert(p && sophia_ip_cause(p) == SOPHIA_IPX_ABSENT && sophia_ip_path(p) == SOPHIA_IPP_ERROR);
    assert(sophia_ss_state(&t.r.s) == SOPHIA_SS_READY);
    image_close(&t);
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    t.r.p.image_error[0] = ENOENT;
    rig_settle(&t.r);
    for (i = 0; i < 32; i++)
        image_step(&t);
    p = sophia_ss_image_producer(&t.img);
    assert(sophia_ip_path(p) == SOPHIA_IPP_ERROR);
    image_close(&t);
    /* A missing companion is a discovery error. */
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    t.r.p.image_error[2] = ENOENT;
    rig_settle(&t.r);
    for (i = 0; i < 32; i++)
        image_step(&t);
    p = sophia_ss_image_producer(&t.img);
    assert(sophia_ip_cause(p) == SOPHIA_IPX_DISCOVERY && sophia_ip_path(p) == SOPHIA_IPP_ERROR);
    image_close(&t);
    /* Denied image-channel: an error, never absent. */
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    t.r.p.image_error[0] = EACCES;
    rig_settle(&t.r);
    for (i = 0; i < 32; i++)
        image_step(&t);
    p = sophia_ss_image_producer(&t.img);
    assert(sophia_ip_cause(p) == SOPHIA_IPX_DISCOVERY);
    image_close(&t);
    assert(fd_count() == base);
}

static void channel_closed_and_session_end(void)
{
    struct image_rig t;
    struct sophia_ic_metadata m = image(60);
    struct sophia_ip *p;
    struct sophia_ip_offer_info info;
    uint64_t offer, ticket;
    unsigned i, base = fd_count();
    int src;
    /* ChannelClosed revoked: clean closure routed from the journal. */
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    image_ready(&t);
    p = sophia_ss_image_producer(&t.img);
    peer_channel(&t.r.p, SOPHIA_IC_CHANNEL_CLOSED, SOPHIA_IC_CLOSED_REVOKED);
    for (i = 0; i < 6; i++)
        image_step(&t);
    assert(sophia_ip_channel(p) == SOPHIA_IPC_CLOSED && sophia_ip_cause(p) == SOPHIA_IPX_UNAVAILABLE);
    assert(sophia_ss_state(&t.r.s) == SOPHIA_SS_READY);
    image_close(&t);
    /* The session ends with an offer in custody: the journal is lost. */
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    image_ready(&t);
    p = sophia_ss_image_producer(&t.img);
    ack_all(&t);
    src = sealed(16, 16);
    assert(sophia_ss_image_begin(&t.img, &m, src, t.r.now, &offer, &ticket) == SOPHIA_IPR_OK);
    for (i = 0; i < 16 && rig_outcome(&t.r, ticket) != SOPHIA_SS_SUBMITTED; i++)
        image_step(&t);
    assert(rig_outcome(&t.r, ticket) == SOPHIA_SS_SUBMITTED);
    image_step(&t);
    peer_close(&t.r.p);
    for (i = 0; i < 4; i++)
        rig_step(&t.r);
    assert(sophia_ss_state(&t.r.s) == SOPHIA_SS_CLOSED);
    assert(sophia_ip_offer(p, offer, &info) == SOPHIA_IPR_OK && info.state == SOPHIA_IPO_UNKNOWN);
    assert(sophia_ip_cause(p) == SOPHIA_IPX_JOURNAL_LOST);
    image_close(&t);
    assert(fd_count() == base);
}

static void unattached_session_unchanged(void)
{
    struct rig r;
    struct sophia_ic_record ready;
    uint8_t b[64];
    size_t n;
    unsigned i;
    /* Without an adapter, an image event is still a protocol failure. */
    rig_ready(&r, &rig_config);
    memset(&ready, 0, sizeof(ready));
    ready.header.kind = SOPHIA_IC_CHANNEL_READY;
    ready.header.epoch = PEER_EPOCH;
    ready.header.sequence = ++r.p.sequence;
    ready.value.channel.channel_generation = GEN;
    ready.value.channel.value = 1;
    assert(sophia_ic_record_encode(b, sizeof(b), &ready, &n) == SOPHIA_IC_OK);
    peer_raw(&r.p, b, n);
    for (i = 0; i < 4; i++)
        rig_step(&r);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_FAILED && !r.s.image);
    rig_close(&r);
}

/* Survivor-driven cases. */
static void status_routing(void)
{
    struct image_rig t;
    struct sophia_ic_metadata m = image(70);
    struct sophia_ip *p;
    struct sophia_ip_offer_info info;
    uint16_t kinds[8];
    uint64_t offer, ticket;
    unsigned i;
    int src;
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    image_ready(&t);
    p = sophia_ss_image_producer(&t.img);
    ack_all(&t);
    src = sealed(16, 16);
    assert(sophia_ss_image_begin(&t.img, &m, src, t.r.now, &offer, &ticket) == SOPHIA_IPR_OK);
    for (i = 0; i < 16 && rig_outcome(&t.r, ticket) != SOPHIA_SS_SUBMITTED; i++)
        image_step(&t);
    /* Status 1 is not final, another resource is not this offer's, and one
     * head event is routed once however many passes see it. */
    peer_status(&t.r.p, offer, 70, 1, 0);
    peer_status(&t.r.p, offer, 71, 4, 0);
    for (i = 0; i < 8; i++)
        image_step(&t);
    assert(sophia_ip_offer(p, offer, &info) == SOPHIA_IPR_OK && info.state == SOPHIA_IPO_BEGUN &&
           !info.status);
    assert(app_events(&t, kinds, 8) == 2);
    ack_all(&t);
    peer_status(&t.r.p, offer, 70, 4, 0);
    for (i = 0; i < 8; i++)
        image_step(&t);
    assert(sophia_ip_offer(p, offer, &info) == SOPHIA_IPR_OK && info.state == SOPHIA_IPO_REFUSED &&
           info.status == SOPHIA_IPS_CANCELLED);
    assert(sophia_ip_channel(p) == SOPHIA_IPC_READY);
    assert(app_events(&t, kinds, 8) == 1);
    ack_all(&t);
    /* A status whose transaction names one offer and resource another
     * belongs to neither. */
    {
        struct sophia_ic_metadata a = image(72), b = image(73);
        uint64_t oa, ob, ta, tb;
        src = sealed(16, 16);
        assert(sophia_ss_image_begin(&t.img, &a, src, t.r.now, &oa, &ta) == SOPHIA_IPR_OK);
        src = sealed(16, 16);
        assert(sophia_ss_image_begin(&t.img, &b, src, t.r.now, &ob, &tb) == SOPHIA_IPR_OK);
        for (i = 0; i < 32 && rig_outcome(&t.r, tb) != SOPHIA_SS_SUBMITTED; i++) {
            image_step(&t);
            (void)app_events(&t, kinds, 8);
            ack_all(&t);
        }
        assert(rig_outcome(&t.r, ta) == SOPHIA_SS_SUBMITTED &&
               rig_outcome(&t.r, tb) == SOPHIA_SS_SUBMITTED);
        peer_status(&t.r.p, oa, 73, 4, 0);
        for (i = 0; i < 6; i++)
            image_step(&t);
        assert(sophia_ip_offer(p, oa, &info) == SOPHIA_IPR_OK && info.state == SOPHIA_IPO_BEGUN);
        assert(sophia_ip_offer(p, ob, &info) == SOPHIA_IPR_OK && info.state == SOPHIA_IPO_BEGUN &&
               !info.status);
    }
    image_close(&t);
}

static void queue_room_keeps_fd(void)
{
    struct image_rig t;
    struct sophia_ic_metadata m = image(80);
    struct sophia_ss_reservation v;
    struct sophia_ip_offer_info info;
    unsigned cursor = 0;
    uint64_t offer = 0, ticket = 0;
    int src;
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    image_ready(&t);
    ack_all(&t);
    src = sealed(16, 16);
    /* Every slot reserved: BUSY, and the caller keeps its descriptor. */
    assert(!sophia_ss_reserve(&t.r.s, 4, 0, &v));
    assert(sophia_ss_image_begin(&t.img, &m, src, t.r.now, &offer, &ticket) == SOPHIA_IPR_BUSY);
    assert(fcntl(src, F_GETFD) >= 0 && !offer);
    assert(!sophia_ip_offer_next(sophia_ss_image_producer(&t.img), &cursor, &info));
    assert(!sophia_ss_cancel(&t.r.s, &v));
    /* Every byte reserved: BUSY as well. */
    assert(!sophia_ss_reserve(&t.r.s, 1, 8192 - 64, &v));
    assert(sophia_ss_image_begin(&t.img, &m, src, t.r.now, &offer, &ticket) == SOPHIA_IPR_BUSY);
    assert(!sophia_ss_cancel(&t.r.s, &v));
    close(src);
    image_close(&t);
}

static void contradictions(void)
{
    struct image_rig t;
    struct sophia_ic_record r;
    struct sophia_ip *p;
    uint8_t b[64], begin[SOPHIA_SS_IMAGE_BEGIN_BYTES];
    size_t n;
    unsigned i;
    /* Discovery naming a kind the welcome did not grant, with coherent
     * DMA-BUF limits: refused by the grant, not by incoherence. */
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED | SOPHIA_IC_KIND_BIT_DMABUF);
    rig_settle(&t.r);
    for (i = 0; i < 32; i++)
        image_step(&t);
    p = sophia_ss_image_producer(&t.img);
    assert(sophia_ip_cause(p) == SOPHIA_IPX_DISCOVERY);
    image_close(&t);
    /* An over-long companion is refused before it overruns its buffer. */
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    {
        static uint8_t longer[SOPHIA_IC_LIMITS_BYTES + 1];
        memcpy(longer, t.set.limits, t.set.limits_bytes);
        t.r.p.image_data[1] = longer;
        t.r.p.image_bytes[1] = sizeof(longer);
    }
    rig_settle(&t.r);
    for (i = 0; i < 32; i++)
        image_step(&t);
    assert(sophia_ip_cause(sophia_ss_image_producer(&t.img)) == SOPHIA_IPX_DISCOVERY);
    assert(t.img.limits_used == 0);
    image_close(&t);
    /* Formats storage larger than the contract's object budget reads no
     * more than that budget, and an over-long formats file is refused. */
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    {
        static uint8_t wide[SOPHIA_IC_MAX_FORMATS_BYTES + 64], served[SOPHIA_IC_MAX_FORMATS_BYTES + 1];
        memset(wide, 0xa5, sizeof(wide));
        memcpy(served, t.set.formats, t.set.formats_bytes);
        t.img.config.formats = wide;
        t.img.config.formats_capacity = sizeof(wide);
        t.r.p.image_data[2] = served;
        t.r.p.image_bytes[2] = sizeof(served);
        rig_settle(&t.r);
        for (i = 0; i < 256 && sophia_ip_cause(sophia_ss_image_producer(&t.img)) !=
                                   SOPHIA_IPX_DISCOVERY; i++)
            image_step(&t);
        assert(sophia_ip_cause(sophia_ss_image_producer(&t.img)) == SOPHIA_IPX_DISCOVERY);
        for (i = SOPHIA_IC_MAX_FORMATS_BYTES; i < sizeof(wide); i++)
            assert(wide[i] == 0xa5);
        assert(t.img.formats_used == 0);
    }
    image_close(&t);
    /* Image bits below r9 fail the welcome. */
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    t.welcome.selected_revision = 8;
    rig_settle(&t.r);
    rig_run(&t.r, 4);
    assert(sophia_ss_state(&t.r.s) == SOPHIA_SS_FAILED);
    image_close(&t);
    /* An image event for another epoch fails the session. */
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    image_ready(&t);
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_IC_CHANNEL_CLOSED;
    r.header.epoch = PEER_EPOCH + 1;
    r.header.sequence = ++t.r.p.sequence;
    r.value.channel.channel_generation = GEN;
    r.value.channel.value = SOPHIA_IC_CLOSED_PEER;
    assert(sophia_ic_record_encode(b, sizeof(b), &r, &n) == SOPHIA_IC_OK);
    peer_raw(&t.r.p, b, n);
    rig_run(&t.r, 4);
    assert(sophia_ss_state(&t.r.s) == SOPHIA_SS_FAILED);
    image_close(&t);
    /* The files client admits ImageBegin only in image mode, and whole. */
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    image_ready(&t);
    ack_all(&t);
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_IC_IMAGE_BEGIN;
    r.header.epoch = PEER_EPOCH;
    r.header.submission = 1;
    r.value.image = image(90);
    r.value.image.offer = 1;
    assert(sophia_ic_record_encode(begin, sizeof(begin), &r, &n) == SOPHIA_IC_OK);
    memset(begin + 16, 0, 8);
    begin[32] = 0; /* offer ID 0: never a valid ImageBegin */
    assert(sophia_sf_client_submit_bytes(&t.r.s.files, begin, n) == SOPHIA_9P_INVALID);
    t.r.s.files.image_mode = 0;
    begin[32] = 1;
    assert(sophia_sf_client_submit_bytes(&t.r.s.files, begin, n) == SOPHIA_9P_ARGUMENT);
    t.r.s.files.image_mode = 1;
    image_close(&t);
}

static void events_after_end(void)
{
    struct image_rig t;
    const struct sophia_sf_record *e;
    unsigned i;
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    image_ready(&t);
    /* An application event, then an image event, then the peer is gone. */
    peer_app_event(&t.r.p);
    peer_channel(&t.r.p, SOPHIA_IC_CHANNEL_CLOSED, SOPHIA_IC_CLOSED_PEER);
    rig_step(&t.r);
    peer_close(&t.r.p);
    for (i = 0; i < 4; i++)
        rig_step(&t.r);
    assert(sophia_ss_state(&t.r.s) == SOPHIA_SS_CLOSED);
    assert(!sophia_ss_event(&t.r.s, &e) && e->header.kind < 256);
    (void)sophia_ss_consume(&t.r.s);
    assert(sophia_ss_event(&t.r.s, &e));
    image_close(&t);
}

static void peer_released(struct peer *p, uint64_t offer, uint64_t resource, uint32_t disposition)
{
    struct sophia_ic_record r;
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_IC_SOURCE_RELEASED;
    r.value.released.offer = offer;
    r.value.released.resource_id = resource;
    r.value.released.resource_generation = 1;
    r.value.released.disposition = disposition;
    peer_image(p, &r);
}

static void peer_admitted(struct peer *p, const struct sophia_ic_metadata *m)
{
    struct sophia_ic_record r;
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_IC_IMAGE_ADMITTED;
    r.value.image = *m;
    peer_image(p, &r);
}

/* Steps, consuming and acknowledging application events, until ticket has
 * a final outcome. */
static enum sophia_ss_outcome settle_ticket(struct image_rig *t, uint64_t ticket)
{
    uint16_t kinds[8];
    unsigned i;
    for (i = 0; i < 32; i++) {
        enum sophia_ss_outcome o = rig_outcome(&t->r, ticket);
        if (o != SOPHIA_SS_ADMITTED_LOCAL && o != SOPHIA_SS_IN_FLIGHT)
            return o;
        image_step(t);
        (void)app_events(t, kinds, 8);
        ack_all(t);
    }
    return rig_outcome(&t->r, ticket);
}

static void run(struct image_rig *t, unsigned passes)
{
    uint16_t kinds[8];
    unsigned i;
    for (i = 0; i < passes; i++) {
        image_step(t);
        (void)app_events(t, kinds, 8);
        ack_all(t);
    }
}

static struct sophia_ip_offer_info offer_info(struct image_rig *t, uint64_t offer)
{
    struct sophia_ip_offer_info info;
    assert(sophia_ip_offer(sophia_ss_image_producer(&t->img), offer, &info) == SOPHIA_IPR_OK);
    return info;
}

static void cancel_before_admission(void)
{
    struct image_rig t;
    struct sophia_ic_metadata m = image(120);
    struct sophia_sf_record staged;
    struct sophia_ip_offer_info info;
    struct sophia_ss_reservation v;
    uint8_t b[256];
    uint64_t offer, ticket, cancel = 0;
    unsigned base = fd_count(), held;
    int src, fd;
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    assert(sophia_ss_image_cancel(&t.img, 1, t.r.now, &cancel) == SOPHIA_IPR_CLOSED);
    image_ready(&t);
    ack_all(&t);
    assert(sophia_ss_image_cancel(&t.img, 1, t.r.now, &cancel) == SOPHIA_IPR_ARGUMENT);
    assert(sophia_ss_image_cancel(&t.img, 1, t.r.now, NULL) == SOPHIA_IPR_ARGUMENT);
    src = sealed(16, 16);
    assert(sophia_ss_image_begin(&t.img, &m, src, t.r.now, &offer, &ticket) == SOPHIA_IPR_OK);
    /* No room: BUSY, and the offer is not marked. */
    assert(!sophia_ss_reserve(&t.r.s, (uint16_t)(t.r.s.slots - t.r.s.queued), 0, &v));
    assert(sophia_ss_image_cancel(&t.img, offer, t.r.now, &cancel) == SOPHIA_IPR_BUSY);
    assert(!offer_info(&t, offer).cancel_requested && !cancel);
    assert(!sophia_ss_cancel(&t.r.s, &v));
    /* Queued behind ImageBegin on the same connection, before admission. */
    assert(rig_outcome(&t.r, ticket) != SOPHIA_SS_SUBMITTED);
    assert(sophia_ss_image_cancel(&t.img, offer, t.r.now, &cancel) == SOPHIA_IPR_OK && cancel);
    assert(cancel > ticket && offer_info(&t, offer).cancel_requested);
    assert(sophia_ss_image_cancel(&t.img, offer, t.r.now, &cancel) == SOPHIA_IPR_REFUSED);
    assert(settle_ticket(&t, ticket) == SOPHIA_SS_SUBMITTED);
    assert(settle_ticket(&t, cancel) == SOPHIA_SS_SUBMITTED);
    /* The transaction file was clunked; its last bytes are still staged. */
    assert(!sophia_sf_decode(t.r.p.staged, peer_get(t.r.p.staged, 4), &staged));
    assert(staged.header.kind == SOPHIA_SF_RESOURCE_CANCEL && staged.header.epoch == PEER_EPOCH);
    assert(staged.value.resource_cancel.transaction == offer &&
           staged.value.resource_cancel.grant_connection_epoch == PEER_EPOCH &&
           staged.value.resource_cancel.grant_content_epoch == 3 &&
           staged.value.resource_cancel.resource_id == 120 &&
           staged.value.resource_cancel.resource_generation == 1);
    /* Admitted after the cancel: the source is never sent; the fd is held. */
    peer_admitted(&t.r.p, &m);
    run(&t, 8);
    assert(owner_receive(&t, b, sizeof(b), &fd) < 0 && errno == EAGAIN);
    info = offer_info(&t, offer);
    assert(info.state == SOPHIA_IPO_ADMITTED && !info.status);
    held = fd_count(); /* the session, channel and the held source */
    /* Cancelled is not a release; SourceReleased settles exactly once. */
    peer_status(&t.r.p, offer, 120, 4, 0);
    run(&t, 6);
    info = offer_info(&t, offer);
    assert(info.state == SOPHIA_IPO_ADMITTED && info.status == SOPHIA_IPS_CANCELLED);
    assert(fd_count() == held);
    peer_released(&t.r.p, offer, 120, SOPHIA_IC_CANCELLED);
    run(&t, 6);
    info = offer_info(&t, offer);
    assert(info.state == SOPHIA_IPO_SETTLED && info.disposition == SOPHIA_IC_CANCELLED);
    assert(fd_count() == held - 1);
    assert(sophia_ss_image_cancel(&t.img, offer, t.r.now, &cancel) == SOPHIA_IPR_REFUSED);
    assert(sophia_ip_channel(sophia_ss_image_producer(&t.img)) == SOPHIA_IPC_READY);
    image_close(&t);
    assert(fd_count() == base);
}

static void cancel_races(void)
{
    struct image_rig t;
    struct sophia_ic_metadata a = image(130), b = image(131), c = image(132);
    struct sophia_ip_offer_info info;
    uint8_t buf[256];
    uint64_t oa, ob, oc, ta, tb, tc, cancel;
    unsigned i, base = fd_count();
    int src, fd;
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    image_ready(&t);
    ack_all(&t);
    /* Acceptance wins: the source was sent before the cancel was seen. */
    src = sealed(16, 16);
    assert(sophia_ss_image_begin(&t.img, &a, src, t.r.now, &oa, &ta) == SOPHIA_IPR_OK);
    assert(settle_ticket(&t, ta) == SOPHIA_SS_SUBMITTED);
    peer_admitted(&t.r.p, &a);
    for (i = 0; i < 8 && owner_receive(&t, buf, sizeof(buf), &fd) < 0; i++)
        image_step(&t);
    assert(fd >= 0);
    close(fd);
    assert(sophia_ss_image_cancel(&t.img, oa, t.r.now, &cancel) == SOPHIA_IPR_OK);
    assert(settle_ticket(&t, cancel) == SOPHIA_SS_SUBMITTED);
    peer_status(&t.r.p, oa, 130, 2, 0);
    run(&t, 6);
    info = offer_info(&t, oa);
    assert(info.state == SOPHIA_IPO_SENT && info.status == SOPHIA_IPS_ACCEPTED && info.cancel_requested);
    peer_released(&t.r.p, oa, 130, SOPHIA_IC_RETAINED);
    run(&t, 6);
    info = offer_info(&t, oa);
    assert(info.state == SOPHIA_IPO_SETTLED && info.disposition == SOPHIA_IC_RETAINED);
    assert(sophia_ip_forget(sophia_ss_image_producer(&t.img), oa) == SOPHIA_IPR_OK);
    /* A refused cancel keeps the mark: no resumed source, wait for release. */
    src = sealed(16, 16);
    assert(sophia_ss_image_begin(&t.img, &b, src, t.r.now, &ob, &tb) == SOPHIA_IPR_OK);
    assert(settle_ticket(&t, tb) == SOPHIA_SS_SUBMITTED);
    t.r.p.policy = P_ERROR;
    t.r.p.error = 22;
    t.r.p.once = 1;
    assert(sophia_ss_image_cancel(&t.img, ob, t.r.now, &cancel) == SOPHIA_IPR_OK);
    assert(settle_ticket(&t, cancel) == SOPHIA_SS_REFUSED);
    assert(sophia_ss_state(&t.r.s) == SOPHIA_SS_READY);
    peer_admitted(&t.r.p, &b);
    run(&t, 8);
    assert(owner_receive(&t, buf, sizeof(buf), &fd) < 0 && errno == EAGAIN);
    info = offer_info(&t, ob);
    assert(info.state == SOPHIA_IPO_ADMITTED && info.cancel_requested);
    assert(sophia_ss_image_cancel(&t.img, ob, t.r.now, &cancel) == SOPHIA_IPR_REFUSED);
    peer_status(&t.r.p, ob, 131, 3, 0);
    peer_released(&t.r.p, ob, 131, SOPHIA_IC_BEFORE_USE);
    run(&t, 8);
    info = offer_info(&t, ob);
    assert(info.state == SOPHIA_IPO_SETTLED && info.status == SOPHIA_IPS_REJECTED);
    assert(sophia_ip_forget(sophia_ss_image_producer(&t.img), ob) == SOPHIA_IPR_OK);
    /* An owner-initiated Cancelled routes without a local mark. */
    src = sealed(16, 16);
    assert(sophia_ss_image_begin(&t.img, &c, src, t.r.now, &oc, &tc) == SOPHIA_IPR_OK);
    assert(settle_ticket(&t, tc) == SOPHIA_SS_SUBMITTED);
    peer_admitted(&t.r.p, &c);
    for (i = 0; i < 8 && owner_receive(&t, buf, sizeof(buf), &fd) < 0; i++)
        image_step(&t);
    assert(fd >= 0);
    close(fd);
    peer_status(&t.r.p, oc, 132, 4, 0);
    peer_released(&t.r.p, oc, 132, SOPHIA_IC_CANCELLED);
    run(&t, 8);
    info = offer_info(&t, oc);
    assert(info.state == SOPHIA_IPO_SETTLED && info.status == SOPHIA_IPS_CANCELLED &&
           !info.cancel_requested);
    assert(sophia_ip_channel(sophia_ss_image_producer(&t.img)) == SOPHIA_IPC_READY);
    image_close(&t);
    assert(fd_count() == base);
}

/* The session ends with a cancel not yet answered: custody is unknown. */
static void cancel_lost_with_session(void)
{
    struct image_rig t;
    struct sophia_ic_metadata m = image(140);
    struct sophia_ip *p;
    uint64_t offer, ticket, cancel;
    unsigned i, base = fd_count();
    int src;
    image_start(&t, SOPHIA_IC_KIND_BIT_SEALED);
    image_ready(&t);
    p = sophia_ss_image_producer(&t.img);
    ack_all(&t);
    src = sealed(16, 16);
    assert(sophia_ss_image_begin(&t.img, &m, src, t.r.now, &offer, &ticket) == SOPHIA_IPR_OK);
    assert(settle_ticket(&t, ticket) == SOPHIA_SS_SUBMITTED);
    peer_admitted(&t.r.p, &m);
    t.r.p.policy = P_HOLD;
    assert(sophia_ss_image_cancel(&t.img, offer, t.r.now, &cancel) == SOPHIA_IPR_OK);
    for (i = 0; i < 6; i++)
        image_step(&t);
    assert(rig_outcome(&t.r, cancel) == SOPHIA_SS_IN_FLIGHT);
    assert(offer_info(&t, offer).state == SOPHIA_IPO_ADMITTED);
    peer_close(&t.r.p);
    for (i = 0; i < 4; i++)
        rig_step(&t.r);
    assert(sophia_ss_state(&t.r.s) == SOPHIA_SS_CLOSED);
    assert(offer_info(&t, offer).state == SOPHIA_IPO_UNKNOWN);
    assert(sophia_ip_cause(p) == SOPHIA_IPX_JOURNAL_LOST);
    assert(sophia_ss_image_cancel(&t.img, offer, t.r.now, &cancel) == SOPHIA_IPR_CLOSED);
    image_close(&t);
    assert(fd_count() == base);
}

/* A refused image cancel is not a byte upload's refused End, whether its
 * resource differs in ID or only in generation. */
#include "native_image_adoption.h"

int main(void)
{
    unsigned base = fd_count();
    native_image_adoption();
    attach_arguments();
    role_grants_remain_exact();
    sealed_offer_end_to_end(SOPHIA_SF_BAR, 0);
    sealed_offer_end_to_end(SOPHIA_SF_DOCK, 1);
    sealed_offer_end_to_end(SOPHIA_SF_LAUNCHER, 1);
    refusals_without_fallback();
    absent_and_denied_discovery();
    channel_closed_and_session_end();
    unattached_session_unchanged();
    status_routing();
    queue_room_keeps_fd();
    contradictions();
    events_after_end();
    cancel_before_admission();
    cancel_races();
    cancel_lost_with_session();
    assert(fd_count() == base);
    puts("shell_image_test: ok");
    return 0;
}

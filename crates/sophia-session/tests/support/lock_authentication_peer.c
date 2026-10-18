/* Independent public afid + private relay client. The optional SDK build uses
 * the pinned C 9P client for the role conversation; the default is raw wire.
 * The protected process obtains the nonce from its own role afid, never argv. */
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <poll.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>
#ifdef SOPHIA_TEST_C_SDK
#include "sophia_9p_client.h"
#include "sophia_launch_auth.h"
#include "sophia_lock_client.h"
#include "sophia_lock_image.h"
#endif

static unsigned char buf[512];
static size_t size, reply_size;
static int channel;
static unsigned tag;
static uint32_t role_root = 1;
static void put(uint64_t n, unsigned bytes) {
    while (bytes--) { assert(size < sizeof buf); buf[size++] = n & 255; n >>= 8; }
}
static void string(const char *s) {
    put(strlen(s), 2);
    while (*s) put((unsigned char)*s++, 1);
}
static uint64_t number(size_t at, unsigned bytes) {
    uint64_t n = 0;
    assert(at + bytes <= sizeof buf);
    for (unsigned i = 0; i < bytes; ++i) n |= (uint64_t)buf[at+i] << (8*i);
    return n;
}
static void begin(unsigned kind) { size = 0; put(0, 4); put(kind, 1); put(tag, 2); }
static int exact(void *data, size_t count) {
    unsigned char *p = data;
    while (count) { ssize_t n = read(channel, p, count); if (n == 0) return 0; assert(n > 0); p += n; count -= (size_t)n; }
    return 1;
}
static void send_request(void) {
    size_t total = size;
    size = 0; put(total, 4);
    size_t sent = 0;
    while (sent < total) { ssize_t n = write(channel, buf + sent, total - sent); assert(n > 0); sent += (size_t)n; }
}
static int receive_reply(void) {
    if (!exact(buf, 4)) return 0;
    reply_size = (size_t)number(0, 4); assert(reply_size >= 7 && reply_size <= sizeof buf);
    assert(exact(buf + 4, reply_size - 4));
    assert(number(5, 2) == tag); ++tag;
    return 1;
}
static void exchange(unsigned kind) {
    send_request(); assert(receive_reply());
    if (buf[4] != kind)
        fprintf(stderr, "tag=%u expected=%u actual=%u errno=%llu\n",
                tag - 1, kind, buf[4], (unsigned long long)(buf[4] == 7 ? number(7, 4) : 0));
    assert(buf[4] == kind);
}
static void version(void) {
    tag = 65535; begin(100); put(512, 4); string("9P2000.L"); exchange(101); tag = 1;
}
static void attach(unsigned afid, unsigned expected) {
    begin(104); put(1, 4); put(afid, 4); string("lock"); string("lock-provider"); put(UINT32_MAX, 4); exchange(expected);
    if (expected == 7) assert(number(7, 4) == 13);
}
static void read_fid(unsigned fid, uint64_t offset, unsigned count, unsigned expected) {
    begin(116); put(fid, 4); put(offset, 8); put(count, 4); exchange(expected);
}
#ifndef SOPHIA_TEST_C_SDK
static void open_name(unsigned fid, const char *name, unsigned mode) {
    begin(110); put(role_root, 4); put(fid, 4); put(1, 2); string(name); exchange(111); assert(number(7, 2) == 1);
    begin(12); put(fid, 4); put(mode, 4); exchange(13);
}
static void relay(int fd, unsigned *relay_tag, const unsigned char nonce[32], int first) {
    int saved_fd = channel; unsigned saved_tag = tag; uint32_t saved_root = role_root;
    role_root = 1;
    channel = fd; tag = *relay_tag;
    if (first) {
        version();
        begin(104); put(1, 4); put(UINT32_MAX, 4); string(""); string("sophia-launch-relay-v1"); put(UINT32_MAX, 4); exchange(105);
        open_name(2, "rpc", 2);
    }
    begin(118); put(2, 4); put(0, 8); put(40, 4); put(1, 4); put(1, 4);
    for (unsigned i = 0; i < 32; ++i) put(nonce[i], 1);
    exchange(119); assert(number(7, 4) == 40);
    read_fid(2, 0, 8, 117);
    assert(number(7, 4) == 8 && number(11, 4) == 1 && number(15, 4) == 0);
    *relay_tag = tag; channel = saved_fd; tag = saved_tag; role_root = saved_root;
}
#endif
#ifdef SOPHIA_TEST_C_SDK
static struct sophia_9p_client role_wire;
static unsigned char role_storage[69632];
static uint64_t now_msec(void) {
    struct timespec now; assert(clock_gettime(CLOCK_MONOTONIC, &now) == 0);
    return (uint64_t)now.tv_sec * 1000 + (uint64_t)now.tv_nsec / 1000000;
}
static void authenticate(int relay_fd, unsigned *relay_tag, int first) {
    struct sophia_9p_client relay_wire;
    struct sophia_la_client auth;
    unsigned char relay_storage[12288];
    (void)relay_tag; (void)first;
    assert(sophia_9p_init(&role_wire, channel, 4096, 8, 32, role_storage, sizeof role_storage) == 0);
    assert(sophia_9p_init(&relay_wire, relay_fd, 4096, 1, 4, relay_storage, sizeof relay_storage) == 0);
    assert(sophia_la_init(&auth, &role_wire, &relay_wire, "lock", "lock-provider", now_msec()) == 0);
    while (!sophia_la_ready(&auth)) {
        assert(sophia_la_service(&auth, now_msec(), 4096) == 0);
        struct timespec delay = {0, 1000000}; nanosleep(&delay, NULL);
    }
    role_root = sophia_la_root(&auth);
    assert(role_root != UINT32_MAX && (fcntl(relay_fd, F_GETFD) & FD_CLOEXEC));
}
#else
static void authenticate(int relay_fd, unsigned *relay_tag, int first) {
    for (;;) {
        begin(102); put(7, 4); string("lock"); string("lock-provider"); put(UINT32_MAX, 4);
        send_request(); assert(receive_reply());
        if (buf[4] == 103) break;
        assert(buf[4] == 7 && number(7, 4) == 11); /* exact cancellation still draining */
        struct timespec delay = {0, 1000000}; nanosleep(&delay, NULL);
    }
    assert(reply_size == 20 && buf[7] == 8); /* QTAUTH */
    attach(7, 7); /* possession of an unverified afid grants no root */
    read_fid(7, 0, 40, 117);
    assert(number(7, 4) == 40 && number(11, 4) == 1 && number(15, 4) == 1);
    unsigned char nonce[32]; memcpy(nonce, buf + 19, 32);
    relay(relay_fd, relay_tag, nonce, first);
    read_fid(7, 40, 8, 117);
    assert(number(7, 4) == 8 && number(11, 4) == 1 && number(15, 4) == 0);
    attach(7, 105);
}
#endif
static void mark(const char *path, const char *value);
#ifdef SOPHIA_TEST_C_SDK
/* Sealed-image mode (argv[3]=="image"): the lock adapter runs against the
 * real runtime endpoint while the ordinary provider authority flow above
 * stays untouched. One service turn, then one adapter dispatch fed with
 * real poll(2) revents from the producer socket - never interest bits. */
static int image_pump(struct sophia_lc_client *client, struct sophia_lc_image *img) {
    int status = sophia_lc_service(client, 4096);
    const struct sophia_lf_record *event = NULL;
    while (sophia_lc_event(client, &event) == 0 && event) {
        assert(sophia_lc_event_consume(client) == 0);
        event = NULL;
    }
    short revents = 0;
    int fd = sophia_lc_image_poll_fd(img);
    if (fd >= 0) {
        struct pollfd wanted = {fd, sophia_lc_image_poll_events(img), 0};
        if (poll(&wanted, 1, 0) > 0) revents = wanted.revents;
    }
    assert(sophia_lc_image_dispatch(img, revents, now_msec()) >= 0);
    struct timespec delay = {0, 1000000};
    nanosleep(&delay, NULL);
    return status;
}

static void run_image(struct sophia_lc_client *client, int endpoint_dir,
                      const char *report) {
    static uint8_t formats[4096];
    struct sophia_lc_image img;
    struct sophia_lc_image_config config;
    memset(&config, 0, sizeof config);
    config.producer.bounds.max_resource_bytes = 1024;
    config.producer.bounds.max_width = 16;
    config.producer.bounds.max_height = 16;
    config.producer.bounds.max_outputs = 1;
    config.producer.bounds.transfer_slots = 1;
    config.producer.transfer_timeout_ms = 2000;
    config.producer.startup_deadline_ms = now_msec() + 60000;
    config.producer.fallback_bytes = 0;
    config.endpoint_dir = endpoint_dir;
    config.formats = formats;
    config.formats_capacity = sizeof formats;
    assert(sophia_lc_image_init(&img, &config) == SOPHIA_IPR_OK);
    /* Attached before any service turn: Negotiated has not arrived. */
    assert(sophia_lc_image_attach(client, &img) == SOPHIA_IPR_OK);
    unsigned guard;
    uint64_t generation;
    for (guard = 0;; ++guard) {
        const struct sophia_lf_record *event = NULL;
        assert(guard < 20000 && image_pump(client, &img) == 0);
        if (sophia_lc_ready(client) && sophia_lc_lock(client, &generation)) break;
        (void)event;
    }
    struct sophia_ip *producer = sophia_lc_image_producer(&img);
    assert(producer);
    for (guard = 0; sophia_ip_channel(producer) != SOPHIA_IPC_READY; ++guard) {
        assert(guard < 20000 && image_pump(client, &img) == 0);
    }
    assert(sophia_ip_path(producer) == SOPHIA_IPP_DESCRIPTOR);
    mark(report, "image-ready\n");

    /* The source names the session's actual lock allocation, derived
     * from the presented lock object - not the transport maxima. */
    const struct sophia_lf_lock *lock = sophia_lc_lock(client, &generation);
    assert(lock && lock->allocation_count >= 1);
    const struct sophia_lf_allocation *alloc = &lock->allocations[0];
    int source = -1;
    void *pixels = NULL;
    size_t bytes = 0;
    assert(sophia_ip_isb_create(alloc->pixel_width, alloc->pixel_height,
                                   &source, &pixels, &bytes) == SOPHIA_IPR_OK);
    assert(bytes == (size_t)alloc->pixel_width * alloc->pixel_height * 4);
    /* Patterned RGB with a full alpha byte: a valid premultiplied
     * BGRA source, so validation can accept every pixel. */
    for (size_t i = 0; i < bytes; ++i)
        ((unsigned char *)pixels)[i] =
            (i % 4 == 3) ? 255 : (unsigned char)(i * 31 + 7);
    assert(sophia_ip_isb_finish(source, pixels, bytes) == SOPHIA_IPR_OK);
    struct sophia_ic_metadata meta;
    memset(&meta, 0, sizeof meta);
    meta.resource_id = 7;
    meta.resource_generation = 1;
    meta.admission_epoch = lock->lock_epoch;
    meta.output = alloc->output;
    meta.allocation = alloc->allocation;
    meta.topology_generation = lock->topology_generation;
    meta.source_kind = SOPHIA_IC_SOURCE_SEALED;
    meta.source_format = SOPHIA_IC_SEALED_BGRA8_PREMULTIPLIED;
    meta.width = alloc->pixel_width;
    meta.height = alloc->pixel_height;
    meta.stride = alloc->pixel_width * 4;
    meta.plane_count = meta.descriptor_count = 1;
    meta.logical_bytes = bytes;
    uint64_t offer = 0;
    int begun;
    for (guard = 0;; ++guard) {
        assert(guard < 20000);
        begun = sophia_lc_image_begin(&img, &meta, source, now_msec(), &offer);
        if (begun != SOPHIA_IPR_BUSY) break;
        assert(image_pump(client, &img) == 0);
    }
    assert(begun == SOPHIA_IPR_OK && offer != 0);
    mark(report, "image-begun\n");
    /* Accepted alone does not prove the proof point: the source custody
     * must also come back as SourceReleased (SETTLED, disposition 1). */
    struct sophia_ip_offer_info info;
    for (guard = 0;; ++guard) {
        assert(guard < 20000 && image_pump(client, &img) == 0);
        assert(sophia_ip_offer(producer, offer, &info) == SOPHIA_IPR_OK);
        if (info.status == SOPHIA_IPS_ACCEPTED && info.state == SOPHIA_IPO_SETTLED)
            break;
    }
    assert(info.disposition == 1);
    mark(report, "image-accepted\n");
    /* The connection now waits as any locked provider does; revocation
     * ends the journal and no release is owed on a dead journal. */
    for (;;) {
        int status = image_pump(client, &img);
        if (status) {
            assert(status == SOPHIA_9P_CLOSED ||
                   sophia_lc_state(client) == SOPHIA_LC_STALE);
            break;
        }
    }
    mark(report, "revoked\n");
}
#endif
static void mark(const char *path, const char *value) {
    FILE *f = fopen(path, "w"); assert(f); assert(fputs(value, f) >= 0); assert(fclose(f) == 0);
}
int main(int argc, char **argv) {
    alarm(20);
    assert(argc == 3 || (argc == 4 && (strcmp(argv[3], "upload") == 0 || strcmp(argv[3], "image") == 0)));
    const char *fd = getenv("SOPHIA_LAUNCH_AUTH_FD"); assert(fd);
    int relay_fd = atoi(fd); assert(relay_fd > 2);
    assert(fcntl(relay_fd, F_SETFD, FD_CLOEXEC) == 0);
    struct sockaddr_un address; memset(&address, 0, sizeof address);
    address.sun_family = AF_UNIX; assert(strlen(argv[1]) < sizeof address.sun_path);
    memcpy(address.sun_path, argv[1], strlen(argv[1]) + 1);
    channel = socket(AF_UNIX, SOCK_STREAM, 0); assert(channel >= 0);
    assert(connect(channel, (struct sockaddr *)&address, sizeof address) == 0);
    version();
    attach(UINT32_MAX, 7);
    begin(102); put(7, 4); string("forged"); string("lock-provider"); put(UINT32_MAX, 4); exchange(7); assert(number(7, 4) == 13);
    unsigned relay_tag = 1;
    authenticate(relay_fd, &relay_tag, 1);
    /* A reset destroys fids and verdicts, but the same launch capability must
       authenticate a fresh attempt on this immutable transport identity. */
    version();
#ifdef SOPHIA_TEST_C_SDK
    read_fid(role_root, 0, 40, 7); assert(number(7, 4) == 9);
#else
    read_fid(7, 0, 40, 7); assert(number(7, 4) == 9);
#endif
    attach(UINT32_MAX, 7);
    authenticate(relay_fd, &relay_tag, 0);
#ifdef SOPHIA_TEST_C_SDK
    struct sophia_lc_client client;
    int image_mode = argc == 4 && strcmp(argv[3], "image") == 0;
    struct sophia_lf_negotiate offer; memset(&offer, 0, sizeof offer);
    offer.minimum_revision = 1;
    offer.maximum_revision = image_mode ? 2 : 1;
    offer.capabilities = image_mode
        ? (SOPHIA_LF_CAP_PRESENT | SOPHIA_LF_CAP_IMAGE_SEALED)
        : SOPHIA_LF_CAP_PRESENT;
    assert(sophia_lc_init_attached(&client, &role_wire, role_root, &offer) == 0);
    if (image_mode) {
        /* The endpoint directory is the lock socket's parent: the
         * runtime binds its image endpoint beside it. */
        char dirbuf[108];
        const char *slash = strrchr(argv[1], '/');
        assert(slash && (size_t)(slash - argv[1]) < sizeof dirbuf);
        memcpy(dirbuf, argv[1], (size_t)(slash - argv[1]));
        dirbuf[slash - argv[1]] = 0;
        int endpoint_dir = open(dirbuf, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
        assert(endpoint_dir >= 0);
        run_image(&client, endpoint_dir, argv[2]);
        return 0;
    }
    for (;;) {
        const struct sophia_lf_record *event = NULL;
        assert(sophia_lc_service(&client, 4096) == 0);
        if (sophia_lc_event(&client, &event) == 0) {
            assert(event); assert(sophia_lc_event_consume(&client) == 0);
        }
        uint64_t generation;
        if (sophia_lc_ready(&client) && sophia_lc_lock(&client, &generation)) break;
        struct timespec delay = {0, 1000000}; nanosleep(&delay, NULL);
    }
    assert(sophia_lc_service(&client, 4096) == 0);
    mark(argv[2], "waiting\n");
    for (;;) {
        int status = sophia_lc_service(&client, 4096);
        if (status) {
            assert(status == SOPHIA_9P_CLOSED || sophia_lc_state(&client) == SOPHIA_LC_STALE);
            break;
        }
        struct timespec delay = {0, 1000000}; nanosleep(&delay, NULL);
    }
    mark(argv[2], "revoked\n");
#else
    /* SDK retries may spend numeric fids. Allocate role-file fids after the
       actual admitted root, not at a guessed offset from the first attempt. */
    assert(role_root < UINT32_MAX - 6);
    uint32_t api_fid = role_root + 1, submit_fid = role_root + 2;
    uint32_t transaction_fid = role_root + 3, events_fid = role_root + 4;
    uint32_t ack_fid = role_root + 5;
    open_name(api_fid, "api", 0); read_fid(api_fid, 0, 128, 117);
    unsigned n = (unsigned)number(7, 4); assert(n < 128 && 11 + n <= reply_size);
    char api[129]; memcpy(api, buf + 11, n); api[n] = 0;
    unsigned long long epoch;
    assert(sscanf(api, "sophia-lock-files version=1 epoch=%llu", &epoch) == 1);
    open_name(submit_fid, "submit", 1); open_name(transaction_fid, "transaction", 2);
    begin(118); put(transaction_fid, 4); put(0, 8); put(48, 4);
    put(48, 4); put(1, 2); put(256, 2); put(epoch, 8); put(1, 8); put(0, 8);
    put(1, 2); put(1, 2); put(0, 2); put(0, 2); put(1, 8);
    exchange(119); assert(number(7, 4) == 48);
    begin(118); put(submit_fid, 4); put(0, 8); put(24, 4); put(epoch, 8); put(1, 8); put(48, 4); put(0, 4);
    exchange(119); assert(number(7, 4) == 24);
    open_name(events_fid, "events", 0); read_fid(events_fid, 0, 480, 117);
    n = (unsigned)number(7, 4); assert(n > 0 && 11 + n <= reply_size);
    size_t at = 11; uint64_t sequence = 0;
    while (at < 11 + n) {
        unsigned length = (unsigned)number(at, 4); assert(length >= 32 && at + length <= 11 + n);
        assert(number(at + 8, 8) == epoch); sequence = number(at + 24, 8); at += length;
    }
    open_name(ack_fid, "ack", 1);
    begin(118); put(ack_fid, 4); put(0, 8); put(16, 4); put(epoch, 8); put(sequence, 8); exchange(119);
    if (argc == 4) {
        /* A real resource upload. The receiver chooses the fragment boundary;
           the child sends one write and requires a correlated ESTALE reply. */
        begin(120); put(transaction_fid, 4); exchange(121);
        open_name(transaction_fid, "transaction", 2);
        begin(118); put(transaction_fid, 4); put(0, 8); put(72, 4);
        put(72, 4); put(1, 2); put(258, 2); put(epoch, 8); put(2, 8); put(0, 8);
        put(2, 8); put(1, 8); put(1, 8); put(2, 4); put(1, 4);
        put(0, 2); put(1, 2); put(0, 4);
        exchange(119); assert(number(7, 4) == 72);
        begin(118); put(submit_fid, 4); put(0, 8); put(24, 4);
        put(epoch, 8); put(2, 8); put(72, 4); put(0, 4); exchange(119);
        /* Raw authentication retains afid 7 until clunk/transport close. */
        unsigned upload_fid = 8;
        begin(110); put(role_root, 4); put(upload_fid, 4); put(2, 2);
        string("upload"); string("0"); exchange(111); assert(number(7, 2) == 2);
        begin(12); put(upload_fid, 4); put(1, 4); exchange(13);
        begin(118); put(upload_fid, 4); put(0, 8); put(8, 4);
        for (unsigned i = 0; i < 8; ++i) put(0x5a, 1);
        exchange(7); assert(number(7, 4) == 116);
        mark(argv[2], "upload-revoked\n");
        return 0;
    }
    begin(116); put(events_fid, 4); put(n, 8); put(480, 4); send_request();
    mark(argv[2], "waiting\n");
    /* Revocation must wake/refuse the retained read or close the connection.
       It cannot produce another event under the retired launch. */
    if (receive_reply()) assert(buf[4] == 7 && number(7, 4) == 116);
    mark(argv[2], "revoked\n");
#endif
    return 0;
}

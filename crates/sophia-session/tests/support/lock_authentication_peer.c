/* Independent public afid + private relay client. The optional SDK build uses
 * the pinned C 9P client for the role conversation; the default is raw wire.
 * The protected process obtains the nonce from its own role afid, never argv. */
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>
#ifdef SOPHIA_TEST_C_SDK
#include "sophia_9p_client.h"
#include "sophia_launch_auth.h"
#include "sophia_lock_client.h"
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
static void mark(const char *path, const char *value) {
    FILE *f = fopen(path, "w"); assert(f); assert(fputs(value, f) >= 0); assert(fclose(f) == 0);
}
int main(int argc, char **argv) {
    alarm(20);
    assert(argc == 3 || (argc == 4 && strcmp(argv[3], "upload") == 0));
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
    struct sophia_lf_negotiate offer; memset(&offer, 0, sizeof offer);
    offer.minimum_revision = offer.maximum_revision = 1;
    offer.capabilities = SOPHIA_LF_CAP_PRESENT;
    assert(sophia_lc_init_attached(&client, &role_wire, role_root, &offer) == 0);
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

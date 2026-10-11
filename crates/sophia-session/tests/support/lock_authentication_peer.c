/* Independent public afid + private relay client. No Sophia codec or SDK.
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

static unsigned char buf[512];
static size_t size, reply_size;
static int channel;
static unsigned tag;
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
static void exchange(unsigned kind) { send_request(); assert(receive_reply()); assert(buf[4] == kind); }
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
static void open_name(unsigned fid, const char *name, unsigned mode) {
    begin(110); put(1, 4); put(fid, 4); put(1, 2); string(name); exchange(111); assert(number(7, 2) == 1);
    begin(12); put(fid, 4); put(mode, 4); exchange(13);
}
static void relay(int fd, unsigned *relay_tag, const unsigned char nonce[32], int first) {
    int saved_fd = channel; unsigned saved_tag = tag;
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
    *relay_tag = tag; channel = saved_fd; tag = saved_tag;
}
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
static void mark(const char *path, const char *value) {
    FILE *f = fopen(path, "w"); assert(f); assert(fputs(value, f) >= 0); assert(fclose(f) == 0);
}
int main(int argc, char **argv) {
    alarm(20);
    assert(argc == 3);
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
    read_fid(7, 0, 40, 7); assert(number(7, 4) == 9);
    attach(UINT32_MAX, 7);
    authenticate(relay_fd, &relay_tag, 0);
    open_name(2, "api", 0); read_fid(2, 0, 128, 117);
    unsigned n = (unsigned)number(7, 4); assert(n < 128 && 11 + n <= reply_size);
    char api[129]; memcpy(api, buf + 11, n); api[n] = 0;
    unsigned long long epoch;
    assert(sscanf(api, "sophia-lock-files version=1 epoch=%llu", &epoch) == 1);
    open_name(3, "submit", 1); open_name(4, "transaction", 2);
    begin(118); put(4, 4); put(0, 8); put(48, 4);
    put(48, 4); put(1, 2); put(256, 2); put(epoch, 8); put(1, 8); put(0, 8);
    put(1, 2); put(1, 2); put(0, 2); put(0, 2); put(1, 8);
    exchange(119); assert(number(7, 4) == 48);
    begin(118); put(3, 4); put(0, 8); put(24, 4); put(epoch, 8); put(1, 8); put(48, 4); put(0, 4);
    exchange(119); assert(number(7, 4) == 24);
    open_name(5, "events", 0); read_fid(5, 0, 480, 117);
    n = (unsigned)number(7, 4); assert(n > 0 && 11 + n <= reply_size);
    size_t at = 11; uint64_t sequence = 0;
    while (at < 11 + n) {
        unsigned length = (unsigned)number(at, 4); assert(length >= 32 && at + length <= 11 + n);
        assert(number(at + 8, 8) == epoch); sequence = number(at + 24, 8); at += length;
    }
    open_name(6, "ack", 1);
    begin(118); put(6, 4); put(0, 8); put(16, 4); put(epoch, 8); put(sequence, 8); exchange(119);
    begin(116); put(5, 4); put(n, 8); put(480, 4); send_request();
    mark(argv[2], "waiting\n");
    /* Revocation must wake/refuse the retained read or close the connection.
       It cannot produce another event under the retired launch. */
    if (receive_reply()) assert(buf[4] == 7 && number(7, 4) == 116);
    mark(argv[2], "revoked\n");
    return 0;
}

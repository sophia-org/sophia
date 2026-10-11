/* Independent 9P2000.L client for the provisioned, conversation-only relay.
 * The public nonce is supplied by the fixture; launch identity never is. */
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static unsigned char buf[512];
static size_t size;
static int channel;
static unsigned tag;
static void put(unsigned long long n, unsigned bytes) {
    while (bytes--) { assert(size < sizeof buf); buf[size++] = n & 255; n >>= 8; }
}
static void string(const char *s) {
    put(strlen(s), 2);
    while (*s) put((unsigned char)*s++, 1);
}
static uint32_t number(size_t at, unsigned bytes) {
    uint32_t n = 0;
    for (unsigned i = 0; i < bytes; ++i) n |= (uint32_t)buf[at+i] << (8*i);
    return n;
}
static void begin(unsigned kind) { size = 0; put(0, 4); put(kind, 1); put(tag, 2); }
static void exact(void *data, size_t count) {
    unsigned char *p = data;
    while (count) { ssize_t n = read(channel, p, count); assert(n > 0); p += n; count -= n; }
}
static void exchange(unsigned kind) {
    size_t total = size;
    size = 0; put(total, 4);
    size_t sent = 0;
    while (sent < total) {
        ssize_t n = write(channel, buf + sent, total - sent); assert(n > 0); sent += n;
    }
    exact(buf, 4);
    total = number(0, 4); assert(total >= 7 && total <= sizeof buf);
    exact(buf + 4, total - 4);
    assert(buf[4] == kind && number(5, 2) == tag);
    ++tag;
}
int main(int argc, char **argv) {
    alarm(15);
    assert(argc == 2 && strlen(argv[1]) == 64);
    const char *fd = getenv("SOPHIA_LAUNCH_AUTH_FD"); assert(fd);
    channel = atoi(fd); assert(channel > 2);
    assert(fcntl(channel, F_SETFD, FD_CLOEXEC) == 0);
    tag = 65535; begin(100); put(512, 4); string("9P2000.L"); exchange(101);
    tag = 1; begin(104); put(1, 4); put(UINT32_MAX, 4);
    string(""); string("sophia-launch-relay-v1"); put(UINT32_MAX, 4); exchange(105);
    begin(110); put(1, 4); put(2, 4); put(1, 2); string("rpc"); exchange(111);
    begin(12); put(2, 4); put(2, 4); exchange(13);
    begin(118); put(2, 4); put(0, 8); put(40, 4); put(1, 4); put(1, 4);
    for (unsigned i = 0; i < 32; ++i) {
        char hex[3] = {argv[1][2*i], argv[1][2*i+1], 0};
        put(strtoul(hex, NULL, 16), 1);
    }
    exchange(119); assert(number(7, 4) == 40);
    begin(116); put(2, 4); put(0, 8); put(8, 4); exchange(117);
    assert(number(7, 4) == 8 && number(11, 4) == 1 && number(15, 4) == 0);
    /* Keep the actual protected child alive for the parent's final custody check. */
    for (;;) pause();
}

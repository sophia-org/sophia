#include "../sophia_launch_auth.h"
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

struct fixture {
    int role[2], relay[2], mode;
    unsigned auths, root_attaches, writes, clunks;
    struct sophia_9p_client r, p;
    struct sophia_la_client auth;
    uint8_t rb[12288], pb[12288], nonce[40];
    uint8_t incoming[2][4096];
    size_t received[2];
};
static uint32_t get(const uint8_t *p) {
    return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
}
static void put(uint8_t *p, uint32_t n) {
    p[0] = n;
    p[1] = n >> 8;
    p[2] = n >> 16;
    p[3] = n >> 24;
}
static void init(struct fixture *f, int mode, uint64_t now) {
    unsigned i;
    memset(f, 0, sizeof *f);
    f->mode = mode;
    assert(!socketpair(AF_UNIX, SOCK_STREAM, 0, f->role));
    assert(!socketpair(AF_UNIX, SOCK_STREAM, 0, f->relay));
    assert(!sophia_9p_init(&f->r, f->role[0], 4096, 1, 2, f->rb, sizeof f->rb));
    assert(!sophia_9p_init(&f->p, f->relay[0], 4096, 1, 2, f->pb, sizeof f->pb));
    put(f->nonce, 1);
    put(f->nonce + 4, 1);
    for (i = 8; i < 40; i++)
        f->nonce[i] = (uint8_t)i;
    assert(!sophia_la_init(&f->auth, &f->r, &f->p, "role", "view", now));
    assert(fcntl(f->relay[0], F_GETFD) & FD_CLOEXEC);
}
static void finish(struct fixture *f) {
    close(f->role[0]);
    close(f->role[1]);
    close(f->relay[0]);
    close(f->relay[1]);
}
static void peer(struct fixture *f, int relay) {
    uint8_t *b = f->incoming[relay], out[512] = {0};
    int fd = relay ? f->relay[1] : f->role[1];
    ssize_t n = recv(fd, b + f->received[relay], 4096 - f->received[relay], MSG_DONTWAIT);
    unsigned type, body = 0;
    uint32_t count;
    if (n < 0) {
        assert(errno == EAGAIN || errno == EWOULDBLOCK);
        return;
    }
    assert(n > 0);
    f->received[relay] += (size_t)n;
    if (f->received[relay] < 4 || f->received[relay] < get(b))
        return;
    n = (ssize_t)f->received[relay];
    f->received[relay] = 0;
    assert(n >= 7 && get(b) == (uint32_t)n);
    if (f->mode == 5)
        return; /* A silent peer cannot renew the deadline. */
    type = b[4] + 1;
    switch (b[4]) {
    case 100:
        body = 14;
        put(out + 7, 4096);
        out[11] = 8;
        memcpy(out + 13, "9P2000.L", 8);
        break;
    case 102:
        assert(!relay);
        f->auths++;
        if (f->mode == 6 && f->auths == 1) {
            type = 7;
            body = 4;
            put(out + 7, 11);
        } else {
            body = 13;
            out[7] = 8;
        }
        break;
    case 104:
        body = 13;
        out[7] = 0x80;
        if (relay)
            assert(get(b + 11) == UINT32_MAX);
        else {
            assert(get(b + 11) != UINT32_MAX);
            f->root_attaches++;
        }
        break;
    case 110:
        assert(relay);
        body = 15;
        out[7] = 1;
        break;
    case 12:
        assert(relay);
        body = 17;
        break;
    case 118:
        assert(relay && get(b + 19) == 40 && n == 63);
        assert(!memcmp(b + 23, f->nonce, 40));
        f->writes++;
        body = 4;
        put(out + 7, f->mode == 4 ? 39 : 40);
        break;
    case 116:
        count = get(b + 19);
        body = 4 + count;
        put(out + 7, count);
        if (count == 40) {
            assert(!relay && get(b + 11) == 0);
            memcpy(out + 11, f->nonce, 40);
            if (f->mode == 1)
                out[15] = 2;
        } else {
            assert(count == 8);
            put(out + 11, 1);
            if (f->mode == 7)
                put(out + 11, 2);
            if ((relay && f->mode == 2) || (!relay && f->mode == 3))
                put(out + 15, 13);
            if (!relay)
                assert(get(b + 11) == 40);
        }
        break;
    case 120:
        f->clunks++;
        break;
    default:
        assert(0);
    }
    put(out, 7 + body);
    out[4] = (uint8_t)type;
    out[5] = b[5];
    out[6] = b[6];
    assert(send(fd, out, 7 + body, MSG_DONTWAIT) == (ssize_t)(7 + body));
}
static int drive(struct fixture *f, uint64_t origin) {
    unsigned i;
    for (i = 0; i < 200; i++) {
        int r = sophia_la_service(&f->auth, origin + i, 4096);
        if (r)
            return r;
        if (sophia_la_ready(&f->auth))
            return 0;
        peer(f, 0);
        peer(f, 1);
    }
    assert(0);
    return 0;
}
static void reuse_launch_relay(void) {
    struct fixture f;
    struct sophia_9p_client next;
    struct sophia_la_client completed;
    uint8_t storage[12288], byte;
    int previous[2];
    uint32_t old_root;
    init(&f, 0, 100);
    assert(!drive(&f, 100));
    completed = f.auth;
    old_root = sophia_la_root(&completed);
    previous[0] = f.role[0]; previous[1] = f.role[1];
    assert(!socketpair(AF_UNIX, SOCK_STREAM, 0, f.role));
    assert(!sophia_9p_init(&next, f.role[0], 4096, 1, 2, storage, sizeof storage));
    assert(!sophia_la_next_endpoint(&f.auth, &completed, &next, "role", "view", 1000));
    assert(!drive(&f, 1000));
    assert(f.root_attaches == 2 && f.writes == 2 && f.clunks == 6);
    assert(f.r.fid_count == 1 && f.r.fids[0] == old_root);
    assert(next.fid_count == 1 && !f.p.fid_count);
    assert(recv(previous[1], &byte, 1, MSG_DONTWAIT) == -1 &&
           (errno == EAGAIN || errno == EWOULDBLOCK));
    {
        struct sophia_9p_handle pending;
        struct sophia_9p_client fresh;
        struct sophia_la_client refused;
        struct sophia_9p_reply reply;
        uint8_t fresh_storage[12288];
        uint32_t fid;
        int pair[2];
        assert(!socketpair(AF_UNIX, SOCK_STREAM, 0, pair));
        assert(!sophia_9p_init(&fresh, pair[0], 4096, 1, 2, fresh_storage, sizeof fresh_storage));
        assert(!sophia_9p_attach(&f.p, "", "sophia-launch-relay-v1", &pending, &fid));
        assert(sophia_la_next_endpoint(&refused, &f.auth, &fresh, "role", "view", 2000) == SOPHIA_9P_BUSY);
        assert(f.p.phase == 2 && f.p.fid_count == 1 && sophia_9p_wants_write(&f.p));
        assert(!sophia_9p_service(&f.p, 4096));
        peer(&f, 1);
        assert(!sophia_9p_service(&f.p, 4096));
        assert(!sophia_9p_peek(&f.p, &reply));
        assert(!sophia_9p_consume(&f.p, reply.handle));
        /* Settling the request does not release its live relay fid. */
        assert(f.p.fid_count == 1 && !sophia_9p_wants_write(&f.p));
        assert(sophia_la_next_endpoint(&refused, &f.auth, &fresh, "role", "view", 2000) == SOPHIA_9P_BUSY);
        assert(!sophia_9p_clunk(&f.p, fid, &pending));
        assert(!sophia_9p_service(&f.p, 4096));
        peer(&f, 1);
        assert(!sophia_9p_service(&f.p, 4096));
        assert(!sophia_9p_peek(&f.p, &reply));
        assert(!sophia_9p_consume(&f.p, reply.handle));
        assert(!f.p.fid_count);
        /* Reset must not discard even an incomplete incoming header. */
        assert(send(f.relay[1], "\007\000\000", 3, MSG_DONTWAIT) == 3);
        assert(!sophia_9p_service(&f.p, 4096));
        assert(f.p.rx_used == 3);
        assert(sophia_la_next_endpoint(&refused, &f.auth, &fresh, "role", "view", 2000) == SOPHIA_9P_BUSY);
        assert(f.p.phase == 2 && f.p.rx_used == 3 && !sophia_9p_wants_write(&fresh));
        close(pair[0]); close(pair[1]);
    }
    close(previous[0]); close(previous[1]);
    finish(&f);
}
static void caller_driven_partial_io(void) {
    struct fixture f;
    uint64_t now = 100, wake;
    uint8_t byte;
    unsigned i;
    init(&f, 6, now);
    assert(recv(f.role[1], &byte, 1, MSG_DONTWAIT) == -1 && errno == EAGAIN);
    assert(recv(f.relay[1], &byte, 1, MSG_DONTWAIT) == -1 && errno == EAGAIN);
    assert(!sophia_la_service(&f.auth, now, 0));
    assert(recv(f.role[1], &byte, 1, MSG_DONTWAIT) == -1 && errno == EAGAIN);
    assert(recv(f.relay[1], &byte, 1, MSG_DONTWAIT) == -1 && errno == EAGAIN);
    for (i = 0; i < 5000 && !sophia_la_ready(&f.auth); i++) {
        assert(!sophia_la_service(&f.auth, now, 1));
        peer(&f, 0);
        peer(&f, 1);
        wake = sophia_la_next_wake(&f.auth);
        if (wake > now && wake < 5100) {
            assert(wake == now + 10);
            assert(!sophia_la_service(&f.auth, wake - 1, 0));
            assert(sophia_la_next_wake(&f.auth) == wake && f.auths == 1);
            now = wake;
        }
    }
    assert(sophia_la_ready(&f.auth) && now == 110);
    assert(f.auths == 2 && f.root_attaches == 1 && f.clunks == 3);
    assert(f.r.fid_count == 1 && !f.p.fid_count);
    finish(&f);
}
int main(void) {
    struct fixture f;
    unsigned mode;
    for (mode = 0; mode <= 7; mode++) {
        int r;
        if (mode == 5)
            continue;
        init(&f, (int)mode, 100);
        r = drive(&f, 100);
        if (mode == 0 || mode == 6) {
            assert(!r && sophia_la_ready(&f.auth));
            assert(sophia_la_root(&f.auth) != UINT32_MAX);
            assert(f.root_attaches == 1 && f.writes == 1 && f.clunks == 3);
            assert(f.r.fid_count == 1 && !f.p.fid_count);
            assert(f.auths == (mode == 6 ? 2u : 1u));
            assert(sophia_la_next_wake(&f.auth) == UINT64_MAX);
        } else {
            assert(r == SOPHIA_9P_INVALID && !f.root_attaches);
            assert(sophia_la_root(&f.auth) == UINT32_MAX);
            assert(sophia_la_service(&f.auth, 999, 4096) == r);
            if (mode == 2 || mode == 3)
                assert(sophia_la_remote_error(&f.auth) == 13);
        }
        finish(&f);
    }
    init(&f, 5, 100);
    assert(!sophia_la_service(&f.auth, 100, 4096));
    peer(&f, 0);
    assert(sophia_la_next_wake(&f.auth) == 5100);
    assert(sophia_la_service(&f.auth, 5100, 4096) == SOPHIA_9P_CLOSED);
    finish(&f);
    init(&f, 0, 100);
    assert(sophia_la_service(&f.auth, 99, 4096) == SOPHIA_9P_CLOSED);
    finish(&f);
    init(&f, 0, 100);
    sophia_la_cancel(&f.auth);
    assert(sophia_la_service(&f.auth, 100, 4096) == SOPHIA_9P_CLOSED);
    finish(&f);
    init(&f, 0, UINT64_MAX - 5000);
    assert(!sophia_la_service(&f.auth, UINT64_MAX - 1, 4096));
    assert(sophia_la_service(&f.auth, UINT64_MAX, 4096) == SOPHIA_9P_CLOSED);
    finish(&f);
    init(&f, 6, 0);
    for (mode = 0; mode < 100 && !f.auths; mode++) {
        assert(!sophia_la_service(&f.auth, mode, 4096));
        peer(&f, 0);
        peer(&f, 1);
    }
    assert(f.auths == 1);
    assert(!sophia_la_service(&f.auth, 4999, 4096));
    assert(sophia_la_next_wake(&f.auth) == 5000);
    assert(sophia_la_service(&f.auth, 5000, 4096) == SOPHIA_9P_CLOSED);
    assert(f.auths == 1 && !f.root_attaches);
    finish(&f);
    reuse_launch_relay();
    caller_driven_partial_io();
    puts("launch authentication: own challenge, exact status, no fallback, cleanup, retry and "
         "deadline passed");
    return 0;
}

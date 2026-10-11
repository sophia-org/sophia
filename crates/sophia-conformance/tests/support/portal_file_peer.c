/* Independent portal record encoder/reader, from sophia-portal-files-v1.kdl.
 * Only the generic C SDK owns the 9P wire. No Rust or portal encoder is linked.
 * Trusted admission and recipient effects belong to the Rust test owner.
 * PEER_MUTATION=1 sends a stale negotiation epoch; =2 changes uploaded bytes.
 */
#define _POSIX_C_SOURCE 200809L
#include "sophia_9p_client.h"
#include "sophia_desktop_connection.h"
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#ifndef PEER_MUTATION
#define PEER_MUTATION 0
#endif

static void require(int yes, const char *why)
{
    if (!yes) { fprintf(stderr, "portal C peer: %s\n", why); exit(1); }
}
static uint64_t now_ms(void)
{
    struct timespec t;
    require(!clock_gettime(CLOCK_MONOTONIC, &t), "clock");
    return (uint64_t)t.tv_sec * 1000u + (uint64_t)t.tv_nsec / 1000000u;
}
static void put(uint8_t *p, uint64_t v, unsigned n)
{
    unsigned i;
    for (i = 0; i < n; ++i) p[i] = (uint8_t)(v >> (i * 8));
}
static uint64_t get(const uint8_t *p, unsigned n)
{
    uint64_t v = 0;
    unsigned i;
    for (i = 0; i < n; ++i) v |= (uint64_t)p[i] << (i * 8);
    return v;
}
struct peer {
    struct sophia_9p_client c;
    uint64_t deadline, offset, sequence;
    uint32_t root, events, submit, ack, api, count, error;
    uint8_t type, data[1024], pending[128];
    size_t used;
};
static void wait_reply(struct peer *p, struct sophia_9p_handle h)
{
    for (;;) {
        struct sophia_9p_reply r;
        struct pollfd fd;
        require(now_ms() < p->deadline, "wire deadline");
        require(sophia_9p_service(&p->c, 65536) >= 0, "wire service");
        if (sophia_9p_peek(&p->c, &r) == SOPHIA_9P_OK) {
            require(r.handle.serial == h.serial && r.handle.slot == h.slot, "reply correlation");
            p->type = r.type; p->error = r.type == 7 ? r.error : 0; p->count = r.count;
            if (r.type == 117) {
                require(r.count <= sizeof(p->data), "read bound");
                memcpy(p->data, r.data, r.count);
            }
            require(!sophia_9p_consume(&p->c, r.handle), "consume");
            return;
        }
        fd.fd = p->c.fd; fd.events = POLLIN | (sophia_9p_wants_write(&p->c) ? POLLOUT : 0);
        fd.revents = 0;
        require(poll(&fd, 1, 2) >= 0, "poll");
    }
}
static uint32_t open_file(struct peer *p, const char *name, int upload, uint32_t flags)
{
    const char *names[2] = {name, "0"};
    struct sophia_9p_handle h;
    uint32_t fid;
    require(!sophia_9p_walk(&p->c, p->root, names, upload ? 2 : 1, &h, &fid), "walk");
    wait_reply(p, h); require(p->type == 111 && p->count == (upload ? 2u : 1u), "walk reply");
    require(!sophia_9p_lopen(&p->c, fid, flags, &h), "open");
    wait_reply(p, h); require(p->type == 13, "open reply");
    return fid;
}
static void clunk(struct peer *p, uint32_t fid)
{
    struct sophia_9p_handle h;
    require(!sophia_9p_clunk(&p->c, fid, &h), "clunk");
    wait_reply(p, h); require(p->type == 121, "clunk reply");
}
static uint32_t write_file(struct peer *p, uint32_t fid, uint64_t offset, const void *b, size_t n)
{
    struct sophia_9p_handle h;
    require(!sophia_9p_write(&p->c, fid, offset, b, n, &h), "write");
    wait_reply(p, h);
    if (p->type == 7) return p->error;
    require(p->type == 119 && p->count == n, "write reply");
    return 0;
}
static void read_file(struct peer *p, uint32_t fid, uint64_t offset, uint32_t n)
{
    struct sophia_9p_handle h;
    require(!sophia_9p_read(&p->c, fid, offset, n, &h), "read");
    wait_reply(p, h);
}
static void record(uint8_t *b, size_t n, unsigned kind, uint64_t id)
{
    memset(b, 0, n); put(b, n, 4); put(b + 4, 1, 2); put(b + 6, kind, 2);
    put(b + 8, 7, 8); put(b + 16, id, 8);
}
static uint32_t submit(struct peer *p, const uint8_t *b, size_t n)
{
    uint8_t control[24] = {0};
    uint32_t tx = open_file(p, "transaction", 0, 2), error;
    put(control, 7, 8); put(control + 8, get(b + 16, 8), 8); put(control + 16, n, 4);
    error = write_file(p, tx, 0, b, n);
    if (!error) error = write_file(p, p->submit, 0, control, sizeof(control));
    clunk(p, tx);
    return error;
}
/* Deliberately split headers and bodies across journal reads. Acknowledgement
 * advances only after a complete, independently validated record. */
static void event(struct peer *p, unsigned kind, size_t size, uint8_t out[88])
{
    size_t n;
    while (p->used < 4 || p->used < get(p->pending, 4)) {
        read_file(p, p->events, p->offset, 13);
        require(p->type == 117 && p->count && p->used + p->count <= sizeof(p->pending), "journal read");
        memcpy(p->pending + p->used, p->data, p->count);
        p->used += p->count; p->offset += p->count;
    }
    n = (size_t)get(p->pending, 4);
    require(n == size && n <= 88, "event size");
    memcpy(out, p->pending, n);
    memmove(p->pending, p->pending + n, p->used - n); p->used -= n;
    require(get(out + 4, 2) == 1 && get(out + 6, 2) == kind && get(out + 8, 8) == 7,
            "event header");
    require(!get(out + 16, 8) && get(out + 24, 8) == ++p->sequence, "event identity");
}
static void ack(struct peer *p)
{
    uint8_t b[16]; put(b, 7, 8); put(b + 8, p->sequence, 8);
    require(!write_file(p, p->ack, 0, b, sizeof(b)), "acknowledgement");
}
static void accepted(struct peer *p, const uint8_t *b, size_t n)
{
    uint8_t e[88];
    require(!submit(p, b, n), "valid candidate refused");
    event(p, 18, 48, e);
    require(get(e + 32, 8) == get(b + 16, 8) && get(e + 40, 2) == get(b + 6, 2), "Submitted correlation");
    require(!get(e + 42, 6), "Submitted padding"); ack(p);
}
static void request(uint8_t b[344], uint64_t id, uint64_t generation)
{
    record(b, 344, 257, id); put(b + 32, 41, 8); put(b + 40, 1, 8); put(b + 48, 2, 8);
    put(b + 56, 1, 2); b[58] = 1; put(b + 60, 11, 2);
    put(b + 72, generation, 8); put(b + 80, 100000, 8); memcpy(b + 88, "UTF8_STRING", 11);
}
static void transfer(struct peer *p)
{
    static const uint8_t payload[] = "independent-c-portal";
    uint8_t b[344], bad[344], e[88], bytes[sizeof(payload) - 1];
    uint32_t writer;
    request(b, 2, 9);
    memcpy(bad, b, sizeof(b)); bad[59] = 1;
    require(submit(p, bad, sizeof(bad)) == 22, "reserved byte must be EINVAL");
    memcpy(bad, b, sizeof(b)); put(bad + 8, 8, 8);
    require(submit(p, bad, sizeof(bad)) == 116, "stale epoch must be ESTALE");
    memcpy(bad, b, sizeof(b)); put(bad + 48, 3, 8);
    require(submit(p, bad, sizeof(bad)) == 13, "foreign namespace must be EACCES");
    require(submit(p, b, sizeof(b) - 1) == 22, "truncated candidate must be EINVAL");
    memcpy(bad, b, sizeof(b)); put(bad + 6, 65535, 2);
    require(submit(p, bad, sizeof(bad)) == 22, "unknown kind must be EINVAL");
    request(b, 2, 8); accepted(p, b, sizeof(b)); event(p, 32, 88, e);
    require(get(e + 32, 8) == 41 && e[40] == 1, "stale source must be Denied");
    { size_t i; for (i = 41; i < 88; ++i) require(!e[i], "Denied must disclose no reason"); }
    ack(p);
    request(b, 3, 9); accepted(p, b, sizeof(b)); event(p, 32, 88, e);
    require(get(e + 32, 8) == 41 && e[40] == 2 && e[41] == 1 && get(e + 42, 2) == 1 &&
            !get(e + 44, 4) && get(e + 48, 8) == 1 && get(e + 56, 8) == 2 &&
            get(e + 64, 8) == 9 && get(e + 72, 8) == 3 && get(e + 80, 8) == 100000,
            "grant correlation"); ack(p);
    record(b, 56, 258, 4); put(b + 32, 41, 8); put(b + 48, sizeof(bytes), 8);
    accepted(p, b, 56); writer = open_file(p, "upload", 1, 1);
    memcpy(bytes, payload, sizeof(bytes));
    if (PEER_MUTATION == 2) bytes[0] = 'X';
    require(write_file(p, writer, 1, bytes, 1) == 22, "upload gap must be EINVAL");
    require(!write_file(p, writer, 0, bytes, 10), "first upload half");
    record(b, 48, 259, 5); put(b + 32, 41, 8); put(b + 40, sizeof(bytes), 8);
    require(submit(p, b, 48) == 22, "incomplete End must be EINVAL");
    require(!write_file(p, writer, 10, bytes + 10, sizeof(bytes) - 10), "second upload half");
    accepted(p, b, 48); clunk(p, writer); event(p, 33, 48, e);
    require(get(e + 32, 8) == 41 && e[40] == 2 && !get(e + 41, 7), "terminal outcome must be Completed");
    ack(p);
    require(submit(p, b, 48) == 114, "acknowledged replay must be EALREADY");
    record(b, 40, 260, 6); put(b + 32, 41, 8);
    require(submit(p, b, 40) == 22, "settled transfer must refuse Cancel");
}
int main(int argc, char **argv)
{
    static struct peer p;
    struct sophia_desktop_connection connection = {-1, 0, 0, 0};
    struct sophia_9p_handle h;
    uint8_t b[40], e[88];
    size_t storage_bytes = sophia_9p_storage_bytes(4096, 4);
    void *storage = malloc(storage_bytes);
    uint32_t limits;
    int r, fd, refused;
    require(argc == 3 && storage, "usage: peer PATH transfer|refused|withhold");
    require(!strcmp(argv[2], "transfer") || !strcmp(argv[2], "refused") ||
            !strcmp(argv[2], "withhold"), "unknown mode");
    refused = strcmp(argv[2], "transfer") != 0;
    p.deadline = now_ms() + 10000;
    r = sophia_desktop_connection_begin(&connection, argv[1]);
    while (r > 0) {
        struct pollfd f = {connection.fd, sophia_desktop_connection_events(&connection), 0};
        require(now_ms() < p.deadline && poll(&f, 1, 2) >= 0, "connect deadline");
        r = r == SOPHIA_DESKTOP_CONNECT_RETRY ? sophia_desktop_connection_begin(&connection, argv[1]) :
            sophia_desktop_connection_finish(&connection, f.revents);
    }
    require(r == SOPHIA_DESKTOP_CONNECTED, "connect"); fd = sophia_desktop_connection_take(&connection);
    require(!sophia_9p_init(&p.c, fd, 4096, 4, 32, storage, storage_bytes), "init");
    require(!sophia_9p_version(&p.c, &h), "version"); wait_reply(&p, h); require(p.type == 101, "version reply");
    require(!sophia_9p_attach(&p.c, "not-authority", "", &h, &p.root), "attach");
    wait_reply(&p, h); require(p.type == 105, "attach reply");
    p.api = open_file(&p, "api", 0, 0); read_file(&p, p.api, 0, 512);
    require(p.type == 117 && p.count < sizeof(p.data), "api read"); p.data[p.count] = 0;
    require(!strcmp((char *)p.data, "sophia-portal-files version=1 role=portal-requester epoch=7 fd_transfer=none\n"), "api identity");
    limits = open_file(&p, "limits", 0, 0); read_file(&p, limits, 0, 512);
    require(p.type == 117 && p.count == 80 && get(p.data, 4) == 80 && get(p.data + 6, 2) == 1 &&
            get(p.data + 8, 8) == 7 && !get(p.data + 16, 8) && !get(p.data + 24, 8) &&
            get(p.data + 40, 4) == 65536 && get(p.data + 72, 8) == 3, "limits identity and bounds");
    clunk(&p, limits);
    p.events = open_file(&p, "events", 0, 0); p.submit = open_file(&p, "submit", 0, 1);
    p.ack = open_file(&p, "ack", 0, 1);
    record(b, sizeof(b), 256, 1); put(b + 32, refused ? 2 : 1, 2); put(b + 34, refused ? 2 : 1, 2);
    if (PEER_MUTATION == 1) put(b + 8, 8, 8);
    accepted(&p, b, sizeof(b));
    if (refused) {
        event(&p, 17, 40, e); require(get(e + 32, 2) == 1 && !get(e + 34, 6), "Refused body");
        if (!strcmp(argv[2], "withhold")) poll(NULL, 0, 350);
        else ack(&p);
        read_file(&p, p.api, 0, 512);
        require(p.type == 7 && p.error == 116, "refusal must fence retained API fid");
    } else {
        event(&p, 16, 48, e);
        require(get(e + 32, 2) == 1 && !get(e + 34, 6) && get(e + 40, 8) == 7, "Negotiated body");
        ack(&p); transfer(&p);
    }
    printf("portal_c_files mode=%s status=complete\n", argv[2]);
    close(fd); free(storage); return 0;
}

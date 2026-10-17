/* Generic supplied-stream peer for Sophia's independent core-export check. */
#include "../sophia_9p_client.h"
#include <assert.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

static struct sophia_9p_reply wait_reply(struct sophia_9p_client *c, unsigned type)
{
    struct sophia_9p_reply r;
    for (;;) {
        struct pollfd fd = {c->fd, POLLIN, 0};
        assert(!sophia_9p_service(c, 17));
        if (!sophia_9p_peek(c, &r)) {
            if (r.type != type)
                fprintf(stderr, "reply type %u, expected %u, errno %u\n", r.type, type, r.error);
            assert(r.type == type);
            return r;
        }
        if (sophia_9p_wants_write(c)) fd.events |= POLLOUT;
        assert(poll(&fd, 1, 1000) >= 0);
    }
}
int main(void)
{
    struct sophia_9p_client c;
    struct sophia_9p_handle h;
    struct sophia_9p_reply r;
    struct sophia_9p_dirent entry;
    struct sophia_9p_attr attr;
    uint32_t root, info, sink;
    uint64_t cookie = 0;
    size_t position, count, bytes = sophia_9p_storage_bytes(4096, 2);
    void *storage = malloc(bytes);
    const char *name = "info";
    alarm(5);
    assert(storage && !sophia_9p_init(&c, STDIN_FILENO, 4096, 2, 4, storage, bytes));
    assert(!sophia_9p_version(&c, &h));
    r = wait_reply(&c, 101); assert(!sophia_9p_consume(&c, r.handle));
    assert(!sophia_9p_attach(&c, "", "", &h, &root));
    r = wait_reply(&c, 105); assert(!sophia_9p_consume(&c, r.handle));
    assert(!sophia_9p_walk(&c, root, &name, 1, &h, &info));
    r = wait_reply(&c, 111); assert(!sophia_9p_consume(&c, r.handle));
    assert(!sophia_9p_getattr(&c, info, &h));
    r = wait_reply(&c, 25);
    assert(!sophia_9p_attributes(&r, &attr));
    printf("attr %u %llu\n", attr.mode, (unsigned long long)attr.size);
    assert(!sophia_9p_consume(&c, r.handle));
    /* Walk before opening the root for listing; an open fid cannot be walked. */
    name = "sink";
    assert(!sophia_9p_walk(&c, root, &name, 1, &h, &sink));
    r = wait_reply(&c, 111); assert(!sophia_9p_consume(&c, r.handle));
    assert(!sophia_9p_lopen(&c, root, 0200000, &h));
    r = wait_reply(&c, 13); assert(!sophia_9p_consume(&c, r.handle));
    do {
        assert(!sophia_9p_readdir(&c, root, cookie, 64, &h));
        r = wait_reply(&c, 41);
        count = r.count; position = 0;
        while (sophia_9p_dirent_next(&r, &position, &entry) == 0) {
            printf("entry %u %.*s\n", entry.type, (int)entry.name_bytes, entry.name);
            cookie = entry.next;
        }
        assert(position == count);
        assert(!sophia_9p_consume(&c, r.handle));
    } while (count);
    assert(!sophia_9p_lopen(&c, sink, 1, &h));
    r = wait_reply(&c, 13); assert(!sophia_9p_consume(&c, r.handle));
    assert(!sophia_9p_write_secret(&c, sink, 0, "test-secret", 11, &h));
    assert(sophia_9p_scrubbing_output(&c));
    r = wait_reply(&c, 119); assert(r.count == 11 && !sophia_9p_scrubbing_output(&c));
    assert(!sophia_9p_consume(&c, r.handle));
    assert(!sophia_9p_clunk(&c, sink, &h));
    r = wait_reply(&c, 121); assert(!sophia_9p_consume(&c, r.handle));
    assert(!sophia_9p_clunk(&c, info, &h));
    r = wait_reply(&c, 121); assert(!sophia_9p_consume(&c, r.handle));
    assert(!sophia_9p_clunk(&c, root, &h));
    r = wait_reply(&c, 121); assert(!sophia_9p_consume(&c, r.handle));
    assert(!c.fid_count);
    sophia_9p_close(&c);
    free(storage);
    return 0;
}

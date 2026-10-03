/* Independent C peer for the lock provider files (t294). It uses only the
 * C SDK's generic 9P client and encodes every lock record by hand from
 * protocol/sophia-lock-files-v1.kdl, so a disagreement with Sophia's codec
 * fails here. One provider session: negotiate with one chord, read the lock
 * object, upload an image, ask for a frame, offer it, see it presented. */
#define _POSIX_C_SOURCE 200809L
#include "sophia_9p_client.h"
#include <assert.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

enum {
    KIND_LOCK = 2,
    KIND_NEGOTIATED = 16,
    KIND_SUBMITTED = 18,
    KIND_OBJECT_PUBLISHED = 19,
    KIND_RESOURCE_STATUS = 33,
    KIND_CANDIDATE_OUTCOME = 35,
    KIND_FRAME_PERMIT = 36,
    KIND_NEGOTIATE = 256,
    KIND_RESOURCE_BEGIN = 258,
    KIND_RESOURCE_END = 259,
    KIND_CANDIDATE = 262,
    KIND_FRAME_DEMAND = 263,
    HEADER = 32
};

static struct sophia_9p_client client;
static uint8_t reply_data[65536];
static uint32_t reply_count;
static uint64_t epoch, submission, events_offset;
static uint32_t root, events, transaction_fid, submit_fid, ack_fid;

static void put16(uint8_t *at, uint16_t v) {
    at[0] = (uint8_t)v;
    at[1] = (uint8_t)(v >> 8);
}
static void put32(uint8_t *at, uint32_t v) {
    put16(at, (uint16_t)v);
    put16(at + 2, (uint16_t)(v >> 16));
}
static void put64(uint8_t *at, uint64_t v) {
    put32(at, (uint32_t)v);
    put32(at + 4, (uint32_t)(v >> 32));
}
static uint16_t get16(const uint8_t *at) {
    return (uint16_t)(at[0] | (at[1] << 8));
}
static uint32_t get32(const uint8_t *at) {
    return (uint32_t)get16(at) | ((uint32_t)get16(at + 2) << 16);
}
static uint64_t get64(const uint8_t *at) {
    return (uint64_t)get32(at) | ((uint64_t)get32(at + 4) << 32);
}

/* Waits for the reply to one request; its data is copied out. */
static struct sophia_9p_reply wait_reply(struct sophia_9p_handle handle) {
    struct sophia_9p_reply reply;
    for (;;) {
        int status = sophia_9p_service(&client, 1u << 20);
        assert(status >= 0);
        while (sophia_9p_peek(&client, &reply) == SOPHIA_9P_OK) {
            if (reply.handle.serial == handle.serial && reply.handle.slot == handle.slot) {
                reply_count = 0;
                if (reply.data != NULL && reply.count <= sizeof reply_data) {
                    memcpy(reply_data, reply.data, reply.count);
                    reply_count = reply.count;
                }
                assert(sophia_9p_consume(&client, handle) == SOPHIA_9P_OK);
                return reply;
            }
            assert(0 && "a reply for another request");
        }
        struct pollfd fd = {client.fd, (short)(POLLIN | (sophia_9p_wants_write(&client) ? POLLOUT : 0)), 0};
        assert(poll(&fd, 1, 5000) > 0);
    }
}

static uint32_t open_path(const char *const *names, size_t count, uint32_t flags) {
    struct sophia_9p_handle handle;
    uint32_t fid;
    assert(sophia_9p_walk(&client, root, names, count, &handle, &fid) == SOPHIA_9P_OK);
    assert(wait_reply(handle).error == 0);
    assert(sophia_9p_lopen(&client, fid, flags, &handle) == SOPHIA_9P_OK);
    assert(wait_reply(handle).error == 0);
    return fid;
}

static uint32_t open_name(const char *name, uint32_t flags) {
    const char *names[1] = {name};
    return open_path(names, 1, flags);
}

static uint32_t read_at(uint32_t fid, uint64_t offset) {
    struct sophia_9p_handle handle;
    assert(sophia_9p_read(&client, fid, offset, 4096, &handle) == SOPHIA_9P_OK);
    assert(wait_reply(handle).error == 0);
    return reply_count;
}

static void write_at(uint32_t fid, uint64_t offset, const void *data, size_t count) {
    struct sophia_9p_handle handle;
    assert(sophia_9p_write(&client, fid, offset, data, count, &handle) == SOPHIA_9P_OK);
    struct sophia_9p_reply reply = wait_reply(handle);
    assert(reply.error == 0 && reply.count == count);
}

/* Stages one candidate in a fresh transaction and submits it. */
static void submit(uint16_t kind, const uint8_t *body, size_t length) {
    uint8_t record[128], control[24] = {0};
    struct sophia_9p_handle handle;
    assert(HEADER + length <= sizeof record);
    memset(record, 0, sizeof record);
    put32(record, (uint32_t)(HEADER + length));
    put16(record + 4, 1);
    put16(record + 6, kind);
    put64(record + 8, epoch);
    put64(record + 16, ++submission);
    memcpy(record + HEADER, body, length);
    transaction_fid = open_name("transaction", 2);
    write_at(transaction_fid, 0, record, HEADER + length);
    put64(control, epoch);
    put64(control + 8, submission);
    put32(control + 16, (uint32_t)(HEADER + length));
    write_at(submit_fid, 0, control, sizeof control);
    assert(sophia_9p_clunk(&client, transaction_fid, &handle) == SOPHIA_9P_OK);
    wait_reply(handle);
}

/* Reads events until one of `kind`; acknowledges everything read. Its body
 * is copied to `body`. */
static void wait_event(uint16_t kind, uint8_t *body, size_t length) {
    for (;;) {
        uint32_t count = read_at(events, events_offset);
        uint32_t at = 0;
        int found = 0;
        uint64_t last = 0;
        while (at + HEADER <= count) {
            uint32_t size = get32(reply_data + at);
            assert(size >= HEADER && at + size <= count);
            assert(get16(reply_data + at + 4) == 1 && get64(reply_data + at + 8) == epoch);
            last = get64(reply_data + at + 24);
            at += size;
            if (get16(reply_data + at - size + 6) == kind) {
                assert(size - HEADER >= length);
                memcpy(body, reply_data + at - size + HEADER, length);
                /* Records after this one are read again by the next wait. */
                found = 1;
                break;
            }
        }
        events_offset += at;
        if (last != 0) {
            uint8_t ack[16];
            put64(ack, epoch);
            put64(ack + 8, last);
            write_at(ack_fid, 0, ack, sizeof ack);
        }
        if (found)
            return;
    }
}

int main(int argc, char **argv) {
    struct sockaddr_un address;
    struct sophia_9p_handle handle;
    static uint8_t storage[1u << 20];
    uint8_t body[96];
    char admitted;
    int fd;
    assert(argc == 2);
    /* The parent authorizes this exact PID before releasing the gate. */
    assert(read(STDIN_FILENO, &admitted, 1) == 1 && admitted == 'G');
    memset(&address, 0, sizeof address);
    address.sun_family = AF_UNIX;
    assert(strlen(argv[1]) < sizeof address.sun_path);
    strcpy(address.sun_path, argv[1]);
    fd = socket(AF_UNIX, SOCK_STREAM, 0);
    assert(fd >= 0 && connect(fd, (struct sockaddr *)&address, sizeof address) == 0);
    assert(fcntl(fd, F_SETFL, O_NONBLOCK) == 0);
    assert(sophia_9p_storage_bytes(8192, 8) <= sizeof storage);
    assert(sophia_9p_init(&client, fd, 8192, 8, 32, storage, sizeof storage) == SOPHIA_9P_OK);
    assert(sophia_9p_version(&client, &handle) == SOPHIA_9P_OK);
    wait_reply(handle);
    assert(sophia_9p_attach(&client, "", "", &handle, &root) == SOPHIA_9P_OK);
    assert(wait_reply(handle).error == 0);

    /* The api line names the epoch every record carries. */
    {
        uint32_t api = open_name("api", 0);
        uint32_t count = read_at(api, 0);
        const char *prefix = "sophia-lock-files version=1 epoch=";
        reply_data[count < sizeof reply_data ? count : sizeof reply_data - 1] = 0;
        assert(strncmp((const char *)reply_data, prefix, strlen(prefix)) == 0);
        epoch = strtoull((const char *)reply_data + strlen(prefix), NULL, 10);
        assert(epoch != 0);
    }
    events = open_name("events", 0);
    submit_fid = open_name("submit", 1);
    ack_fid = open_name("ack", 1);

    /* Negotiate: present and chords, one chord Alt+b. */
    memset(body, 0, sizeof body);
    put16(body, 1);
    put16(body + 2, 1);
    put16(body + 4, 1);
    put64(body + 8, 3);
    put32(body + 16, 0x62);
    put16(body + 20, 1u << 2);
    submit(KIND_NEGOTIATE, body, 24);
    wait_event(KIND_NEGOTIATED, body, 16);
    assert(get16(body) == 1 && get16(body + 2) == 1 && get64(body + 8) == 3);
    wait_event(KIND_OBJECT_PUBLISHED, body, 24);
    assert(get16(body) == KIND_LOCK);

    /* The lock object Session published: locked, one 2x2 allocation. */
    uint64_t lock_epoch, output_id, output_generation, allocation_id, allocation_generation;
    {
        uint32_t lock = open_name("lock", 0);
        uint32_t count = read_at(lock, 0);
        assert(count == HEADER + 32 + 56 && get16(reply_data + 6) == KIND_LOCK);
        const uint8_t *object = reply_data + HEADER;
        lock_epoch = get64(object);
        assert(lock_epoch != 0 && get16(object + 16) == 3 && get16(object + 18) == 1);
        const uint8_t *allocation = object + 32;
        output_id = get64(allocation);
        output_generation = get64(allocation + 8);
        allocation_id = get64(allocation + 16);
        allocation_generation = get64(allocation + 24);
        assert(get32(allocation + 32) == 2 && get32(allocation + 36) == 2);
    }

    /* Upload a 2x2 premultiplied BGRA8 image through slot 0. */
    memset(body, 0, sizeof body);
    put64(body, 1);
    put64(body + 8, 5);
    put64(body + 16, 1);
    put32(body + 24, 2);
    put32(body + 28, 2);
    put16(body + 32, 0);
    put16(body + 34, 1);
    submit(KIND_RESOURCE_BEGIN, body, 40);
    wait_event(KIND_RESOURCE_STATUS, body, 40);
    assert(get16(body + 24) == 1 && get64(body + 32) == 16);
    {
        const char *names[2] = {"upload", "0"};
        uint8_t pixels[16];
        uint32_t upload = open_path(names, 2, 1);
        memset(pixels, 0x7f, sizeof pixels);
        write_at(upload, 0, pixels, sizeof pixels);
    }
    memset(body, 0, sizeof body);
    put64(body, 1);
    put64(body + 8, 5);
    put64(body + 16, 1);
    put64(body + 24, 16);
    submit(KIND_RESOURCE_END, body, 32);
    wait_event(KIND_RESOURCE_STATUS, body, 40);
    assert(get16(body + 24) == 2);

    /* Ask for a frame, then offer it under the permit. */
    memset(body, 0, sizeof body);
    put64(body, 2);
    put64(body + 8, lock_epoch);
    put64(body + 16, allocation_id);
    put64(body + 24, allocation_generation);
    put64(body + 32, 1);
    submit(KIND_FRAME_DEMAND, body, 40);
    wait_event(KIND_FRAME_PERMIT, body, 48);
    assert(get64(body + 24) == 1);
    uint64_t permit = get64(body + 32);

    memset(body, 0, sizeof body);
    put64(body, 3);
    put64(body + 8, lock_epoch);
    put64(body + 16, output_id);
    put64(body + 24, output_generation);
    put64(body + 32, allocation_id);
    put64(body + 40, allocation_generation);
    put64(body + 48, 1);
    put64(body + 56, permit);
    put64(body + 64, 5);
    put64(body + 72, 1);
    submit(KIND_CANDIDATE, body, 96);
    wait_event(KIND_CANDIDATE_OUTCOME, body, 56);
    assert(get16(body + 40) == 2 && "presented");
    puts("lock_files_peer status=presented");
    (void)KIND_SUBMITTED;
    close(fd);
    return 0;
}

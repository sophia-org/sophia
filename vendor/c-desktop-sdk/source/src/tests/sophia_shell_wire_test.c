#include "../sophia_shell_wire.h"
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static const uint8_t welcome_bytes[] = {
    0x53,0x4f,0x50,0x48,1,0,97,0,0,0,0,0,0,0,0,0,28,0,0,0,0,0,0,0,
    1,0,0,0,5,0,0,0,0,0,0,0,1,0,0,0,0,0,0,0,16,0,128,0,16,0,0,0
};

struct fixture {
    int peers[2];
    struct sophia_shell_wire wire;
    uint8_t rx[256], tx[256];
};

static void start(struct fixture *f)
{
    assert(socketpair(AF_UNIX, SOCK_STREAM, 0, f->peers) == 0);
    assert(sophia_shell_wire_init(&f->wire, f->peers[0], f->rx, sizeof(f->rx),
                                 f->tx, sizeof(f->tx)) == SOPHIA_SHELL_OK);
}

static void finish(struct fixture *f)
{
    assert(close(f->peers[0]) == 0);
    assert(close(f->peers[1]) == 0);
}

static void framing(void)
{
    struct sophia_shell_frame frame;
    uint8_t data[128];
    size_t count = 123;
    assert(sophia_shell_frame_decode(welcome_bytes, sizeof(welcome_bytes), &frame) == SOPHIA_SHELL_OK);
    assert(frame.kind == 97 && frame.transaction == 0 && frame.payload_bytes == 28);
    assert(sophia_shell_frame_encode(data, sizeof(data), frame.kind, frame.transaction,
        frame.payload, frame.payload_bytes, &count) == SOPHIA_SHELL_OK);
    assert(count == sizeof(welcome_bytes) && !memcmp(data, welcome_bytes, count));
    for (size_t cut = 0; cut < count; ++cut)
        assert(sophia_shell_frame_decode(data, cut, &frame) == SOPHIA_SHELL_INVALID);
    assert(sophia_shell_frame_decode(data, count + 1, &frame) == SOPHIA_SHELL_INVALID);
    const size_t corrupt[] = {0,4,6,8,16,20};
    for (size_t i = 0; i < sizeof(corrupt)/sizeof(*corrupt); ++i) {
        memcpy(data, welcome_bytes, sizeof(welcome_bytes));
        data[corrupt[i]] ^= 0xff;
        assert(sophia_shell_frame_decode(data, sizeof(welcome_bytes), &frame) == SOPHIA_SHELL_INVALID);
    }
    memset(data, 0x42, sizeof(data));
    count = 123;
    assert(sophia_shell_frame_encode(data, 24, 102, 1, data, 1, &count) == SOPHIA_SHELL_INVALID);
    assert(count == 123 && data[0] == 0x42);
    assert(sophia_shell_frame_encode(data, sizeof(data), 102, 0, NULL, 0, &count) == SOPHIA_SHELL_INVALID);
    assert(sophia_shell_frame_encode(data, sizeof(data), 32, 1, NULL, 0, &count) == SOPHIA_SHELL_INVALID);
    assert(sophia_shell_frame_encode(data, sizeof(data), 102, 1, data, SIZE_MAX, &count) == SOPHIA_SHELL_INVALID);
    assert(sophia_shell_frame_encode(data, sizeof(data), 102, 1, data, 5, &count) == SOPHIA_SHELL_OK);
    for (size_t i = 24; i < 29; ++i)
        assert(data[i] == 0x42);
    assert(sophia_shell_frame_encode(data, sizeof(data), 102, UINT64_MAX, NULL, 0, &count) == SOPHIA_SHELL_OK);
    assert(sophia_shell_frame_decode(data, count, &frame) == SOPHIA_SHELL_OK && frame.transaction == UINT64_MAX);
}

static void fragmented_fifo(void)
{
    struct fixture f;
    start(&f);
    int flags = fcntl(f.peers[0], F_GETFL);
    struct sophia_shell_frame frame;
    assert(sophia_shell_wire_receive(&f.wire, 0, &frame) == SOPHIA_SHELL_AGAIN);
    assert(sophia_shell_wire_receive(&f.wire, 256, &frame) == SOPHIA_SHELL_AGAIN);
    for (size_t i = 0; i < sizeof(welcome_bytes); ++i) {
        assert(send(f.peers[1], welcome_bytes + i, 1, 0) == 1);
        int result = sophia_shell_wire_receive(&f.wire, 1, &frame);
        assert(result == (i + 1 == sizeof(welcome_bytes) ? SOPHIA_SHELL_FRAME : SOPHIA_SHELL_AGAIN));
        assert(f.wire.rx_used == i + 1);
    }
    assert(frame.payload == f.rx + 24 && frame.payload_bytes == 28);
    assert(send(f.peers[1], welcome_bytes, sizeof(welcome_bytes), 0) == (ssize_t)sizeof(welcome_bytes));
    assert(sophia_shell_wire_receive(&f.wire, 256, &frame) == SOPHIA_SHELL_FRAME);
    assert(frame.payload[4] == 5 && f.wire.rx_used == sizeof(welcome_bytes));
    assert(sophia_shell_wire_consume(&f.wire) == SOPHIA_SHELL_OK);
    assert(sophia_shell_wire_receive(&f.wire, 24, &frame) == SOPHIA_SHELL_AGAIN);
    assert(f.wire.rx_used == 24);
    assert(sophia_shell_wire_consume(&f.wire) == SOPHIA_SHELL_AGAIN);
    assert(sophia_shell_wire_receive(&f.wire, 28, &frame) == SOPHIA_SHELL_FRAME);
    assert(sophia_shell_wire_consume(&f.wire) == SOPHIA_SHELL_OK);
    assert(sophia_shell_wire_receive(&f.wire, 256, &frame) == SOPHIA_SHELL_AGAIN);
    assert(fcntl(f.peers[0], F_GETFL) == flags);
    finish(&f);
}

static void partial_write(void)
{
    struct fixture f;
    start(&f);
    uint8_t payload[] = {1,2,3,4}, expected[64], received[64];
    size_t total;
    assert(sophia_shell_frame_encode(expected, sizeof(expected), 102, 7, payload, 4, &total) == 0);
    assert(sophia_shell_wire_queue(&f.wire, 102, 7, payload, 4) == SOPHIA_SHELL_OK);
    memset(payload, 0xff, sizeof(payload));
    assert(sophia_shell_wire_flush(&f.wire, 0) == SOPHIA_SHELL_AGAIN && f.wire.tx_sent == 0);
    for (size_t sent = 0; sent < total; ++sent) {
        assert(sophia_shell_wire_queue(&f.wire, 102, 8, payload, 4) == SOPHIA_SHELL_BUSY);
        int result = sophia_shell_wire_flush(&f.wire, 1);
        assert(result == (sent + 1 == total ? SOPHIA_SHELL_OK : SOPHIA_SHELL_AGAIN));
        assert(recv(f.peers[1], received + sent, 1, MSG_DONTWAIT) == 1);
        if (sent + 1 != total)
            assert(f.wire.tx_sent == sent + 1 && f.wire.tx_used == total);
    }
    assert(!memcmp(received, expected, total));
    assert(!f.wire.tx_used && !f.wire.tx_sent);
    assert(sophia_shell_wire_flush(&f.wire, 256) == SOPHIA_SHELL_OK);
    assert(recv(f.peers[1], received, sizeof(received), MSG_DONTWAIT) == -1 && errno == EAGAIN);
    assert(sophia_shell_wire_queue(&f.wire, 97, 0, payload, 4) == SOPHIA_SHELL_INVALID);
    finish(&f);
}

static void terminal_errors(void)
{
    const size_t cuts[] = {0,1,23,24,51};
    for (size_t i = 0; i < sizeof(cuts)/sizeof(*cuts); ++i) {
        struct fixture f;
        start(&f);
        if (cuts[i])
            assert(send(f.peers[1], welcome_bytes, cuts[i], 0) == (ssize_t)cuts[i]);
        assert(shutdown(f.peers[1], SHUT_WR) == 0);
        struct sophia_shell_frame frame;
        int expected = cuts[i] ? SOPHIA_SHELL_INVALID : SOPHIA_SHELL_CLOSED;
        assert(sophia_shell_wire_receive(&f.wire, 256, &frame) == expected);
        assert(sophia_shell_wire_receive(&f.wire, 256, &frame) == expected);
        assert(sophia_shell_wire_consume(&f.wire) == expected);
        finish(&f);
    }
    struct fixture f;
    start(&f);
    assert(sophia_shell_wire_queue(&f.wire, 102, 3, NULL, 0) == SOPHIA_SHELL_OK);
    assert(shutdown(f.peers[1], SHUT_RDWR) == 0);
    assert(sophia_shell_wire_flush(&f.wire, 256) == SOPHIA_SHELL_IO_ERROR);
    assert(f.wire.tx_used == 24 && f.wire.tx_sent == 0);
    assert(sophia_shell_wire_flush(&f.wire, 256) == SOPHIA_SHELL_IO_ERROR);
    finish(&f);
    start(&f);
    assert(sophia_shell_wire_queue(&f.wire, 102, 4, NULL, 0) == SOPHIA_SHELL_OK);
    assert(sophia_shell_wire_flush(&f.wire, 1) == SOPHIA_SHELL_AGAIN && f.wire.tx_sent == 1);
    assert(shutdown(f.peers[1], SHUT_RDWR) == 0);
    assert(sophia_shell_wire_flush(&f.wire, 256) == SOPHIA_SHELL_IO_ERROR);
    assert(f.wire.tx_used == 24 && f.wire.tx_sent == 1);
    finish(&f);
}

static void oversize_and_direction(void)
{
    for (unsigned mode = 0; mode < 3; ++mode) {
        struct fixture f;
        start(&f);
        uint8_t bytes[sizeof(welcome_bytes)];
        memcpy(bytes, welcome_bytes, sizeof(bytes));
        if (mode == 0) { bytes[16] = 0; bytes[17] = 2; }
        if (mode == 1) { bytes[16] = 1; bytes[17] = 0; bytes[18] = 1; }
        if (mode == 2) { bytes[6] = 96; }
        assert(send(f.peers[1], bytes, sizeof(bytes), 0) == (ssize_t)sizeof(bytes));
        struct sophia_shell_frame frame;
        assert(sophia_shell_wire_receive(&f.wire, 256, &frame) == SOPHIA_SHELL_INVALID);
        assert(f.wire.rx_used == 24);
        assert(recv(f.peers[0], bytes, 28, MSG_DONTWAIT) == 28);
        finish(&f);
    }
}

static void kernel_backpressure(void)
{
    int peers[2], buffer_size = 4096;
    assert(socketpair(AF_UNIX, SOCK_STREAM, 0, peers) == 0);
    assert(setsockopt(peers[0], SOL_SOCKET, SO_SNDBUF, &buffer_size, sizeof(buffer_size)) == 0);
    uint8_t *rx = malloc(SOPHIA_SHELL_MAX_FRAME_BYTES), *tx = malloc(SOPHIA_SHELL_MAX_FRAME_BYTES);
    uint8_t *payload = malloc(SOPHIA_SHELL_MAX_PAYLOAD_BYTES), *received = malloc(SOPHIA_SHELL_MAX_FRAME_BYTES);
    assert(rx && tx && payload && received);
    for (size_t i = 0; i < SOPHIA_SHELL_MAX_PAYLOAD_BYTES; ++i)
        payload[i] = (uint8_t)i;
    struct sophia_shell_wire wire;
    assert(sophia_shell_wire_init(&wire, peers[0], rx, SOPHIA_SHELL_MAX_FRAME_BYTES,
                                  tx, SOPHIA_SHELL_MAX_FRAME_BYTES) == SOPHIA_SHELL_OK);
    assert(sophia_shell_wire_queue(&wire, 167, 9, payload, SOPHIA_SHELL_MAX_PAYLOAD_BYTES) == SOPHIA_SHELL_OK);
    assert(sophia_shell_wire_flush(&wire, SOPHIA_SHELL_MAX_FRAME_BYTES) == SOPHIA_SHELL_AGAIN);
    size_t stalled = wire.tx_sent;
    assert(stalled > 0 && stalled < SOPHIA_SHELL_MAX_FRAME_BYTES);
    assert(sophia_shell_wire_flush(&wire, SOPHIA_SHELL_MAX_FRAME_BYTES) == SOPHIA_SHELL_AGAIN);
    assert(wire.tx_sent == stalled);
    size_t used = 0;
    for (unsigned turns = 0; used != SOPHIA_SHELL_MAX_FRAME_BYTES && turns < 128; ++turns) {
        assert(sophia_shell_wire_queue(&wire, 102, 10, NULL, 0) == SOPHIA_SHELL_BUSY);
        ssize_t got = recv(peers[1], received + used, SOPHIA_SHELL_MAX_FRAME_BYTES - used, MSG_DONTWAIT);
        assert(got > 0);
        used += (size_t)got;
        int result = sophia_shell_wire_flush(&wire, SOPHIA_SHELL_MAX_FRAME_BYTES);
        assert(result == SOPHIA_SHELL_OK || result == SOPHIA_SHELL_AGAIN);
        if (result == SOPHIA_SHELL_OK) {
            while (used < SOPHIA_SHELL_MAX_FRAME_BYTES) {
                got = recv(peers[1], received + used, SOPHIA_SHELL_MAX_FRAME_BYTES - used, MSG_DONTWAIT);
                assert(got > 0);
                used += (size_t)got;
            }
        }
    }
    assert(used == SOPHIA_SHELL_MAX_FRAME_BYTES && !wire.tx_used);
    struct sophia_shell_frame frame;
    assert(sophia_shell_frame_decode(received, used, &frame) == SOPHIA_SHELL_OK);
    assert(frame.kind == 167 && frame.transaction == 9 && frame.payload_bytes == SOPHIA_SHELL_MAX_PAYLOAD_BYTES);
    assert(!memcmp(frame.payload, payload, frame.payload_bytes));
    close(peers[0]); close(peers[1]);
    free(rx); free(tx); free(payload); free(received);
}

static void negotiation(void)
{
    struct sophia_shell_hello request = {1,6,1};
    struct sophia_shell_frame frame;
    struct sophia_shell_welcome welcome;
    uint8_t bytes[sizeof(welcome_bytes)];
    assert(sophia_shell_frame_decode(welcome_bytes, sizeof(welcome_bytes), &frame) == SOPHIA_SHELL_OK);
    assert(sophia_shell_welcome_decode(&frame, request, &welcome) == SOPHIA_SHELL_OK);
    assert(welcome.connection_epoch == 5 && welcome.revision == 1);
    const size_t offsets[] = {24,26,28,36,44,46,48,50};
    for (size_t i = 0; i < sizeof(offsets)/sizeof(*offsets); ++i) {
        memcpy(bytes, welcome_bytes, sizeof(bytes));
        bytes[offsets[i]] = (i == 1 || i == 7) ? 1 : 0;
        assert(sophia_shell_frame_decode(bytes, sizeof(bytes), &frame) == SOPHIA_SHELL_OK);
        welcome.connection_epoch = 777;
        assert(sophia_shell_welcome_decode(&frame, request, &welcome) == SOPHIA_SHELL_INVALID);
        assert(welcome.connection_epoch == 777);
    }
    memcpy(bytes, welcome_bytes, sizeof(bytes));
    bytes[24] = 6; bytes[36] = 3; bytes[37] = 6;
    request.required_capabilities = 0x601;
    assert(sophia_shell_frame_decode(bytes, sizeof(bytes), &frame) == SOPHIA_SHELL_OK);
    assert(sophia_shell_welcome_decode(&frame, request, &welcome) == SOPHIA_SHELL_OK);
    bytes[37] = 4; /* activation without indicators */
    request.required_capabilities = 1;
    assert(sophia_shell_welcome_decode(&frame, request, &welcome) == SOPHIA_SHELL_INVALID);
    bytes[37] = 6; bytes[24] = 5; /* revision contradiction */
    assert(sophia_shell_welcome_decode(&frame, request, &welcome) == SOPHIA_SHELL_INVALID);
    size_t count;
    assert(sophia_shell_hello_encode(bytes, sizeof(bytes),
        (struct sophia_shell_hello){7,7,0x9a0}, &count) == SOPHIA_SHELL_OK);
    memcpy(bytes, welcome_bytes, sizeof(bytes));
    bytes[24]=7; bytes[36]=0xa0; bytes[37]=9;
    assert(sophia_shell_frame_decode(bytes, sizeof(bytes), &frame)==SOPHIA_SHELL_OK);
    assert(sophia_shell_welcome_decode(&frame, (struct sophia_shell_hello){7,7,0x9a0}, &welcome)==SOPHIA_SHELL_OK);
    assert(welcome.capabilities==0x9a0 && welcome.revision==7);
    assert(sophia_shell_welcome_decode(&frame, (struct sophia_shell_hello){1,7,1}, &welcome)==SOPHIA_SHELL_INVALID);
    bytes[36]=0xa1;
    assert(sophia_shell_welcome_decode(&frame, (struct sophia_shell_hello){7,7,0x9a0}, &welcome)==SOPHIA_SHELL_INVALID);
    const struct sophia_shell_hello invalid[] = {{0,6,1},{6,1,1},{1,8,1},{1,6,0},
        {1,5,0x601},{1,6,0x401},{1,6,0x101},{1,6,0x41},{1,6,0x11},{1,6,0x801},{6,7,0x9a0},{7,7,0x9a1},{7,7,0x980},{7,7,0x8a0}};
    for (size_t i = 0; i < sizeof(invalid)/sizeof(*invalid); ++i)
        assert(sophia_shell_hello_encode(bytes, sizeof(bytes), invalid[i], &count) == SOPHIA_SHELL_INVALID);
}

int main(void)
{
    alarm(10);
    framing();
    fragmented_fifo();
    partial_write();
    terminal_errors();
    oversize_and_direction();
    kernel_backpressure();
    negotiation();
    alarm(0);
    puts("sophia_shell_wire controls=7 status=pass native=false");
    return 0;
}

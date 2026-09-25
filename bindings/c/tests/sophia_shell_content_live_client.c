/* Independent r5 socket client. Only framing comes from the independent
 * descriptor client; content fields below are encoded from the wire contract. */
#define main descriptor_proof_main
#include "sophia_shell_v1_client.c"
#undef main
#include <sys/time.h>

struct content_peer {
    int fd;
    uint8_t grant[16], output[16], allocation[16], resource[16];
    uint64_t facts, scale;
    uint8_t popup[16];
    uint64_t popup_scale;
    uint32_t reservation;
};

static void require(int condition, const char *message) {
    if (!condition) {
        fprintf(stderr, "content C client: %s\n", message);
        exit(1);
    }
}

static void content_receive(struct content_peer *peer, struct frame *frame,
                            uint16_t kind, size_t length) {
    require(receive_frame(peer->fd, frame), "bounded receive failed");
    require(frame->kind == kind && frame->payload_len == length,
            "unexpected content kind or payload length");
    require(memcmp(frame->bytes + FRAME_HEADER_LEN, peer->grant, 16) == 0,
            "content grant changed");
    require((kind == 161u) == (frame->transaction == 0), "transaction form changed");
}

static void content_send(struct content_peer *peer, uint16_t kind, uint64_t tx,
                         uint8_t *payload, size_t length) {
    memcpy(payload, peer->grant, 16);
    require(send_frame(peer->fd, kind, tx, payload, length), "bounded send failed");
}


static void candidate(struct content_peer *peer, uint64_t generation, unsigned count) {
    struct frame frame;
    uint8_t demand[58] = {0}, begin[80] = {0}, chunk[328] = {0}, end[40] = {0};
    uint8_t *p;
    memcpy(demand + 16, peer->output, 16); write_u64(demand + 48, generation);
    write_u16(demand + 56, 1); content_send(peer, 176, 9, demand, sizeof(demand));
    content_receive(peer, &frame, 177, 64); p = frame.bytes + FRAME_HEADER_LEN;
    require(memcmp(p + 16, peer->output, 16) == 0 && read_u64(p + 32) == generation &&
            read_u64(p + 40) && read_u16(p + 48) == 1, "permit changed");
    write_u64(begin + 16, generation); memcpy(begin + 24, peer->output, 16);
    write_u64(begin + 40, peer->facts); write_u64(begin + 48, read_u64(p + 40));
    write_u64(begin + 56, generation);
    write_u32(begin + 64, count); write_u32(begin + 68, count); write_u32(begin + 72, count);
    content_send(peer, 172, 10, begin, sizeof(begin));
    write_u64(chunk + 16, generation); write_u32(chunk + 28, count);
    write_u32(chunk + 32, count); write_u32(chunk + 36, count);
    for (unsigned i = 0; i < count; ++i) {
        p = chunk + 40 + i * 64;
        memcpy(p, i ? peer->popup : peer->allocation, 16);
        write_u64(p + 16, i ? peer->popup_scale : peer->scale);
        write_u16(p + 24, i ? 2 : 1); write_u16(p + 26, 1);
        write_u32(p + 36, i ? 0 : peer->reservation); write_u16(p + 40, i ? 0 : 65535);
        if (i) { write_u32(p + 44, 3); write_u32(p + 48, 4); write_u32(p + 52, 2); write_u32(p + 56, 1); }
        p = chunk + 40 + count * 64 + i * 32;
        memcpy(p, peer->resource, 16); write_u16(p + 16, (uint16_t)i);
        write_u32(p + 20, i ? 0 : 3); write_u32(p + 24, i ? 0 : 4);
        p = chunk + 40 + count * 96 + i * 48;
        write_u16(p, (uint16_t)i); write_u16(p + 2, 1); write_u64(p + 4, i + 1);
        write_u64(p + 12, generation); write_u64(p + 20, i + 1);
        write_u32(p + 28, i ? 0 : 3); write_u32(p + 32, i ? 0 : 4);
        write_u32(p + 36, 2); write_u32(p + 40, 1);
    }
    content_send(peer, 173, 11, chunk, 40 + count * 144);
    write_u64(end + 16, generation); write_u32(end + 24, count);
    write_u32(end + 28, count); write_u32(end + 32, count);
    content_send(peer, 174, 12, end, sizeof(end));
}

static uint64_t presented(struct content_peer *peer, uint64_t generation) {
    struct frame frame;
    uint64_t epoch = 0;
    for (unsigned kind = 1; kind <= 2; ++kind) {
        content_receive(peer, &frame, 175, 68);
        const uint8_t *p = frame.bytes + FRAME_HEADER_LEN;
        require(read_u64(p + 16) == generation && memcmp(p + 24, peer->output, 16) == 0 &&
                read_u16(p + 40) == kind && !read_u16(p + 42), "candidate outcome identity changed");
        epoch = read_u64(p + 44);
        require((kind == 2) == (epoch != 0), "presentation epoch on wrong outcome");
    }
    return epoch;
}

static void action(struct content_peer *peer, uint64_t generation, uint64_t epoch, uint16_t kind) {
    struct frame frame;
    uint8_t ack[112];
    content_receive(peer, &frame, 179, 112);
    const uint8_t *p = frame.bytes + FRAME_HEADER_LEN;
    require(memcmp(p + 16, peer->output, 16) == 0 && read_u64(p + 32) == generation &&
            read_u64(p + 40) == epoch && read_u64(p + 48) == generation &&
            memcmp(p + 56, peer->popup, 16) == 0 && read_u16(p + 104) == kind &&
            !read_u16(p + 106) && read_u64(p + 96) == generation - 1, "action identity changed");
    require(read_u64(p + 72) == (kind == 2 ? 0 : 2) &&
            read_u64(p + 80) == (kind == 2 ? 0 : generation) &&
            read_u64(p + 88) == (kind == 2 ? 0 : 2), "action target changed");
    memcpy(ack, p, sizeof(ack)); write_u16(ack + 104, 1); memset(ack + 106, 0, 6);
    content_send(peer, 180, 20, ack, sizeof(ack));
}

static void invalidated(struct content_peer *peer, const uint8_t allocation[16]) {
    struct frame frame;
    content_receive(peer, &frame, 164, 160);
    const uint8_t *p = frame.bytes + FRAME_HEADER_LEN;
    require(read_u16(p + 24) == 4 && read_u16(p + 26) == 8 &&
            memcmp(p + 48, allocation, 16) == 0, "expected exact allocation loss");
}

static void lifecycle(struct content_peer *peer) {
    uint64_t epoch = presented(peer, 1);
    uint8_t request[120] = {0};
    struct frame frame;
    memcpy(request + 16, peer->output, 16); write_u64(request + 32, 2);
    write_u16(request + 40, 1); write_u16(request + 42, 2); write_u16(request + 44, 1);
    memcpy(request + 64, peer->allocation, 16); write_u64(request + 80, epoch);
    write_u32(request + 88, 3); write_u32(request + 92, 4); write_u32(request + 96, 2); write_u32(request + 100, 1);
    write_u32(request + 104, 16); write_u32(request + 108, 8);
    write_u64(request + 80, epoch + 1);
    content_send(peer, 163, 21, request, sizeof(request));
    content_receive(peer, &frame, 164, 160);
    const uint8_t *stale = frame.bytes + FRAME_HEADER_LEN;
    require(read_u64(stale + 16) == 2 && read_u16(stale + 24) == 2 && read_u16(stale + 26) == 1,
            "stale parent receipt was not rejected");
    write_u64(request + 32, 3); write_u64(request + 80, epoch);
    content_send(peer, 163, 22, request, sizeof(request));
    content_receive(peer, &frame, 164, 160);
    const uint8_t *p = frame.bytes + FRAME_HEADER_LEN;
    require(read_u64(p + 16) == 3 && read_u16(p + 24) == 1 && !read_u16(p + 26) &&
            memcmp(p + 32, peer->output, 16) == 0 && memcmp(p + 64, peer->allocation, 16) == 0 &&
            read_u32(p + 104) == 3 && read_u32(p + 108) == 5 && read_u32(p + 112) == 16 && read_u32(p + 116) == 8,
            "popout grant changed physical anchor or parent");
    memcpy(peer->popup, p + 48, 16); peer->popup_scale = read_u64(p + 80);
    candidate(peer, 2, 2); epoch = presented(peer, 2); action(peer, 2, epoch, 1);
    candidate(peer, 3, 2); epoch = presented(peer, 3); action(peer, 3, epoch, 2);
    /* ACK is only receipt: the owner must withdraw, independent of peer pixels. */
    invalidated(peer, peer->popup);
    candidate(peer, 4, 1); (void)presented(peer, 4); invalidated(peer, peer->allocation);
}

int main(int argc, char **argv) {
    struct content_peer peer = {0};
    struct frame frame;
    uint8_t hello[12] = {0}, request[120] = {0}, begin[80] = {0};
    uint8_t chunk[56] = {0}, end[48] = {0};
    uint8_t *p;
    int full_lifecycle;
    struct timeval timeout = {5, 0};
    require(argc == 4 && (strcmp(argv[1], "content-proof") == 0 || strcmp(argv[1], "content-lifecycle") == 0) &&
            strcmp(argv[2], "--socket") == 0, "expected content-proof --socket PATH");
    full_lifecycle = strcmp(argv[1], "content-lifecycle") == 0;
    peer.fd = connect_when_ready(argv[3]);
    require(peer.fd >= 0, "connect failed");
    require(setsockopt(peer.fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout)) == 0 &&
            setsockopt(peer.fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout)) == 0,
            "socket deadline failed");
    write_u16(hello, 5); write_u16(hello + 2, 6); write_u64(hello + 4, full_lifecycle ? 385 : 129);
    require(send_frame(peer.fd, 96, 0, hello, sizeof(hello)) &&
            receive_frame(peer.fd, &frame), "hello/welcome failed");
    p = frame.bytes + FRAME_HEADER_LEN;
    require(frame.kind == 97 && frame.transaction == 0 && frame.payload_len == 28 &&
            read_u16(p) >= 5 && read_u16(p) <= 6 && !read_u16(p + 2) &&
            read_u64(p + 4) && (read_u64(p + 12) & (full_lifecycle ? 385u : 129u)) == (full_lifecycle ? 385u : 129u), "invalid welcome");
    write_u64(peer.grant, read_u64(p + 4));
    require(receive_frame(peer.fd, &frame), "limits missing");
    p = frame.bytes + FRAME_HEADER_LEN;
    require(frame.kind == 161 && frame.transaction == 0 && frame.payload_len == 264 &&
            memcmp(p, peer.grant, 8) == 0 && read_u64(p + 8), "invalid limits identity");
    memcpy(peer.grant, p, 16);
    content_receive(&peer, &frame, 162, 72);
    p = frame.bytes + FRAME_HEADER_LEN;
    require(read_u32(p + 24) == 1 && !read_u32(p + 28) &&
            read_u32(p + 48) == 64 && read_u32(p + 52) == 64 &&
            read_u32(p + 56) == 1 && read_u32(p + 60) == 1, "unexpected output facts");
    peer.facts = read_u64(p + 16); memcpy(peer.output, p + 32, 16);
    memcpy(request + 16, peer.output, 16); write_u64(request + 32, 1);
    write_u16(request + 40, 1); write_u16(request + 42, 1); write_u16(request + 44, 1);
    write_u32(request + 104, 64); write_u32(request + 108, full_lifecycle ? 16 : 32);
    content_send(&peer, 163, 3, request, sizeof(request));
    content_receive(&peer, &frame, 164, 160);
    p = frame.bytes + FRAME_HEADER_LEN;
    require(read_u64(p + 16) == 1 && read_u16(p + 24) == 1 && !read_u16(p + 26) &&
            memcmp(p + 32, peer.output, 16) == 0 && read_u32(p + 96) == 64 &&
            read_u32(p + 100) == (full_lifecycle ? 16u : 32u) && read_u32(p + 120) == 1 &&
            read_u32(p + 124) == 1, "panel grant changed");
    memcpy(peer.allocation, p + 48, 16); peer.scale = read_u64(p + 80);
    peer.reservation = read_u32(p + 128) < 24 ? read_u32(p + 128) : 24;
    write_u64(peer.resource, 1); write_u64(peer.resource + 8, 1);
    memcpy(begin + 16, peer.resource, 16); write_u32(begin + 32, 2);
    write_u32(begin + 36, 1); write_u32(begin + 40, 1); write_u32(begin + 44, 1);
    write_u16(begin + 48, 1); write_u32(begin + 52, 1); write_u64(begin + 56, 8);
    content_send(&peer, 165, 1, begin, 64);
    content_receive(&peer, &frame, 166, 48);
    require(read_u16(frame.bytes + FRAME_HEADER_LEN + 32) == 1 &&
            memcmp(frame.bytes + FRAME_HEADER_LEN + 16, peer.resource, 16) == 0,
            "resource not admitted");
    memcpy(chunk + 16, peer.resource, 16); write_u32(chunk + 36, 8);
    memcpy(chunk + 48, (uint8_t[]){0, 0, 255, 255, 0, 128, 0, 128}, 8);
    content_send(&peer, 167, 1, chunk, 56);
    memcpy(end + 16, peer.resource, 16); write_u64(end + 32, 8); write_u32(end + 40, 1);
    content_send(&peer, 168, 1, end, 48);
    content_receive(&peer, &frame, 166, 48);
    require(read_u16(frame.bytes + FRAME_HEADER_LEN + 32) == 2 &&
            memcmp(frame.bytes + FRAME_HEADER_LEN + 16, peer.resource, 16) == 0,
            "resource not accepted");
    candidate(&peer, 1, 1);
    if (full_lifecycle) { lifecycle(&peer); }
    else {
        content_receive(&peer, &frame, 175, 68); p = frame.bytes + FRAME_HEADER_LEN;
        require(read_u64(p + 16) == 1 && memcmp(p + 24, peer.output, 16) == 0 &&
                read_u16(p + 40) == 3 && read_u16(p + 42) == 9 && !read_u64(p + 44),
                "expected exact renderer-failure outcome, not simulated presentation");
    }
    memset(end, 0, sizeof(end)); memcpy(end + 16, peer.resource, 16);
    content_send(&peer, 170, 2, end, 32);
    content_receive(&peer, &frame, 171, 34); p = frame.bytes + FRAME_HEADER_LEN;
    require(memcmp(p + 16, peer.resource, 16) == 0 && !read_u16(p + 32), "release changed");
    close(peer.fd);
    puts("c_content_transport schema=1 status=complete protected_socket=true native_presentation=false");
    return 0;
}

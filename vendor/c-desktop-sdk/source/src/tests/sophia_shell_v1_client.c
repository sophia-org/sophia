#define _POSIX_C_SOURCE 200809L

#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

#define FRAME_HEADER_LEN 24u
#define FRAME_CAPACITY (FRAME_HEADER_LEN + 65536u)
#define MAX_DESCRIPTORS 16u

struct frame {
    uint16_t kind;
    uint64_t transaction;
    size_t payload_len;
    uint8_t bytes[FRAME_CAPACITY];
};

struct snapshot {
    uint64_t connection_epoch;
    uint64_t generation;
    uint64_t output;
    uint16_t count;
    uint16_t slots[MAX_DESCRIPTORS];
    uint64_t generations[MAX_DESCRIPTORS];
};

static uint16_t read_u16(const uint8_t *bytes) {
    return (uint16_t)bytes[0] | ((uint16_t)bytes[1] << 8);
}

static uint32_t read_u32(const uint8_t *bytes) {
    return (uint32_t)bytes[0] | ((uint32_t)bytes[1] << 8) |
        ((uint32_t)bytes[2] << 16) | ((uint32_t)bytes[3] << 24);
}

static uint64_t read_u64(const uint8_t *bytes) {
    return (uint64_t)read_u32(bytes) | ((uint64_t)read_u32(bytes + 4) << 32);
}

static void write_u16(uint8_t *bytes, uint16_t value) {
    bytes[0] = (uint8_t)value;
    bytes[1] = (uint8_t)(value >> 8);
}

static void write_u32(uint8_t *bytes, uint32_t value) {
    bytes[0] = (uint8_t)value;
    bytes[1] = (uint8_t)(value >> 8);
    bytes[2] = (uint8_t)(value >> 16);
    bytes[3] = (uint8_t)(value >> 24);
}

static void write_u64(uint8_t *bytes, uint64_t value) {
    write_u32(bytes, (uint32_t)value);
    write_u32(bytes + 4, (uint32_t)(value >> 32));
}

static int read_exact(int socket_fd, uint8_t *bytes, size_t length) {
    while (length > 0) {
        ssize_t received = read(socket_fd, bytes, length);
        if (received <= 0) return 0;
        bytes += (size_t)received;
        length -= (size_t)received;
    }
    return 1;
}

static int write_exact(int socket_fd, const uint8_t *bytes, size_t length) {
    while (length > 0) {
        ssize_t written = write(socket_fd, bytes, length);
        if (written <= 0) return 0;
        bytes += (size_t)written;
        length -= (size_t)written;
    }
    return 1;
}

static int receive_frame(int socket_fd, struct frame *frame) {
    uint32_t payload_len;
    if (!read_exact(socket_fd, frame->bytes, FRAME_HEADER_LEN)) return 0;
    if (memcmp(frame->bytes, "SOPH", 4u) != 0 || read_u16(frame->bytes + 4) != 1u ||
        read_u32(frame->bytes + 20) != 0u)
        return 0;
    frame->kind = read_u16(frame->bytes + 6);
    frame->transaction = read_u64(frame->bytes + 8);
    payload_len = read_u32(frame->bytes + 16);
    if (payload_len > 65536u) return 0;
    frame->payload_len = payload_len;
    return read_exact(socket_fd, frame->bytes + FRAME_HEADER_LEN, payload_len);
}

static int send_frame(
    int socket_fd,
    uint16_t kind,
    uint64_t transaction,
    const uint8_t *payload,
    size_t payload_len
) {
    uint8_t bytes[FRAME_CAPACITY];
    if (payload_len > 65536u) return 0;
    memcpy(bytes, "SOPH", 4u);
    write_u16(bytes + 4, 1u);
    write_u16(bytes + 6, kind);
    write_u64(bytes + 8, transaction);
    write_u32(bytes + 16, (uint32_t)payload_len);
    write_u32(bytes + 20, 0u);
    memcpy(bytes + FRAME_HEADER_LEN, payload, payload_len);
    return write_exact(socket_fd, bytes, FRAME_HEADER_LEN + payload_len);
}

static int connect_when_ready(const char *path) {
    struct sockaddr_un address;
    struct timespec pause = {0, 10000000};
    int attempt;
    if (strlen(path) >= sizeof(address.sun_path)) return -1;
    memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
    memcpy(address.sun_path, path, strlen(path) + 1u);
    for (attempt = 0; attempt < 200; ++attempt) {
        int socket_fd = socket(AF_UNIX, SOCK_STREAM, 0);
        if (socket_fd < 0) return -1;
        if (connect(socket_fd, (struct sockaddr *)&address, sizeof(address)) == 0)
            return socket_fd;
        close(socket_fd);
        if (errno != ENOENT && errno != ECONNREFUSED) return -1;
        nanosleep(&pause, NULL);
    }
    return -1;
}

static int send_hello(int socket_fd) {
    uint8_t payload[12] = {0};
    write_u16(payload, 1u);
    write_u16(payload + 2, 1u);
    write_u64(payload + 4, UINT64_C(1));
    return send_frame(socket_fd, 96u, 0u, payload, sizeof(payload));
}

static int receive_welcome(int socket_fd, uint64_t *connection_epoch) {
    struct frame frame;
    const uint8_t *payload;
    if (!receive_frame(socket_fd, &frame) || frame.kind != 97u ||
        frame.transaction != 0u || frame.payload_len != 28u)
        return 0;
    payload = frame.bytes + FRAME_HEADER_LEN;
    *connection_epoch = read_u64(payload + 4);
    return read_u16(payload) == 1u && read_u16(payload + 2) == 0u &&
        *connection_epoch != 0u && (read_u64(payload + 12) & UINT64_C(1)) != 0u &&
        read_u16(payload + 20) > 0u && read_u16(payload + 20) <= MAX_DESCRIPTORS &&
        read_u16(payload + 22) > 0u && read_u16(payload + 22) <= 128u &&
        read_u16(payload + 24) > 0u && read_u16(payload + 26) == 0u;
}

static int receive_snapshot(
    int socket_fd,
    uint64_t expected_epoch,
    struct snapshot *snapshot,
    uint64_t *transaction
) {
    struct frame frame;
    const uint8_t *payload;
    uint64_t broker_epoch;
    uint64_t revocation_epoch;
    size_t offset = 52u;
    size_t index;
    if (!receive_frame(socket_fd, &frame) || frame.kind != 98u ||
        frame.transaction == 0u || frame.payload_len < offset)
        return 0;
    payload = frame.bytes + FRAME_HEADER_LEN;
    snapshot->connection_epoch = read_u64(payload);
    snapshot->generation = read_u64(payload + 8);
    snapshot->output = read_u64(payload + 16);
    broker_epoch = read_u64(payload + 32);
    revocation_epoch = read_u64(payload + 40);
    snapshot->count = read_u16(payload + 48);
    if (snapshot->connection_epoch != expected_epoch || snapshot->generation == 0u ||
        snapshot->output == 0u || read_u64(payload + 24) == 0u || broker_epoch == 0u ||
        revocation_epoch == 0u || snapshot->count > MAX_DESCRIPTORS ||
        read_u16(payload + 50) != 0u)
        return 0;
    for (index = 0; index < snapshot->count; ++index) {
        uint16_t label_len;
        size_t prior;
        size_t other;
        if (frame.payload_len - offset < 60u) return 0;
        snapshot->slots[index] = read_u16(payload + offset);
        snapshot->generations[index] = read_u64(payload + offset + 4);
        label_len = read_u16(payload + offset + 57);
        if (snapshot->slots[index] == 0u || snapshot->generations[index] == 0u ||
            payload[offset + 2] > 3u || payload[offset + 3] > 2u ||
            read_u64(payload + offset + 12) == 0u ||
            read_u64(payload + offset + 20) != broker_epoch ||
            read_u64(payload + offset + 28) != revocation_epoch ||
            read_u64(payload + offset + 36) != expected_epoch ||
            read_u16(payload + offset + 44) != snapshot->slots[index] ||
            read_u16(payload + offset + 46) != 0u ||
            read_u64(payload + offset + 48) != snapshot->generations[index] ||
            payload[offset + 56] > 1u || label_len > 128u ||
            (payload[offset + 56] == 0u && label_len != 0u))
            return 0;
        for (prior = 0; prior < index; ++prior)
            if (snapshot->slots[prior] == snapshot->slots[index]) return 0;
        offset += 59u;
        if (frame.payload_len - offset < (size_t)label_len + 1u) return 0;
        for (other = 0; other < label_len; ++other)
            if (payload[offset + other] < 0x20u || payload[offset + other] == 0x7fu)
                return 0;
        if (payload[offset + label_len] > 1u ||
            (payload[offset + 56u - 59u] == 0u && payload[offset + label_len] != 0u))
            return 0;
        offset += (size_t)label_len + 1u;
    }
    if (offset != frame.payload_len) return 0;
    *transaction = frame.transaction;
    return 1;
}

static int send_candidate(
    int socket_fd,
    uint64_t transaction,
    const struct snapshot *snapshot,
    uint64_t candidate_generation,
    int visible,
    uint8_t reservation_edge,
    uint16_t reservation_thickness
) {
    uint8_t payload[40u + MAX_DESCRIPTORS * 12u] = {0};
    size_t index;
    size_t length = 40u + (visible ? (size_t)snapshot->count * 12u : 0u);
    if (transaction == 0u || candidate_generation == 0u ||
        (visible && snapshot->count == 0u))
        return 0;
    /* A reservation belongs to a visible candidate, and its edge and
     * thickness are present together or absent together. */
    if ((reservation_edge == 0u) != (reservation_thickness == 0u) ||
        (reservation_edge != 0u && (!visible || reservation_edge > 4u)))
        return 0;
    write_u64(payload, snapshot->connection_epoch);
    write_u64(payload + 8, snapshot->generation);
    write_u64(payload + 16, candidate_generation);
    write_u64(payload + 24, snapshot->output);
    payload[32] = visible ? 1u : 0u;
    payload[33] = reservation_edge;
    write_u16(payload + 34, visible ? snapshot->slots[0] : 0u);
    write_u16(payload + 36, visible ? snapshot->count : 0u);
    write_u16(payload + 38, reservation_thickness);
    for (index = 0; visible && index < snapshot->count; ++index) {
        write_u16(payload + 40u + index * 12u, snapshot->slots[index]);
        write_u64(payload + 44u + index * 12u, snapshot->generations[index]);
    }
    return send_frame(socket_fd, 99u, transaction, payload, length);
}

static int receive_outcome(
    int socket_fd,
    uint64_t expected_transaction,
    uint64_t connection_epoch,
    uint64_t candidate_generation,
    uint16_t expected_kind,
    uint64_t *presentation_epoch
) {
    struct frame frame;
    const uint8_t *payload;
    if (!receive_frame(socket_fd, &frame) || frame.kind != 100u ||
        frame.transaction != expected_transaction || frame.payload_len != 28u)
        return 0;
    payload = frame.bytes + FRAME_HEADER_LEN;
    *presentation_epoch = read_u64(payload + 16);
    return read_u64(payload) == connection_epoch &&
        read_u64(payload + 8) == candidate_generation &&
        read_u16(payload + 24) == expected_kind && read_u16(payload + 26) == 0u &&
        ((expected_kind == 2u) == (*presentation_epoch != 0u));
}

static int receive_activation_and_ack(
    int socket_fd,
    uint64_t connection_epoch,
    uint64_t candidate_generation,
    uint64_t presentation_epoch,
    uint16_t selected_slot,
    uint64_t selected_generation
) {
    struct frame frame;
    const uint8_t *payload;
    uint8_t ack[20] = {0};
    uint64_t activation;
    if (!receive_frame(socket_fd, &frame) || frame.kind != 101u ||
        frame.transaction == 0u || frame.payload_len != 76u)
        return 0;
    payload = frame.bytes + FRAME_HEADER_LEN;
    activation = read_u64(payload + 24);
    if (read_u64(payload) != connection_epoch ||
        read_u64(payload + 8) != candidate_generation ||
        read_u64(payload + 16) != presentation_epoch || activation == 0u ||
        read_u64(payload + 32) == 0u || read_u64(payload + 56) != connection_epoch ||
        read_u16(payload + 64) != selected_slot || read_u16(payload + 66) != 0u ||
        read_u64(payload + 68) != selected_generation)
        return 0;
    write_u64(ack, connection_epoch);
    write_u64(ack + 8, activation);
    write_u16(ack + 16, 1u);
    return send_frame(socket_fd, 102u, frame.transaction, ack, sizeof(ack));
}

static int run_proof(const char *socket_path) {
    struct snapshot first;
    struct snapshot second;
    uint64_t connection_epoch;
    uint64_t first_transaction;
    uint64_t second_transaction;
    uint64_t presentation_epoch;
    uint64_t ignored_epoch;
    int socket_fd = connect_when_ready(socket_path);
    if (socket_fd < 0 || !send_hello(socket_fd) ||
        !receive_welcome(socket_fd, &connection_epoch) ||
        !receive_snapshot(socket_fd, connection_epoch, &first, &first_transaction) ||
        !send_candidate(socket_fd, first_transaction, &first, 1u, 1, 2u, 28u) ||
        !receive_outcome(socket_fd, first_transaction, connection_epoch, 1u, 1u, &ignored_epoch) ||
        !receive_outcome(
            socket_fd, first_transaction, connection_epoch, 1u, 2u, &presentation_epoch
        ) ||
        !receive_activation_and_ack(
            socket_fd, connection_epoch, 1u, presentation_epoch,
            first.slots[0], first.generations[0]
        ) ||
        !receive_snapshot(socket_fd, connection_epoch, &second, &second_transaction) ||
        !send_candidate(socket_fd, second_transaction, &second, 2u, 0, 0u, 0u) ||
        !receive_outcome(
            socket_fd, second_transaction, connection_epoch, 2u, 1u, &ignored_epoch
        ) ||
        !receive_outcome(
            socket_fd, second_transaction, connection_epoch, 2u, 2u, &ignored_epoch
        )) {
        if (socket_fd >= 0) close(socket_fd);
        return 0;
    }
    close(socket_fd);
    printf(
        "sophia_shell_c_proof schema=1 status=complete descriptors=%u "
        "activations=1 withdrawn=true\n",
        (unsigned)first.count
    );
    return 1;
}

int main(int argc, char **argv) {
    const char *socket_path;
    if (argc != 2 || strcmp(argv[1], "--proof") != 0) {
        fputs("sophia-shell-c: only --proof is supported\n", stderr);
        return 2;
    }
    socket_path = getenv("SOPHIA_SHELL_SOCKET");
    if (socket_path == NULL || socket_path[0] == '\0') {
        fputs("sophia-shell-c: SOPHIA_SHELL_SOCKET is required\n", stderr);
        return 2;
    }
    if (!run_proof(socket_path)) {
        fputs("sophia-shell-c: conformance proof failed\n", stderr);
        return 1;
    }
    return 0;
}

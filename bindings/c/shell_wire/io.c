#include "fields.h"
#include <errno.h>
#include <sys/socket.h>

static int disjoint(const void *a, size_t an, const void *b, size_t bn)
{
    uintptr_t x = (uintptr_t)a, y = (uintptr_t)b;
    return an <= UINTPTR_MAX - x && bn <= UINTPTR_MAX - y && (x + an <= y || y + bn <= x);
}

int sophia_shell_wire_init(struct sophia_shell_wire *wire, int fd,
                          uint8_t *rx, size_t rx_capacity, uint8_t *tx, size_t tx_capacity)
{
    if (!wire || fd < 0 || !rx || !tx || rx_capacity < SOPHIA_SHELL_HEADER_BYTES ||
        tx_capacity < SOPHIA_SHELL_HEADER_BYTES || rx_capacity > SOPHIA_SHELL_MAX_FRAME_BYTES ||
        tx_capacity > SOPHIA_SHELL_MAX_FRAME_BYTES || !disjoint(rx, rx_capacity, tx, tx_capacity) ||
        !disjoint(wire, sizeof(*wire), rx, rx_capacity) || !disjoint(wire, sizeof(*wire), tx, tx_capacity))
        return SOPHIA_SHELL_ARGUMENT;
    *wire = (struct sophia_shell_wire){.fd = fd, .rx = rx, .tx = tx,
        .rx_capacity = rx_capacity, .tx_capacity = tx_capacity,
        .rx_needed = SOPHIA_SHELL_HEADER_BYTES};
    return SOPHIA_SHELL_OK;
}

int sophia_shell_wire_queue(struct sophia_shell_wire *wire, uint16_t kind,
                           uint64_t transaction, const void *payload, size_t bytes)
{
    if (!wire)
        return SOPHIA_SHELL_ARGUMENT;
    if (wire->terminal)
        return wire->terminal;
    if (wire->tx_used)
        return SOPHIA_SHELL_BUSY;
    if (shell_direction(kind) != 1)
        return SOPHIA_SHELL_INVALID;
    return sophia_shell_frame_encode(wire->tx, wire->tx_capacity, kind, transaction,
                                    payload, bytes, &wire->tx_used);
}

static int io_failure(struct sophia_shell_wire *wire)
{
    wire->terminal = SOPHIA_SHELL_IO_ERROR;
    return wire->terminal;
}

int sophia_shell_wire_flush(struct sophia_shell_wire *wire, size_t byte_budget)
{
    if (!wire)
        return SOPHIA_SHELL_ARGUMENT;
    if (wire->terminal)
        return wire->terminal;
    for (unsigned calls = 0; wire->tx_used && byte_budget && calls < SOPHIA_SHELL_MAX_IO_CALLS; ++calls) {
        size_t amount = wire->tx_used - wire->tx_sent;
        if (amount > byte_budget)
            amount = byte_budget;
        ssize_t sent = send(wire->fd, wire->tx + wire->tx_sent, amount, MSG_DONTWAIT | MSG_NOSIGNAL);
        if (sent < 0) {
            if (errno == EINTR)
                continue;
            if (errno == EAGAIN || errno == EWOULDBLOCK)
                return SOPHIA_SHELL_AGAIN;
            return io_failure(wire);
        }
        if (!sent)
            return io_failure(wire);
        wire->tx_sent += (size_t)sent;
        byte_budget -= (size_t)sent;
        if (wire->tx_sent == wire->tx_used) {
            wire->tx_used = wire->tx_sent = 0;
            return SOPHIA_SHELL_OK;
        }
    }
    return wire->tx_used ? SOPHIA_SHELL_AGAIN : SOPHIA_SHELL_OK;
}

int sophia_shell_wire_receive(struct sophia_shell_wire *wire, size_t byte_budget,
                             struct sophia_shell_frame *frame)
{
    if (!wire || !frame)
        return SOPHIA_SHELL_ARGUMENT;
    if (wire->terminal)
        return wire->terminal;
    for (unsigned calls = 0;;) {
        if (wire->rx_used >= SOPHIA_SHELL_HEADER_BYTES) {
            if (shell_header(wire->rx, wire->rx_capacity, &wire->rx_needed) ||
                shell_direction(shell_get16(wire->rx + 6)) != 0) {
                wire->terminal = SOPHIA_SHELL_INVALID;
                return wire->terminal;
            }
            if (wire->rx_used == wire->rx_needed) {
                int result = sophia_shell_frame_decode(wire->rx, wire->rx_used, frame);
                return result == SOPHIA_SHELL_OK ? SOPHIA_SHELL_FRAME : result;
            }
        }
        if (!byte_budget || calls == SOPHIA_SHELL_MAX_IO_CALLS)
            return SOPHIA_SHELL_AGAIN;
        size_t amount = wire->rx_needed - wire->rx_used;
        if (amount > byte_budget)
            amount = byte_budget;
        ++calls;
        ssize_t received = recv(wire->fd, wire->rx + wire->rx_used, amount, MSG_DONTWAIT);
        if (received < 0) {
            if (errno == EINTR)
                continue;
            if (errno == EAGAIN || errno == EWOULDBLOCK)
                return SOPHIA_SHELL_AGAIN;
            return io_failure(wire);
        }
        if (!received) {
            wire->terminal = wire->rx_used ? SOPHIA_SHELL_INVALID : SOPHIA_SHELL_CLOSED;
            return wire->terminal;
        }
        wire->rx_used += (size_t)received;
        byte_budget -= (size_t)received;
    }
}

int sophia_shell_wire_consume(struct sophia_shell_wire *wire)
{
    if (!wire)
        return SOPHIA_SHELL_ARGUMENT;
    if (wire->terminal)
        return wire->terminal;
    if (wire->rx_used < SOPHIA_SHELL_HEADER_BYTES || wire->rx_used != wire->rx_needed)
        return SOPHIA_SHELL_AGAIN;
    wire->rx_used = 0;
    wire->rx_needed = SOPHIA_SHELL_HEADER_BYTES;
    return SOPHIA_SHELL_OK;
}

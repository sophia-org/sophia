#ifndef SOPHIA_SHELL_OUTBOX_H
#define SOPHIA_SHELL_OUTBOX_H
#include "sophia_shell_wire.h"
#ifdef __cplusplus
extern "C" {
#endif
#define SOPHIA_SHELL_OUTBOX_MAX_RECORDS 64u
#define SOPHIA_SHELL_OUTBOX_MAX_BYTES 262144u
#define SOPHIA_SHELL_OUTBOX_CONTROL_BYTES 256u

enum sophia_shell_outbound_class {
    SOPHIA_SHELL_OUTBOUND_BULK = 0,
    SOPHIA_SHELL_OUTBOUND_CONTROL = 1
};
struct sophia_shell_outbound_frame {
    const uint8_t *bytes;
    size_t length;
    enum sophia_shell_outbound_class class;
};
struct sophia_shell_outbox_record {
    uint8_t *bytes;
    size_t length, sent;
    enum sophia_shell_outbound_class class;
    int ready;
};
struct sophia_shell_outbox_reservation {
    struct sophia_shell_outbox *owner;
    uint64_t serial;
};
/* One serial connection owner; initialize zeroed/fresh storage once. Fields are
 * private state exposed for C allocation only. The fixed record table and every
 * copied frame are owned here until its final byte or explicit disconnect.
 * Do not also use wire_queue/flush on this socket: there must be ONE writer. */
struct sophia_shell_outbox {
    struct sophia_shell_outbox_record records[SOPHIA_SHELL_OUTBOX_MAX_RECORDS];
    size_t max_bytes, reserve_bytes, bytes, bulk_bytes;
    unsigned max_records, reserve_records, head, count, bulk_records;
    int terminal;
    uint64_t reservation_serial;
    unsigned reservation_index, reservation_count;
};
/* Caller chooses bounds within negotiated limits. Reservations are a subset of
 * this one aggregate budget. At least two 256-byte control records are reserved.
 * Return ARGUMENT without changing out for incoherent settings. No allocation. */
int sophia_shell_outbox_init(struct sophia_shell_outbox *out, size_t max_bytes,
    unsigned max_records, size_t reserve_bytes, unsigned reserve_records);
/* Own one frame, or an atomic ordered pair (e.g. native ACK then Activate).
 * Complete envelope/direction/class validated before allocation; payload must
 * already be encoded by its typed codec. BUSY or allocation failure preserves
 * every prior record and copies neither member. OK transfers copies, not the
 * caller's buffers. No I/O or callbacks during the commit. */
int sophia_shell_outbox_push(struct sophia_shell_outbox *out,
    const struct sophia_shell_outbound_frame *frames, unsigned count);
/* Reserve one or two exact-size control frames BEFORE applying a local input
 * effect. One reservation may be open. It owns FIFO positions and the same
 * aggregate bytes/records as encoded frames; flush stops at an uncommitted
 * position. Failure preserves ticket and queue. The ticket cannot outlive this
 * initialized owner; dispose invalidates it, never reuse it on reconnect. */
int sophia_shell_outbox_reserve(struct sophia_shell_outbox *out,
    const size_t *lengths, unsigned count, struct sophia_shell_outbox_reservation *ticket);
/* Commit exact frame sizes/count to that reservation without allocation or I/O.
 * Refusal retains the reservation for exact retry; no frame becomes visible.
 * Caller must not reapply its effect after a refused commit. */
int sophia_shell_outbox_commit(struct sophia_shell_outbox *out,
    struct sophia_shell_outbox_reservation ticket,
    const struct sophia_shell_outbound_frame *frames, unsigned count);
/* Sole socket writer. MSG_DONTWAIT/NOSIGNAL, bounded by bytes and 32 syscalls
 * (including EINTR). No record can overtake a partial frame. Full record/byte
 * charges stay live until its final byte; OK is kernel write, not peer receipt.
 * Error latches but retains all unsent/partial owners until disconnect disposal. */
int sophia_shell_outbox_flush(struct sophia_shell_outbox *out, int fd, size_t byte_budget);
/* Only after the caller has ended this connection; never a resource-release or
 * ACK receipt. Frees exact queued buffers and resets storage to zero. */
void sophia_shell_outbox_dispose(struct sophia_shell_outbox *out);
#ifdef __cplusplus
}
#endif
#endif

#ifndef SOPHIA_SHELL_WIRE_H
#define SOPHIA_SHELL_WIRE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define SOPHIA_SHELL_HEADER_BYTES 24u
#define SOPHIA_SHELL_MAX_PAYLOAD_BYTES 65536u
#define SOPHIA_SHELL_MAX_FRAME_BYTES (SOPHIA_SHELL_HEADER_BYTES + SOPHIA_SHELL_MAX_PAYLOAD_BYTES)
#define SOPHIA_SHELL_MAX_IO_CALLS 32u
#define SOPHIA_SHELL_WIRE_MAX_REVISION 7u
#define SOPHIA_SHELL_CAP_DESCRIPTOR_SWITCHER (UINT64_C(1) << 0)
#define SOPHIA_SHELL_CAP_WORK_AREA_RESERVATION (UINT64_C(1) << 1)
#define SOPHIA_SHELL_CAP_TAB_GROUPS (UINT64_C(1) << 2)
#define SOPHIA_SHELL_CAP_SHORTCUT_CATALOG (UINT64_C(1) << 3)
#define SOPHIA_SHELL_CAP_REFERENCE_SHEET (UINT64_C(1) << 4)
#define SOPHIA_SHELL_CAP_APPLICATION_CATALOG (UINT64_C(1) << 5)
#define SOPHIA_SHELL_CAP_APPLICATION_LAUNCHER (UINT64_C(1) << 6)
#define SOPHIA_SHELL_CAP_CONTENT_SURFACE (UINT64_C(1) << 7)
#define SOPHIA_SHELL_CAP_CONTENT_DISCRETE_INPUT (UINT64_C(1) << 8)
#define SOPHIA_SHELL_CAP_VIEW_INDICATORS (UINT64_C(1) << 9)
#define SOPHIA_SHELL_CAP_INDICATOR_ACTIVATION (UINT64_C(1) << 10)
#define SOPHIA_SHELL_CAP_NATIVE_LAUNCHER (UINT64_C(1) << 11)

enum sophia_shell_wire_result {
    SOPHIA_SHELL_OK = 0,
    SOPHIA_SHELL_FRAME = 1,
    SOPHIA_SHELL_AGAIN = 2,
    SOPHIA_SHELL_BUSY = 3,
    SOPHIA_SHELL_CLOSED = 4,
    SOPHIA_SHELL_INVALID = -1,
    SOPHIA_SHELL_IO_ERROR = -2,
    SOPHIA_SHELL_ARGUMENT = -3
};

struct sophia_shell_frame {
    uint16_t kind;
    uint64_t transaction;
    const uint8_t *payload;
    size_t payload_bytes;
};

/* Framing only: validates the shell kind, transaction class and envelope, NOT
 * the message payload or authority. Output arguments stay unchanged on error. */
int sophia_shell_frame_encode(uint8_t *dst, size_t capacity, uint16_t kind,
                             uint64_t transaction, const void *payload,
                             size_t payload_bytes, size_t *frame_bytes);
int sophia_shell_frame_decode(const uint8_t *src, size_t bytes,
                             struct sophia_shell_frame *frame);

/* Single-threaded caller-owned storage; no allocation, connect, close or fd
 * flags changed. fd must be a caller-authenticated connected stream socket.
 * rx/tx must be disjoint from each other and this struct and outlive it.
 * Members are private state, exposed only to allow allocation without an ABI
 * allocator. Initialize a fresh instance/storage for a fresh connection. */
struct sophia_shell_wire {
    int fd, terminal;
    uint8_t *rx, *tx;
    size_t rx_capacity, tx_capacity;
    size_t rx_used, rx_needed, tx_used, tx_sent;
};

int sophia_shell_wire_init(struct sophia_shell_wire *wire, int fd,
                          uint8_t *rx, size_t rx_capacity,
                          uint8_t *tx, size_t tx_capacity);
/* One outbound frame: BUSY preserves the original frame, including after a
 * partial write. Queue copies payload; caller may release its input on OK.
 * Only shell-to-Session kinds are accepted. No payload/epoch validation here. */
int sophia_shell_wire_queue(struct sophia_shell_wire *wire, uint16_t kind,
                           uint64_t transaction, const void *payload, size_t bytes);
/* Each call transfers at most byte_budget bytes and makes at most 32 syscalls,
 * including EINTR. MSG_DONTWAIT/MSG_NOSIGNAL avoid blocking/SIGPIPE. Zero budget
 * does no I/O. OK means final byte handed to the kernel, NOT peer receipt. */
int sophia_shell_wire_flush(struct sophia_shell_wire *wire, size_t byte_budget);
/* Reads exactly one Session-to-shell frame. FRAME borrows rx until consume.
 * No next-frame read while it is outstanding. Terminal EOF, truncation, framing
 * or I/O errors latch; pending bytes remain owned until caller disposes them. */
int sophia_shell_wire_receive(struct sophia_shell_wire *wire, size_t byte_budget,
                             struct sophia_shell_frame *frame);
int sophia_shell_wire_consume(struct sophia_shell_wire *wire);

struct sophia_shell_hello {
    uint16_t minimum_revision, maximum_revision;
    uint64_t required_capabilities;
};

struct sophia_shell_welcome {
    uint16_t revision;
    uint64_t connection_epoch, capabilities;
    uint16_t max_descriptors, max_label_bytes, max_pending_activations;
};

/* Revision 1-7 vocabulary. The caller selects required capabilities explicitly.
 * Native launcher requires revision 7, catalog, content and discrete input.
 * Runtime availability is separate; this library does not enable a provider.
 * This validates structure, requested revision/capabilities and dependencies;
 * it does not authenticate the peer, grant content, or implement its lifecycle. */
int sophia_shell_hello_encode(uint8_t *dst, size_t capacity,
                             struct sophia_shell_hello hello, size_t *frame_bytes);
int sophia_shell_welcome_decode(const struct sophia_shell_frame *frame,
                               struct sophia_shell_hello requested,
                               struct sophia_shell_welcome *welcome);

#ifdef __cplusplus
}
#endif
#endif

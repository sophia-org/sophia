#ifndef SOPHIA_SHELL_FILES_CLIENT_H
#define SOPHIA_SHELL_FILES_CLIENT_H
#include "sophia_9p_client.h"
#include "sophia_shell_files.h"

/* The caller explicitly chooses this native file backend and supplies the
 * admitted fd through wire. No discovery, protocol sniffing or fallback.
 * Role-aware initialization supports bar, native launcher and persistent dock. */
enum sophia_sf_profile { SOPHIA_SF_BAR, SOPHIA_SF_LAUNCHER, SOPHIA_SF_DOCK };
struct sophia_sf_operation {
    struct sophia_9p_handle handle;
    uint8_t active;
};
struct sophia_sf_client {
    struct sophia_9p_client *wire;
    uint64_t epoch, next_submission, sequence, consumed_sequence, acked_sequence;
    uint64_t event_offset, ack_pending, object_generation, object_qid;
    struct sophia_sf_negotiate offer;
    enum sophia_sf_profile profile;
    uint8_t *object_storage;
    size_t object_capacity;
    struct sophia_sf_limits limits;
    uint32_t root, fids[4], object_fid, upload_fid, api_fid, api_iounit;
    uint32_t iounit[4], object_iounit, upload_iounit;
    uint16_t object_kind;
    int terminal, object_status;
    uint8_t bootstrap, negotiated, have_limits, event_ready, object_ready;
    uint8_t submit_stage, submitted, submit_replied, object_stage, upload_stage, refused,
        upload_closing;
    size_t tx_size, tx_offset, event_used, object_used, upload_sent, upload_size, api_used;
    uint64_t upload_offset;
    uint16_t upload_slot;
    uint32_t remote_error;
    struct sophia_sf_resource_begin resource;
    struct sophia_sf_record event, object;
    struct sophia_sf_operation boot_op, event_op, submit_op, ack_op, object_op, upload_op;
    uint8_t tx[8192], event_bytes[1024], object_bytes[1024];
    /* An upload chunk is borrowed until its Twrite reply is processed. */
    const uint8_t *upload_data;
};
/* wire must be fresh and exclusively owned by this client until disposal.
 * The epoch is learned from api after attach. The caller supplies its offer.
 * Initialization starts version/attach/api/setup/Negotiate, all nonblocking. */
int sophia_sf_client_init(struct sophia_sf_client *, struct sophia_9p_client *,
                          struct sophia_sf_negotiate);
/* storage is exclusive, caller-owned object scratch (at most 4 MiB), borrowed
 * until disposal; it must not overlap client or wire storage. Decoded row/text
 * views survive until the next object fetch.
 * NULL with capacity 0 selects the inline 1 KiB buffer. The base initializer
 * selects BAR with that buffer. Launcher/dock offers require their exact masks
 * and a revision range containing 7/8. No implicit role/protocol fallback. */
int sophia_sf_client_init_profile(struct sophia_sf_client *, struct sophia_9p_client *,
                                  struct sophia_sf_negotiate, enum sophia_sf_profile, void *storage,
                                  size_t capacity);
int sophia_sf_client_service(struct sophia_sf_client *, size_t byte_budget);
int sophia_sf_client_ready(const struct sophia_sf_client *);
/* Copies one whole value; header epoch/submission are assigned here. Return
 * BUSY preserves caller ownership. Submitted means custody only. */
int sophia_sf_client_submit(struct sophia_sf_client *, const struct sophia_sf_record *);
/* Borrow until event_consume. Consumption advances local processing; ack is a
 * separate explicit operation releasing only server journal retention. */
int sophia_sf_client_event(struct sophia_sf_client *, const struct sophia_sf_record **);
int sophia_sf_client_event_consume(struct sophia_sf_client *);
int sophia_sf_client_ack(struct sophia_sf_client *);
/* Fetch the announced object through a fresh pin; compare generation and qid.
 * A superseded announcement returns AGAIN via object_result, ready to retry. */
int sophia_sf_client_object(struct sophia_sf_client *, uint16_t kind, uint64_t generation,
                            uint64_t qid);
int sophia_sf_client_object_result(struct sophia_sf_client *, const struct sophia_sf_record **);
/* Begin copies its record; wait for status 1 before opening upload/N. Supply
 * one bounded chunk at a time, advancing by positive short Rwrite counts.
 * End is allowed only after all declared bytes are acknowledged. Cancel waits
 * for the writer to open and any writes to finish, preserving their order. */
int sophia_sf_client_upload_begin(struct sophia_sf_client *, struct sophia_sf_resource_begin);
int sophia_sf_client_upload_chunk(struct sophia_sf_client *, const void *, size_t);
/* Chunk storage may be released when ready becomes true or pending becomes
 * false. A terminal session also releases every borrowed chunk. */
int sophia_sf_client_upload_ready(const struct sophia_sf_client *);
int sophia_sf_client_upload_pending(const struct sophia_sf_client *);
int sophia_sf_client_upload_end(struct sophia_sf_client *, uint64_t transaction);
int sophia_sf_client_upload_cancel(struct sophia_sf_client *, uint64_t transaction);
#endif

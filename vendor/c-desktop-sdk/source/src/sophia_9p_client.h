#ifndef SOPHIA_9P_CLIENT_H
#define SOPHIA_9P_CLIENT_H
#include <stddef.h>
#include <stdint.h>

#define SOPHIA_9P_REQUESTS 32u
#define SOPHIA_9P_FIDS 128u
#define SOPHIA_9P_NOTAG UINT16_MAX
#define SOPHIA_9P_NOFID UINT32_MAX
enum sophia_9p_result {
    SOPHIA_9P_OK = 0,
    SOPHIA_9P_AGAIN = 1,
    SOPHIA_9P_BUSY = 2,
    SOPHIA_9P_INVALID = -1,
    SOPHIA_9P_IO = -2,
    SOPHIA_9P_CLOSED = -3,
    SOPHIA_9P_ARGUMENT = -4
};
struct sophia_9p_handle {
    uint64_t serial;
    uint16_t slot;
};
struct sophia_9p_qid {
    uint64_t path;
    uint32_t version;
    uint8_t type;
};
struct sophia_9p_attr {
    uint64_t valid, nlink, size;
    struct sophia_9p_qid qid;
    uint32_t mode;
};
struct sophia_9p_dirent {
    struct sophia_9p_qid qid;
    uint64_t next;
    uint8_t type;
    uint16_t name_bytes;
    const uint8_t *name; /* Borrowed until reply consumption; not NUL-terminated. */
};
struct sophia_9p_reply {
    struct sophia_9p_handle handle;
    uint8_t type;
    uint32_t count, error, iounit;
    struct sophia_9p_qid qid;
    /* Borrowed until consume: read/directory/attribute bytes or a walk qid table. */
    const uint8_t *data;
};
/* Private state exposed for caller-owned allocation. Do not modify it. */
struct sophia_9p_slot {
    uint64_t serial, completed, offset;
    size_t bytes, sent;
    uint32_t fid, count;
    uint16_t tag, old_slot;
    uint8_t type, state, flushing, consumed, secret;
};
struct sophia_9p_client {
    int fd, terminal;
    uint32_t offered, msize, next_fid;
    uint16_t next_tag, capacity;
    uint64_t serial, completed;
    uint8_t phase;
    uint8_t *storage, *rx;
    size_t rx_used, rx_needed;
    struct sophia_9p_slot slots[SOPHIA_9P_REQUESTS * 2];
    uint32_t fids[SOPHIA_9P_FIDS];
    uint16_t fid_count, fid_limit;
};
/* One msize receive buffer and two msize slots per ordinary request. Every
 * admitted request owns completion storage and its own reserved flush slot.
 * No allocator, connect/close, descriptor flag changes, or ambient endpoints.
 * fd and disjoint storage must outlive this single-threaded instance. */
size_t sophia_9p_storage_bytes(uint32_t msize, uint16_t requests);
int sophia_9p_init(struct sophia_9p_client *, int fd, uint32_t msize, uint16_t requests,
                   uint16_t fids, void *storage, size_t bytes);
int sophia_9p_version(struct sophia_9p_client *, struct sophia_9p_handle *);
/* Tauth reserves an afid, not authority. Rauth must carry QTAUTH. Use ordinary
 * read/write on that afid for the export's conversation, then explicit attach.
 * Claims use n_uname=NOUID. No credential or environment lookup is performed. */
int sophia_9p_auth(struct sophia_9p_client *, const char *uname, const char *aname,
                   struct sophia_9p_handle *, uint32_t *afid);
int sophia_9p_attach_with_afid(struct sophia_9p_client *, uint32_t afid,
                              const char *uname, const char *aname,
                              struct sophia_9p_handle *, uint32_t *fid);
int sophia_9p_attach(struct sophia_9p_client *, const char *uname, const char *aname,
                     struct sophia_9p_handle *, uint32_t *fid);
int sophia_9p_walk(struct sophia_9p_client *, uint32_t from, const char *const *names, size_t count,
                   struct sophia_9p_handle *, uint32_t *fid);
int sophia_9p_lopen(struct sophia_9p_client *, uint32_t fid, uint32_t flags,
                    struct sophia_9p_handle *);
int sophia_9p_read(struct sophia_9p_client *, uint32_t fid, uint64_t offset, uint32_t count,
                   struct sophia_9p_handle *);
/* Requests all basic attributes (mask 0x3fff); only fields named by valid may
 * be used. Unknown/unsupported attributes remain the server's decision. */
int sophia_9p_getattr(struct sophia_9p_client *, uint32_t fid, struct sophia_9p_handle *);
/* Copy passive fields out of a retained Rgetattr reply. Unlike the reply's
 * borrowed bytes, the returned value survives reply consumption. */
int sophia_9p_attributes(const struct sophia_9p_reply *, struct sophia_9p_attr *);
/* count is capped to one negotiated reply. Entries have strictly increasing
 * cookies, file/directory qids and matching d_type, and bounded safe names. */
int sophia_9p_readdir(struct sophia_9p_client *, uint32_t fid, uint64_t cookie, uint32_t count,
                      struct sophia_9p_handle *);
/* Iterate a retained Rreaddir reply from position=0. OK yields one entry;
 * AGAIN means end. On error/end position and output are unchanged. All names
 * borrow the reply slot; consume only after the caller has finished with them. */
int sophia_9p_dirent_next(const struct sophia_9p_reply *, size_t *position,
                         struct sophia_9p_dirent *);
int sophia_9p_write(struct sophia_9p_client *, uint32_t fid, uint64_t offset, const void *data,
                    size_t count, struct sophia_9p_handle *);
/* Same wire request, but scrub each sent byte in SDK storage and any remaining
 * bytes on transport failure or explicit close. Never moves/reallocates caller
 * storage. The caller still owns and must clear its original data. */
int sophia_9p_write_secret(struct sophia_9p_client *, uint32_t fid, uint64_t offset,
                           const void *data, size_t count, struct sophia_9p_handle *);
int sophia_9p_scrubbing_output(const struct sophia_9p_client *);
/* Idempotent local retirement: scrub queued secret bytes, stop all I/O and
 * retain already validated replies for draining. Does not close the borrowed
 * fd or free storage. Call before abandoning/reinitializing a client that may
 * hold secrets; C has no automatic destructor. Caller closes its descriptor. */
void sophia_9p_close(struct sophia_9p_client *);
int sophia_9p_clunk(struct sophia_9p_client *, uint32_t fid, struct sophia_9p_handle *);
/* Refuses clunks, version, flushes, completed requests and duplicate flushes.
 * Original replies that beat Rflush remain deliverable; unanswered auth/attach/walk
 * reservations are undone. Tags survive through consume AND any pending flush. */
int sophia_9p_flush(struct sophia_9p_client *, struct sophia_9p_handle old,
                    struct sophia_9p_handle *);
/* At most byte_budget bytes and 32 syscalls in EACH direction, including EINTR.
 * Partial I/O remains owned. A terminal result latches; caller closes the fd. */
int sophia_9p_service(struct sophia_9p_client *, size_t byte_budget);
/* Nonzero while request bytes remain unsent on a live client: poll POLLOUT. */
int sophia_9p_wants_write(const struct sophia_9p_client *);
/* Arrival FIFO; peek borrows a stable reply until explicit consume. */
int sophia_9p_peek(struct sophia_9p_client *, struct sophia_9p_reply *);
int sophia_9p_consume(struct sophia_9p_client *, struct sophia_9p_handle);
#endif

#include "session_internal.h"

int sf_session_upload_write_drive(struct sophia_sf_client *c)
{
    uint64_t row = (uint64_t)c->resource.width_px * 4;
    uint64_t chunk = row ? (c->limits.max_chunk_bytes / row) * row : 0;
    unsigned window = c->wire->capacity > 5 ? c->wire->capacity - 5u : 1u;
    if (!chunk)
        return SOPHIA_9P_INVALID;
    if (window > SOPHIA_SF_MAX_QUEUED_UPLOAD_WRITES)
        window = SOPHIA_SF_MAX_QUEUED_UPLOAD_WRITES;
    /* Reserve journal, ACK, object, record and paired-submit request slots.
     * A full generic output queue simply lowers this pass's window. */
    while (!c->upload_wait && !c->upload_draining && !c->upload_closing &&
           c->upload_count < window && c->upload_issued < c->upload_size) {
        unsigned index = (c->upload_head + c->upload_count) % SOPHIA_SF_MAX_QUEUED_UPLOAD_WRITES;
        struct sophia_sf_upload_write *w = &c->upload_writes[index];
        uint64_t offset = c->upload_offset - c->upload_sent + c->upload_issued;
        size_t n = c->upload_size - c->upload_issued;
        int r;
        if (n > chunk - offset % chunk)
            n = (size_t)(chunk - offset % chunk);
        if (n > c->wire->msize - 23u)
            n = c->wire->msize - 23u;
        if (c->upload_iounit && n > c->upload_iounit)
            n = c->upload_iounit;
        memset(w, 0, sizeof(*w));
        r = sophia_9p_write(c->wire, c->upload_fid, offset,
                           c->upload_data + c->upload_issued, n, &w->op.handle);
        if (r)
            return r == SOPHIA_9P_BUSY ? 0 : r;
        w->count = (uint32_t)n;
        w->op.active = 1;
        c->upload_issued += n;
        c->upload_count++;
    }
    return 0;
}

static int retire(struct sophia_sf_client *c)
{
    while (c->upload_count) {
        struct sophia_sf_upload_write *w = &c->upload_writes[c->upload_head];
        if (!w->ready)
            break;
        if (c->upload_closing) {
            /* A terminal resource event fences recovery, but already-issued
             * replies must drain before the borrowed buffer is released. */
        } else if (c->upload_draining) {
            if (w->type == 7 && w->error == 116) {
                c->remote_error = w->error;
                c->upload_closing = 1;
            } else if (w->type != 7 || w->error != 22)
                return SOPHIA_9P_INVALID;
        } else if (w->type == 119) {
            if (!w->reply_count || w->reply_count > w->count)
                return SOPHIA_9P_INVALID;
            c->upload_sent += w->reply_count;
            c->upload_offset += w->reply_count;
            if (w->reply_count < w->count)
                c->upload_draining = 1;
        } else if (w->type == 7) {
            if (w->error == 11) {
                c->upload_draining = 1;
                c->upload_wait = 1;
            } else {
                c->remote_error = w->error;
                c->upload_closing = 1;
            }
        } else
            return SOPHIA_9P_INVALID;
        memset(w, 0, sizeof(*w));
        c->upload_head = (c->upload_head + 1u) % SOPHIA_SF_MAX_QUEUED_UPLOAD_WRITES;
        c->upload_count--;
    }
    if (!c->upload_count) {
        c->upload_issued = c->upload_sent;
        c->upload_draining = 0;
        if (!c->upload_closing && c->upload_sent == c->upload_size) {
            c->upload_data = NULL;
            c->upload_stage = 4;
        }
    }
    return 0;
}

int sf_session_upload_write_reply(struct sophia_sf_client *c, const struct sophia_9p_reply *r)
{
    unsigned i;
    for (i = 0; i < c->upload_count; i++) {
        struct sophia_sf_upload_write *w =
            &c->upload_writes[(c->upload_head + i) % SOPHIA_SF_MAX_QUEUED_UPLOAD_WRITES];
        if (sf_same(&w->op, r->handle)) {
            w->op.active = 0;
            w->ready = 1;
            w->type = r->type;
            w->reply_count = r->count;
            w->error = r->error;
            return retire(c);
        }
    }
    return SOPHIA_9P_INVALID;
}

int sophia_sf_client_upload_retry(struct sophia_sf_client *c)
{
    if (!c || !c->upload_wait || c->upload_draining || c->upload_count || c->upload_closing)
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal)
        return c->terminal;
    c->upload_wait = 0;
    return 0;
}

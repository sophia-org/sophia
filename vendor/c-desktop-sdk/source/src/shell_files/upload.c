#include "session_internal.h"

int sophia_sf_client_upload_ready(const struct sophia_sf_client *c)
{
    return c && !c->terminal && c->upload_stage == 4 && !c->upload_closing;
}

int sophia_sf_client_upload_pending(const struct sophia_sf_client *c)
{
    return c && !c->terminal && c->upload_stage != 0;
}

int sophia_sf_client_upload_begin(struct sophia_sf_client *c, struct sophia_sf_resource_begin v)
{
    struct sophia_sf_record r = {0};
    uint64_t row, chunk, rows;
    int status;
    if (!c)
        return SOPHIA_9P_ARGUMENT;
    if (!sophia_sf_client_ready(c) || c->upload_stage)
        return SOPHIA_9P_BUSY;
    row = (uint64_t)v.width_px * 4;
    if (!row || !v.height_px || v.slot >= c->limits.max_open_transfers ||
        v.grant_connection_epoch != c->epoch ||
        v.grant_content_epoch != c->limits.grant_content_epoch ||
        v.width_px > c->limits.max_width_px || v.height_px > c->limits.max_height_px ||
        v.total_bytes > c->limits.max_resource_bytes)
        return SOPHIA_9P_ARGUMENT;
    /* Limits validation still enforces the legacy relationships. On the file
     * wire the upload owns a chunk budget, independent of socket framing. */
    chunk = c->limits.max_chunk_bytes;
    rows = chunk / row;
    if (!rows || v.chunk_count != (v.height_px + rows - 1) / rows)
        return SOPHIA_9P_ARGUMENT;
    r.header.kind = SOPHIA_SF_RESOURCE_BEGIN;
    r.value.resource_begin = v;
    status = sophia_sf_client_submit(c, &r);
    if (!status) {
        c->resource = v;
        c->upload_slot = v.slot;
        c->upload_stage = 1;
        c->upload_offset = 0;
        c->upload_size = c->upload_sent = 0;
        c->upload_data = NULL;
        c->upload_closing = 0;
    }
    return status;
}
int sf_session_upload_drive(struct sophia_sf_client *c)
{
    int r;
    size_t n;
    char slot[2];
    const char *names[2] = {"upload", slot};
    if (!c->upload_stage || c->upload_op.active || c->event_ready)
        return 0;
    if (c->upload_closing) {
        c->upload_data = NULL;
        c->upload_size = c->upload_sent = 0;
        if (c->upload_fid == UINT32_MAX) {
            c->upload_stage = 0;
            return 0;
        }
        c->upload_stage = 7;
    }
    switch (c->upload_stage) {
    case 2:
        slot[0] = (char)('0' + c->upload_slot);
        slot[1] = 0;
        r = sophia_9p_walk(c->wire, c->root, names, 2, &c->upload_op.handle, &c->upload_fid);
        break;
    case 3:
        r = sophia_9p_lopen(c->wire, c->upload_fid, 1, &c->upload_op.handle);
        break;
    case 5:
        n = c->upload_size - c->upload_sent;
        if (n > c->wire->msize - 23u)
            n = c->wire->msize - 23u;
        if (c->upload_iounit && n > c->upload_iounit)
            n = c->upload_iounit;
        r = sophia_9p_write(c->wire, c->upload_fid, c->upload_offset,
                            c->upload_data + c->upload_sent, n, &c->upload_op.handle);
        break;
    case 7:
        r = sophia_9p_clunk(c->wire, c->upload_fid, &c->upload_op.handle);
        break;
    default:
        return 0;
    }
    return sf_started(&c->upload_op, r);
}
int sf_session_upload_reply(struct sophia_sf_client *c, const struct sophia_9p_reply *r)
{
    if (r->type == 7) {
        c->remote_error = r->error;
        if (c->upload_stage == 2 || c->upload_stage == 7)
            c->upload_fid = UINT32_MAX;
        c->upload_closing = 1;
        return 0;
    }
    switch (c->upload_stage) {
    case 2:
        if (r->count != 2) {
            c->upload_fid = UINT32_MAX;
            c->upload_closing = 1;
        } else
            c->upload_stage = 3;
        break;
    case 3:
        c->upload_iounit = r->iounit;
        c->upload_stage = 4;
        break;
    case 5:
        if (!r->count || r->count > c->upload_size - c->upload_sent)
            return SOPHIA_9P_INVALID;
        c->upload_sent += r->count;
        c->upload_offset += r->count;
        if (c->upload_sent == c->upload_size) {
            c->upload_data = NULL;
            c->upload_stage = 4;
        }
        break;
    case 7:
        c->upload_fid = UINT32_MAX;
        c->upload_stage = 0;
        break;
    default:
        return SOPHIA_9P_INVALID;
    }
    return 0;
}
int sf_session_upload_event(struct sophia_sf_client *c, const struct sophia_sf_record *r)
{
    const struct sophia_sf_resource_status *v = &r->value.resource_status;
    if (!c->upload_stage || v->resource_id != c->resource.resource_id ||
        v->resource_generation != c->resource.resource_generation ||
        v->grant_connection_epoch != c->resource.grant_connection_epoch ||
        v->grant_content_epoch != c->resource.grant_content_epoch)
        return 0;
    if (v->status == 1) {
        if (c->upload_stage != 1 || c->upload_closing)
            return SOPHIA_9P_INVALID;
        c->upload_stage = 2;
    } else
        c->upload_closing = 1;
    return 0;
}
int sophia_sf_client_upload_chunk(struct sophia_sf_client *c, const void *bytes, size_t count)
{
    if (!c || !bytes || !count)
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal)
        return c->terminal;
    if (c->upload_stage != 4 || c->upload_closing)
        return SOPHIA_9P_BUSY;
    if (count > c->resource.total_bytes - c->upload_offset)
        return SOPHIA_9P_ARGUMENT;
    c->upload_data = bytes;
    c->upload_size = count;
    c->upload_sent = 0;
    c->upload_stage = 5;
    return 0;
}
int sophia_sf_client_upload_end(struct sophia_sf_client *c, uint64_t transaction)
{
    struct sophia_sf_record r = {0};
    struct sophia_sf_resource_end *v = &r.value.resource_end;
    int status;
    if (!c)
        return SOPHIA_9P_ARGUMENT;
    if (c->upload_stage != 4 || c->upload_closing)
        return SOPHIA_9P_BUSY;
    if (c->upload_offset != c->resource.total_bytes)
        return SOPHIA_9P_ARGUMENT;
    r.header.kind = SOPHIA_SF_RESOURCE_END;
    v->transaction = transaction;
    v->grant_connection_epoch = c->resource.grant_connection_epoch;
    v->grant_content_epoch = c->resource.grant_content_epoch;
    v->resource_id = c->resource.resource_id;
    v->resource_generation = c->resource.resource_generation;
    v->total_bytes = c->resource.total_bytes;
    v->chunk_count = c->resource.chunk_count;
    status = sophia_sf_client_submit(c, &r);
    if (!status)
        c->upload_stage = 6;
    return status;
}
int sophia_sf_client_upload_cancel(struct sophia_sf_client *c, uint64_t transaction)
{
    struct sophia_sf_record r = {0};
    struct sophia_sf_resource_cancel *v = &r.value.resource_cancel;
    int status;
    if (!c)
        return SOPHIA_9P_ARGUMENT;
    if (c->upload_stage != 4 || c->upload_closing || c->upload_op.active)
        return SOPHIA_9P_BUSY;
    r.header.kind = SOPHIA_SF_RESOURCE_CANCEL;
    v->transaction = transaction;
    v->grant_connection_epoch = c->resource.grant_connection_epoch;
    v->grant_content_epoch = c->resource.grant_content_epoch;
    v->resource_id = c->resource.resource_id;
    v->resource_generation = c->resource.resource_generation;
    status = sophia_sf_client_submit(c, &r);
    if (!status)
        c->upload_stage = 6;
    return status;
}

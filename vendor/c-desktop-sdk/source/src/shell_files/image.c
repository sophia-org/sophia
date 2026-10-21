/* Shell r9 and r10 seams for the image adapter: the negotiated event codec,
 * image and owned-ring event headers, whole-file reads of the image
 * discovery files and owned-ring-v1 control writes. With image_mode clear
 * every path here is unused. */
#include "session_internal.h"
#include "../sophia_image_channel.h"
#include "../sophia_owned_ring.h"
#include "../sophia_shell_image.h"

int sf_session_image_r9(const struct sophia_sf_client *c)
{
    return c->image_mode && c->negotiated &&
           c->welcome.selected_revision == SOPHIA_SF_REVISION_IMAGE_CHANNEL &&
           (c->welcome.capabilities & SOPHIA_SF_CAP_SEALED_IMAGE);
}

int sf_session_image_r10(const struct sophia_sf_client *c)
{
    return c->image_mode && c->negotiated &&
           c->welcome.selected_revision == SOPHIA_SF_REVISION_OWNED_RING &&
           (c->welcome.capabilities & SOPHIA_SF_CAP_OWNED_RING);
}

int sf_session_image_kind(uint16_t kind)
{
    return (kind >= SOPHIA_IC_IMAGE_ADMITTED && kind <= SOPHIA_IC_CHANNEL_CLOSED) ||
           kind == SOPHIA_SS_RING_EVENT;
}

int sf_session_event_decode(const struct sophia_sf_client *c, const uint8_t *b, size_t n,
                            struct sophia_sf_record *r)
{
    /* D4E1 is dispatched by magic before any version-1 header decode; it
     * appears only at r10 with bit 15. Only the header is filled. */
    if (sf_session_image_r10(c) && sophia_or_is_event(b, n)) {
        struct sophia_or_event v;
        if (sophia_or_event_decode(b, n, &v) != SOPHIA_IC_OK ||
            v.identity.connection_epoch != c->epoch)
            return -1;
        memset(r, 0, sizeof(*r));
        r->header.kind = SOPHIA_SS_RING_EVENT;
        r->header.epoch = c->epoch;
        r->header.sequence = v.sequence;
        return 0;
    }
    if (sf_session_image_r9(c) || sf_session_image_r10(c)) {
        uint16_t kind = (uint16_t)sf_get(b + 6, 2);
        /* r10 keeps only the channel state records. */
        if (sf_session_image_kind(kind) &&
            (sf_session_image_r9(c) || kind == SOPHIA_IC_CHANNEL_READY ||
             kind == SOPHIA_IC_CHANNEL_CLOSED)) {
            struct sophia_ic_record v;
            if (sophia_ic_record_decode(b, n, &v) != SOPHIA_IC_OK || v.header.kind != kind ||
                v.header.epoch != c->epoch)
                return -1;
            memset(r, 0, sizeof(*r));
            r->header.kind = kind;
            r->header.epoch = v.header.epoch;
            r->header.submission = v.header.submission;
            r->header.sequence = v.header.sequence;
            return 0;
        }
        if (sophia_sf_decode_revision(b, n, c->welcome.selected_revision, r))
            return -1;
    } else if (sophia_sf_decode(b, n, r))
        return -1;
    if (r->header.epoch != c->epoch || r->header.kind < 16 || r->header.kind >= 256 ||
        !sf_session_event_allowed(c, r))
        return -1;
    return 0;
}

int sf_session_file_read(struct sophia_sf_client *c, const char *name, void *buf, size_t cap)
{
    if (!name || !buf || !cap)
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal)
        return c->terminal;
    if (!c->negotiated || c->file_stage || c->file_ready)
        return SOPHIA_9P_BUSY;
    c->file_name = name;
    c->file_buf = buf;
    c->file_cap = cap;
    c->file_used = 0;
    c->file_probe = 0;
    c->file_status = 0;
    c->file_error = 0;
    c->file_stage = 1;
    return 0;
}

int sf_session_file_result(struct sophia_sf_client *c, size_t *bytes, uint32_t *error)
{
    if (!c->file_ready)
        return SOPHIA_9P_BUSY;
    c->file_ready = 0;
    *bytes = c->file_used;
    *error = c->file_error;
    return c->file_status;
}

int sf_session_file_drive(struct sophia_sf_client *c)
{
    size_t room = c->file_cap - c->file_used;
    uint32_t count = c->wire->msize - 11u;
    int r;
    if (!c->file_stage || c->file_op.active)
        return 0;
    /* At capacity, one more byte proves the file is not larger. Bound the
     * room before narrowing it, so a large buffer cannot wrap the count. */
    if (!room)
        count = 1;
    else if (room < count)
        count = (uint32_t)room;
    if (c->file_iounit && count > c->file_iounit)
        count = c->file_iounit;
    switch (c->file_stage) {
    case 1:
        r = sophia_9p_walk(c->wire, c->root, &c->file_name, 1, &c->file_op.handle, &c->file_fid);
        break;
    case 2:
        r = sophia_9p_lopen(c->wire, c->file_fid, 0, &c->file_op.handle);
        break;
    case 3:
        r = sophia_9p_read(c->wire, c->file_fid, c->file_used, count, &c->file_op.handle);
        break;
    default:
        r = sophia_9p_clunk(c->wire, c->file_fid, &c->file_op.handle);
        break;
    }
    return sf_started(&c->file_op, r);
}

static int file_finish(struct sophia_sf_client *c, int status, uint32_t error)
{
    c->file_status = status;
    c->file_error = error;
    c->file_stage = 4;
    return 0;
}

int sf_session_file_reply(struct sophia_sf_client *c, const struct sophia_9p_reply *r)
{
    if (c->file_stage == 4) {
        if (r->type != 121)
            return SOPHIA_9P_INVALID;
        c->file_stage = 0;
        c->file_fid = UINT32_MAX;
        c->file_ready = 1;
        return 0;
    }
    if (r->type == 7) {
        uint32_t error = r->error;
        int status = error == 11 ? SOPHIA_9P_AGAIN : SOPHIA_9P_INVALID;
        if (c->file_stage == 1) {
            /* No fid was created; nothing to clunk. */
            c->file_stage = 0;
            c->file_ready = 1;
            c->file_status = status;
            c->file_error = error;
            return 0;
        }
        return file_finish(c, status, error);
    }
    if (c->file_stage == 1) {
        if (r->type != 111 || r->count != 1)
            return SOPHIA_9P_INVALID;
        c->file_stage = 2;
        return 0;
    }
    if (c->file_stage == 2) {
        if (r->type != 13)
            return SOPHIA_9P_INVALID;
        c->file_iounit = r->iounit;
        c->file_stage = 3;
        return 0;
    }
    if (r->type != 117)
        return file_finish(c, SOPHIA_9P_INVALID, 0);
    if (!r->count)
        return file_finish(c, 0, 0);
    if (r->count > c->file_cap - c->file_used)
        return file_finish(c, SOPHIA_9P_INVALID, 0);
    memcpy(c->file_buf + c->file_used, r->data, r->count);
    c->file_used += r->count;
    return 0;
}

int sf_session_image_begin_valid(const struct sophia_sf_client *c, const uint8_t *b, size_t n)
{
    struct sophia_ic_record v;
    return sophia_ic_record_decode(b, n, &v) == SOPHIA_IC_OK &&
           v.header.kind == SOPHIA_IC_IMAGE_BEGIN && v.header.epoch == c->epoch &&
           v.header.submission && !v.header.sequence;
}

int sf_session_control_write(struct sophia_sf_client *c, const void *b, size_t n)
{
    if (!b || !n || n > sizeof(c->ctl_bytes))
        return SOPHIA_9P_ARGUMENT;
    if (c->terminal)
        return c->terminal;
    if (!c->negotiated || c->ctl_stage || c->ctl_ready)
        return SOPHIA_9P_BUSY;
    memcpy(c->ctl_bytes, b, n);
    c->ctl_used = n;
    c->ctl_status = 0;
    c->ctl_error = 0;
    c->ctl_stage = c->ctl_open ? 3 : 1;
    return 0;
}

int sf_session_control_result(struct sophia_sf_client *c, uint32_t *error)
{
    if (!c->ctl_ready)
        return SOPHIA_9P_BUSY;
    c->ctl_ready = 0;
    *error = c->ctl_error;
    return c->ctl_status;
}

int sf_session_control_drive(struct sophia_sf_client *c)
{
    static const char *const name = SOPHIA_OR_CONTROL_FILE;
    int r;
    if (!c->ctl_stage || c->ctl_op.active)
        return 0;
    if (c->ctl_stage == 1)
        r = sophia_9p_walk(c->wire, c->root, &name, 1, &c->ctl_op.handle, &c->ctl_fid);
    else if (c->ctl_stage == 2)
        r = sophia_9p_lopen(c->wire, c->ctl_fid, 1, &c->ctl_op.handle);
    else if (c->ctl_stage == 3)
        r = sophia_9p_write(c->wire, c->ctl_fid, 0, c->ctl_bytes, (uint32_t)c->ctl_used,
                            &c->ctl_op.handle);
    else
        r = sophia_9p_clunk(c->wire, c->ctl_fid, &c->ctl_op.handle);
    return sf_started(&c->ctl_op, r);
}

static int control_done(struct sophia_sf_client *c, int status, uint32_t error)
{
    c->ctl_stage = 0;
    c->ctl_status = status;
    c->ctl_error = error;
    c->ctl_ready = 1;
    return 0;
}

int sf_session_control_reply(struct sophia_sf_client *c, const struct sophia_9p_reply *r)
{
    if (c->ctl_stage == 4) {
        if (r->type != 121)
            return SOPHIA_9P_INVALID;
        c->ctl_fid = UINT32_MAX;
        return control_done(c, SOPHIA_9P_INVALID, c->ctl_error);
    }
    if (r->type == 7) {
        if (c->ctl_stage == 2) {
            /* The walked fid is clunked before the refusal is reported. */
            c->ctl_error = r->error;
            c->ctl_stage = 4;
            return 0;
        }
        return control_done(c, r->error == 11 && c->ctl_stage == 3 ? SOPHIA_9P_AGAIN
                                                                   : SOPHIA_9P_INVALID,
                            r->error);
    }
    if (c->ctl_stage == 1) {
        if (r->type != 111 || r->count != 1)
            return SOPHIA_9P_INVALID;
        c->ctl_stage = 2;
        return 0;
    }
    if (c->ctl_stage == 2) {
        if (r->type != 13)
            return SOPHIA_9P_INVALID;
        c->ctl_open = 1;
        c->ctl_stage = 3;
        return 0;
    }
    if (r->type != 119 || r->count != c->ctl_used)
        return control_done(c, SOPHIA_9P_INVALID, 0);
    return control_done(c, 0, 0);
}

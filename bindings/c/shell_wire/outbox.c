#include "../sophia_shell_outbox.h"
#include "fields.h"
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>

int sophia_shell_outbox_init(struct sophia_shell_outbox *out, size_t bytes,
    unsigned records, size_t reserved_bytes, unsigned reserved_records)
{
    if (!out || bytes>SOPHIA_SHELL_OUTBOX_MAX_BYTES || records>SOPHIA_SHELL_OUTBOX_MAX_RECORDS ||
        reserved_records<2 || records<reserved_records || reserved_bytes>bytes ||
        reserved_bytes<(size_t)reserved_records*SOPHIA_SHELL_OUTBOX_CONTROL_BYTES)
        return SOPHIA_SHELL_ARGUMENT;
    *out=(struct sophia_shell_outbox){.max_bytes=bytes,.max_records=records,
        .reserve_bytes=reserved_bytes,.reserve_records=reserved_records};
    return SOPHIA_SHELL_OK;
}
static int control_kind(uint16_t kind)
{
    switch (kind) {
    case 96: case 163: case 168: case 169: case 170: case 174: case 176:
    case 178: case 180: case 185: case 188: case 194: case 195:
        return 1;
    default: return 0;
    }
}
int sophia_shell_outbox_push(struct sophia_shell_outbox *out,
    const struct sophia_shell_outbound_frame *frames, unsigned count)
{
    if (!out || !frames || !count || count>2 || !out->max_records) return SOPHIA_SHELL_ARGUMENT;
    if (out->terminal) return out->terminal;
    size_t bytes=0, bulk_bytes=0; unsigned bulk_records=0;
    for (unsigned i=0; i<count; ++i) {
        struct sophia_shell_frame f;
        if (sophia_shell_frame_decode(frames[i].bytes,frames[i].length,&f)!=SOPHIA_SHELL_OK ||
            shell_direction(f.kind)!=1 ||
            (frames[i].class!=SOPHIA_SHELL_OUTBOUND_BULK && frames[i].class!=SOPHIA_SHELL_OUTBOUND_CONTROL))
            return SOPHIA_SHELL_INVALID;
        if (frames[i].class==SOPHIA_SHELL_OUTBOUND_CONTROL &&
            (!control_kind(f.kind) || frames[i].length>SOPHIA_SHELL_OUTBOX_CONTROL_BYTES))
            return SOPHIA_SHELL_INVALID;
        bytes+=frames[i].length;
        if (frames[i].class==SOPHIA_SHELL_OUTBOUND_BULK) {
            bulk_bytes+=frames[i].length; ++bulk_records;
        }
    }
    if (count>out->max_records-out->count || bytes>out->max_bytes-out->bytes ||
        bulk_records>out->max_records-out->reserve_records-out->bulk_records ||
        bulk_bytes>out->max_bytes-out->reserve_bytes-out->bulk_bytes) return SOPHIA_SHELL_BUSY;
    uint8_t *owned[2]={0};
    for (unsigned i=0; i<count; ++i) {
        owned[i]=malloc(frames[i].length);
        if (!owned[i]) {
            for (unsigned j=0; j<i; ++j) free(owned[j]);
            return SOPHIA_SHELL_BUSY;
        }
        memcpy(owned[i],frames[i].bytes,frames[i].length);
    }
    /* Prevalidated fixed cells; no failure, allocation, I/O or callback after
     * ownership enters the FIFO. An atomic pair cannot leave only its ACK. */
    for (unsigned i=0; i<count; ++i) {
        unsigned index=(out->head+out->count+i)%out->max_records;
        out->records[index]=(struct sophia_shell_outbox_record){owned[i],frames[i].length,0,frames[i].class};
    }
    out->count+=count; out->bytes+=bytes;
    out->bulk_records+=bulk_records; out->bulk_bytes+=bulk_bytes;
    return SOPHIA_SHELL_OK;
}
int sophia_shell_outbox_flush(struct sophia_shell_outbox *out, int fd, size_t budget)
{
    if (!out || !out->max_records || fd<0) return SOPHIA_SHELL_ARGUMENT;
    if (out->terminal) return out->terminal;
    for (unsigned calls=0; out->count && budget && calls<SOPHIA_SHELL_MAX_IO_CALLS; ++calls) {
        struct sophia_shell_outbox_record *r=&out->records[out->head];
        size_t amount=r->length-r->sent;
        if (amount>budget) amount=budget;
        ssize_t n=send(fd,r->bytes+r->sent,amount,MSG_DONTWAIT|MSG_NOSIGNAL);
        if (n<0) {
            if (errno==EINTR) continue;
            if (errno==EAGAIN || errno==EWOULDBLOCK) return SOPHIA_SHELL_AGAIN;
            out->terminal=SOPHIA_SHELL_IO_ERROR; return out->terminal;
        }
        if (!n) {out->terminal=SOPHIA_SHELL_IO_ERROR; return out->terminal;}
        r->sent+=(size_t)n; budget-=(size_t)n;
        if (r->sent==r->length) {
            out->bytes-=r->length;
            if (r->class==SOPHIA_SHELL_OUTBOUND_BULK) {
                out->bulk_bytes-=r->length; --out->bulk_records;
            }
            free(r->bytes); *r=(struct sophia_shell_outbox_record){0};
            --out->count; out->head=(out->head+1)%out->max_records;
        }
    }
    return out->count ? SOPHIA_SHELL_AGAIN : SOPHIA_SHELL_OK;
}
void sophia_shell_outbox_dispose(struct sophia_shell_outbox *out)
{
    if (!out) return;
    for (unsigned i=0; i<out->count; ++i) free(out->records[(out->head+i)%out->max_records].bytes);
    *out=(struct sophia_shell_outbox){0};
}

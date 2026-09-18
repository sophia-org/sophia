#include "../sophia_shell_upload.h"
#include <stdlib.h>
#include <string.h>

struct upload_slot {
    enum sophia_shell_upload_state state;
    struct sophia_shell_resource_begin begin;
    uint8_t *pixels;
    uint64_t begin_tx, end_tx, control_tx;
    uint64_t chunk_tx[4096];
    uint32_t chunk_bytes, ordinal;
    int cancel_requested;
};
struct sophia_shell_upload {
    struct sophia_shell_content_limits limits;
    struct upload_slot slots[SOPHIA_SHELL_UPLOAD_SLOTS];
    uint8_t scratch[SOPHIA_SHELL_MAX_FRAME_BYTES];
    uint64_t greatest_id;
    unsigned cursor;
};
static int transferring(enum sophia_shell_upload_state state)
{
    return state==SOPHIA_UPLOAD_STAGED || state==SOPHIA_UPLOAD_BEGIN_PENDING ||
        state==SOPHIA_UPLOAD_CHUNKS || state==SOPHIA_UPLOAD_END_PENDING ||
        state==SOPHIA_UPLOAD_CANCEL_READY || state==SOPHIA_UPLOAD_CANCEL_PENDING;
}
static void empty(struct upload_slot *s)
{
    struct sophia_shell_resource_key key=s->begin.key;
    free(s->pixels); *s=(struct upload_slot){0}; s->begin.key=key;
}
int sophia_shell_upload_new(const struct sophia_shell_frame *frame, struct sophia_shell_upload **out)
{
    if (!out) return SOPHIA_SHELL_ARGUMENT;
    struct sophia_shell_content_limits limits;
    int r=sophia_shell_content_limits_decode(frame,&limits);
    if (r!=SOPHIA_SHELL_OK) return r;
    struct sophia_shell_upload *owner=calloc(1,sizeof(*owner));
    if (!owner) return SOPHIA_SHELL_BUSY;
    owner->limits=limits; *out=owner; return SOPHIA_SHELL_OK;
}
int sophia_shell_upload_stage(struct sophia_shell_upload *o,
    uint32_t w, uint32_t h, uint32_t n, uint32_t d, const uint8_t *pixels, size_t bytes, unsigned *slot)
{
    if (!o || !pixels || !slot) return SOPHIA_SHELL_ARGUMENT;
    const struct sophia_shell_content_limits *l=&o->limits;
    if (!w || !h || w>l->max_width_px || h>l->max_height_px ||
        !n || !d || n>l->max_scale_numerator || d>l->max_scale_denominator ||
        bytes!=(uint64_t)w*h*4 || bytes>l->max_resource_bytes) return SOPHIA_SHELL_INVALID;
    uint32_t a=n,b=d;
    while (b) {uint32_t next=a%b; a=b; b=next;}
    if (a!=1) return SOPHIA_SHELL_INVALID;
    uint32_t row=w*4, payload=l->max_frame_payload-48;
    if (payload>l->max_chunk_bytes) payload=l->max_chunk_bytes;
    uint32_t rows=payload/row;
    if (!rows) return SOPHIA_SHELL_INVALID;
    uint32_t chunks=h/rows+(h%rows!=0), prototype_rows=65488/row;
    /* The published Begin codec also enforces its prototype chunk count.
     * Refuse an incompatible tightened profile, never exceed its chunk bound. */
    if (!prototype_rows || chunks!=h/prototype_rows+(h%prototype_rows!=0) || chunks>4096)
        return SOPHIA_SHELL_INVALID;
    unsigned selected=SOPHIA_SHELL_UPLOAD_SLOTS, live=0, unminted=0;
    uint64_t staging=0, held=0;
    for (unsigned i=0; i<SOPHIA_SHELL_UPLOAD_SLOTS; ++i) {
        const struct upload_slot *s=&o->slots[i];
        if (s->state==SOPHIA_UPLOAD_EMPTY) {
            if (selected==SOPHIA_SHELL_UPLOAD_SLOTS && s->begin.key.resource.generation<UINT64_MAX &&
                (s->begin.key.resource.id || o->greatest_id<l->max_resource_ids)) selected=i;
        } else {
            ++live; held+=s->begin.total_bytes; unminted+=!s->begin.key.resource.id;
            if (transferring(s->state)) staging+=s->begin.total_bytes;
        }
    }
    /* Conservative reservation: pending Retire remains resident-charged until
     * exact Released, including when the peer rejects that Retire. */
    if (selected==SOPHIA_SHELL_UPLOAD_SLOTS ||
        (!o->slots[selected].begin.key.resource.id && o->greatest_id+unminted>=l->max_resource_ids) ||
        live>=l->max_live_resources ||
        held+bytes>l->max_resident_bytes || staging+bytes>l->max_staging_bytes)
        return SOPHIA_SHELL_BUSY;
    for (size_t i=0; i<bytes; i+=4)
        if (pixels[i]>pixels[i+3] || pixels[i+1]>pixels[i+3] || pixels[i+2]>pixels[i+3])
            return SOPHIA_SHELL_INVALID;
    uint8_t *copy=malloc(bytes);
    if (!copy) return SOPHIA_SHELL_BUSY;
    memcpy(copy,pixels,bytes);
    struct upload_slot *s=&o->slots[selected];
    s->begin=(struct sophia_shell_resource_begin){
        {l->grant,s->begin.key.resource},w,h,n,d,1,chunks,bytes};
    s->pixels=copy; s->chunk_bytes=rows*row; s->state=SOPHIA_UPLOAD_STAGED;
    *slot=selected; return SOPHIA_SHELL_OK;
}
static int offer(struct sophia_shell_upload *o, struct sophia_shell_outbox *out,
    unsigned index, uint64_t tx)
{
    struct upload_slot *s=&o->slots[index]; size_t length=0;
    struct sophia_shell_resource_begin begin=s->begin;
    uint16_t kind=0; int r;
    if (s->state==SOPHIA_UPLOAD_STAGED) {
        unsigned open=0;
        for (unsigned i=0; i<SOPHIA_SHELL_UPLOAD_SLOTS; ++i)
            open+=transferring(o->slots[i].state) && o->slots[i].state!=SOPHIA_UPLOAD_STAGED;
        if (open>=o->limits.max_open_transfers) return SOPHIA_SHELL_BUSY;
        if (!begin.key.resource.id) {
            if (o->greatest_id>=o->limits.max_resource_ids) return SOPHIA_SHELL_BUSY;
            begin.key.resource.id=o->greatest_id+1;
        }
        if (begin.key.resource.generation==UINT64_MAX) return SOPHIA_SHELL_INVALID;
        ++begin.key.resource.generation; kind=165;
        r=sophia_shell_resource_begin_encode(o->scratch,sizeof(o->scratch),tx,&begin,&length);
    } else if (s->state==SOPHIA_UPLOAD_CHUNKS) {
        if (s->ordinal<s->begin.chunk_count) {
            uint64_t offset=(uint64_t)s->ordinal*s->chunk_bytes;
            uint64_t bytes=s->begin.total_bytes-offset;
            if (bytes>s->chunk_bytes) bytes=s->chunk_bytes;
            struct sophia_shell_resource_chunk chunk={s->begin.key,s->ordinal,offset,s->pixels+offset,(size_t)bytes};
            kind=167; r=sophia_shell_resource_chunk_encode(o->scratch,sizeof(o->scratch),tx,&chunk,&length);
        } else {
            struct sophia_shell_resource_end end={s->begin.key,s->begin.total_bytes,s->begin.chunk_count};
            kind=168; r=sophia_shell_resource_end_encode(o->scratch,sizeof(o->scratch),tx,&end,&length);
        }
    } else if (s->state==SOPHIA_UPLOAD_CANCEL_READY || s->state==SOPHIA_UPLOAD_RETIRE_READY) {
        kind=s->state==SOPHIA_UPLOAD_CANCEL_READY?169:170;
        if (kind==170) {
            uint64_t retiring=s->begin.total_bytes;
            for (unsigned i=0; i<SOPHIA_SHELL_UPLOAD_SLOTS; ++i)
                if (o->slots[i].state==SOPHIA_UPLOAD_RELEASE_PENDING) retiring+=o->slots[i].begin.total_bytes;
            if (retiring>o->limits.max_retiring_bytes) return SOPHIA_SHELL_BUSY;
        }
        r=sophia_shell_resource_control_encode(o->scratch,sizeof(o->scratch),kind,tx,&s->begin.key,&length);
    } else return SOPHIA_SHELL_AGAIN;
    if (r!=SOPHIA_SHELL_OK) return r;
    if (length>SOPHIA_SHELL_HEADER_BYTES+o->limits.max_frame_payload) return SOPHIA_SHELL_INVALID;
    struct sophia_shell_outbound_frame frame={o->scratch,length,
        kind==165 || kind==167 ? SOPHIA_SHELL_OUTBOUND_BULK : SOPHIA_SHELL_OUTBOUND_CONTROL};
    r=sophia_shell_outbox_push(out,&frame,1);
    if (r!=SOPHIA_SHELL_OK) return r;
    /* Fixed owner transitions only after the FIFO copied the complete frame. */
    if (kind==165) {
        s->begin=begin; s->begin_tx=tx; s->state=SOPHIA_UPLOAD_BEGIN_PENDING;
        if (begin.key.resource.id>o->greatest_id) o->greatest_id=begin.key.resource.id;
    } else if (kind==167) s->chunk_tx[s->ordinal++]=tx;
    else if (kind==168) {s->end_tx=tx; s->state=SOPHIA_UPLOAD_END_PENDING;}
    else {s->control_tx=tx; s->state=kind==169?SOPHIA_UPLOAD_CANCEL_PENDING:SOPHIA_UPLOAD_RELEASE_PENDING;}
    return SOPHIA_SHELL_OK;
}
int sophia_shell_upload_pump(struct sophia_shell_upload *o, struct sophia_shell_outbox *out, uint64_t *tx)
{
    if (!o || !out || !tx || !*tx || *tx==UINT64_MAX) return SOPHIA_SHELL_ARGUMENT;
    int result=SOPHIA_SHELL_AGAIN;
    for (unsigned count=0; count<SOPHIA_SHELL_UPLOAD_SLOTS; ++count) {
        unsigned index=o->cursor; o->cursor=(o->cursor+1)%SOPHIA_SHELL_UPLOAD_SLOTS;
        int r=offer(o,out,index,*tx);
        if (r==SOPHIA_SHELL_OK) {++*tx; return r;}
        if (r==SOPHIA_SHELL_BUSY) result=r;
        else if (r!=SOPHIA_SHELL_AGAIN) return r;
    }
    return result;
}
static int transfer_tx(const struct upload_slot *s, uint64_t tx)
{
    if (tx==s->begin_tx || (s->end_tx && tx==s->end_tx) || (s->control_tx && tx==s->control_tx)) return 1;
    for (uint32_t i=0; i<s->ordinal; ++i) if (s->chunk_tx[i]==tx) return 1;
    return 0;
}
int sophia_shell_upload_reply(struct sophia_shell_upload *o, const struct sophia_shell_frame *frame)
{
    if (!o) return SOPHIA_SHELL_ARGUMENT;
    struct sophia_shell_resource_reply r;
    int result=sophia_shell_resource_reply_decode(frame,&r);
    if (result!=SOPHIA_SHELL_OK) return result;
    if (r.key.grant.connection_epoch!=o->limits.grant.connection_epoch ||
        r.key.grant.content_grant_epoch!=o->limits.grant.content_grant_epoch) return SOPHIA_SHELL_INVALID;
    struct upload_slot *s=NULL;
    for (unsigned i=0; i<SOPHIA_SHELL_UPLOAD_SLOTS; ++i) {
        struct upload_slot *candidate=&o->slots[i];
        if (candidate->state!=SOPHIA_UPLOAD_EMPTY && candidate->state!=SOPHIA_UPLOAD_STAGED &&
            candidate->begin.key.resource.id==r.key.resource.id &&
            candidate->begin.key.resource.generation==r.key.resource.generation) s=candidate;
    }
    if (!s) return SOPHIA_SHELL_INVALID;
    if (r.kind==171) {
        if (s->state!=SOPHIA_UPLOAD_RELEASE_PENDING || r.transaction!=s->control_tx) return SOPHIA_SHELL_INVALID;
        empty(s); return SOPHIA_SHELL_OK;
    }
    if (r.next_ordinal) return SOPHIA_SHELL_INVALID;
    if (r.status==1) {
        if (s->state!=SOPHIA_UPLOAD_BEGIN_PENDING || r.transaction!=s->begin_tx ||
            r.admitted_bytes!=s->begin.total_bytes) return SOPHIA_SHELL_INVALID;
        s->state=s->cancel_requested?SOPHIA_UPLOAD_CANCEL_READY:SOPHIA_UPLOAD_CHUNKS;
    } else if (r.status==2) {
        if (s->state!=SOPHIA_UPLOAD_END_PENDING || r.transaction!=s->end_tx ||
            r.admitted_bytes!=s->begin.total_bytes) return SOPHIA_SHELL_INVALID;
        s->state=s->cancel_requested?SOPHIA_UPLOAD_RETIRE_READY:SOPHIA_UPLOAD_RESIDENT;
    } else if (s->state==SOPHIA_UPLOAD_RELEASE_PENDING) {
        if (r.status!=3 || r.transaction!=s->control_tx || r.admitted_bytes) return SOPHIA_SHELL_INVALID;
        s->state=SOPHIA_UPLOAD_RESIDENT; s->control_tx=0;
    } else {
        if (!transferring(s->state) || r.admitted_bytes || !transfer_tx(s,r.transaction) ||
            (r.status==4 && (s->state!=SOPHIA_UPLOAD_CANCEL_PENDING || r.transaction!=s->control_tx)))
            return SOPHIA_SHELL_INVALID;
        empty(s);
    }
    return SOPHIA_SHELL_OK;
}
int sophia_shell_upload_cancel(struct sophia_shell_upload *o, unsigned i)
{
    if (!o || i>=SOPHIA_SHELL_UPLOAD_SLOTS) return SOPHIA_SHELL_ARGUMENT;
    struct upload_slot *s=&o->slots[i];
    if (s->state==SOPHIA_UPLOAD_STAGED) empty(s);
    else if (s->state==SOPHIA_UPLOAD_BEGIN_PENDING || s->state==SOPHIA_UPLOAD_END_PENDING) s->cancel_requested=1;
    else if (s->state==SOPHIA_UPLOAD_CHUNKS) s->state=SOPHIA_UPLOAD_CANCEL_READY;
    else return SOPHIA_SHELL_INVALID;
    return SOPHIA_SHELL_OK;
}
int sophia_shell_upload_retire(struct sophia_shell_upload *o, unsigned i)
{
    if (!o || i>=SOPHIA_SHELL_UPLOAD_SLOTS) return SOPHIA_SHELL_ARGUMENT;
    if (o->slots[i].state!=SOPHIA_UPLOAD_RESIDENT) return SOPHIA_SHELL_INVALID;
    o->slots[i].state=SOPHIA_UPLOAD_RETIRE_READY; return SOPHIA_SHELL_OK;
}
int sophia_shell_upload_inspect(const struct sophia_shell_upload *o, unsigned i,
                               struct sophia_shell_upload_snapshot *out)
{
    if (!o || !out || i>=SOPHIA_SHELL_UPLOAD_SLOTS) return SOPHIA_SHELL_ARGUMENT;
    const struct upload_slot *s=&o->slots[i];
    *out=(struct sophia_shell_upload_snapshot){s->state,s->begin.key,s->begin.total_bytes,s->ordinal};
    return SOPHIA_SHELL_OK;
}
void sophia_shell_upload_dispose(struct sophia_shell_upload *o)
{
    if (!o) return;
    for (unsigned i=0; i<SOPHIA_SHELL_UPLOAD_SLOTS; ++i) free(o->slots[i].pixels);
    free(o);
}

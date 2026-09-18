#include "../sophia_shell_upload.h"
#include "../shell_wire/fields.h"
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

void *__real_malloc(size_t);
void *__real_calloc(size_t,size_t);
void __real_free(void *);
static struct {void *pointer; size_t bytes;} owned[32];
static size_t owned_bytes, owner_bytes;
static int fail_allocate;
static void *track(void *p,size_t bytes)
{
    if (!p) return p;
    for (unsigned i=0; i<32; ++i) if (!owned[i].pointer) {
        owned[i].pointer=p; owned[i].bytes=bytes; owned_bytes+=bytes; return p;
    }
    abort();
}
void *__wrap_malloc(size_t bytes)
{
    if (fail_allocate) {fail_allocate=0; return NULL;}
    return track(__real_malloc(bytes),bytes);
}
void *__wrap_calloc(size_t count,size_t bytes)
{
    if (fail_allocate) {fail_allocate=0; return NULL;}
    return track(__real_calloc(count,bytes),count*bytes);
}
void __wrap_free(void *p)
{
    if (!p) return;
    for (unsigned i=0; i<32; ++i) if (owned[i].pointer==p) {
        owned_bytes-=owned[i].bytes; owned[i].pointer=NULL; __real_free(p); return;
    }
    abort();
}

static uint8_t limits_bytes[288], peer_bytes[SOPHIA_SHELL_MAX_FRAME_BYTES];
static uint8_t rx[SOPHIA_SHELL_MAX_FRAME_BYTES], tx_bytes[SOPHIA_SHELL_MAX_FRAME_BYTES];
static struct sophia_shell_frame limits;
static struct sophia_shell_outbox outbox;
static struct sophia_shell_wire wire;
static struct sophia_shell_upload *owner;
static int sockets[2];
static uint64_t next_tx;
static unsigned nibble(char c)
{
    if (c>='0' && c<='9') return (unsigned)(c-'0');
    if (c>='a' && c<='f') return (unsigned)(c-'a')+10;
    abort();
}
static struct sophia_shell_upload_snapshot inspect(unsigned slot)
{
    struct sophia_shell_upload_snapshot s;
    assert(sophia_shell_upload_inspect(owner,slot,&s)==SOPHIA_SHELL_OK); return s;
}
static void setup(void)
{
    assert(socketpair(AF_UNIX,SOCK_STREAM,0,sockets)==0);
    int size=4096; assert(setsockopt(sockets[0],SOL_SOCKET,SO_SNDBUF,&size,sizeof(size))==0);
    assert(sophia_shell_wire_init(&wire,sockets[0],rx,sizeof(rx),tx_bytes,sizeof(tx_bytes))==SOPHIA_SHELL_OK);
    assert(sophia_shell_outbox_init(&outbox,131072,8,1024,4)==SOPHIA_SHELL_OK);
    assert(sophia_shell_upload_new(&limits,&owner)==SOPHIA_SHELL_OK); next_tx=1; owner_bytes=owned_bytes;
}
static void teardown(void)
{
    close(sockets[0]); close(sockets[1]);
    sophia_shell_outbox_dispose(&outbox); sophia_shell_upload_dispose(owner); owner=NULL; assert(!owned_bytes);
}
static struct sophia_shell_frame drain(void)
{
    size_t used=0; unsigned visits=0;
    while (outbox.count) {
        assert(++visits<1000);
        int r=sophia_shell_outbox_flush(&outbox,sockets[0],4096);
        assert(r==SOPHIA_SHELL_OK || r==SOPHIA_SHELL_AGAIN);
        ssize_t n=recv(sockets[1],peer_bytes+used,sizeof(peer_bytes)-used,MSG_DONTWAIT);
        if (n<0) assert(errno==EAGAIN || errno==EWOULDBLOCK); else {assert(n>0); used+=(size_t)n;}
    }
    /* The fixture offers one frame per drain. Consume any final kernel bytes. */
    size_t total=used>=24 ? 24+shell_get32(peer_bytes+16) : 24;
    while (used<total) {
        ssize_t n=recv(sockets[1],peer_bytes+used,sizeof(peer_bytes)-used,0); assert(n>0); used+=(size_t)n;
        if (used>=24) total=24+shell_get32(peer_bytes+16);
    }
    struct sophia_shell_frame f;
    assert(sophia_shell_frame_decode(peer_bytes,used,&f)==SOPHIA_SHELL_OK); return f;
}
static struct sophia_shell_frame pump(void)
{
    uint64_t tx=next_tx;
    assert(sophia_shell_upload_pump(owner,&outbox,&next_tx)==SOPHIA_SHELL_OK && next_tx==tx+1);
    struct sophia_shell_frame f=drain(); assert(f.transaction==tx); return f;
}
static int reply(struct sophia_shell_resource_key key, uint64_t transaction,
                 uint16_t status, uint16_t reason, uint64_t admitted)
{
    uint8_t p[48]={0}, bytes[72]; size_t n;
    shell_put64(p,key.grant.connection_epoch); shell_put64(p+8,key.grant.content_grant_epoch);
    shell_put64(p+16,key.resource.id); shell_put64(p+24,key.resource.generation);
    shell_put16(p+32,status?status:reason);
    if (status) {shell_put16(p+34,reason); shell_put64(p+40,admitted);}
    assert(sophia_shell_frame_encode(bytes,sizeof(bytes),status?166:171,transaction,p,status?48:34,&n)==SOPHIA_SHELL_OK);
    assert(send(sockets[1],bytes,n,MSG_NOSIGNAL)==(ssize_t)n);
    struct sophia_shell_frame f;
    assert(sophia_shell_wire_receive(&wire,sizeof(rx),&f)==SOPHIA_SHELL_FRAME);
    int r=sophia_shell_upload_reply(owner,&f);
    assert(sophia_shell_wire_consume(&wire)==SOPHIA_SHELL_OK); return r;
}
static unsigned stage(void)
{
    uint8_t pixels[8]={255,255,255,255,128,64,0,128}; unsigned slot=99;
    assert(sophia_shell_upload_stage(owner,2,1,1,1,pixels,sizeof(pixels),&slot)==SOPHIA_SHELL_OK);
    return slot;
}
static void resident(unsigned slot, int large)
{
    struct sophia_shell_frame f=pump(); assert(f.kind==165);
    struct sophia_shell_upload_snapshot s=inspect(slot);
    assert(s.state==SOPHIA_UPLOAD_BEGIN_PENDING);
    assert(shell_get64(f.payload+16)==s.key.resource.id && shell_get64(f.payload+24)==s.key.resource.generation);
    assert(reply(s.key,f.transaction,1,0,s.bytes)==SOPHIA_SHELL_OK);
    uint32_t ordinal=0; uint64_t offset=0;
    while ((f=pump()).kind==167) {
        assert(shell_get32(f.payload+32)==ordinal++ && shell_get64(f.payload+40)==offset);
        uint32_t count=shell_get32(f.payload+36); assert(f.payload_bytes==48+count);
        if (large) for (uint32_t i=0; i<count; ++i) assert(f.payload[48+i]==255);
        else {const uint8_t expected[8]={255,255,255,255,128,64,0,128}; assert(count==8 && !memcmp(f.payload+48,expected,8));}
        offset+=count;
    }
    assert(f.kind==168 && offset==s.bytes && shell_get32(f.payload+40)==ordinal);
    assert(reply(s.key,f.transaction,2,0,s.bytes)==SOPHIA_SHELL_OK);
    assert(inspect(slot).state==SOPHIA_UPLOAD_RESIDENT);
}
static uint64_t retire(unsigned slot)
{
    assert(sophia_shell_upload_retire(owner,slot)==SOPHIA_SHELL_OK);
    struct sophia_shell_frame f=pump(); assert(f.kind==170);
    assert(inspect(slot).state==SOPHIA_UPLOAD_RELEASE_PENDING); return f.transaction;
}
static void held_and_reuse(void)
{
    setup(); unsigned old=stage(); resident(old,0);
    struct sophia_shell_upload_snapshot kept=inspect(old); uint64_t old_retire=retire(old);
    for (unsigned cycle=1; cycle<=1000; ++cycle) {
        unsigned slot=stage(); assert(slot!=old); resident(slot,0);
        struct sophia_shell_upload_snapshot s=inspect(slot);
        assert(s.key.resource.id==2 && s.key.resource.generation==cycle);
        uint64_t tx=retire(slot);
        struct sophia_shell_resource_key wrong=s.key; ++wrong.resource.generation;
        assert(reply(wrong,tx,0,0,0)==SOPHIA_SHELL_INVALID);
        assert(inspect(slot).state==SOPHIA_UPLOAD_RELEASE_PENDING && inspect(slot).bytes==8);
        assert(reply(s.key,tx,0,0,0)==SOPHIA_SHELL_OK);
        assert(inspect(slot).state==SOPHIA_UPLOAD_EMPTY && !inspect(slot).bytes);
        assert(owned_bytes==owner_bytes+8); /* Actual retained old pixel allocation, not only metadata. */
        struct sophia_shell_upload_snapshot still=inspect(old);
        assert(still.state==SOPHIA_UPLOAD_RELEASE_PENDING && still.bytes==kept.bytes);
        assert(still.key.resource.id==kept.key.resource.id && still.key.resource.generation==kept.key.resource.generation);
    }
    assert(reply(kept.key,old_retire,0,0,0)==SOPHIA_SHELL_OK);
    assert(inspect(old).state==SOPHIA_UPLOAD_EMPTY);
    teardown();
}
static void refusal_and_transactions(void)
{
    setup(); unsigned slot=stage(); struct sophia_shell_frame f=pump(); uint64_t begin=f.transaction;
    struct sophia_shell_upload_snapshot s=inspect(slot);
    assert(s.key.resource.id==1 && s.key.resource.generation==1);
    assert(reply(s.key,begin+10,1,0,8)==SOPHIA_SHELL_INVALID);
    assert(reply(s.key,begin,1,0,7)==SOPHIA_SHELL_INVALID);
    assert(inspect(slot).state==SOPHIA_UPLOAD_BEGIN_PENDING);
    assert(reply(s.key,begin,3,1,0)==SOPHIA_SHELL_OK);
    assert(inspect(slot).state==SOPHIA_UPLOAD_EMPTY);
    assert(stage()==slot); resident(slot,0); s=inspect(slot);
    assert(s.key.resource.id==1 && s.key.resource.generation==2);
    uint64_t rt=retire(slot);
    assert(reply(s.key,rt,3,2,0)==SOPHIA_SHELL_OK); /* Retire refusal is NOT release. */
    assert(inspect(slot).state==SOPHIA_UPLOAD_RESIDENT && inspect(slot).bytes==8);
    assert(reply(s.key,rt,0,0,0)==SOPHIA_SHELL_INVALID);
    uint64_t retry=retire(slot); assert(retry!=rt);
    assert(reply(s.key,rt,0,0,0)==SOPHIA_SHELL_INVALID);
    struct sophia_shell_resource_key wrong=s.key; ++wrong.grant.content_grant_epoch;
    assert(reply(wrong,retry,0,0,0)==SOPHIA_SHELL_INVALID);
    assert(reply(s.key,retry,0,0,0)==SOPHIA_SHELL_OK);
    assert(reply(s.key,retry,0,0,0)==SOPHIA_SHELL_INVALID);
    teardown();
}
static void cancel_and_timeout(void)
{
    setup(); unsigned slot=stage(); assert(sophia_shell_upload_cancel(owner,slot)==SOPHIA_SHELL_OK);
    assert(!inspect(slot).key.resource.id && !inspect(slot).bytes && next_tx==1);
    assert(stage()==slot); struct sophia_shell_frame f=pump(); uint64_t begin=f.transaction;
    struct sophia_shell_upload_snapshot s=inspect(slot);
    assert(sophia_shell_upload_cancel(owner,slot)==SOPHIA_SHELL_OK);
    assert(reply(s.key,begin,1,0,8)==SOPHIA_SHELL_OK);
    f=pump(); assert(f.kind==169);
    assert(reply(s.key,f.transaction,4,11,0)==SOPHIA_SHELL_OK);
    assert(stage()==slot); f=pump(); begin=f.transaction; s=inspect(slot);
    assert(reply(s.key,begin,1,0,8)==SOPHIA_SHELL_OK);
    f=pump(); assert(f.kind==167); uint64_t chunk=f.transaction;
    f=pump(); assert(f.kind==168); uint64_t end=f.transaction;
    assert(sophia_shell_upload_cancel(owner,slot)==SOPHIA_SHELL_OK);
    assert(reply(s.key,chunk,2,0,8)==SOPHIA_SHELL_INVALID);
    assert(reply(s.key,end,2,0,8)==SOPHIA_SHELL_OK);
    assert(inspect(slot).state==SOPHIA_UPLOAD_RETIRE_READY);
    f=pump(); assert(f.kind==170); assert(reply(s.key,f.transaction,0,0,0)==SOPHIA_SHELL_OK);
    assert(stage()==slot); f=pump(); begin=f.transaction; s=inspect(slot);
    assert(reply(s.key,begin,1,0,8)==SOPHIA_SHELL_OK);
    f=pump(); assert(f.kind==167); f=pump(); assert(f.kind==168);
    assert(reply(s.key,begin,3,6,0)==SOPHIA_SHELL_OK); /* Timeout keeps original Begin transaction. */
    assert(inspect(slot).state==SOPHIA_UPLOAD_EMPTY);
    teardown();
}
static void staging_bounds(void)
{
    setup(); unsigned slot=99; uint8_t pixel[4]={255,0,0,0};
    assert(sophia_shell_upload_stage(owner,1,1,1,1,pixel,4,&slot)==SOPHIA_SHELL_INVALID && slot==99);
    memset(pixel,255,4);
    fail_allocate=1;
    assert(sophia_shell_upload_stage(owner,1,1,1,1,pixel,4,&slot)==SOPHIA_SHELL_BUSY && slot==99);
    assert(owned_bytes==owner_bytes && inspect(0).state==SOPHIA_UPLOAD_EMPTY);
    assert(sophia_shell_upload_stage(owner,1,1,2,2,pixel,4,&slot)==SOPHIA_SHELL_INVALID && slot==99);
    assert(sophia_shell_upload_stage(owner,8193,1,1,1,pixel,4,&slot)==SOPHIA_SHELL_INVALID && slot==99);
    static uint8_t pixels[131072]; memset(pixels,255,sizeof(pixels));
    assert(sophia_shell_upload_stage(owner,128,256,5,4,pixels,sizeof(pixels),&slot)==SOPHIA_SHELL_OK);
    memset(pixels,0,sizeof(pixels)); /* Owner must upload its immutable copy. */
    resident(slot,1); assert(inspect(slot).bytes==sizeof(pixels));
    teardown();
}
static void blocked_begin(void)
{
    setup(); unsigned slot=stage();
    uint8_t bytes[64]; size_t n;
    struct sophia_shell_resource_key key={{11,3},{55,1}};
    assert(sophia_shell_resource_control_encode(bytes,sizeof(bytes),170,999,&key,&n)==SOPHIA_SHELL_OK);
    struct sophia_shell_outbound_frame frame={bytes,n,SOPHIA_SHELL_OUTBOUND_BULK};
    for (unsigned i=0; i<4; ++i) assert(sophia_shell_outbox_push(&outbox,&frame,1)==SOPHIA_SHELL_OK);
    assert(sophia_shell_upload_pump(owner,&outbox,&next_tx)==SOPHIA_SHELL_BUSY);
    assert(next_tx==1 && inspect(slot).state==SOPHIA_UPLOAD_STAGED);
    assert(!inspect(slot).key.resource.id && !inspect(slot).key.resource.generation);
    teardown(); /* No Begin ever entered this deliberately saturated connection. */
}
static void tightened_limits(void)
{
    uint8_t saved[264]; memcpy(saved,limits.payload,sizeof(saved));
    uint8_t *p=limits_bytes+24;
    shell_put32(p+96,1); shell_put32(p+100,1);
    setup(); stage(); unsigned slot=99; uint8_t pixels[8]={0};
    assert(sophia_shell_upload_stage(owner,2,1,1,1,pixels,8,&slot)==SOPHIA_SHELL_BUSY && slot==99);
    teardown(); memcpy(p,saved,sizeof(saved));
    shell_put32(p+88,1); shell_put32(p+84,4); /* Coherent bound, incompatible prototype chunk shape. */
    setup();
    assert(sophia_shell_upload_stage(owner,1,2,1,1,pixels,8,&slot)==SOPHIA_SHELL_INVALID && slot==99);
    teardown(); memcpy(p,saved,sizeof(saved));
}
static void aggregate_and_transfer_limits(void)
{
    uint8_t saved[264]; memcpy(saved,limits.payload,sizeof(saved));
    uint8_t *p=limits_bytes+24;
    shell_put32(p+104,1);
    setup(); unsigned first=stage(), second=stage(); assert(first!=second);
    struct sophia_shell_frame f=pump(); uint64_t begin=f.transaction;
    struct sophia_shell_upload_snapshot a=inspect(first);
    assert(a.key.resource.id==1 && !inspect(second).key.resource.id);
    assert(sophia_shell_upload_pump(owner,&outbox,&next_tx)==SOPHIA_SHELL_BUSY && next_tx==2);
    assert(reply(a.key,begin,1,0,8)==SOPHIA_SHELL_OK);
    f=pump(); assert(f.kind==167); f=pump(); assert(f.kind==168);
    assert(reply(a.key,f.transaction,2,0,8)==SOPHIA_SHELL_OK);
    f=pump(); assert(f.kind==165 && inspect(second).key.resource.id==2);
    assert(owned_bytes==owner_bytes+16);
    teardown(); memcpy(p,saved,sizeof(saved));
    for (size_t off=24; off<=48; off+=8) shell_put64(p+off,8);
    shell_put64(p+56,24);
    setup(); first=stage(); resident(first,0); a=inspect(first);
    uint64_t tx=retire(first); unsigned slot=99; uint8_t pixels[8]={0};
    assert(sophia_shell_upload_stage(owner,2,1,1,1,pixels,8,&slot)==SOPHIA_SHELL_BUSY && slot==99);
    assert(reply(a.key,tx,0,0,0)==SOPHIA_SHELL_OK);
    assert(sophia_shell_upload_stage(owner,2,1,1,1,pixels,8,&slot)==SOPHIA_SHELL_OK);
    teardown(); memcpy(p,saved,sizeof(saved));
}
int main(int argc,char **argv)
{
    assert(argc==2); FILE *file=fopen(argv[1],"r"); assert(file);
    char line[4096]; int found=0;
    while (fgets(line,sizeof(line),file)) {
        if (strncmp(line,"content-161 ",12)) continue;
        char *hex=line+12; size_t n=strcspn(hex,"\r\n"); assert(n==sizeof(limits_bytes)*2);
        for (size_t i=0; i<n/2; ++i) limits_bytes[i]=(uint8_t)(16*nibble(hex[2*i])+nibble(hex[2*i+1]));
        found=1; break;
    }
    fclose(file); assert(found);
    assert(sophia_shell_frame_decode(limits_bytes,sizeof(limits_bytes),&limits)==SOPHIA_SHELL_OK);
    fail_allocate=1; struct sophia_shell_upload *failed=NULL;
    assert(sophia_shell_upload_new(&limits,&failed)==SOPHIA_SHELL_BUSY && !failed && !owned_bytes);
    aggregate_and_transfer_limits(); held_and_reuse(); refusal_and_transactions(); cancel_and_timeout(); staging_bounds(); blocked_begin(); tightened_limits();
    puts("sophia_shell_upload private_socket=pass held_release_cycles=1000 supplied_server=true native=false");
    return 0;
}

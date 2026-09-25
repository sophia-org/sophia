/* t100 reduced initial welcome limits through the independent C upload owner
 * (the files bemenu vendors). Floor profile S=M, R=2M, T=M with the resource
 * bound M unchanged at 4 MiB: one transfer stages at a time, two full-size
 * resources can be resident, and one full-size retirement is in flight. A
 * reduced value below M is refused at decode. Supplied server replies only. */
#include "../sophia_shell_upload.h"
#include "../shell_wire/fields.h"
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#define MIB (UINT64_C(1024)*1024)
static uint8_t golden[288], limits_bytes[288], peer_bytes[SOPHIA_SHELL_MAX_FRAME_BYTES];
static uint8_t rx[SOPHIA_SHELL_MAX_FRAME_BYTES], tx_bytes[SOPHIA_SHELL_MAX_FRAME_BYTES];
static uint8_t pixels[4*1024*1024];
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
/* Limits payload offsets: staging 32, resident 40, retiring 48, session 56. */
static void profile(uint64_t staging, uint64_t resident, uint64_t retiring, uint64_t session)
{
    memcpy(limits_bytes,golden,sizeof(golden));
    shell_put64(limits_bytes+24+32,staging); shell_put64(limits_bytes+24+40,resident);
    shell_put64(limits_bytes+24+48,retiring); shell_put64(limits_bytes+24+56,session);
    assert(sophia_shell_frame_decode(limits_bytes,sizeof(limits_bytes),&limits)==SOPHIA_SHELL_OK);
}
static struct sophia_shell_upload_snapshot inspect(unsigned slot)
{
    struct sophia_shell_upload_snapshot s;
    assert(sophia_shell_upload_inspect(owner,slot,&s)==SOPHIA_SHELL_OK); return s;
}
static void setup(void)
{
    assert(socketpair(AF_UNIX,SOCK_STREAM,0,sockets)==0);
    assert(sophia_shell_wire_init(&wire,sockets[0],rx,sizeof(rx),tx_bytes,sizeof(tx_bytes))==SOPHIA_SHELL_OK);
    assert(sophia_shell_outbox_init(&outbox,131072,8,1024,4)==SOPHIA_SHELL_OK);
    assert(sophia_shell_upload_new(&limits,&owner)==SOPHIA_SHELL_OK); next_tx=1;
}
static void teardown(void)
{
    close(sockets[0]); close(sockets[1]);
    sophia_shell_outbox_dispose(&outbox); sophia_shell_upload_dispose(owner); owner=NULL;
}
static struct sophia_shell_frame drain(void)
{
    size_t used=0; unsigned visits=0;
    while (outbox.count) {
        assert(++visits<100000);
        int r=sophia_shell_outbox_flush(&outbox,sockets[0],65536);
        assert(r==SOPHIA_SHELL_OK || r==SOPHIA_SHELL_AGAIN);
        ssize_t n=recv(sockets[1],peer_bytes+used,sizeof(peer_bytes)-used,MSG_DONTWAIT);
        if (n<0) assert(errno==EAGAIN || errno==EWOULDBLOCK); else {assert(n>0); used+=(size_t)n;}
    }
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
/* One full-size resource: 1024 x 1024 opaque BGRA is exactly M. */
static int stage(unsigned *slot)
{
    return sophia_shell_upload_stage(owner,1024,1024,1,1,pixels,sizeof(pixels),slot);
}
static void resident(unsigned slot)
{
    struct sophia_shell_frame f=pump(); assert(f.kind==165);
    struct sophia_shell_upload_snapshot s=inspect(slot);
    assert(s.state==SOPHIA_UPLOAD_BEGIN_PENDING && s.bytes==4*MIB);
    assert(reply(s.key,f.transaction,1,0,s.bytes)==SOPHIA_SHELL_OK);
    uint64_t offset=0;
    while ((f=pump()).kind==167) offset+=shell_get32(f.payload+36);
    assert(f.kind==168 && offset==s.bytes);
    assert(reply(s.key,f.transaction,2,0,s.bytes)==SOPHIA_SHELL_OK);
    assert(inspect(slot).state==SOPHIA_UPLOAD_RESIDENT);
}
static void floor_profile(void)
{
    profile(4*MIB,8*MIB,4*MIB,64*MIB);
    setup();
    unsigned first=99, second=99;
    assert(stage(&first)==SOPHIA_SHELL_OK);
    /* S=M: a second full-size transfer waits for the first, it is not refused. */
    assert(stage(&second)==SOPHIA_SHELL_BUSY && second==99);
    resident(first);
    /* R=2M: the second full-size resource fits beside the first. */
    assert(stage(&second)==SOPHIA_SHELL_OK && second!=first);
    resident(second);
    /* T=M: one full-size retirement in flight; a second waits for Released. */
    assert(sophia_shell_upload_retire(owner,first)==SOPHIA_SHELL_OK);
    struct sophia_shell_frame f=pump(); assert(f.kind==170);
    uint64_t released=f.transaction; struct sophia_shell_resource_key key=inspect(first).key;
    assert(inspect(first).state==SOPHIA_UPLOAD_RELEASE_PENDING);
    assert(sophia_shell_upload_retire(owner,second)==SOPHIA_SHELL_OK);
    uint64_t tx=next_tx;
    assert(sophia_shell_upload_pump(owner,&outbox,&next_tx)==SOPHIA_SHELL_BUSY && next_tx==tx);
    assert(inspect(second).state==SOPHIA_UPLOAD_RETIRE_READY);
    assert(reply(key,released,0,0,0)==SOPHIA_SHELL_OK);
    f=pump(); assert(f.kind==170);
    assert(inspect(second).state==SOPHIA_UPLOAD_RELEASE_PENDING);
    teardown();
}
static void incoherent_profiles(void)
{
    const uint64_t below=4*MIB-4;
    const uint64_t cases[][4]={
        {below,8*MIB,4*MIB,64*MIB},
        {4*MIB,below,4*MIB,64*MIB},
        {4*MIB,8*MIB,below,64*MIB},
        {4*MIB,8*MIB,4*MIB,16*MIB-4},
    };
    for (size_t i=0; i<sizeof(cases)/sizeof(cases[0]); ++i) {
        profile(cases[i][0],cases[i][1],cases[i][2],cases[i][3]);
        struct sophia_shell_content_limits decoded;
        assert(sophia_shell_content_limits_decode(&limits,&decoded)==SOPHIA_SHELL_INVALID);
        struct sophia_shell_upload *refused=NULL;
        assert(sophia_shell_upload_new(&limits,&refused)==SOPHIA_SHELL_INVALID && !refused);
    }
}
int main(int argc,char **argv)
{
    assert(argc==2); FILE *file=fopen(argv[1],"r"); assert(file);
    char line[4096]; int found=0;
    while (fgets(line,sizeof(line),file)) {
        if (strncmp(line,"content-161 ",12)) continue;
        char *hex=line+12; size_t n=strcspn(hex,"\r\n"); assert(n==sizeof(golden)*2);
        for (size_t i=0; i<n/2; ++i) golden[i]=(uint8_t)(16*nibble(hex[2*i])+nibble(hex[2*i+1]));
        found=1; break;
    }
    fclose(file); assert(found);
    memset(pixels,255,sizeof(pixels));
    floor_profile(); incoherent_profiles();
    puts("sophia_shell_reduced_limits floor=4/8/4 resource=4MiB incoherent=refused supplied_server=true native=false");
    return 0;
}

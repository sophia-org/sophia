#include "../sophia_shell_native_lifecycle.h"
#include "../shell_wire/fields.h"
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int __real_sophia_shell_outbox_commit(struct sophia_shell_outbox *,struct sophia_shell_outbox_reservation,
                                    const struct sophia_shell_outbound_frame *,unsigned);
static unsigned fail_commit, commits;
int __wrap_sophia_shell_outbox_commit(struct sophia_shell_outbox *o,struct sophia_shell_outbox_reservation t,
                                    const struct sophia_shell_outbound_frame *f,unsigned n)
{
    ++commits;
    if (fail_commit) {--fail_commit; return SOPHIA_SHELL_BUSY;}
    return __real_sophia_shell_outbox_commit(o,t,f,n);
}
static struct sophia_shell_native_lifecycle *owner;
static struct sophia_shell_outbox outbox;
static uint64_t tx;
static struct sophia_shell_wire inbound;
static uint8_t inbound_rx[1024],inbound_tx[24],inbound_send[1024];
static int inbound_pending;
static int sockets[2];
static uint8_t limits_bytes[288],wire_bytes[1024],received[8192];
static struct sophia_shell_frame limits;
static unsigned edits, refuse_edit;
static int edit(void *unused,uint16_t kind,const uint8_t *text,size_t bytes)
{
    (void)unused;
    assert(kind==1 && bytes==1 && text[0]=='a');
    assert(outbox.reservation_count==1 && outbox.count && outbox.bytes>=148);
    ++edits; return !refuse_edit;
}
static struct sophia_shell_native_lifecycle_snapshot inspect(void)
{
    struct sophia_shell_native_lifecycle_snapshot v;
    assert(sophia_shell_native_lifecycle_inspect(owner,&v)==SOPHIA_SHELL_OK); return v;
}
static void setup(void)
{
    assert(socketpair(AF_UNIX,SOCK_STREAM,0,sockets)==0);
    assert(sophia_shell_outbox_init(&outbox,65536,16,1024,4)==SOPHIA_SHELL_OK);
    assert(sophia_shell_native_lifecycle_new(&limits,9,&outbox,&owner)==SOPHIA_SHELL_OK);
    tx=1; edits=refuse_edit=fail_commit=commits=0; inbound_pending=0;
    assert(sophia_shell_wire_init(&inbound,sockets[0],inbound_rx,sizeof(inbound_rx),inbound_tx,sizeof(inbound_tx))==SOPHIA_SHELL_OK);
}
static void teardown(void)
{
    close(sockets[0]); close(sockets[1]); sophia_shell_outbox_dispose(&outbox);
    sophia_shell_native_lifecycle_dispose(owner);
}
static struct sophia_shell_frame frame(uint16_t kind,uint64_t transaction,const uint8_t *payload,size_t bytes)
{
    size_t n;
    assert(sophia_shell_frame_encode(wire_bytes,sizeof(wire_bytes),kind,transaction,payload,bytes,&n)==SOPHIA_SHELL_OK);
    struct sophia_shell_frame f;
    assert(sophia_shell_frame_decode(wire_bytes,n,&f)==SOPHIA_SHELL_OK); return f;
}
static int receive(struct sophia_shell_frame f)
{
    if (!inbound_pending) {
        size_t n;
        assert(sophia_shell_frame_encode(inbound_send,sizeof(inbound_send),f.kind,f.transaction,f.payload,f.payload_bytes,&n)==SOPHIA_SHELL_OK);
        assert(send(sockets[1],inbound_send,n,MSG_NOSIGNAL)==(ssize_t)n);
    }
    struct sophia_shell_frame actual;
    assert(sophia_shell_wire_receive(&inbound,sizeof(inbound_rx),&actual)==SOPHIA_SHELL_FRAME);
    assert(actual.kind==f.kind && actual.transaction==f.transaction && actual.payload_bytes==f.payload_bytes);
    assert(!memcmp(actual.payload,f.payload,f.payload_bytes));
    int r=sophia_shell_native_lifecycle_receive(owner,&actual,&tx,edit,NULL);
    inbound_pending=r==SOPHIA_SHELL_BUSY;
    if (!inbound_pending) assert(sophia_shell_wire_consume(&inbound)==SOPHIA_SHELL_OK);
    return r;
}
static size_t drain(uint16_t first,uint16_t second,uint16_t third)
{
    size_t total=outbox.bytes,used=0; unsigned turns=0;
    assert(total<=sizeof(received));
    while (outbox.count || used<total) {
        assert(++turns<1000);
        int r=sophia_shell_outbox_flush(&outbox,sockets[0],256);
        assert(r==SOPHIA_SHELL_OK || r==SOPHIA_SHELL_AGAIN);
        ssize_t n=recv(sockets[1],received+used,sizeof(received)-used,MSG_DONTWAIT);
        if (n<0) assert(errno==EAGAIN || errno==EWOULDBLOCK); else {assert(n>0); used+=(size_t)n;}
    }
    size_t offset=0; uint16_t kinds[]={first,second,third};
    for (unsigned i=0; i<3 && kinds[i]; ++i) {
        assert(offset+24<=used); size_t n=24+shell_get32(received+offset+16);
        assert(n<=used-offset); struct sophia_shell_frame f;
        assert(sophia_shell_frame_decode(received+offset,n,&f)==SOPHIA_SHELL_OK && f.kind==kinds[i]); offset+=n;
    }
    assert(offset==used); return used;
}
static void grant(uint8_t *p) {shell_put64(p,11); shell_put64(p+8,3);}
static int opening(uint64_t id,uint64_t catalog)
{
    uint8_t p[56]={0}; grant(p); shell_put64(p+16,id); shell_put64(p+24,2); shell_put64(p+32,7);
    shell_put64(p+40,catalog); shell_put64(p+48,1); return receive(frame(187,90,p,sizeof(p)));
}
static void scene(uint64_t revision)
{
    struct sophia_shell_native_candidate b={{11,3},0,{2,7},1,2,revision,1,1,9,revision,2,2,{1,2}};
    struct sophia_shell_native_chunk c={.grant={11,3},.surface_count=1,.placement_count=1,.target_count=2,
        .surface={{1,2},1,1,{0}},.placements={{{9,1},0,0}},
        .targets={{1,1,1,0,0,20,10},{2,1,2,0,10,20,10}}};
    assert(sophia_shell_native_lifecycle_offer(owner,&b,&c,&tx)==SOPHIA_SHELL_OK);
    assert(sophia_shell_native_lifecycle_pump(owner,&tx)==SOPHIA_SHELL_OK);
}
static struct sophia_shell_frame outcome(uint64_t request,uint64_t generation,uint16_t kind,uint64_t epoch)
{
    uint8_t p[68]={0}; grant(p); shell_put64(p+16,generation); shell_put64(p+24,2); shell_put64(p+32,7);
    shell_put16(p+40,kind); shell_put16(p+42,kind>2?1:0); shell_put64(p+44,epoch);
    return frame(175,request,p,sizeof(p));
}
static void binding(uint8_t *p,uint64_t generation,uint64_t epoch,uint64_t revision,uint64_t lease)
{
    uint64_t fields[]={11,3,1,2,7,1,2,9,generation,epoch,revision,revision,lease};
    for (unsigned i=0; i<13; ++i) shell_put64(p+8*i,fields[i]);
}
static struct sophia_shell_frame focus(uint64_t generation,uint64_t epoch,uint64_t revision,uint64_t lease)
{
    uint8_t p[104]; binding(p,generation,epoch,revision,lease); return frame(191,92,p,sizeof(p));
}
static struct sophia_shell_frame input(uint64_t generation,uint64_t epoch,uint64_t bound_revision,
                                     uint64_t lease,uint64_t event,uint64_t revision,int accept)
{
    uint8_t p[133]={0}; binding(p,generation,epoch,bound_revision,lease);
    shell_put64(p+104,event); shell_put64(p+112,revision); shell_put64(p+120,100+event);
    shell_put16(p+128,accept?17:1); shell_put16(p+130,accept?0:1); p[132]='a';
    return frame(193,100+event,p,accept?132:133);
}
static void presented(void)
{
    assert(opening(1,9)==SOPHIA_SHELL_OK); scene(1); drain(189,190,174);
    assert(receive(outcome(1,1,1,0))==SOPHIA_SHELL_OK);
    assert(receive(outcome(1,1,2,10))==SOPHIA_SHELL_OK);
    assert(receive(focus(1,10,1,1))==SOPHIA_SHELL_OK);
}
static void presentation_and_edit(void)
{
    setup(); assert(opening(1,9)==SOPHIA_SHELL_OK); scene(1); drain(189,190,174);
    assert(receive(focus(1,10,1,1))==SOPHIA_SHELL_INVALID);
    assert(receive(outcome(1,1,2,10))==SOPHIA_SHELL_INVALID && !inspect().presented);
    assert(receive(outcome(2,1,1,0))==SOPHIA_SHELL_INVALID);
    assert(receive(outcome(1,1,1,0))==SOPHIA_SHELL_OK && !inspect().presented);
    assert(receive(input(1,10,1,1,1,2,0))==SOPHIA_SHELL_INVALID && !edits && !outbox.count);
    assert(receive(focus(1,10,1,1))==SOPHIA_SHELL_INVALID);
    assert(receive(outcome(1,1,2,10))==SOPHIA_SHELL_OK && inspect().presented);
    for (unsigned i=0; i<12; ++i) {
        struct sophia_shell_frame f=focus(1,10,1,1); uint8_t p[104]; memcpy(p,f.payload,sizeof(p));
        shell_put64(p+8*i,shell_get64(p+8*i)+1);
        assert(receive(frame(191,92,p,sizeof(p)))==SOPHIA_SHELL_INVALID && !inspect().focused);
    }
    assert(receive(focus(1,10,1,1))==SOPHIA_SHELL_OK);
    for (unsigned i=0; i<13; ++i) {
        struct sophia_shell_frame f=input(1,10,1,1,1,2,0); uint8_t p[133]; memcpy(p,f.payload,sizeof(p));
        shell_put64(p+8*i,shell_get64(p+8*i)+1);
        assert(receive(frame(193,101,p,sizeof(p)))==SOPHIA_SHELL_INVALID && !edits);
    }
    struct sophia_shell_frame f=input(1,10,1,1,1,2,0); fail_commit=1; uint64_t before=tx;
    assert(receive(f)==SOPHIA_SHELL_BUSY && edits==1 && tx==before+1 && inspect().state_revision==2);
    assert(outbox.count==1 && outbox.reservation_count==1);
    assert(sophia_shell_outbox_flush(&outbox,sockets[0],1024)==SOPHIA_SHELL_AGAIN);
    assert(receive(f)==SOPHIA_SHELL_OK && edits==1 && tx==before+1 && commits==2);
    drain(194,0,0); assert(shell_get16(received+24+120)==1);
    assert(receive(f)==SOPHIA_SHELL_INVALID && edits==1); /* Exact replay cannot edit twice. */
    assert(receive(input(1,10,1,1,2,1,1))==SOPHIA_SHELL_OK);
    drain(194,0,0); assert(shell_get16(received+24+120)==2 && !inspect().activation_pending);
    before=tx; scene(2); drain(189,190,174);
    assert(receive(outcome(before,2,1,0))==SOPHIA_SHELL_OK && inspect().candidate_generation==1);
    assert(receive(outcome(before,2,2,11))==SOPHIA_SHELL_OK && !inspect().focused);
    uint8_t revoke[108]={0}; binding(revoke,1,10,1,1); shell_put16(revoke+104,1);
    assert(receive(frame(192,200,revoke,sizeof(revoke)))==SOPHIA_SHELL_OK);
    assert(receive(focus(2,11,2,2))==SOPHIA_SHELL_OK);
    refuse_edit=1;
    assert(receive(input(2,11,2,2,3,3,0))==SOPHIA_SHELL_OK && edits==2 && inspect().state_revision==3);
    drain(194,0,0); assert(shell_get16(received+24+120)==2);
    teardown();
}
static void saturation_before_effect(void)
{
    setup(); presented(); uint8_t p[32]={0}; grant(p); shell_put64(p+16,1); shell_put64(p+24,1);
    struct sophia_shell_frame f=frame(170,99,p,sizeof(p));
    struct sophia_shell_outbound_frame full={wire_bytes,f.payload_bytes+24,SOPHIA_SHELL_OUTBOUND_CONTROL};
    for (unsigned i=0; i<16; ++i) assert(sophia_shell_outbox_push(&outbox,&full,1)==SOPHIA_SHELL_OK);
    uint64_t before=tx;
    assert(receive(input(1,10,1,1,1,2,0))==SOPHIA_SHELL_BUSY);
    assert(!edits && tx==before && inspect().state_revision==1 && outbox.count==16);
    /* Disposing this deliberately saturated connection is not input acceptance. */
    teardown();
}
static void keyboard_activation(void)
{
    setup(); presented(); uint64_t ack_tx=tx;
    struct sophia_shell_frame f=input(1,10,1,1,1,1,1);
    assert(receive(f)==SOPHIA_SHELL_OK && !edits && inspect().activation_pending);
    drain(194,195,0);
    assert(shell_get64(received+8)==ack_tx && shell_get64(received+148+8)==ack_tx+1);
    assert(shell_get16(received+148+24+120)==1 && shell_get16(received+148+24+122)==2);
    uint8_t activation[124]; memcpy(activation,received+148+24,sizeof(activation));
    assert(receive(input(1,10,1,1,2,1,1))==SOPHIA_SHELL_OK); drain(194,0,0);
    assert(shell_get16(received+24+120)==2 && inspect().activation_pending);
    uint8_t result[128]={0}; memcpy(result,activation,sizeof(activation)); shell_put16(result+124,1);
    assert(receive(frame(196,ack_tx+2,result,sizeof(result)))==SOPHIA_SHELL_INVALID && inspect().activation_pending);
    shell_put16(result+122,1);
    assert(receive(frame(196,ack_tx+1,result,sizeof(result)))==SOPHIA_SHELL_INVALID && inspect().activation_pending);
    shell_put16(result+122,2);
    assert(receive(frame(196,ack_tx+1,result,sizeof(result)))==SOPHIA_SHELL_OK);
    assert(!inspect().activation_pending && inspect().closing && !inspect().focused);
    teardown();
}
static struct sophia_shell_frame action(uint64_t event,uint16_t kind,uint64_t target_generation)
{
    uint8_t p[112]={0}; grant(p);
    uint64_t fields[]={2,7,1,10,1,1,2,1,target_generation,1,event};
    for (unsigned i=0; i<11; ++i) shell_put64(p+16+8*i,fields[i]);
    shell_put16(p+104,kind); shell_put16(p+106,kind==3?1:0);
    if (kind==2) memset(p+72,0,24);
    return frame(179,300+event,p,sizeof(p));
}
static void pointer_and_catalog(void)
{
    setup(); assert(opening(1,9)==SOPHIA_SHELL_OK);
    assert(receive(action(1,1,1))==SOPHIA_SHELL_INVALID && !outbox.count);
    scene(1); drain(189,190,174); assert(receive(outcome(1,1,1,0))==SOPHIA_SHELL_OK);
    assert(receive(action(1,1,1))==SOPHIA_SHELL_INVALID && !outbox.count);
    assert(receive(outcome(1,1,2,10))==SOPHIA_SHELL_OK);
    assert(receive(focus(1,10,1,1))==SOPHIA_SHELL_OK);
    assert(receive(action(1,1,2))==SOPHIA_SHELL_OK); drain(180,0,0);
    assert(shell_get16(received+24+104)==2 && !inspect().activation_pending);
    uint64_t ack_tx=tx;
    assert(receive(action(2,1,1))==SOPHIA_SHELL_OK); drain(180,195,0);
    assert(shell_get16(received+136+24+120)==2 && shell_get16(received+136+24+122)==1);
    uint8_t result[128]={0}; memcpy(result,received+136+24,124); shell_put16(result+124,5); shell_put16(result+126,2);
    assert(receive(action(2,3,1))==SOPHIA_SHELL_OK && !outbox.count && inspect().activation_pending);
    assert(receive(frame(196,ack_tx+1,result,sizeof(result)))==SOPHIA_SHELL_OK && !inspect().activation_pending);
    assert(sophia_shell_native_lifecycle_catalog(owner,10)==SOPHIA_SHELL_OK && !inspect().focused);
    assert(receive(input(1,10,1,1,1,2,0))==SOPHIA_SHELL_INVALID && !edits);
    uint8_t p[28]={0}; grant(p); shell_put64(p+16,1); shell_put16(p+24,1);
    assert(receive(frame(197,400,p,sizeof(p)))==SOPHIA_SHELL_OK && !inspect().open && !inspect().presented);
    assert(opening(1,10)==SOPHIA_SHELL_INVALID);
    assert(opening(2,9)==SOPHIA_SHELL_INVALID);
    assert(opening(2,10)==SOPHIA_SHELL_OK && inspect().state_revision==1);
    teardown();
}
static void old_outcome_after_close(void)
{
    setup(); presented(); uint64_t ack_tx=tx;
    assert(receive(input(1,10,1,1,1,1,1))==SOPHIA_SHELL_OK); drain(194,195,0);
    uint8_t result[128]={0}; memcpy(result,received+148+24,124); shell_put16(result+124,1);
    uint8_t p[28]={0}; grant(p); shell_put64(p+16,1); shell_put16(p+24,11);
    assert(receive(frame(197,400,p,sizeof(p)))==SOPHIA_SHELL_OK);
    assert(inspect().activation_pending && !inspect().open);
    assert(opening(2,9)==SOPHIA_SHELL_OK);
    assert(receive(frame(196,ack_tx+1,result,sizeof(result)))==SOPHIA_SHELL_OK);
    assert(inspect().open && !inspect().closing && !inspect().activation_pending);
    teardown();
}
static void rejected_candidate_preserves_shown(void)
{
    setup(); presented(); uint64_t request=tx; scene(1); drain(189,190,174);
    assert(inspect().candidate_pending && inspect().candidate_generation==1 && inspect().focused);
    assert(receive(outcome(request,2,3,0))==SOPHIA_SHELL_OK);
    assert(!inspect().candidate_pending && inspect().candidate_generation==1 && inspect().focused);
    assert(receive(action(1,2,0))==SOPHIA_SHELL_OK); drain(180,0,0);
    assert(inspect().closing && !inspect().focused && shell_get16(received+24+104)==1);
    teardown();
}
static unsigned nibble(char c)
{
    if (c>='0' && c<='9') return (unsigned)(c-'0');
    if (c>='a' && c<='f') return (unsigned)(c-'a')+10;
    abort();
}
int main(int argc,char **argv)
{
    assert(argc==2); FILE *file=fopen(argv[1],"r"); assert(file);
    static char line[4096]; int found=0;
    while (fgets(line,sizeof(line),file)) {
        if (strncmp(line,"content-161 ",12)) continue;
        char *hex=line+12; size_t n=strcspn(hex,"\r\n"); assert(n==sizeof(limits_bytes)*2);
        for (size_t i=0; i<n/2; ++i) limits_bytes[i]=(uint8_t)(16*nibble(hex[2*i])+nibble(hex[2*i+1]));
        found=1; break;
    }
    fclose(file); assert(found);
    assert(sophia_shell_frame_decode(limits_bytes,sizeof(limits_bytes),&limits)==SOPHIA_SHELL_OK);
    presentation_and_edit(); saturation_before_effect(); keyboard_activation(); pointer_and_catalog(); old_outcome_after_close(); rejected_candidate_preserves_shown();
    puts("sophia_shell_native_lifecycle fifo=pass reserved_edit=pass supplied_presentation=true native=false");
    return 0;
}

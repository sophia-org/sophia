#include "../sophia_shell_outbox.h"
#include "../sophia_shell_native_launcher.h"
#include "../sophia_shell_content_resource.h"
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

void *__real_malloc(size_t);
void __real_free(void *);
ssize_t __real_send(int,const void *,size_t,int);
static unsigned allocation_call, allocation_fail, allocations, frees, sends;
static int interrupted;
void *__wrap_malloc(size_t n)
{
    ++allocation_call;
    if (allocation_fail && allocation_call==allocation_fail) return NULL;
    void *p=__real_malloc(n); if (p) ++allocations; return p;
}
void __wrap_free(void *p) {if (p) ++frees; __real_free(p);}
ssize_t __wrap_send(int fd,const void *p,size_t n,int flags)
{
    ++sends;
    if (interrupted) {errno=EINTR; return -1;}
    return __real_send(fd,p,n,flags);
}
static uint8_t ack_bytes[256], activation_bytes[256], bulk_bytes[SOPHIA_SHELL_MAX_FRAME_BYTES];
static struct sophia_shell_outbound_frame pair[2], bulk;
static void frames(void)
{
    const struct sophia_shell_native_event e={
        {{11,3},4,{2,7},{1,2},9,5,6,3,2,8},10,2};
    struct sophia_shell_native_ack ack={e,1};
    struct sophia_shell_native_activation activation={e,1,1};
    size_t an,bn,cn;
    assert(sophia_shell_native_ack_encode(ack_bytes,sizeof(ack_bytes),1,&ack,&an)==SOPHIA_SHELL_OK);
    assert(sophia_shell_native_activation_encode(activation_bytes,sizeof(activation_bytes),2,&activation,&bn)==SOPHIA_SHELL_OK);
    static uint8_t pixels[65488]; memset(pixels,255,sizeof(pixels));
    struct sophia_shell_resource_chunk chunk={{{11,3},{9,1}},0,0,pixels,sizeof(pixels)};
    assert(sophia_shell_resource_chunk_encode(bulk_bytes,sizeof(bulk_bytes),3,&chunk,&cn)==SOPHIA_SHELL_OK);
    pair[0]=(struct sophia_shell_outbound_frame){ack_bytes,an,SOPHIA_SHELL_OUTBOUND_CONTROL};
    pair[1]=(struct sophia_shell_outbound_frame){activation_bytes,bn,SOPHIA_SHELL_OUTBOUND_CONTROL};
    bulk=(struct sophia_shell_outbound_frame){bulk_bytes,cn,SOPHIA_SHELL_OUTBOUND_BULK};
}
static void atomic_refusal(void)
{
    struct sophia_shell_outbox out, saved;
    memset(&out,0xa5,sizeof(out)); memcpy(&saved,&out,sizeof(saved));
    assert(sophia_shell_outbox_init(&out,511,4,511,2)==SOPHIA_SHELL_ARGUMENT);
    assert(!memcmp(&out,&saved,sizeof(out)));
    assert(sophia_shell_outbox_init(&out,262145,4,512,2)==SOPHIA_SHELL_ARGUMENT);
    assert(sophia_shell_outbox_init(&out,2048,65,512,2)==SOPHIA_SHELL_ARGUMENT);
    assert(sophia_shell_outbox_init(&out,2048,4,256,1)==SOPHIA_SHELL_ARGUMENT);
    assert(sophia_shell_outbox_init(&out,2048,4,512,2)==SOPHIA_SHELL_OK);
    assert(sophia_shell_outbox_push(&out,pair,1)==SOPHIA_SHELL_OK);
    memcpy(&saved,&out,sizeof(saved));
    for (unsigned fail=1; fail<=2; ++fail) {
        allocation_call=0; allocation_fail=fail;
        unsigned a=allocations,f=frees;
        assert(sophia_shell_outbox_push(&out,pair,2)==SOPHIA_SHELL_BUSY);
        assert(!memcmp(&out,&saved,sizeof(out)));
        assert(allocations-a==frees-f && allocations-a==fail-1);
    }
    allocation_fail=0;
    struct sophia_shell_outbound_frame bad[2]={pair[0],pair[1]};
    --bad[1].length;
    unsigned a=allocations;
    assert(sophia_shell_outbox_push(&out,bad,2)==SOPHIA_SHELL_INVALID);
    assert(allocations==a && !memcmp(&out,&saved,sizeof(out)));
    assert(sophia_shell_outbox_push(&out,pair,2)==SOPHIA_SHELL_OK);
    memcpy(&saved,&out,sizeof(saved));
    assert(sophia_shell_outbox_push(&out,pair,2)==SOPHIA_SHELL_BUSY);
    assert(!memcmp(&out,&saved,sizeof(out))); /* Only one cell left: neither enters. */
    sophia_shell_outbox_dispose(&out);
}
static void byte_reservation(void)
{
    struct sophia_shell_outbox out;
    assert(sophia_shell_outbox_init(&out,bulk.length+512,64,512,2)==SOPHIA_SHELL_OK);
    assert(sophia_shell_outbox_push(&out,&bulk,1)==SOPHIA_SHELL_OK);
    struct sophia_shell_outbound_frame ordinary=pair[0]; ordinary.class=SOPHIA_SHELL_OUTBOUND_BULK;
    assert(sophia_shell_outbox_push(&out,&ordinary,1)==SOPHIA_SHELL_BUSY);
    assert(sophia_shell_outbox_push(&out,pair,2)==SOPHIA_SHELL_OK);
    assert(out.count==3 && out.bulk_records==1 && out.bytes==bulk.length+pair[0].length+pair[1].length);
    sophia_shell_outbox_dispose(&out);
    assert(sophia_shell_outbox_init(&out,262144,4,512,2)==SOPHIA_SHELL_OK);
    assert(sophia_shell_outbox_push(&out,&ordinary,1)==SOPHIA_SHELL_OK);
    assert(sophia_shell_outbox_push(&out,&ordinary,1)==SOPHIA_SHELL_OK);
    assert(sophia_shell_outbox_push(&out,&ordinary,1)==SOPHIA_SHELL_BUSY);
    assert(sophia_shell_outbox_push(&out,pair,2)==SOPHIA_SHELL_OK);
    sophia_shell_outbox_dispose(&out);
    ordinary=bulk; ordinary.class=SOPHIA_SHELL_OUTBOUND_CONTROL;
    assert(sophia_shell_outbox_init(&out,262144,4,512,2)==SOPHIA_SHELL_OK);
    assert(sophia_shell_outbox_push(&out,&ordinary,1)==SOPHIA_SHELL_INVALID);
    sophia_shell_outbox_dispose(&out);
}
static void socket_fifo(void)
{
    int fd[2]; assert(socketpair(AF_UNIX,SOCK_STREAM,0,fd)==0);
    int size=4096; assert(setsockopt(fd[0],SOL_SOCKET,SO_SNDBUF,&size,sizeof(size))==0);
    struct sophia_shell_outbox out;
    assert(sophia_shell_outbox_init(&out,262144,4,512,2)==SOPHIA_SHELL_OK);
    assert(sophia_shell_outbox_push(&out,&bulk,1)==SOPHIA_SHELL_OK);
    assert(sophia_shell_outbox_push(&out,pair,2)==SOPHIA_SHELL_OK);
    size_t charged=out.bytes; unsigned f=frees;
    sends=0;
    assert(sophia_shell_outbox_flush(&out,fd[0],0)==SOPHIA_SHELL_AGAIN && sends==0);
    assert(sophia_shell_outbox_flush(&out,fd[0],7)==SOPHIA_SHELL_AGAIN);
    assert(out.records[out.head].sent==7 && out.bytes==charged && out.count==3 && frees==f);
    assert(sophia_shell_outbox_flush(&out,fd[0],262144)==SOPHIA_SHELL_AGAIN);
    assert(out.records[out.head].sent>7 && out.records[out.head].sent<bulk.length);
    assert(out.bytes==charged && out.count==3 && frees==f); /* Kernel EAGAIN, real partial frame. */
    static uint8_t expected[70000], received[70000];
    memcpy(expected,bulk.bytes,bulk.length);
    memcpy(expected+bulk.length,pair[0].bytes,pair[0].length);
    memcpy(expected+bulk.length+pair[0].length,pair[1].bytes,pair[1].length);
    memset(ack_bytes,0,sizeof(ack_bytes)); memset(activation_bytes,0,sizeof(activation_bytes));
    size_t used=0; unsigned turns=0;
    while (out.count || used<charged) {
        assert(++turns<10000);
        ssize_t n=recv(fd[1],received+used,sizeof(received)-used,MSG_DONTWAIT);
        if (n<0) assert(errno==EAGAIN || errno==EWOULDBLOCK); else {assert(n>0); used+=(size_t)n;}
        int r=sophia_shell_outbox_flush(&out,fd[0],4096);
        assert(r==SOPHIA_SHELL_OK || r==SOPHIA_SHELL_AGAIN);
    }
    assert(used==charged && !memcmp(expected,received,used));
    assert(!out.bytes && !out.bulk_bytes && !out.bulk_records && frees==f+3);
    frames();
    /* Ring wrap remains ordered across repeated pairs. */
    for (unsigned i=0; i<1000; ++i) {
        assert(sophia_shell_outbox_push(&out,pair,2)==SOPHIA_SHELL_OK);
        assert(sophia_shell_outbox_flush(&out,fd[0],1024)==SOPHIA_SHELL_OK);
        size_t need=pair[0].length+pair[1].length; used=0;
        while (used<need) {ssize_t n=recv(fd[1],received+used,need-used,0); assert(n>0); used+=(size_t)n;}
        assert(!memcmp(received,pair[0].bytes,pair[0].length));
        assert(!memcmp(received+pair[0].length,pair[1].bytes,pair[1].length));
        assert(!out.bytes && !out.count);
    }
    sophia_shell_outbox_dispose(&out); close(fd[0]); close(fd[1]);
}
static void errors(void)
{
    int fd[2]; assert(socketpair(AF_UNIX,SOCK_STREAM,0,fd)==0);
    struct sophia_shell_outbox out;
    assert(sophia_shell_outbox_init(&out,2048,4,512,2)==SOPHIA_SHELL_OK);
    assert(sophia_shell_outbox_push(&out,pair,2)==SOPHIA_SHELL_OK);
    size_t charge=out.bytes;
    interrupted=1; sends=0;
    assert(sophia_shell_outbox_flush(&out,fd[0],2048)==SOPHIA_SHELL_AGAIN);
    assert(sends==32 && out.bytes==charge && !out.records[out.head].sent);
    interrupted=0; close(fd[1]);
    unsigned f=frees;
    assert(sophia_shell_outbox_flush(&out,fd[0],2048)==SOPHIA_SHELL_IO_ERROR);
    assert(out.count==2 && out.bytes==charge && frees==f);
    assert(sophia_shell_outbox_push(&out,pair,1)==SOPHIA_SHELL_IO_ERROR);
    assert(sophia_shell_outbox_flush(&out,fd[0],2048)==SOPHIA_SHELL_IO_ERROR);
    sophia_shell_outbox_dispose(&out); assert(frees==f+2); close(fd[0]);
}
static void reservations(void)
{
    struct sophia_shell_outbox out;
    assert(sophia_shell_outbox_init(&out,2048,4,512,2)==SOPHIA_SHELL_OK);
    size_t lengths[2]={pair[0].length,pair[1].length};
    struct sophia_shell_outbox_reservation ticket={NULL,99};
    for (unsigned fail=1; fail<=2; ++fail) {
        allocation_call=0; allocation_fail=fail;
        unsigned a=allocations,f=frees;
        assert(sophia_shell_outbox_reserve(&out,lengths,2,&ticket)==SOPHIA_SHELL_BUSY);
        assert(!out.count && !out.bytes && !ticket.owner && ticket.serial==99);
        assert(allocations-a==frees-f);
    }
    allocation_fail=0;
    assert(sophia_shell_outbox_reserve(&out,lengths,2,&ticket)==SOPHIA_SHELL_OK);
    struct sophia_shell_outbox_reservation copy=ticket;
    assert(sophia_shell_outbox_reserve(&out,lengths,2,&copy)==SOPHIA_SHELL_BUSY);
    assert(copy.owner==ticket.owner && copy.serial==ticket.serial);
    assert(sophia_shell_outbox_push(&out,pair,1)==SOPHIA_SHELL_OK);
    int fd[2]; assert(socketpair(AF_UNIX,SOCK_STREAM,0,fd)==0);
    sends=0; size_t bytes=out.bytes; unsigned a=allocations,f=frees;
    assert(sophia_shell_outbox_flush(&out,fd[0],2048)==SOPHIA_SHELL_AGAIN && sends==0);
    struct sophia_shell_outbound_frame bad[2]={pair[0],pair[1]}; --bad[1].length;
    assert(sophia_shell_outbox_commit(&out,ticket,bad,2)==SOPHIA_SHELL_INVALID);
    copy.owner=NULL; assert(sophia_shell_outbox_commit(&out,copy,pair,2)==SOPHIA_SHELL_INVALID);
    copy=ticket; ++copy.serial; assert(sophia_shell_outbox_commit(&out,copy,pair,2)==SOPHIA_SHELL_INVALID);
    assert(out.bytes==bytes && out.count==3 && allocations==a && frees==f);
    assert(sophia_shell_outbox_flush(&out,fd[0],2048)==SOPHIA_SHELL_AGAIN && sends==0);
    assert(sophia_shell_outbox_commit(&out,ticket,pair,2)==SOPHIA_SHELL_OK);
    assert(out.bytes==bytes && out.count==3 && allocations==a && frees==f);
    assert(sophia_shell_outbox_commit(&out,ticket,pair,2)==SOPHIA_SHELL_INVALID);
    assert(sophia_shell_outbox_flush(&out,fd[0],2048)==SOPHIA_SHELL_OK);
    uint8_t got[768]; size_t used=0;
    while (used<bytes) {ssize_t n=recv(fd[1],got+used,sizeof(got)-used,0); assert(n>0); used+=(size_t)n;}
    assert(used==bytes && !memcmp(got,pair[0].bytes,pair[0].length));
    assert(!memcmp(got+pair[0].length,pair[1].bytes,pair[1].length));
    assert(!memcmp(got+pair[0].length+pair[1].length,pair[0].bytes,pair[0].length));
    assert(sophia_shell_outbox_reserve(&out,lengths,2,&copy)==SOPHIA_SHELL_OK);
    assert(copy.serial!=ticket.serial);
    assert(sophia_shell_outbox_commit(&out,ticket,pair,2)==SOPHIA_SHELL_INVALID);
    sophia_shell_outbox_dispose(&out); close(fd[0]); close(fd[1]);
}
int main(void)
{
    frames(); atomic_refusal(); byte_reservation(); socket_fifo(); errors(); reservations();
    assert(allocations==frees);
    puts("sophia_shell_outbox atomic_pair=pass real_partial_io=pass cycles=1000 native=false");
    return 0;
}

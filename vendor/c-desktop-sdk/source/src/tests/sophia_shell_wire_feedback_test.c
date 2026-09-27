#include "../sophia_shell_content_control.h"
#include "../shell_wire/fields.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static unsigned nibble(char c)
{
    if (c>='0' && c<='9') return (unsigned)(c-'0');
    if (c>='a' && c<='f') return (unsigned)(c-'a')+10;
    abort();
}
static void invalid(struct sophia_shell_frame f)
{
    struct sophia_shell_content_feedback out, old;
    memset(&out,0xa5,sizeof(out)); memcpy(&old,&out,sizeof(old));
    assert(sophia_shell_content_feedback_decode(&f,&out)==SOPHIA_SHELL_INVALID);
    assert(!memcmp(&old,&out,sizeof(out)));
}
static void change(struct sophia_shell_frame f, size_t offset, unsigned width, uint64_t value)
{
    uint8_t p[672]; assert(f.payload_bytes<=sizeof(p)); memcpy(p,f.payload,f.payload_bytes);
    f.payload=p;
    if (width==2) shell_put16(p+offset,(uint16_t)value);
    if (width==4) shell_put32(p+offset,(uint32_t)value);
    if (width==8) shell_put64(p+offset,value);
    invalid(f);
}
static void decoded(struct sophia_shell_frame f)
{
    struct sophia_shell_content_feedback v;
    assert(sophia_shell_content_feedback_decode(&f,&v)==SOPHIA_SHELL_OK);
    assert(v.kind==f.kind && v.transaction==f.transaction);
    assert(v.grant.connection_epoch==11 && v.grant.content_grant_epoch==3);
    for (size_t n=0; n<f.payload_bytes; ++n) {
        struct sophia_shell_frame cut=f; cut.payload_bytes=n; invalid(cut);
    }
    struct sophia_shell_frame bad=f; ++bad.payload_bytes; invalid(bad);
    bad=f; bad.transaction=0; invalid(bad);
    bad=f; bad.kind=180; invalid(bad);
    change(f,0,8,0); change(f,8,8,0);
    if (f.kind==162) {
        const struct sophia_shell_output_facts *a=&v.value.facts;
        assert(a->generation==1 && a->count==1 && a->outputs[0].output.id==2);
        assert(a->outputs[0].output.generation==7 && a->outputs[0].local_width==1920);
        assert(a->outputs[0].local_height==1080 && a->outputs[0].scale_numerator==1);
        assert(a->outputs[0].scale_denominator==1 && a->outputs[0].scale_generation==1);
        change(f,16,8,0); change(f,24,4,17); change(f,28,4,1);
        change(f,32,8,0); change(f,40,8,0); change(f,48,4,0); change(f,52,4,0);
        change(f,56,4,0); change(f,56,4,33); change(f,60,4,0); change(f,60,4,5);
        change(f,64,8,0);
    } else if (f.kind==164) {
        const struct sophia_shell_allocation_result *a=&v.value.allocation;
        assert(a->request_id==1 && a->status==1 && !a->reason);
        assert(a->output.id==2 && a->output.generation==7);
        assert(a->allocation.id==1 && a->allocation.generation==2 && !a->parent.id && !a->parent.generation);
        assert(a->scale_generation==1 && a->logical.width==2 && a->logical.height==1);
        assert(a->pixel.width==2 && a->pixel.height==1 && !a->pixel.x && !a->logical.y);
        assert(a->scale_numerator==1 && a->scale_denominator==1 && a->allowed_reservation_extent==1);
        change(f,16,8,0); change(f,24,2,0); change(f,24,2,5); change(f,26,2,1);
        change(f,28,4,1); change(f,32,8,0); change(f,40,8,0); change(f,48,8,0);
        change(f,56,8,0); change(f,64,8,1); change(f,80,8,0);
        change(f,96,4,0); change(f,100,4,0); change(f,112,4,0); change(f,116,4,0);
        change(f,120,4,33); change(f,124,4,0); change(f,128,4,513);
        for (size_t off=132; off<140; off+=2) {change(f,off,2,513); change(f,off,2,(uint16_t)-513);}
        change(f,156,4,1);
    } else if (f.kind==175) {
        const struct sophia_shell_candidate_outcome *a=&v.value.candidate;
        assert(a->generation==4 && a->output.id==2 && a->output.generation==7);
        assert(a->kind==2 && !a->reason && a->presentation_epoch==5);
        assert(a->work_area_generation==6 && a->wm_commit_generation==7);
        change(f,16,8,0); change(f,24,8,0); change(f,32,8,0); change(f,40,2,1);
        change(f,40,2,5); change(f,42,2,1); change(f,44,8,0);
    } else if (f.kind==177) {
        const struct sophia_shell_frame_permit *a=&v.value.permit;
        assert(a->output.id==2 && a->output.generation==7 && a->demand_id==1 && a->permit_id==2);
        assert(a->state==1 && !a->reason && a->ttl_ms==250 && a->max_candidate_bytes==8192);
        change(f,16,8,0); change(f,24,8,0); change(f,32,8,0); change(f,40,8,0);
        change(f,48,2,0); change(f,48,2,5); change(f,50,2,1);
        change(f,52,4,0); change(f,52,4,251); change(f,56,4,0); change(f,56,4,8193);
        change(f,60,4,1);
    } else {
        const struct sophia_shell_content_action *a=&v.value.action;
        assert(a->kind==1 && !a->reason && a->identity.output.id==2 && a->identity.output.generation==7);
        assert(a->identity.candidate_generation==4 && a->identity.presentation_epoch==5);
        assert(a->identity.interaction_generation==4 && a->identity.allocation.id==1);
        assert(a->identity.allocation.generation==2 && a->identity.target_id==1);
        assert(a->identity.target_generation==2 && a->identity.action_id==3 && a->identity.event_id==8);
        for (size_t off=16; off<=96; off+=8) change(f,off,8,0);
        change(f,104,2,0); change(f,104,2,4); change(f,104,2,2); change(f,106,2,1); change(f,108,4,1);
    }
}
static int encode(uint8_t *dst, size_t capacity, uint16_t kind, uint64_t tx, size_t *n)
{
    struct sophia_shell_candidate_end e={{11,3},4,1,1,1};
    struct sophia_shell_frame_demand d={{11,3},{2,7},{0,0},1,1};
    struct sophia_shell_demand_cancel c={{11,3},{2,7},1,2};
    struct sophia_shell_content_action_ack a={{11,3},{{2,7},4,5,4,{1,2},1,2,3,8},1};
    switch (kind) {
    case 174: return sophia_shell_candidate_end_encode(dst,capacity,tx,&e,n);
    case 176: return sophia_shell_frame_demand_encode(dst,capacity,tx,&d,n);
    case 178: return sophia_shell_demand_cancel_encode(dst,capacity,tx,&c,n);
    case 180: return sophia_shell_content_action_ack_encode(dst,capacity,tx,&a,n);
    default: abort();
    }
}
static void encoded(struct sophia_shell_frame f, const uint8_t *golden, size_t bytes)
{
    uint8_t dst[192], old[192]; memset(dst,0xa5,sizeof(dst)); memcpy(old,dst,sizeof(dst));
    for (size_t cap=0; cap<bytes; ++cap) {
        size_t n=999;
        assert(encode(dst,cap,f.kind,f.transaction,&n)==SOPHIA_SHELL_INVALID);
        assert(n==999 && !memcmp(dst,old,sizeof(dst)));
    }
    size_t n=999;
    assert(encode(dst,sizeof(dst),f.kind,0,&n)==SOPHIA_SHELL_INVALID);
    assert(n==999 && !memcmp(dst,old,sizeof(dst)));
    assert(encode(dst,sizeof(dst),f.kind,f.transaction,&n)==SOPHIA_SHELL_OK);
    assert(n==bytes && !memcmp(dst,golden,bytes));
}
static void variants(void)
{
    uint8_t p[672]={0}; shell_put64(p,11); shell_put64(p+8,3); shell_put64(p+16,1);
    shell_put32(p+24,16);
    for (unsigned i=0; i<16; ++i) {
        uint8_t *q=p+32+40*i;
        shell_put64(q,i+1); shell_put64(q+8,7); shell_put32(q+16,1920); shell_put32(q+20,1080);
        shell_put32(q+24,5); shell_put32(q+28,4); shell_put64(q+32,8);
    }
    struct sophia_shell_frame f={162,1,p,sizeof(p)};
    struct sophia_shell_content_feedback v;
    assert(sophia_shell_content_feedback_decode(&f,&v)==SOPHIA_SHELL_OK && v.value.facts.count==16);
    change(f,32+40*15,8,1); /* Duplicate ID even with a different generation. */
    shell_put64(p+32+40*15+8,99); change(f,32+40*15,8,1);
    shell_put32(p+56,2); shell_put32(p+60,2); invalid(f);
    shell_put32(p+24,0); f.payload_bytes=32;
    assert(sophia_shell_content_feedback_decode(&f,&v)==SOPHIA_SHELL_OK && !v.value.facts.count);
    memset(p+16,0,sizeof(p)-16); f.kind=164; f.payload_bytes=160;
    shell_put64(p+16,2); shell_put16(p+24,2); shell_put16(p+26,12);
    shell_put64(p+32,2); shell_put64(p+40,7);
    assert(sophia_shell_content_feedback_decode(&f,&v)==SOPHIA_SHELL_OK);
    for (size_t off=80; off<156; ++off) {p[off]=1; invalid(f); p[off]=0;}
    shell_put16(p+24,3); invalid(f); shell_put64(p+48,1); shell_put64(p+56,2);
    assert(sophia_shell_content_feedback_decode(&f,&v)==SOPHIA_SHELL_OK);
    shell_put16(p+24,4); invalid(f); shell_put64(p+16,0);
    assert(sophia_shell_content_feedback_decode(&f,&v)==SOPHIA_SHELL_OK);
    /* Revocation may retain geometry; no synthetic zero requirement for status 4. */
    shell_put32(p+88,UINT32_MAX); shell_put16(p+132,(uint16_t)-512);
    assert(sophia_shell_content_feedback_decode(&f,&v)==SOPHIA_SHELL_OK);
    assert(v.value.allocation.logical.x==-1 && v.value.allocation.margins[0]==-512);
    memset(p+16,0,sizeof(p)-16); f.kind=175; f.payload_bytes=68;
    shell_put64(p+16,4); shell_put64(p+24,2); shell_put64(p+32,7);
    for (unsigned kind=1; kind<=4; ++kind) {
        shell_put16(p+40,kind); shell_put16(p+42,kind>2?12:0); shell_put64(p+44,kind==2?5:0);
        assert(sophia_shell_content_feedback_decode(&f,&v)==SOPHIA_SHELL_OK);
    }
    memset(p+16,0,sizeof(p)-16); f.kind=177; f.payload_bytes=64;
    shell_put64(p+16,2); shell_put64(p+24,7); shell_put64(p+32,1);
    for (unsigned state=2; state<=4; ++state) {
        shell_put16(p+48,state); shell_put16(p+50,12);
        assert(sophia_shell_content_feedback_decode(&f,&v)==SOPHIA_SHELL_OK);
    }
    memset(p+16,0,sizeof(p)-16); f.kind=179; f.payload_bytes=112;
    for (size_t off=16; off<=64; off+=8) shell_put64(p+off,1);
    shell_put64(p+96,1); shell_put16(p+104,2);
    assert(sophia_shell_content_feedback_decode(&f,&v)==SOPHIA_SHELL_OK);
    change(f,72,8,1); change(f,80,8,1); change(f,88,8,1);
    shell_put16(p+104,3); shell_put16(p+106,12);
    assert(sophia_shell_content_feedback_decode(&f,&v)==SOPHIA_SHELL_OK);
}
static void control_bounds(void)
{
    uint8_t dst[192], old[192]; memset(dst,0xa5,sizeof(dst)); memcpy(old,dst,sizeof(dst));
    size_t n=999;
    for (unsigned mode=0; mode<6; ++mode) {
        struct sophia_shell_candidate_end e={{11,3},4,1,1,1};
        if (mode==0) e.grant.connection_epoch=0;
        if (mode==1) e.grant.content_grant_epoch=0;
        if (mode==2) e.generation=0;
        if (mode==3) e.surface_count=9;
        if (mode==4) e.placement_count=33;
        if (mode==5) e.target_count=65;
        assert(sophia_shell_candidate_end_encode(dst,sizeof(dst),1,&e,&n)==SOPHIA_SHELL_INVALID);
    }
    for (unsigned mode=0; mode<8; ++mode) {
        struct sophia_shell_frame_demand d={{11,3},{2,7},{0,0},1,1};
        if (mode==0) d.grant.connection_epoch=0;
        if (mode==1) d.grant.content_grant_epoch=0;
        if (mode==2) d.output.id=0;
        if (mode==3) d.output.generation=0;
        if (mode==4) d.allocation.id=1;
        if (mode==5) d.demand_id=0;
        if (mode==6) d.reason=0;
        if (mode==7) d.reason=4;
        assert(sophia_shell_frame_demand_encode(dst,sizeof(dst),1,&d,&n)==SOPHIA_SHELL_INVALID);
    }
    for (unsigned mode=0; mode<5; ++mode) {
        struct sophia_shell_demand_cancel c={{11,3},{2,7},1,2};
        if (mode==0) c.grant.connection_epoch=0;
        if (mode==1) c.grant.content_grant_epoch=0;
        if (mode==2) c.output.id=0;
        if (mode==3) c.output.generation=0;
        if (mode==4) c.demand_id=0;
        assert(sophia_shell_demand_cancel_encode(dst,sizeof(dst),1,&c,&n)==SOPHIA_SHELL_INVALID);
    }
    for (unsigned mode=0; mode<15; ++mode) {
        struct sophia_shell_content_action_ack a={{11,3},{{2,7},4,5,4,{1,2},1,2,3,8},1};
        uint64_t *fields[]={&a.grant.connection_epoch,&a.grant.content_grant_epoch,&a.identity.output.id,
            &a.identity.output.generation,&a.identity.candidate_generation,&a.identity.presentation_epoch,
            &a.identity.interaction_generation,&a.identity.allocation.id,&a.identity.allocation.generation,
            &a.identity.target_id,&a.identity.target_generation,&a.identity.action_id,&a.identity.event_id};
        if (mode<13) *fields[mode]=0;
        else a.disposition=mode==13?0:3;
        assert(sophia_shell_content_action_ack_encode(dst,sizeof(dst),1,&a,&n)==SOPHIA_SHELL_INVALID);
    }
    assert(n==999 && !memcmp(dst,old,sizeof(dst)));
    struct sophia_shell_content_action_ack dismiss={{11,3},{{2,7},4,5,4,{1,2},0,0,0,8},2};
    assert(sophia_shell_content_action_ack_encode(dst,sizeof(dst),1,&dismiss,&n)==SOPHIA_SHELL_OK);
    struct sophia_shell_demand_cancel cancel={{11,3},{2,7},1,0};
    assert(sophia_shell_demand_cancel_encode(dst,sizeof(dst),1,&cancel,&n)==SOPHIA_SHELL_OK);
}
int main(int argc, char **argv)
{
    assert(argc==2); FILE *file=fopen(argv[1],"r"); assert(file);
    static char line[4096]; static uint8_t bytes[2048]; unsigned count=0;
    while (fgets(line,sizeof(line),file)) {
        char *hex=strchr(line,' '); assert(hex); ++hex;
        size_t n=strcspn(hex,"\r\n"); assert(n%2==0 && n/2<=sizeof(bytes));
        for (size_t i=0; i<n/2; ++i) bytes[i]=(uint8_t)(16*nibble(hex[2*i])+nibble(hex[2*i+1]));
        struct sophia_shell_frame f; assert(sophia_shell_frame_decode(bytes,n/2,&f)==SOPHIA_SHELL_OK);
        if (f.kind==162 || f.kind==164 || f.kind==175 || f.kind==177 || f.kind==179) {decoded(f); ++count;}
        if (f.kind==174 || f.kind==176 || f.kind==178 || f.kind==180) {encoded(f,bytes,n/2); ++count;}
    }
    assert(!ferror(file) && count==9); fclose(file); variants(); control_bounds();
    puts("sophia_shell_content_feedback kinds=9 golden=pass lifecycle=not_run");
    return 0;
}

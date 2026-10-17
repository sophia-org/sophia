/* Independent sealed-only fixture decoder for proposed Sophia x6jp929h.
 * Test code, not a public codec or an authenticated admission implementation. */
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef __linux__
#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
#endif

static uint64_t get(const unsigned char *p, unsigned n) {
  uint64_t v = 0;
  unsigned i;
  for (i = 0; i < n; ++i) v |= (uint64_t)p[i] << (8 * i);
  return v;
}
static int zeros(const unsigned char *p, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i) if (p[i]) return 0;
  return 1;
}
static int between(uint64_t n, uint64_t lo, uint64_t hi) { return n >= lo && n <= hi; }
static int metadata(const unsigned char *p, size_t n) {
  uint64_t w, h, size;
  if (n != 96) return 0;
  if (!get(p, 8) || !get(p+8, 8) || !get(p+16, 8)) return 0;
  if (get(p+24,8)!=6 || get(p+32,8)!=7 || get(p+40,8)!=8 || get(p+48,8)!=9) return 0;
  if (get(p+56,4)!=1 || get(p+60,4)!=1) return 0;
  w=get(p+64,4); h=get(p+68,4);
  if (!between(w,1,4096) || !between(h,1,4096)) return 0;
  size=w*h*4; /* Dimensions bounded before multiplying. */
  return size<=4194304 && get(p+72,4)==w*4 && get(p+76,2)==1 &&
    get(p+78,2)==1 && get(p+80,8)==size && get(p+88,8)==0;
}
static int channel(const unsigned char *p, size_t n, unsigned fds) {
  unsigned kind;
  if (n<40 || get(p,4)!=n || get(p+4,2)!=1 || !between(get(p+8,2),1,2) ||
      get(p+10,2)!=fds || get(p+12,4) || get(p+16,8)!=1 || get(p+24,8)!=2) return 0;
  kind=(unsigned)get(p+6,2);
  if (kind==1) return n==40 && !fds && !get(p+32,8);
  if (kind!=2 || n!=200 || fds!=1 || !metadata(p+40,96)) return 0;
  return get(p+32,8)==get(p+40,8) && !get(p+136,8) &&
    get(p+144,4)==get(p+112,4) && zeros(p+148,52);
}
static int record(const unsigned char *p, size_t n) {
  unsigned kind;
  uint64_t sub, seq, resource, offers;
  if (n<32 || get(p,4)!=n || get(p+4,2)!=1 || get(p+8,8)!=1) return 0;
  kind=(unsigned)get(p+6,2); sub=get(p+16,8); seq=get(p+24,8);
  if ((kind==0x7000 && (!sub || seq)) ||
      (kind>=0x7001 && kind<=0x7004 && (sub || !seq)) ||
      (kind>=0x7005 && kind<=0x7006 && (sub || seq))) return 0;
  switch (kind) {
  case 0x7000: case 0x7001: return metadata(p+32,n-32);
  case 0x7002:
    return n==64 && get(p+32,8) && get(p+40,8) && get(p+48,8) &&
      between(get(p+56,4),1,5) && !get(p+60,4);
  case 0x7003: case 0x7004:
    return n==48 && get(p+32,8)==2 && between(get(p+40,4),1,kind==0x7003?3:4) && !get(p+44,4);
  case 0x7005:
    if (n!=136 || !get(p+32,8) || !get(p+40,8) || get(p+128,8)) return 0;
    resource=get(p+48,8); offers=get(p+84,4);
    return between(resource,1,4194304) && get(p+56,8)>=resource && zeros(p+64,20) &&
      between(offers,1,4) && between(get(p+88,4),1,offers) && !get(p+92,4) &&
      between(get(p+96,4),1,16) && between(get(p+100,4),1,256) &&
      between(get(p+104,4),1,4096) && between(get(p+108,4),1,4096) &&
      get(p+112,4)==2000 && get(p+116,4)==500 && get(p+120,4)==65536 && get(p+124,4)==1;
  case 0x7006:
    /* No GPU rows authorized by this sealed-only fixture's discovery. */
    return n==64 && get(p+32,8) && get(p+40,8) && get(p+48,8) &&
      !get(p+56,4) && !get(p+60,4);
  default: return 0;
  }
}
static int discovery(const unsigned char *p, size_t n, uint64_t decoded[15]) {
  static const char *keys[]={"role","connection_epoch","channel_generation","endpoint","source_kinds",
    "max_packet_bytes","max_fds","max_offers","max_packets_per_turn","hello_timeout_ms",
    "validation_timeout_ms","validation_chunk_bytes","capture_interest_timeout_ms","formats_generation","limits_generation"};
  static const uint64_t values[]={0,1,2,0,1,200,4,4,1,250,2000,65536,500,9,1};
  const char header[]="sophia-image-channel version=1\n";
  char copy[2049], *at, *end, *value, *tail;
  unsigned i;
  if (n<sizeof(header)-1 || n>2048 || p[n-1]!='\n' || memchr(p,0,n) ||
      memcmp(p,header,sizeof(header)-1)) return 0;
  memcpy(copy,p,n); copy[n]=0; at=copy+sizeof(header)-1;
  for (i=0;i<15;++i) {
    size_t length=strlen(keys[i]); uint64_t number;
    end=strchr(at,'\n'); if (!end) return 0; *end=0;
    if (strlen(at)<length+1 || memcmp(at,keys[i],length) || at[length]!='=') return 0;
    value=at+length+1;
    if (i==3) {
      size_t j, size=strlen(value);
      if (!size || size>64 || !strcmp(value,".") || !strcmp(value,"..")) return 0;
      for (j=0;j<size;++j) if (!((value[j]>='a'&&value[j]<='z') || (value[j]>='A'&&value[j]<='Z') ||
          (value[j]>='0'&&value[j]<='9') || value[j]=='_' || value[j]=='-' || value[j]=='.')) return 0;
    } else {
      if (!*value || (value[0]=='0' && value[1])) return 0;
      for (tail=value;*tail;++tail) if (*tail<'0'||*tail>'9') return 0;
      errno=0; number=strtoull(value,&tail,10);
      if (errno || *tail || (i==0 ? !between(number,1,2) :
          (i==2 || i==13 || i==14) ? number==0 : number!=values[i])) return 0;
      if (decoded) decoded[i]=number;
    }
    at=end+1;
  }
  return !*at;
}
static int accepts(const char *family,const unsigned char *b,size_t n,unsigned fds) {
  if (!strcmp(family,"channel")) return channel(b,n,fds);
  if (!strcmp(family,"record")) return record(b,n);
  if (!strcmp(family,"discovery")) return discovery(b,n,NULL);
  return 0;
}
enum refusal { Accept, Length, Version, Role, Reserved, Epoch, Generation, FdCount,
  Offer, SourceKind, PlaneCount, PlaneLayout, Dimensions, Stride, ByteLength,
  Modifier, DiscoveryFields, Decimal, Endpoint, Constants, Coherence, Capacity,
  FormatsUnsupported, Malformed };
static enum refusal coherent(const unsigned char *d,size_t dn,const unsigned char *l,size_t ln,
                             const unsigned char *f,size_t fn) {
  uint64_t v[15]={0};
  if (!discovery(d,dn,v) || !record(l,ln) || !record(f,fn) ||
      get(l+6,2)!=0x7005 || get(f+6,2)!=0x7006) return Malformed;
  if (get(l+8,8)!=v[1] || get(f+8,8)!=v[1] || get(l+32,8)!=v[2] || get(f+32,8)!=v[2] ||
      get(l+40,8)!=v[14] || get(f+48,8)!=v[14] || get(f+40,8)!=v[13]) return Coherence;
  return Accept;
}
static enum refusal classify(const char *family,const unsigned char *p,size_t n,unsigned fds,
                             const unsigned char *l,const unsigned char *f) {
  if (!strcmp(family,"discovery")) {
    char text[2049],*at,*end;
    if (discovery(p,n,NULL)) return coherent(p,n,l,136,f,64);
    if (!n || n>2048 || p[n-1]!='\n' || memchr(p,0,n)) return DiscoveryFields;
    memcpy(text,p,n);text[n]=0; at=strchr(text,'\n'); if (!at) return DiscoveryFields; ++at;
    while (*at) {
      char *eq,*value,*tail;
      end=strchr(at,'\n'); if (!end) return DiscoveryFields; *end=0;
      eq=strchr(at,'=');if (!eq) return DiscoveryFields;*eq=0;value=eq+1;
      if (!strcmp(at,"endpoint")) {
        size_t j,size=strlen(value);
        if (!size || size>64 || !strcmp(value,".") || !strcmp(value,"..")) return Endpoint;
        for (j=0;j<size;++j) if (!((value[j]>='a'&&value[j]<='z') || (value[j]>='A'&&value[j]<='Z') ||
            (value[j]>='0'&&value[j]<='9') || value[j]=='_' || value[j]=='-' || value[j]=='.')) return Endpoint;
      } else {
        if (!*value || (value[0]=='0'&&value[1])) return Decimal;
        for (tail=value;*tail;++tail) if (*tail<'0'||*tail>'9') return Decimal;
        errno=0; (void)strtoull(value,&tail,10); if (errno || *tail) return Decimal;
        if ((!strcmp(at,"max_packet_bytes")&&strcmp(value,"200")) ||
            (!strcmp(at,"max_fds")&&strcmp(value,"4"))) return Constants;
      }
      at=end+1;
    }
    return DiscoveryFields;
  }
  if (n<(!strcmp(family,"channel")?40u:32u) || get(p,4)!=n) return Length;
  if (get(p+4,2)!=1) return Version;
  if (!strcmp(family,"channel")) {
    uint64_t w,h;
    if (!between(get(p+8,2),1,2)) return Role;
    if (get(p+12,4)) return Reserved;
    if (get(p+16,8)!=1) return Epoch;
    if (get(p+24,8)!=2) return Generation;
    if (get(p+10,2)!=fds) return FdCount;
    if (get(p+6,2)==1 && get(p+32,8)) return Offer;
    if (n==200 && get(p+6,2)==2) {
      if (get(p+32,8)!=get(p+40,8)) return Offer;
      if (get(p+96,4)!=1) return SourceKind;
      if (get(p+116,2)!=1) return PlaneCount;
      if (get(p+118,2)!=get(p+116,2)) return FdCount;
      if (!zeros(p+150,50)) return Reserved;
      w=get(p+104,4);h=get(p+108,4);
      if (!between(w,1,4096)||!between(h,1,4096)) return Dimensions;
      if (get(p+112,4)!=w*4) return Stride;
      if (get(p+120,8)!=w*h*4 || w*h*4>4194304) return ByteLength;
      if (get(p+128,8)) return Modifier;
      if (get(p+136,8)||get(p+148,2)) return PlaneLayout;
    }
  } else if (!strcmp(family,"record")) {
    if (get(p+8,8)!=1) return Epoch;
    if (get(p+6,2)==0x7005 && n==136) {
      if (get(p+128,8)) return Reserved;
      if (get(p+88,4)>get(p+84,4)) return Capacity;
    }
    if (get(p+6,2)==0x7006 && n>=64) {
      if (64+40*get(p+56,4)!=n) return Length;
      if (get(p+60,4)) return Reserved;
      if (get(p+56,4)>4096) return Capacity;
      if (get(p+56,4)) return FormatsUnsupported;
    }
  }
  return accepts(family,p,n,fds)?Accept:Malformed;
}
struct vector { const char *name, *family; int accepted; unsigned fds; const char *hex; enum refusal reason; };
#ifdef __linux__
static int sealed_source_matches(const unsigned char *p,size_t n,int fd) {
  struct stat st;
  int seals;
  const int required=F_SEAL_WRITE|F_SEAL_GROW|F_SEAL_SHRINK|F_SEAL_SEAL;
  if (n!=200 || !channel(p,n,1)) return 0;
  if (fstat(fd,&st)) return 0;
  seals=fcntl(fd,F_GET_SEALS);
  return seals>=0 && (seals&required)==required && st.st_size>=0 &&
    (uint64_t)st.st_size==get(p+120,8);
}
static void check_file_sizes(const unsigned char *p,size_t n) {
  const unsigned sizes[]={0,7,8,9}; unsigned i;
  for (i=0;i<4;++i) {
    int fd=memfd_create("draft-size",MFD_ALLOW_SEALING|MFD_CLOEXEC);
    assert(fd>=0 && ftruncate(fd,sizes[i])==0);
    assert(fcntl(fd,F_ADD_SEALS,F_SEAL_WRITE|F_SEAL_GROW|F_SEAL_SHRINK|F_SEAL_SEAL)==0);
    assert(sealed_source_matches(p,n,fd)==(sizes[i]==8));
    close(fd);
  }
  { int fd=open("/dev/null",O_RDWR|O_CLOEXEC);
    assert(fd>=0 && !sealed_source_matches(p,n,fd)); close(fd); }
}
#endif
static const struct vector vectors[]={
#include "image_channel_draft_vectors.h"
};
static size_t unhex(const char *s,unsigned char *b) {
  size_t i,n=strlen(s)/2;
  for (i=0;i<n;++i) { unsigned value;assert(sscanf(s+2*i,"%2x",&value)==1);b[i]=(unsigned char)value; }
  return n;
}
static void put64(unsigned char *b,uint64_t value) {
  unsigned i;for(i=0;i<8;++i)b[i]=(unsigned char)(value>>(8*i));
}
static void check_coherence(unsigned char *d,size_t dn,const unsigned char *l,const unsigned char *f) {
  const char *keys[]={"channel_generation=","limits_generation=","formats_generation="};
  unsigned k,j;
  for(k=0;k<3;++k) {
    unsigned char nd[2049],nl[136],nf[64];char *at;
    memcpy(nd,d,dn);nd[dn]=0;memcpy(nl,l,136);memcpy(nf,f,64);
    at=strstr((char *)nd,keys[k]);assert(at);at[strlen(keys[k])]='3';
    if(k==0) {put64(nl+32,3);put64(nf+32,3);}
    if(k==1) {put64(nl+40,3);put64(nf+48,3);}
    if(k==2) put64(nf+40,3);
    assert(coherent(nd,dn,nl,136,nf,64)==Accept);
    assert(coherent(nd,dn,l,136,f,64)==Coherence);
    for(j=0;j<(k==2?1u:2u);++j) {
      unsigned char bl[136],bf[64];memcpy(bl,nl,136);memcpy(bf,nf,64);
      if(k==0) put64(j?bf+32:bl+32,4);
      if(k==1) put64(j?bf+48:bl+40,4);
      if(k==2) put64(bf+40,4);
      assert(coherent(nd,dn,bl,136,bf,64)==Coherence);
    }
  }
}
int main(void) {
  unsigned i;
  unsigned char limits[136],formats[64],discovery_bytes[2049];size_t dn=0;
  for(i=0;i<54;++i) {
    if(!strcmp(vectors[i].name,"shell-limits"))assert(unhex(vectors[i].hex,limits)==136);
    if(!strcmp(vectors[i].name,"shell-sealed-formats"))assert(unhex(vectors[i].hex,formats)==64);
    if(!strcmp(vectors[i].name,"shell-discovery"))dn=unhex(vectors[i].hex,discovery_bytes);
  }
  assert(dn);check_coherence(discovery_bytes,dn,limits,formats);
  assert(sizeof(vectors)/sizeof(vectors[0])==54);
  for (i=0;i<54;++i) {
    const struct vector *v=&vectors[i];
    unsigned char bytes[2049]; size_t j, n=strlen(v->hex)/2;
    assert(n<sizeof(bytes));
    for (j=0;j<n;++j) { unsigned value; assert(sscanf(v->hex+2*j,"%2x",&value)==1); bytes[j]=(unsigned char)value; }
    { enum refusal r=classify(v->family,bytes,n,v->fds,limits,formats);
      if(r!=v->reason || (r==Accept)!=v->accepted) { fprintf(stderr,"case %s reason %d expected %d\n",v->name,r,v->reason);abort(); } }
    if (v->accepted) {
#ifdef __linux__
      if (!strcmp(v->family,"channel") && n==200) check_file_sizes(bytes,n);
#endif
      for (j=0;j<n;++j) assert(!accepts(v->family,bytes,j,v->fds));
      bytes[n]=0; assert(!accepts(v->family,bytes,n+1,v->fds));
    }
  }
  { size_t size=64+40*4097; unsigned char *large=calloc(1,size);
    assert(large);memcpy(large,formats,64);
    for(i=0;i<4;++i)large[i]=(unsigned char)(size>>(8*i));
    large[56]=1;large[57]=16;
    assert(classify("record",large,size,0,limits,formats)==Capacity);free(large); }
  puts("draft image codec: 54 vectors, prefix and formats capacity controls passed");
  return 0;
}

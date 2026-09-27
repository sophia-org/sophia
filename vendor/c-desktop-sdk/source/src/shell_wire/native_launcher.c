#include "../sophia_shell_native_launcher.h"
#include "fields.h"
#include "text.h"
#include <limits.h>

static int nonzero_words(const uint8_t *p, size_t words)
{
    for (size_t i = 0; i < words; ++i)
        if (!shell_get64(p + 8*i)) return 0;
    return 1;
}
static int zero(const uint8_t *p, size_t n)
{
    for (size_t i = 0; i < n; ++i) if (p[i]) return 0;
    return 1;
}
static int margins(const uint8_t *p)
{
    for (size_t i = 0; i < 8; i += 2) {
        uint16_t raw = shell_get16(p+i);
        int value = raw <= INT16_MAX ? raw : (int)raw - 65536;
        if (value < -512 || value > 512) return 0;
    }
    return 1;
}
static int reason(uint16_t v) { return v >= 1 && v <= 12; }
static int event(const uint8_t *p)
{
    return nonzero_words(p, 14) && shell_get64(p+112) >= shell_get64(p+88);
}
static int activation(const uint8_t *p)
{
    uint16_t cause = shell_get16(p+120), slot = shell_get16(p+122);
    return event(p) && shell_get64(p+112) == shell_get64(p+88) &&
        cause >= 1 && cause <= 2 && slot && slot <= 4096;
}
static int rect(const uint8_t *p)
{
    uint32_t x=shell_get32(p), y=shell_get32(p+4), w=shell_get32(p+8), h=shell_get32(p+12);
    return x <= INT32_MAX && y <= INT32_MAX && w && h &&
        (uint64_t)x+w <= INT32_MAX && (uint64_t)y+h <= INT32_MAX;
}

static int chunk(const uint8_t *p, size_t n)
{
    if (n < 40 || !nonzero_words(p,3)) return 0;
    uint32_t ns=shell_get32(p+28), np=shell_get32(p+32), nt=shell_get32(p+36);
    if (ns > 1 || np > 32 || nt > 32 || n != 40u+64u*ns+32u*np+48u*nt) return 0;
    p += 40;
    for (uint32_t i=0; i<ns; ++i, p+=64) {
        if (!nonzero_words(p,3) || shell_get16(p+24)!=3 || shell_get16(p+26)<1 ||
            shell_get16(p+26)>4 || !margins(p+28) || shell_get32(p+36) ||
            shell_get16(p+40)!=UINT16_MAX || !zero(p+42,22)) return 0;
    }
    for (uint32_t i=0; i<np; ++i, p+=32) {
        if (!nonzero_words(p,2) || shell_get16(p+16) || shell_get16(p+18) ||
            shell_get32(p+20)>INT32_MAX || shell_get32(p+24)>INT32_MAX || shell_get32(p+28)) return 0;
    }
    for (uint32_t i=0; i<nt; ++i, p+=48) {
        if (shell_get16(p) || shell_get16(p+2)!=2 || !nonzero_words(p+4,3) ||
            shell_get64(p+20)>4096 || !rect(p+28) || shell_get32(p+44)) return 0;
    }
    return 1;
}

static int begin(const uint8_t *p, size_t n)
{
    if (n < 108 || !nonzero_words(p,8) || shell_get32(p+64)!=1 ||
        !shell_get32(p+68) || shell_get32(p+68)>32 || shell_get32(p+76) ||
        !nonzero_words(p+80,3)) return 0;
    uint16_t selected=shell_get16(p+104), count=shell_get16(p+106);
    if (count>32 || shell_get32(p+72)!=count || n!=108u+2u*count) return 0;
    int found=0;
    for (uint16_t i=0; i<count; ++i) {
        uint16_t row=shell_get16(p+108+2*i);
        if (!row || row>4096) return 0;
        for (uint16_t j=0; j<i; ++j) if (shell_get16(p+108+2*j)==row) return 0;
        found |= row==selected;
    }
    return count ? found : selected==0;
}

int sophia_shell_native_launcher_validate(const struct sophia_shell_frame *f)
{
    if (!f || !f->payload) return SOPHIA_SHELL_ARGUMENT;
    const uint8_t *p=f->payload; size_t n=f->payload_bytes;
    if (!f->transaction || n<16 || n>SOPHIA_SHELL_MAX_PAYLOAD_BYTES || !nonzero_words(p,2))
        return SOPHIA_SHELL_INVALID;
    int valid=0;
    switch (f->kind) {
    case 187: valid=n==56 && nonzero_words(p,6) && shell_get64(p+48)==1; break;
    case 188:
        if (n==84 && nonzero_words(p,6) && margins(p+76)) {
            uint16_t op=shell_get16(p+64), edge=shell_get16(p+66);
            valid=op>=1 && op<=3 && edge>=1 && edge<=4 &&
                (op==1 ? zero(p+48,16) : nonzero_words(p+48,2)) &&
                (op==3 ? zero(p+68,16) : shell_get32(p+68) && shell_get32(p+72));
        } break;
    case 189: valid=begin(p,n); break;
    case 190: valid=chunk(p,n); break;
    case 191: valid=n==104 && nonzero_words(p,13); break;
    case 192: valid=n==108 && nonzero_words(p,13) && reason(shell_get16(p+104)) && !shell_get16(p+106); break;
    case 193:
        if (n>=132 && event(p) && shell_get64(p+120)) {
            uint16_t kind=shell_get16(p+128), len=shell_get16(p+130);
            valid=kind>=1 && kind<=17 && len<=256 && n==132u+len &&
                (kind==17 ? shell_get64(p+112)==shell_get64(p+88) : shell_get64(p+112)>shell_get64(p+88)) &&
                (kind==1 ? len && shell_text_valid(p+132,len) : len==0);
        } break;
    case 194: valid=n==124 && event(p) && shell_get16(p+120)>=1 && shell_get16(p+120)<=2 && !shell_get16(p+122); break;
    case 195: valid=n==124 && activation(p); break;
    case 196:
        if (n==128 && activation(p)) {
            uint16_t status=shell_get16(p+124), why=shell_get16(p+126);
            valid=status>=1 && status<=5 && why<=12 && ((status==1)==(why==0));
        } break;
    case 197: valid=n==28 && nonzero_words(p,3) && reason(shell_get16(p+24)) && !shell_get16(p+26); break;
    default: break;
    }
    return valid ? SOPHIA_SHELL_OK : SOPHIA_SHELL_INVALID;
}

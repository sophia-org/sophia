#include "../sophia_shell_catalog_actions.h"
#include "fields.h"
#include "text.h"
#include <limits.h>
#include <string.h>

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
    if (ns > 8 || np > 32 || nt > 64 || n != 40u+64u*ns+32u*np+48u*nt) return 0;
    p += 40;
    for (uint32_t i=0; i<ns; ++i, p+=64) {
        if (!nonzero_words(p,3) || shell_get16(p+24)!=1 || shell_get16(p+26)<1 ||
            shell_get16(p+26)>4 || !margins(p+28) || shell_get32(p+36)>512 ||
            shell_get16(p+40)!=UINT16_MAX || !zero(p+42,22)) return 0;
    }
    for (uint32_t i=0; i<np; ++i, p+=32) {
        if (!nonzero_words(p,2) || shell_get16(p+16)>=8 || shell_get16(p+18) ||
            shell_get32(p+20)>INT32_MAX || shell_get32(p+24)>INT32_MAX || shell_get32(p+28)) return 0;
    }
    for (uint32_t i=0; i<nt; ++i, p+=48) {
        if (shell_get16(p)>=8 || shell_get16(p+2)!=3 || !nonzero_words(p+4,3) ||
            shell_get64(p+20)>4096 || !rect(p+28) || shell_get32(p+44)) return 0;
    }
    return 1;
}


static int activation(const uint8_t *p)
{
    return nonzero_words(p,13) && shell_get64(p+88)<=4096 &&
        shell_get16(p+104)==1 && !shell_get16(p+106) && !shell_get32(p+108) &&
        shell_get64(p+112);
}
int sophia_shell_catalog_action_validate(const struct sophia_shell_frame *f)
{
    if (!f || !f->payload) return SOPHIA_SHELL_ARGUMENT;
    const uint8_t *p=f->payload; size_t n=f->payload_bytes;
    if (!f->transaction || n<16 || n>SOPHIA_SHELL_MAX_PAYLOAD_BYTES || !nonzero_words(p,2))
        return SOPHIA_SHELL_INVALID;
    int valid=0;
    switch (f->kind) {
    case 198:
        valid=n==88 && nonzero_words(p,8) && shell_get32(p+64)<=8 &&
            shell_get32(p+68)<=32 && shell_get32(p+72)<=64 && !shell_get32(p+76) && shell_get64(p+80);
        break;
    case 199: valid=chunk(p,n); break;
    case 200: valid=n==120 && activation(p); break;
    case 201:
        valid=n==124 && activation(p) && shell_get16(p+120)>=1 &&
            shell_get16(p+120)<=5 && !shell_get16(p+122); break;
    case 202:
        if (n>=24) {
            uint16_t slot=shell_get16(p+16), length=shell_get16(p+20);
            valid=slot && slot<=4096 && !shell_get16(p+18) && !shell_get16(p+22) &&
                length<=256 && n==24u+length && shell_text_valid(p+24,length) &&
                ((length>11 && !memcmp(p+24,"registered:",11)) ||
                 (length>8 && !memcmp(p+24,"desktop:",8)));
        } break;
    default: break;
    }
    return valid ? SOPHIA_SHELL_OK : SOPHIA_SHELL_INVALID;
}

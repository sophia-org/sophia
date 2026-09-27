/* Independent r5 byte-contract reader. No Sophia headers or generated codec.
 * Input is a named hex-frame corpus, not an admitted-session proof. */
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint16_t u16(const unsigned char *p) {
    return (uint16_t)((uint16_t)p[0] | ((uint16_t)p[1] << 8));
}
static uint32_t u32(const unsigned char *p) {
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) |
        ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}
static uint64_t u64(const unsigned char *p) {
    return (uint64_t)u32(p) | ((uint64_t)u32(p + 4) << 32);
}
static bool identity(const unsigned char *p) { return u64(p) && u64(p + 8); }
static bool scale(uint32_t n, uint32_t d) {
    if (!n || n > 32 || !d || d > 4) return false;
    while (d) { uint32_t r = n % d; n = d; d = r; }
    return n == 1;
}

static bool valid(const unsigned char *b, size_t len) {
    /* Fixed sizes independently summed from the normative field sequences. */
    static const size_t sizes[] = {
        12, 264, 0, 120, 160, 64, 48, 0, 48, 32, 32,
        34, 80, 0, 40, 68, 58, 64, 48, 112, 112
    };
    if (len < 24 || u32(b) != 0x48504f53 || u16(b + 4) != 1 ||
        u32(b + 20) != 0 || u32(b + 16) > 65536 || len != 24 + (size_t)u32(b + 16)) return false;
    unsigned kind = u16(b + 6);
    if (kind < 160 || kind > 180) return false;
    if ((u64(b + 8) == 0) != (kind < 162)) return false;
    const unsigned char *p = b + 24;
    size_t n = len - 24;
    if (sizes[kind - 160] && n != sizes[kind - 160]) return false;
    if (kind != 160 && (n < 16 || !identity(p))) return false;
    switch (kind) {
    case 160: return u16(p) >= 1 && u16(p) <= 4 && !u16(p + 2) && u64(p + 4);
    case 161:
        return u64(p + 16) && u64(p + 64) == 1 && !u64(p + 72) &&
            u32(p + 80) <= 65536 && u32(p + 84) <= 65488 && !u32(p + 260);
    case 162: {
        if (n < 32 || u32(p + 24) > 16 || u32(p + 28) || !u64(p + 16)) return false;
        size_t count = u32(p + 24);
        if (n != 32 + 40 * count) return false;
        for (size_t i = 0; i < count; i++) {
            const unsigned char *r = p + 32 + 40 * i;
            if (!identity(r) || !u32(r + 16) || !u32(r + 20) ||
                !scale(u32(r + 24), u32(r + 28)) || !u64(r + 32)) return false;
        }
        return true;
    }
    case 163: return identity(p + 16) && u64(p + 32) && !u16(p + 46);
    case 164: return identity(p + 32) && !u32(p + 28) && !u32(p + 156);
    case 165: {
        if (!identity(p + 16) || !u32(p + 32) || u32(p + 32) > 8192 ||
            !u32(p + 36) || u32(p + 36) > 4096 || !scale(u32(p + 40), u32(p + 44)) ||
            u16(p + 48) != 1 || u16(p + 50)) return false;
        uint64_t row = (uint64_t)u32(p + 32) * 4;
        uint64_t total = row * u32(p + 36);
        uint64_t capacity = 65488 / row;
        return total <= 4194304 && total == u64(p + 56) && capacity &&
            u32(p + 52) == (u32(p + 36) + capacity - 1) / capacity;
    }
    case 166: return identity(p + 16) && u16(p + 32) >= 1 && u16(p + 32) <= 4 && u16(p + 34) <= 12;
    case 167:
        return n >= 48 && identity(p + 16) && u32(p + 36) && u32(p + 36) <= 65488 &&
            n == 48 + (size_t)u32(p + 36) && u64(p + 40) <= 4194304 &&
            u64(p + 40) + u32(p + 36) <= 4194304;
    case 168: return identity(p + 16) && u64(p + 32) && u64(p + 32) <= 4194304 && !u32(p + 44);
    case 169: case 170: return identity(p + 16);
    case 171: return identity(p + 16) && u16(p + 32) <= 12;
    case 172: return u64(p + 16) && identity(p + 24) && u64(p + 40) && u64(p + 48) &&
        u64(p + 56) && u32(p + 64) <= 8 && u32(p + 68) <= 32 && u32(p + 72) <= 64 && !u32(p + 76);
    case 173: {
        if (n < 40 || !u64(p + 16)) return false;
        size_t ns = u32(p + 28), np = u32(p + 32), nt = u32(p + 36);
        if (ns > 8 || np > 32 || nt > 64 || n != 40 + ns * 64 + np * 32 + nt * 48) return false;
        for (size_t i = 0; i < ns; i++) {
            const unsigned char *r = p + 40 + i * 64;
            if (!identity(r) || u16(r + 42) || u32(r + 60)) return false;
        }
        for (size_t i = 0; i < np; i++) {
            const unsigned char *r = p + 40 + ns * 64 + i * 32;
            if (!identity(r) || u16(r + 18) || u32(r + 28)) return false;
        }
        for (size_t i = 0; i < nt; i++) {
            const unsigned char *r = p + 40 + ns * 64 + np * 32 + i * 48;
            if (!u64(r + 4) || !u64(r + 12) || !u64(r + 20) || u32(r + 44)) return false;
        }
        return true;
    }
    case 174: return u64(p + 16) && u32(p + 24) <= 8 && u32(p + 28) <= 32 && u32(p + 32) <= 64 && !u32(p + 36);
    case 175: return u64(p + 16) && identity(p + 24) && u16(p + 40) >= 1 && u16(p + 40) <= 4 &&
        u16(p + 42) <= 12 && ((u64(p + 44) != 0) == (u16(p + 40) == 2));
    case 176: return identity(p + 16) && u64(p + 48) && u16(p + 56) >= 1 && u16(p + 56) <= 3;
    case 177: return identity(p + 16) && u64(p + 32) && u16(p + 48) >= 1 && u16(p + 48) <= 4 && !u32(p + 60);
    case 178: return identity(p + 16) && u64(p + 32);
    case 179: case 180:
        return identity(p + 16) && u64(p + 32) && u64(p + 40) && u64(p + 48) &&
            identity(p + 56) && u64(p + 96) && u16(p + 104) >= 1 &&
            u16(p + 104) <= (kind == 179 ? 3 : 2) &&
            (kind == 179 ? u16(p + 106) <= 12 : u16(p + 106) == 0) && !u32(p + 108);
    default: return false;
    }
}

static int nibble(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    return -1;
}

int main(int argc, char **argv) {
    if (argc != 3 || (strcmp(argv[1], "--valid") && strcmp(argv[1], "--malformed"))) return 2;
    bool expected = !strcmp(argv[1], "--valid");
    FILE *file = fopen(argv[2], "r"); if (!file) return 2;
    char *line = malloc(131300); unsigned char *bytes = malloc(65560);
    if (!line || !bytes) { free(line); free(bytes); fclose(file); return 2; }
    unsigned cases = 0;
    int result = 0;
    while (fgets(line, 131300, file)) {
        char *hex = strchr(line, ' ');
        if (!hex) { result = 2; break; }
        *hex++ = '\0';
        size_t length = strcspn(hex, "\r\n");
        if (length % 2 || length / 2 > 65560) { result = 2; break; }
        for (size_t i = 0; i < length / 2; i++) {
            int a = nibble(hex[i * 2]), b = nibble(hex[i * 2 + 1]);
            if (a < 0 || b < 0) { result = 2; break; }
            bytes[i] = (unsigned char)((a << 4) | b);
        }
        if (result) break;
        if (valid(bytes, length / 2) != expected) {
            fprintf(stderr, "content corpus mismatch: %s\n", line); result = 1; break;
        }
        cases++;
    }
    if (ferror(file) || !cases) result = 2;
    if (!result) printf("sophia_shell_content_codec schema=1 status=complete mode=%s cases=%u\n", argv[1] + 2, cases);
    free(line); free(bytes); fclose(file); return result;
}

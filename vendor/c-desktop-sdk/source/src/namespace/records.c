#include "internal.h"
#include <errno.h>
#include <string.h>

struct span { const unsigned char *p; size_t n; };
static int invalid(void) { errno = EINVAL; return -1; }
static int equal(struct span s, const char *text) {
    size_t n = strlen(text);
    return s.n == n && !memcmp(s.p, text, n);
}
static int utf8(const unsigned char *p, size_t n, int controls) {
    size_t i = 0, j, count;
    uint32_t value, minimum;
    while (i < n) {
        unsigned c = p[i++];
        if (c < 0x80) { value = c; count = 0; minimum = 0; }
        else if (c >= 0xc2 && c <= 0xdf) { value = c & 31; count = 1; minimum = 0x80; }
        else if (c >= 0xe0 && c <= 0xef) { value = c & 15; count = 2; minimum = 0x800; }
        else if (c >= 0xf0 && c <= 0xf4) { value = c & 7; count = 3; minimum = 0x10000; }
        else return 0;
        if (count > n - i) return 0;
        for (j = 0; j < count; ++j) {
            c = p[i++];
            if ((c & 0xc0) != 0x80) return 0;
            value = (value << 6) | (c & 63);
        }
        if (value < minimum || value > 0x10ffff || (value >= 0xd800 && value <= 0xdfff)) return 0;
        if (controls && (value < 32 || (value >= 127 && value <= 159))) return 0;
    }
    return 1;
}
int sophia_ns_path_valid(const unsigned char *p, size_t n) {
    size_t start = 0, i;
    if (!n || n > 1023 || !utf8(p, n, 1)) return 0;
    for (i = 0; i <= n; ++i) {
        if (i == n || p[i] == '/') {
            struct span part = {p + start, i - start};
            if (!part.n || part.n > 255 || equal(part, ".") || equal(part, "..") ||
                (!start && equal(part, SOPHIA_NS_DISCOVERY_FILE))) return 0;
            start = i + 1;
        }
    }
    return 1;
}
static int number(struct span s, int zero, uint64_t *out) {
    uint64_t value = 0;
    size_t i;
    if (!s.n || (s.p[0] == '0' && (!zero || s.n != 1))) return 0;
    for (i = 0; i < s.n; ++i) {
        unsigned digit;
        if (s.p[i] < '0' || s.p[i] > '9') return 0;
        digit = s.p[i] - '0';
        if (value > (UINT64_MAX - digit) / 10) return 0;
        value = value * 10 + digit;
    }
    *out = value;
    return 1;
}
static int fields(struct span line, struct span *out, size_t count) {
    size_t start = 0, i, field = 0;
    for (i = 0; i <= line.n; ++i) {
        if (i == line.n || line.p[i] == '\t') {
            if (field == count) return 0;
            out[field].p = line.p + start;
            out[field++].n = i - start;
            start = i + 1;
        }
    }
    return field == count;
}
static int interface_valid(struct span s) {
    size_t i;
    if (!s.n || s.n > 255) return 0;
    for (i = 0; i < s.n; ++i) {
        unsigned c = s.p[i];
        if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
              (c >= '0' && c <= '9') || c == '_' || c == '-')) return 0;
    }
    return 1;
}
int sophia_ns_parse(struct sophia_ns_discovery *out, const void *bytes, size_t length) {
    struct sophia_ns_discovery next;
    const unsigned char *p = bytes, *end;
    struct span f[8], line;
    uint64_t count, version;
    size_t offset = 0, i;
    if (!out || !p || !length || length > SOPHIA_NS_MAX_BYTES ||
        p[length - 1] != '\n' || !utf8(p, length, 0)) return invalid();
    memset(&next, 0, sizeof next);
    end = memchr(p, '\n', length);
    if (!end) return invalid();
    line.p = p; line.n = (size_t)(end - p);
    if (!fields(line, f, 4) || !equal(f[0], "sophia-namespace") || !equal(f[1], "1") ||
        !number(f[2], 0, &next.revision) || !number(f[3], 1, &count) || count > SOPHIA_NS_MAX_ENDPOINTS) return invalid();
    offset = line.n + 1;
    while (offset < length) {
        struct sophia_ns_endpoint *e;
        if (next.count >= count) return invalid();
        end = memchr(p + offset, '\n', length - offset);
        if (!end) return invalid();
        line.p = p + offset; line.n = (size_t)(end - line.p);
        offset += line.n + 1;
        if (!fields(line, f, 8) || !equal(f[0], "endpoint") ||
            !sophia_ns_path_valid(f[1].p, f[1].n) || !interface_valid(f[3]) ||
            !number(f[4], 0, &version) || version > UINT32_MAX) return invalid();
        e = &next.endpoints[next.count];
        memcpy(e->path, f[1].p, f[1].n);
        memcpy(e->interface, f[3].p, f[3].n);
        e->version = (uint32_t)version;
        if (!number(f[5], 0, &e->service) || !number(f[6], 0, &e->generation)) return invalid();
        if (equal(f[2], "9p2000.L")) e->protocol = SOPHIA_NS_9P2000L;
        else if (equal(f[2], "x11")) e->protocol = SOPHIA_NS_X11;
        else return invalid();
        if (equal(f[7], "available")) e->availability = SOPHIA_NS_AVAILABLE;
        else if (equal(f[7], "waiting")) e->availability = SOPHIA_NS_WAITING;
        else if (equal(f[7], "recovering")) e->availability = SOPHIA_NS_RECOVERING;
        else return invalid();
        for (i = 0; i < next.count; ++i) {
            const char *old = next.endpoints[i].path;
            size_t n = strlen(old);
            if (!strcmp(old, e->path) || (!strncmp(old, e->path, f[1].n) && old[f[1].n] == '/') ||
                (!strncmp(e->path, old, n) && e->path[n] == '/')) return invalid();
        }
        ++next.count;
    }
    if (next.count != count) return invalid();
    *out = next;
    return 0;
}
const struct sophia_ns_endpoint *sophia_ns_find(const struct sophia_ns_discovery *view, const char *path) {
    size_t i;
    if (!view || !path || view->count > SOPHIA_NS_MAX_ENDPOINTS) return NULL;
    for (i = 0; i < view->count; ++i)
        if (memchr(view->endpoints[i].path, 0, sizeof view->endpoints[i].path) &&
            !strcmp(view->endpoints[i].path, path)) return &view->endpoints[i];
    return NULL;
}

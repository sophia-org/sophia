#include "session_internal.h"

int sf_api_profile(const uint8_t *b, size_t n, enum sophia_sf_profile profile)
{
    const char *name = profile == SOPHIA_SF_BAR        ? "bar"
                       : profile == SOPHIA_SF_LAUNCHER ? "launcher"
                       : profile == SOPHIA_SF_DOCK ? "dock" : "descriptor";
    const size_t start = sizeof("sophia-shell-files version=1 role=") - 1;
    size_t size = strlen(name);
    return n > start + size && !memcmp(b + start, name, size) && b[start + size] == ' '
               ? 0
               : SOPHIA_9P_INVALID;
}

/* Consume the complete api file before accepting its epoch: a valid prefix
 * followed by another line must never authorize record submission. */
int sf_api_epoch(const uint8_t *b, size_t n, uint64_t *epoch)
{
    static const char prefix[] = "sophia-shell-files version=1 role=";
    static const char middle[] = " epoch=";
    static const char suffix[] = " fd_transfer=none\n";
    size_t at = sizeof(prefix) - 1, start;
    uint64_t value = 0;
    if (!b || !epoch || n > 256 || n <= at || memcmp(b, prefix, at))
        return SOPHIA_9P_INVALID;
    if (b[at] < 'a' || b[at] > 'z')
        return SOPHIA_9P_INVALID;
    while (at < n && ((b[at] >= 'a' && b[at] <= 'z') || (b[at] >= '0' && b[at] <= '9') ||
                      b[at] == '_' || b[at] == '-'))
        at++;
    if (n - at < sizeof(middle) - 1 || memcmp(b + at, middle, sizeof(middle) - 1))
        return SOPHIA_9P_INVALID;
    at += sizeof(middle) - 1;
    start = at;
    if (at == n || b[at] < '1' || b[at] > '9')
        return SOPHIA_9P_INVALID;
    while (at < n && b[at] >= '0' && b[at] <= '9') {
        unsigned digit = b[at++] - '0';
        if (value > (UINT64_MAX - digit) / 10)
            return SOPHIA_9P_INVALID;
        value = value * 10 + digit;
    }
    if (at == start || n - at != sizeof(suffix) - 1 || memcmp(b + at, suffix, sizeof(suffix) - 1))
        return SOPHIA_9P_INVALID;
    *epoch = value;
    return 0;
}

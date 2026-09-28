#include "roles_internal.h"

enum text_restriction { NO_CONTROLS = 1, NO_BIDI = 2 };
static int text_valid(struct sophia_sf_text text, size_t maximum, unsigned restrictions)
{
    size_t at = 0;
    if (text.size > maximum || (text.size && !text.data))
        return -1;
    while (at < text.size) {
        uint32_t cp = text.data[at++], minimum = 0;
        unsigned trailing = 0, i;
        if (cp < 0x80) { /* ASCII scalar. */
        } else if (cp >= 0xc2 && cp <= 0xdf) {
            cp &= 0x1f;
            trailing = 1;
            minimum = 0x80;
        } else if (cp >= 0xe0 && cp <= 0xef) {
            cp &= 0x0f;
            trailing = 2;
            minimum = 0x800;
        } else if (cp >= 0xf0 && cp <= 0xf4) {
            cp &= 7;
            trailing = 3;
            minimum = 0x10000;
        } else
            return -1;
        if (trailing > text.size - at)
            return -1;
        for (i = 0; i < trailing; i++) {
            unsigned next = text.data[at++];
            if ((next & 0xc0) != 0x80)
                return -1;
            cp = (cp << 6) | (next & 0x3f);
        }
        if (cp < minimum || cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff))
            return -1;
        if ((restrictions & NO_CONTROLS) && (cp <= 0x1f || (cp >= 0x7f && cp <= 0x9f)))
            return -1;
        if ((restrictions & NO_BIDI) &&
            ((cp >= 0x202a && cp <= 0x202e) || (cp >= 0x2066 && cp <= 0x2069)))
            return -1;
    }
    return 0;
}
int sf_text_check(struct sophia_sf_text text, size_t maximum)
{
    return text_valid(text, maximum, 0);
}
int sf_launcher_text_check(struct sophia_sf_text text, size_t maximum)
{
    return text_valid(text, maximum, NO_CONTROLS | NO_BIDI);
}
int sf_descriptor_text_check(struct sophia_sf_text text, size_t maximum)
{
    return text_valid(text, maximum, NO_CONTROLS);
}
int sf_text_take(const uint8_t *b, size_t maximum, struct sophia_sf_text *out)
{
    struct sophia_sf_text value;
    value.size = (uint16_t)sf_get(b, 2);
    value.data = b + 4;
    if (value.size > maximum || !sf_zero(b + 2, 2) ||
        !sf_zero(b + 4 + value.size, maximum - value.size) || sf_text_check(value, maximum))
        return -1;
    *out = value;
    return 0;
}
void sf_text_put(uint8_t *b, size_t maximum, struct sophia_sf_text text)
{
    sf_put(b, text.size, 2);
    sf_put(b + 2, 0, 2);
    if (text.size)
        memcpy(b + 4, text.data, text.size);
    memset(b + 4 + text.size, 0, maximum - text.size);
}

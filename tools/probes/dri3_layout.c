/* Explicit DRI3 allocations and synthetic pixels only; no capture. It sends no
 * input; with --report-keys it only reports the focus changes and key presses
 * it receives.
 * Build: cc -std=c11 -O2 -Wall -Wextra -Werror tools/probes/dri3_layout.c \
 *           -o /tmp/sophia-dri3-layout $(pkg-config --cflags --libs xcb xcb-dri3 xcb-present gbm)
 */
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <gbm.h>
#include <inttypes.h>
#include <limits.h>
#include <poll.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/sysmacros.h>
#include <time.h>
#include <unistd.h>
#include <xcb/dri3.h>
#include <xcb/present.h>
#include <xcb/xcb.h>
#include <xcb/xcbext.h>

enum { MAX_PLANES = 4, MAX_MODIFIERS = 16384, MAX_FRAMES = 120 };
#define IMPLICIT_MODIFIER UINT64_C(0x00ffffffffffffff)

struct options {
    int x, y, width, height;
    uint32_t format, frames, timeout_ms, hold_ms;
    uint64_t modifier;
    bool list_only, has_geometry, has_format, has_modifier, suboptimal, managed, report_keys;
};

struct buffer {
    struct gbm_bo *bo;
    xcb_pixmap_t pixmap;
    uint32_t serial;
    uint64_t submitted_ms;
    bool busy, complete, idle;
};

struct probe {
    struct options options;
    xcb_connection_t *connection;
    xcb_window_t window;
    xcb_colormap_t colormap;
    xcb_present_event_t event_id;
    xcb_special_event_t *events;
    struct gbm_device *device;
    int render_fd;
    uint64_t deadline_ms;
    struct buffer buffers[2];
    unsigned submitted, completed, idled;
    unsigned keys_reported;
    bool keys_overflowed;
};

static uint64_t now_ms(void)
{
    struct timespec value;
    if (clock_gettime(CLOCK_MONOTONIC, &value) != 0)
        _exit(125);
    return (uint64_t)value.tv_sec * 1000 + (uint64_t)value.tv_nsec / 1000000;
}

static void expired(int signal_number)
{
    static const char message[] = "dri3_layout status=failed stage=process_deadline\n";
    (void)signal_number;
    (void)write(STDERR_FILENO, message, sizeof(message) - 1);
    _exit(124);
}

static bool fail(const char *stage)
{
    fprintf(stderr, "dri3_layout status=failed stage=%s errno=%d\n", stage, errno);
    return false;
}

static bool x_error(const char *stage, const xcb_generic_error_t *error)
{
    fprintf(stderr, "dri3_layout status=failed stage=%s x_error=%u major=%u minor=%u sequence=%u resource=%" PRIu32 "\n",
            stage, error->error_code, error->major_code, error->minor_code,
            error->sequence, error->resource_id);
    return false;
}

static bool wait_socket(struct probe *probe, const char *stage)
{
    uint64_t now = now_ms();
    if (now >= probe->deadline_ms) {
        errno = ETIMEDOUT;
        return fail(stage);
    }
    if (xcb_connection_has_error(probe->connection)) {
        errno = EPIPE;
        return fail(stage);
    }
    struct pollfd socket = { .fd = xcb_get_file_descriptor(probe->connection), .events = POLLIN };
    int result = poll(&socket, 1, (int)(probe->deadline_ms - now));
    if (result < 0 && errno == EINTR)
        return true;
    if (result <= 0) {
        if (result == 0)
            errno = ETIMEDOUT;
        return fail(stage);
    }
    if (socket.revents & (POLLERR | POLLHUP | POLLNVAL)) {
        errno = EPIPE;
        return fail(stage);
    }
    return true;
}

static void *reply(struct probe *probe, unsigned sequence, const char *stage)
{
    void *answer = NULL;
    xcb_generic_error_t *error = NULL;
    if (xcb_flush(probe->connection) <= 0) {
        fail(stage);
        return NULL;
    }
    for (;;) {
        if (xcb_poll_for_reply(probe->connection, sequence, &answer, &error)) {
            if (error) {
                x_error(stage, error);
                free(error);
                free(answer);
                return NULL;
            }
            if (!answer)
                fail(stage);
            return answer;
        }
        if (!wait_socket(probe, stage))
            return NULL;
    }
}

/* Whether the window's geometry is the requested one. An override-redirect
 * window is placed by its client, so any change is refused. A managed window
 * (--managed) is placed by the window manager: its position may differ, its
 * size may not. */
static bool geometry_matches(const struct options *options, int x, int y, int width, int height)
{
    return width == options->width && height == options->height &&
        (options->managed || (x == options->x && y == options->y));
}

/* The window's creation attributes in mask order; returns the mask. Only an
 * unmanaged window is override-redirect, which keeps it out of every policy
 * the window manager applies. */
static uint32_t window_attributes(const struct options *options, xcb_colormap_t colormap, uint32_t values[4])
{
    unsigned count = 0;
    uint32_t mask = XCB_CW_BACK_PIXEL | XCB_CW_EVENT_MASK | XCB_CW_COLORMAP;
    values[count++] = 0xff304038;
    if (!options->managed) {
        mask |= XCB_CW_OVERRIDE_REDIRECT;
        values[count++] = 1;
    }
    values[count++] = XCB_EVENT_MASK_STRUCTURE_NOTIFY |
        (options->report_keys ? XCB_EVENT_MASK_KEY_PRESS | XCB_EVENT_MASK_FOCUS_CHANGE : 0);
    values[count++] = colormap;
    return mask;
}

/* With --report-keys, at most this many key presses are reported; the next
 * one is named as an overflow once. */
#define MAX_KEY_REPORTS 64

/* With --report-keys, each focus change and key press of the window, on
 * stderr, wherever the ordinary queue is drained: focus can arrive while the
 * window is mapped, before the hold begins. Each report says whether the
 * event was synthetic (SendEvent sets the high bit of its type), so a key
 * some client sent is never taken for a routed one. */
static void report_input(struct probe *probe, const xcb_generic_event_t *event)
{
    unsigned synthetic = (event->response_type & 0x80) ? 1u : 0u;
    switch (event->response_type & 127) {
    case XCB_KEY_PRESS:
        if (probe->keys_reported < MAX_KEY_REPORTS) {
            fprintf(stderr, "dri3_layout stage=key keycode=%u synthetic=%u\n",
                    (unsigned)((const xcb_key_press_event_t *)event)->detail, synthetic);
            probe->keys_reported++;
        } else if (!probe->keys_overflowed) {
            fprintf(stderr, "dri3_layout stage=key_overflow reported=%u\n", probe->keys_reported);
            probe->keys_overflowed = true;
        }
        break;
    case XCB_FOCUS_IN:
    case XCB_FOCUS_OUT: {
        const xcb_focus_in_event_t *focus = (const xcb_focus_in_event_t *)event;
        fprintf(stderr, "dri3_layout stage=focus state=%s source=event mode=%u detail=%u synthetic=%u\n",
                (event->response_type & 127) == XCB_FOCUS_IN ? "in" : "out", focus->mode, focus->detail, synthetic);
        break;
    }
    default:
        break;
    }
}

/* Unchecked void requests report their errors on the ordinary event queue.
 * Present events use a separate XGE queue and cannot be consumed here. At most
 * `limit` events are taken; *more says whether the limit stopped the drain. */
static bool drain_events(struct probe *probe, bool queued_only, unsigned limit, bool *more)
{
    xcb_generic_event_t *event = NULL;
    unsigned taken = 0;
    while (taken < limit && (event = queued_only ? xcb_poll_for_queued_event(probe->connection) :
                                                xcb_poll_for_event(probe->connection))) {
        bool ok = true;
        taken++;
        if (event->response_type == 0)
            ok = x_error("request", (xcb_generic_error_t *)event);
        else if (probe->options.report_keys)
            report_input(probe, event);
        if ((event->response_type & 127) == XCB_CONFIGURE_NOTIFY) {
            xcb_configure_notify_event_t *configure = (void *)event;
            if (configure->window == probe->window &&
                !geometry_matches(&probe->options, configure->x, configure->y, configure->width, configure->height)) {
                errno = EINVAL;
                ok = fail("geometry_changed");
            }
        }
        free(event);
        if (!ok)
            return false;
    }
    if (more)
        *more = taken == limit;
    if (xcb_connection_has_error(probe->connection)) {
        errno = EPIPE;
        return fail("connection");
    }
    return true;
}

static bool ordinary_events(struct probe *probe, bool queued_only)
{
    return drain_events(probe, queued_only, UINT_MAX, NULL);
}

static bool barrier(struct probe *probe, const char *stage)
{
    void *answer = reply(probe, xcb_get_input_focus(probe->connection).sequence, stage);
    if (!answer)
        return false;
    free(answer);
    return ordinary_events(probe, false);
}

static bool number(const char *text, uint64_t *value)
{
    if (!*text || *text == '-' || *text == '+')
        return false;
    char *end;
    errno = 0;
    unsigned long long parsed = strtoull(text, &end, 0);
    if (errno || *end)
        return false;
    *value = (uint64_t)parsed;
    return true;
}

static bool geometry(const char *text, struct options *options)
{
    int *fields[] = { &options->x, &options->y, &options->width, &options->height };
    for (unsigned i = 0; i < 4; ++i) {
        char *end;
        errno = 0;
        long value = strtol(text, &end, 10);
        long minimum = i < 2 ? INT16_MIN : 1;
        long maximum = i < 2 ? INT16_MAX : 4096;
        if (errno || end == text || value < minimum || value > maximum ||
            (i < 3 ? *end != ',' : *end != '\0'))
            return false;
        *fields[i] = (int)value;
        text = end + (i < 3);
    }
    return true;
}

static void usage(FILE *output)
{
    fprintf(output, "usage: dri3_layout --geometry X,Y,W,H --format XR24|AR24\n"
                    "       [--modifier VALUE] [--list-only] [--frames 1..120] [--timeout-ms 100..10000]\n"
                    "       [--suboptimal] [--hold-ms 0..60000] [--managed] [--report-keys]\n"
                    "Present requires --modifier. List-only never maps or presents a window.\n"
                    "Managed windows are placed by the window manager; their size still must not change.\n"
                    "Geometry: signed 16-bit position, positive dimensions <=4096.\n"
                    "--report-keys reports focus changes and key presses during the hold on stderr.\n");
}

static bool parse(int argc, char **argv, struct options *options)
{
    *options = (struct options){ .frames = 4, .timeout_ms = 5000 };
    for (int index = 1; index < argc; ++index) {
        const char *arg = argv[index];
        if (!strcmp(arg, "--list-only")) {
            options->list_only = true;
            continue;
        }
        if (!strcmp(arg, "--suboptimal")) {
            options->suboptimal = true;
            continue;
        }
        if (!strcmp(arg, "--managed")) {
            options->managed = true;
            continue;
        }
        if (!strcmp(arg, "--report-keys")) {
            options->report_keys = true;
            continue;
        }
        if (index + 1 >= argc)
            return false;
        const char *value = argv[++index];
        if (!strcmp(arg, "--geometry")) {
            if (!geometry(value, options))
                return false;
            options->has_geometry = true;
        } else if (!strcmp(arg, "--format")) {
            if (!strcmp(value, "XR24"))
                options->format = GBM_FORMAT_XRGB8888;
            else if (!strcmp(value, "AR24"))
                options->format = GBM_FORMAT_ARGB8888;
            else
                return false;
            options->has_format = true;
        } else {
            uint64_t parsed;
            if (!number(value, &parsed))
                return false;
            if (!strcmp(arg, "--modifier")) {
                options->modifier = parsed;
                options->has_modifier = true;
            } else if (!strcmp(arg, "--frames") && parsed >= 1 && parsed <= MAX_FRAMES) {
                options->frames = (uint32_t)parsed;
            } else if (!strcmp(arg, "--timeout-ms") && parsed >= 100 && parsed <= 10000) {
                options->timeout_ms = (uint32_t)parsed;
            } else if (!strcmp(arg, "--hold-ms") && parsed <= 60000) {
                options->hold_ms = (uint32_t)parsed;
            } else {
                return false;
            }
        }
    }
    return options->has_geometry && options->has_format &&
        options->x >= INT16_MIN && options->x <= INT16_MAX &&
        options->y >= INT16_MIN && options->y <= INT16_MAX &&
        options->width > 0 && options->width <= 4096 &&
        options->height > 0 && options->height <= 4096 &&
        (options->list_only || options->has_modifier) &&
        (!options->has_modifier || (options->modifier != IMPLICIT_MODIFIER && options->modifier != UINT64_MAX));
}

static bool versions(struct probe *probe)
{
    xcb_prefetch_extension_data(probe->connection, &xcb_dri3_id);
    xcb_prefetch_extension_data(probe->connection, &xcb_present_id);
    if (!barrier(probe, "extensions"))
        return false;
    const xcb_query_extension_reply_t *dri3 = xcb_get_extension_data(probe->connection, &xcb_dri3_id);
    const xcb_query_extension_reply_t *present = xcb_get_extension_data(probe->connection, &xcb_present_id);
    if (!dri3 || !dri3->present || !present || !present->present) {
        errno = ENOTSUP;
        return fail("extensions_unavailable");
    }
    xcb_dri3_query_version_reply_t *d = reply(probe,
        xcb_dri3_query_version(probe->connection, 1, 2).sequence, "dri3_version");
    if (!d)
        return false;
    bool supported = d->major_version > 1 || (d->major_version == 1 && d->minor_version >= 2);
    printf("dri3_layout stage=dri3_version major=%" PRIu32 " minor=%" PRIu32 "\n", d->major_version, d->minor_version);
    free(d);
    if (!supported) {
        errno = ENOTSUP;
        return fail("dri3_1_2_required");
    }
    xcb_present_query_version_reply_t *p = reply(probe,
        xcb_present_query_version(probe->connection, 1, 2).sequence, "present_version");
    if (!p)
        return false;
    printf("dri3_layout stage=present_version major=%" PRIu32 " minor=%" PRIu32 "\n", p->major_version, p->minor_version);
    free(p);
    return true;
}

static xcb_visualid_t visual(const xcb_screen_t *screen, uint8_t depth)
{
    for (xcb_depth_iterator_t depths = xcb_screen_allowed_depths_iterator(screen); depths.rem; xcb_depth_next(&depths)) {
        if (depths.data->depth != depth)
            continue;
        for (xcb_visualtype_iterator_t visuals = xcb_depth_visuals_iterator(depths.data); visuals.rem; xcb_visualtype_next(&visuals)) {
            const xcb_visualtype_t *v = visuals.data;
            if (v->_class == XCB_VISUAL_CLASS_TRUE_COLOR && v->red_mask == 0xff0000 &&
                v->green_mask == 0xff00 && v->blue_mask == 0xff)
                return v->visual_id;
        }
    }
    return XCB_NONE;
}

static bool property(struct probe *probe, const char *name, uint32_t count, const uint32_t *values)
{
    xcb_intern_atom_reply_t *atom = reply(probe,
        xcb_intern_atom(probe->connection, 0, (uint16_t)strlen(name), name).sequence, "atom");
    if (!atom)
        return false;
    xcb_change_property(probe->connection, XCB_PROP_MODE_REPLACE, probe->window,
                        atom->atom, XCB_ATOM_CARDINAL, 32, count, values);
    free(atom);
    return true;
}

static bool open_window_and_device(struct probe *probe, const xcb_screen_t *screen)
{
    uint8_t depth = probe->options.format == GBM_FORMAT_ARGB8888 ? 32 : 24;
    xcb_visualid_t chosen = visual(screen, depth);
    if (chosen == XCB_NONE) {
        errno = ENOTSUP;
        return fail("visual");
    }
    probe->colormap = xcb_generate_id(probe->connection);
    xcb_create_colormap(probe->connection, XCB_COLORMAP_ALLOC_NONE, probe->colormap, screen->root, chosen);
    probe->window = xcb_generate_id(probe->connection);
    uint32_t values[4];
    uint32_t mask = window_attributes(&probe->options, probe->colormap, values);
    xcb_create_window(probe->connection, depth, probe->window, screen->root,
        (int16_t)probe->options.x, (int16_t)probe->options.y,
        (uint16_t)probe->options.width, (uint16_t)probe->options.height, 0,
        XCB_WINDOW_CLASS_INPUT_OUTPUT, chosen, mask, values);
    if (!barrier(probe, "create_window"))
        return false;
    xcb_dri3_open_reply_t *opened = reply(probe,
        xcb_dri3_open(probe->connection, probe->window, 0).sequence, "dri3_open");
    if (!opened)
        return false;
    int *fds = xcb_dri3_open_reply_fds(probe->connection, opened);
    if (opened->nfd != 1 || !fds) {
        if (fds)
            for (unsigned i = 0; i < opened->nfd; ++i)
                close(fds[i]);
        free(opened);
        errno = ENOTSUP;
        return fail("dri3_open_fd_count");
    }
    probe->render_fd = fds[0];
    free(opened);
    struct stat identity;
    if (fstat(probe->render_fd, &identity) != 0)
        return fail("render_identity");
    if (!S_ISCHR(identity.st_mode)) {
        errno = ENODEV;
        return fail("render_not_character_device");
    }
    printf("dri3_layout stage=device fs_device=%ju inode=%ju rdev=%ju major=%u minor=%u\n",
        (uintmax_t)identity.st_dev, (uintmax_t)identity.st_ino, (uintmax_t)identity.st_rdev,
        major(identity.st_rdev), minor(identity.st_rdev));
    probe->device = gbm_create_device(probe->render_fd);
    return probe->device != NULL || fail("gbm_device");
}

static bool modifiers(struct probe *probe, const char *phase)
{
    const uint32_t formats[] = { GBM_FORMAT_XRGB8888, GBM_FORMAT_ARGB8888 };
    for (unsigned f = 0; f < 2; ++f) {
        xcb_dri3_get_supported_modifiers_reply_t *r = reply(probe,
            xcb_dri3_get_supported_modifiers(probe->connection, probe->window,
                f == 0 ? 24 : 32, 32).sequence, "modifiers");
        if (!r)
            return false;
        uint64_t total = (uint64_t)r->num_window_modifiers + r->num_screen_modifiers;
        if (total > MAX_MODIFIERS || (uint64_t)r->length != total * 2) {
            free(r);
            errno = EOVERFLOW;
            return fail("modifier_reply_bounds");
        }
        uint64_t *rows[] = {
            xcb_dri3_get_supported_modifiers_window_modifiers(r),
            xcb_dri3_get_supported_modifiers_screen_modifiers(r),
        };
        uint32_t counts[] = { r->num_window_modifiers, r->num_screen_modifiers };
        for (unsigned scope = 0; scope < 2; ++scope) {
            printf("dri3_layout stage=modifiers phase=%s scope=%s format=0x%08" PRIx32 " count=%" PRIu32 "\n",
                phase, scope == 0 ? "window" : "screen", formats[f], counts[scope]);
            for (uint32_t i = 0; i < counts[scope]; ++i)
                printf("dri3_layout stage=modifier phase=%s scope=%s format=0x%08" PRIx32 " modifier=0x%016" PRIx64 "\n",
                    phase, scope == 0 ? "window" : "screen", formats[f], rows[scope][i]);
        }
        free(r);
    }
    return true;
}

static bool allocate(struct probe *probe, struct buffer *buffer, unsigned index)
{
    const struct options *o = &probe->options;
    errno = 0;
    buffer->bo = gbm_bo_create_with_modifiers2(probe->device, (uint32_t)o->width,
        (uint32_t)o->height, o->format, &o->modifier, 1, GBM_BO_USE_RENDERING);
    if (!buffer->bo)
        return fail("explicit_allocation");
    uint64_t actual = gbm_bo_get_modifier(buffer->bo);
    if (actual != o->modifier || actual == IMPLICIT_MODIFIER || actual == UINT64_MAX ||
        gbm_bo_get_format(buffer->bo) != o->format ||
        gbm_bo_get_width(buffer->bo) != (uint32_t)o->width || gbm_bo_get_height(buffer->bo) != (uint32_t)o->height) {
        fprintf(stderr, "dri3_layout status=failed stage=allocation_metadata requested=0x%016" PRIx64 " actual=0x%016" PRIx64 "\n", o->modifier, actual);
        return false;
    }
    int planes = gbm_bo_get_plane_count(buffer->bo);
    if (planes < 1 || planes > MAX_PLANES) {
        errno = EOVERFLOW;
        return fail("plane_count");
    }
    printf("dri3_layout event=allocation buffer=%u format=%" PRIu32 " modifier=%" PRIu64 " width=%d height=%d planes=%d\n",
        index, o->format, actual, o->width, o->height, planes);
    return true;
}

static bool fill(struct probe *probe, struct buffer *buffer, unsigned index)
{
    uint32_t stride = 0;
    void *mapping = NULL;
    const struct options *o = &probe->options;
    errno = 0;
    uint8_t *pixels = gbm_bo_map(buffer->bo, 0, 0, (uint32_t)o->width, (uint32_t)o->height,
                               GBM_BO_TRANSFER_WRITE, &stride, &mapping);
    if (!pixels || pixels == MAP_FAILED)
        return fail("gbm_map_write_unavailable");
    if (stride < (uint32_t)o->width * 4) {
        gbm_bo_unmap(buffer->bo, mapping);
        errno = EOVERFLOW;
        return fail("map_stride");
    }
    const uint32_t colors[] = { 0xff26384a, 0xff4a3826, 0xff304538, 0xff43344a };
    for (int y = 0; y < o->height; ++y) {
        for (int x = 0; x < o->width; ++x) {
            unsigned quadrant = (unsigned)(x >= o->width / 2) + 2 * (unsigned)(y >= o->height / 2);
            uint32_t pixel = colors[quadrant] ^ (index ? 0x00080808 : 0);
            uint8_t bytes[] = { (uint8_t)pixel, (uint8_t)(pixel >> 8),
                                (uint8_t)(pixel >> 16), (uint8_t)(pixel >> 24) };
            memcpy(pixels + (size_t)y * stride + (size_t)x * 4, bytes, sizeof(bytes));
        }
    }
    /* GBM owns staging and writeback synchronization; never mmap an opaque plane. */
    gbm_bo_unmap(buffer->bo, mapping);
    printf("dri3_layout stage=pixels buffer=%u method=gbm_map opaque=true\n", index);
    return true;
}

static bool export_planes(struct probe *probe, struct buffer *buffer, unsigned index, bool import)
{
    if (gbm_bo_get_modifier(buffer->bo) != probe->options.modifier ||
        gbm_bo_get_format(buffer->bo) != probe->options.format) {
        errno = EINVAL;
        return fail("layout_changed_after_initialization");
    }
    int count = gbm_bo_get_plane_count(buffer->bo);
    int32_t fds[MAX_PLANES] = { -1, -1, -1, -1 };
    uint32_t strides[MAX_PLANES] = { 0 }, offsets[MAX_PLANES] = { 0 };
    bool ok = false;
    if (count < 1 || count > MAX_PLANES) {
        errno = EOVERFLOW;
        return fail("plane_count_changed");
    }
    for (int p = 0; p < count; ++p) {
        errno = 0;
        fds[p] = gbm_bo_get_fd_for_plane(buffer->bo, p);
        strides[p] = gbm_bo_get_stride_for_plane(buffer->bo, p);
        offsets[p] = gbm_bo_get_offset(buffer->bo, p);
        struct stat metadata;
        if (fds[p] < 0 || fstat(fds[p], &metadata) != 0 || metadata.st_size <= 0 ||
            !strides[p] || (uint64_t)offsets[p] >= (uint64_t)metadata.st_size) {
            fail("plane_metadata_unreadable");
            goto out;
        }
        printf("dri3_layout stage=plane buffer=%u plane=%d stride=%" PRIu32 " offset=%" PRIu32 " allocation_bytes=%jd\n",
            index, p, strides[p], offsets[p], (intmax_t)metadata.st_size);
    }
    if (import) {
        buffer->pixmap = xcb_generate_id(probe->connection);
        const struct options *o = &probe->options;
        xcb_dri3_pixmap_from_buffers(probe->connection, buffer->pixmap, probe->window,
            (uint8_t)count, (uint16_t)o->width, (uint16_t)o->height,
            strides[0], offsets[0], strides[1], offsets[1], strides[2], offsets[2], strides[3], offsets[3],
            o->format == GBM_FORMAT_ARGB8888 ? 32 : 24, 32, gbm_bo_get_modifier(buffer->bo), fds);
        /* XCB takes ownership of the exported descriptors; the GBM BO remains ours. */
        for (int p = 0; p < count; ++p)
            fds[p] = -1;
        if (!barrier(probe, "pixmap_import"))
            goto out;
        xcb_get_geometry_reply_t *geometry = reply(probe,
            xcb_get_geometry(probe->connection, buffer->pixmap).sequence, "pixmap_geometry");
        if (!geometry)
            goto out;
        ok = geometry->width == o->width && geometry->height == o->height &&
             geometry->depth == (o->format == GBM_FORMAT_ARGB8888 ? 32 : 24);
        free(geometry);
        if (!ok) {
            errno = EINVAL;
            fail("pixmap_geometry_mismatch");
        }
    } else {
        ok = true;
    }
out:
    for (int p = 0; p < count; ++p)
        if (fds[p] >= 0)
            close(fds[p]);
    return ok;
}

static const char *mode_name(uint8_t mode)
{
    switch (mode) {
    case XCB_PRESENT_COMPLETE_MODE_COPY: return "copy";
    case XCB_PRESENT_COMPLETE_MODE_FLIP: return "flip";
    case XCB_PRESENT_COMPLETE_MODE_SKIP: return "skip";
    case XCB_PRESENT_COMPLETE_MODE_SUBOPTIMAL_COPY: return "suboptimal";
    default: return "unknown";
    }
}

static bool present_events(struct probe *probe)
{
    if (!ordinary_events(probe, false))
        return false;
    xcb_generic_event_t *event;
    while ((event = xcb_poll_for_special_event(probe->connection, probe->events))) {
        xcb_ge_generic_event_t *generic = (void *)event;
        uint32_t serial;
        if (generic->event_type == XCB_PRESENT_COMPLETE_NOTIFY)
            serial = ((xcb_present_complete_notify_event_t *)event)->serial;
        else if (generic->event_type == XCB_PRESENT_IDLE_NOTIFY)
            serial = ((xcb_present_idle_notify_event_t *)event)->serial;
        else {
            free(event);
            errno = EPROTO;
            return fail("unexpected_present_event");
        }
        struct buffer *buffer = NULL;
        for (unsigned i = 0; i < 2; ++i)
            if (probe->buffers[i].busy && probe->buffers[i].serial == serial)
                buffer = &probe->buffers[i];
        if (!buffer) {
            free(event);
            errno = EPROTO;
            return fail("unmatched_present_serial");
        }
        bool valid;
        if (generic->event_type == XCB_PRESENT_COMPLETE_NOTIFY) {
            xcb_present_complete_notify_event_t *complete = (void *)event;
            valid = !buffer->complete && complete->window == probe->window &&
                    complete->event == probe->event_id && complete->kind == XCB_PRESENT_COMPLETE_KIND_PIXMAP;
            if (valid) {
                buffer->complete = true;
                ++probe->completed;
                printf("dri3_layout event=complete serial=%" PRIu32 " pixmap=%" PRIu32 " mode=%s ust=%" PRIu64 " msc=%" PRIu64 " feedback_after_submit_ms=%" PRIu64 "\n",
                    serial, buffer->pixmap, mode_name(complete->mode), complete->ust, complete->msc,
                    now_ms() - buffer->submitted_ms);
            }
        } else {
            xcb_present_idle_notify_event_t *idle = (void *)event;
            valid = !buffer->idle && idle->window == probe->window &&
                    idle->event == probe->event_id && idle->pixmap == buffer->pixmap;
            if (valid) {
                buffer->idle = true;
                ++probe->idled;
                printf("dri3_layout event=idle serial=%" PRIu32 " pixmap=%" PRIu32 "\n", serial, buffer->pixmap);
            }
        }
        free(event);
        if (!valid) {
            errno = EPROTO;
            return fail("present_identity_or_duplicate");
        }
    }
    /* Do not read again here: that could queue a special event just before poll(). */
    return ordinary_events(probe, true);
}

static bool run_frames(struct probe *probe)
{
    unsigned count = probe->options.frames < 2 ? 1 : 2;
    for (unsigned i = 0; i < count; ++i)
        if (!allocate(probe, &probe->buffers[i], i) || !fill(probe, &probe->buffers[i], i) ||
            !export_planes(probe, &probe->buffers[i], i, true))
            return false;
    probe->event_id = xcb_generate_id(probe->connection);
    probe->events = xcb_register_for_special_xge(probe->connection, &xcb_present_id, probe->event_id, NULL);
    if (!probe->events) {
        errno = ENOMEM;
        return fail("present_event_queue");
    }
    xcb_present_select_input(probe->connection, probe->event_id, probe->window,
        XCB_PRESENT_EVENT_MASK_COMPLETE_NOTIFY | XCB_PRESENT_EVENT_MASK_IDLE_NOTIFY);
    uint32_t opaque[] = { 0, 0, (uint32_t)probe->options.width, (uint32_t)probe->options.height };
    uint32_t opacity = UINT32_MAX;
    if (!property(probe, "_NET_WM_OPAQUE_REGION", 4, opaque) ||
        !property(probe, "_NET_WM_WINDOW_OPACITY", 1, &opacity))
        return false;
    xcb_map_window(probe->connection, probe->window);
    if (!barrier(probe, "map_window") || !modifiers(probe, "mapped"))
        return false;
    xcb_get_geometry_reply_t *geometry = reply(probe,
        xcb_get_geometry(probe->connection, probe->window).sequence, "window_geometry");
    if (!geometry)
        return false;
    bool exact = geometry_matches(&probe->options, geometry->x, geometry->y, geometry->width, geometry->height);
    free(geometry);
    if (!exact) {
        errno = EINVAL;
        return fail("requested_geometry_not_applied");
    }
    while (probe->completed < probe->options.frames) {
        if (!present_events(probe))
            return false;
        bool sent = false;
        /* Complete permits the next slot; Idle still owns the previous scanout. */
        if (probe->submitted < probe->options.frames && probe->completed == probe->submitted) {
            for (unsigned i = 0; i < count; ++i) {
                unsigned slot = (probe->submitted + i) % count;
                struct buffer *buffer = &probe->buffers[slot];
                if (buffer->busy && !(buffer->complete && buffer->idle))
                    continue;
                buffer->busy = true;
                buffer->complete = buffer->idle = false;
                buffer->serial = ++probe->submitted;
                buffer->submitted_ms = now_ms();
                xcb_present_pixmap(probe->connection, probe->window, buffer->pixmap, buffer->serial,
                    0, 0, 0, 0, 0, 0, 0,
                    probe->options.suboptimal ? XCB_PRESENT_OPTION_SUBOPTIMAL : 0,
                    0, 0, 0, 0, NULL);
                printf("dri3_layout event=submit serial=%" PRIu32 " pixmap=%" PRIu32 " buffer=%u format=%" PRIu32 " modifier=%" PRIu64 " suboptimal=%u\n",
                    buffer->serial, buffer->pixmap, slot, probe->options.format, gbm_bo_get_modifier(buffer->bo),
                    probe->options.suboptimal ? 1u : 0u);
                if (xcb_flush(probe->connection) <= 0)
                    return fail("present_flush");
                sent = true;
                break;
            }
        }
        if (!sent && probe->completed < probe->options.frames && !wait_socket(probe, "present_deadline"))
            return false;
    }
    return modifiers(probe, "completed") && present_events(probe);
}

/* The hold with --report-keys: the window still presents nothing more, but it
 * drains its ordinary events until the hold's deadline, with the same request
 * error and geometry checks as before the hold, and reports focus and keys
 * (report_input). It first reports whether the window holds the input focus
 * now, since a focus change may have been drained before the hold began. Each
 * drain takes a bounded batch, so a stream of events cannot outlast the hold. */
static bool hold_reporting_keys(struct probe *probe)
{
    probe->deadline_ms = now_ms() + probe->options.hold_ms;
    xcb_get_input_focus_reply_t *focus =
        reply(probe, xcb_get_input_focus(probe->connection).sequence, "hold_focus");
    if (!focus)
        return false;
    fprintf(stderr, "dri3_layout stage=focus state=%s source=query\n", focus->focus == probe->window ? "in" : "out");
    free(focus);
    for (;;) {
        bool more = false;
        if (!drain_events(probe, false, 64, &more))
            return false;
        uint64_t now = now_ms();
        if (now >= probe->deadline_ms)
            return true;
        if (more)
            continue;
        struct pollfd socket = { .fd = xcb_get_file_descriptor(probe->connection), .events = POLLIN };
        uint64_t remaining = probe->deadline_ms - now;
        int result = poll(&socket, 1, remaining > INT_MAX ? INT_MAX : (int)remaining);
        if (result < 0 && errno != EINTR)
            return fail("hold_poll");
        if (result > 0 && (socket.revents & (POLLERR | POLLHUP | POLLNVAL))) {
            errno = EPIPE;
            return fail("hold_connection");
        }
    }
}

static void cleanup(struct probe *probe)
{
    if (probe->connection) {
        if (probe->window)
            xcb_destroy_window(probe->connection, probe->window);
        for (unsigned i = 0; i < 2; ++i)
            if (probe->buffers[i].pixmap)
                xcb_free_pixmap(probe->connection, probe->buffers[i].pixmap);
        if (probe->colormap)
            xcb_free_colormap(probe->connection, probe->colormap);
        if (probe->events)
            xcb_unregister_for_special_event(probe->connection, probe->events);
        xcb_flush(probe->connection);
        xcb_disconnect(probe->connection);
    }
    /* The last Flip need not receive Idle before teardown; never free it earlier. */
    for (unsigned i = 0; i < 2; ++i)
        if (probe->buffers[i].bo)
            gbm_bo_destroy(probe->buffers[i].bo);
    if (probe->device)
        gbm_device_destroy(probe->device);
    if (probe->render_fd >= 0)
        close(probe->render_fd);
}

int main(int argc, char **argv)
{
    if (argc == 2 && !strcmp(argv[1], "--help")) {
        usage(stdout);
        return 0;
    }
    struct probe probe = { .render_fd = -1 };
    if (!parse(argc, argv, &probe.options)) {
        usage(stderr);
        return 2;
    }
    const char *display = getenv("DISPLAY");
    if (!display || !*display) {
        errno = ENOENT;
        fail("display_unset");
        return 2;
    }
    setvbuf(stdout, NULL, _IOLBF, 0);
    struct sigaction action = { .sa_handler = expired };
    sigemptyset(&action.sa_mask);
    if (sigaction(SIGALRM, &action, NULL) != 0) {
        fail("deadline_handler");
        return 1;
    }
    /* Bound XCB setup and library calls too; this is not kernel cancellation. */
    struct itimerval timer = { .it_value = {
        .tv_sec = probe.options.timeout_ms / 1000,
        .tv_usec = (probe.options.timeout_ms % 1000) * 1000,
    } };
    if (setitimer(ITIMER_REAL, &timer, NULL) != 0) {
        fail("deadline_timer");
        return 1;
    }
    probe.deadline_ms = now_ms() + probe.options.timeout_ms;
    int screen_index = 0;
    probe.connection = xcb_connect(NULL, &screen_index);
    bool ok = probe.connection && !xcb_connection_has_error(probe.connection);
    if (!ok)
        fail("authenticated_connect");
    const xcb_setup_t *setup = ok ? xcb_get_setup(probe.connection) : NULL;
    xcb_screen_iterator_t screens = setup ? xcb_setup_roots_iterator(setup) : (xcb_screen_iterator_t){0};
    for (int i = 0; i < screen_index && screens.rem; ++i)
        xcb_screen_next(&screens);
    if (ok && !screens.rem) {
        errno = ENODEV;
        ok = fail("screen");
    }
    printf("dri3_layout stage=start list_only=%u x=%d y=%d width=%d height=%d format=0x%08" PRIx32 " frames=%" PRIu32 " timeout_ms=%" PRIu32 " managed=%u\n",
        probe.options.list_only, probe.options.x, probe.options.y, probe.options.width, probe.options.height,
        probe.options.format, probe.options.frames, probe.options.timeout_ms, probe.options.managed);
    if (ok)
        ok = versions(&probe) && open_window_and_device(&probe, screens.data) && modifiers(&probe, "unmapped");
    if (ok && probe.options.list_only && probe.options.has_modifier)
        ok = allocate(&probe, &probe.buffers[0], 0) && export_planes(&probe, &probe.buffers[0], 0, false);
    if (ok && !probe.options.list_only)
        ok = run_frames(&probe);
    if (ok && !probe.options.list_only && probe.options.hold_ms) {
        /* The window stays mapped with its last frame and presents nothing
         * more: a client whose content is static. The deadline bounded the
         * frames; the hold has its own bound. */
        timer = (struct itimerval){0};
        (void)setitimer(ITIMER_REAL, &timer, NULL);
        /* On stderr: a session that captures its client's stdout still
         * shows a harness that the hold has begun. */
        fprintf(stderr, "dri3_layout stage=holding hold_ms=%" PRIu32 "\n", probe.options.hold_ms);
        if (probe.options.report_keys) {
            ok = hold_reporting_keys(&probe);
        } else {
            struct timespec rest = {
                .tv_sec = probe.options.hold_ms / 1000,
                .tv_nsec = (long)(probe.options.hold_ms % 1000) * 1000000L,
            };
            while (nanosleep(&rest, &rest) != 0 && errno == EINTR) {
            }
        }
    }
    cleanup(&probe);
    timer = (struct itimerval){0};
    (void)setitimer(ITIMER_REAL, &timer, NULL);
    printf("dri3_layout event=finished result=%s window=%" PRIu32 " submitted=%u completed=%u idle=%u\n",
        ok ? "pass" : "fail", probe.window, probe.submitted, probe.completed, probe.idled);
    return ok ? 0 : 1;
}

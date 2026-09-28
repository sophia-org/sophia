/* Independent C99 content peer for Sophia's 9P2000.L shell file export.
 *
 * It links only the pinned C desktop SDK (libsophia-desktop, libsophia-9p),
 * built without its IPC library, and no Rust encoder. Modes:
 *
 *   content-proof --socket PATH   the content conformance host scenario
 *   content-serve                 the GPU content proof scenario; endpoint from
 *                                 SOPHIA_SHELL_9P_SOCKET only
 *   content-malformed --socket PATH
 *                                 boundary controls over raw 9P; every record
 *                                 below is encoded by hand from the offsets in
 *                                 protocol/sophia-shell-files-v1.kdl
 *
 * The first two use the public session API. PEER_MUTATION builds red
 * controls of content-proof that the host must refuse. */
#define _POSIX_C_SOURCE 200809L
#include "sophia_desktop_connection.h"
#include "sophia_shell_session.h"
#include <errno.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#ifndef PEER_MUTATION
#define PEER_MUTATION 0
#endif
/* 1: upload different (valid) pixels; 2: request another geometry;
 * 3: exit after the first candidate outcome, without retiring or awaiting
 *    release, and (content-serve) before the remaining renders. */

#define CAP_DESCRIPTOR_SWITCHER 1u
#define CAP_CONTENT_SURFACE 128u
#define DEADLINE_MS 10000u

static const char *mode_name = "peer";

static void require(int condition, const char *message)
{
    if (!condition) {
        fprintf(stderr, "shell content file peer (%s): %s\n", mode_name, message);
        exit(1);
    }
}

static uint64_t now_ms(void)
{
    struct timespec t;
    require(!clock_gettime(CLOCK_MONOTONIC, &t), "clock");
    return (uint64_t)t.tv_sec * 1000u + (uint64_t)t.tv_nsec / 1000000u;
}

static int connect_admitted(const char *path, uint64_t deadline)
{
    struct sophia_desktop_connection connection = {-1, 0, 0, 0};
    int result = sophia_desktop_connection_begin(&connection, path);
    while (result > 0) {
        require(now_ms() < deadline, "connect deadline");
        if (result == SOPHIA_DESKTOP_CONNECT_RETRY) {
            poll(NULL, 0, SOPHIA_DESKTOP_CONNECT_RETRY_MS);
            result = sophia_desktop_connection_begin(&connection, path);
        } else {
            struct pollfd p = {connection.fd, sophia_desktop_connection_events(&connection), 0};
            require(poll(&p, 1, 2) >= 0, "connect poll");
            result = sophia_desktop_connection_finish(&connection, p.revents);
        }
    }
    require(result == SOPHIA_DESKTOP_CONNECTED, "connect refused");
    return sophia_desktop_connection_take(&connection);
}

/* ---- Session scenarios -------------------------------------------------- */

struct scenario {
    struct sophia_ss s;
    uint64_t deadline, epoch, content_epoch;
    uint64_t output_id, output_generation, facts_generation;
    uint32_t output_width, output_height, thickness;
    uint64_t allocation_id, allocation_generation, scale_generation;
    uint32_t reservation, surface_width, surface_height;
    uint64_t permit, generation, ticket, outputs_generation, outputs_qid;
    uint16_t outcome_kind, outcome_reason;
    int stage, outputs_owed, outputs_fetching, have_output, granted, admitted, accepted;
    int have_permit, have_outcome, released;
    const uint8_t *pixels;
    size_t pixel_bytes;
    uint32_t pixel_width, pixel_height;
    int32_t destination_x, destination_y;
};

static void service(struct scenario *c)
{
    struct pollfd p = {sophia_ss_poll_fd(&c->s), sophia_ss_poll_events(&c->s), 0};
    int timeout = sophia_ss_timeout(&c->s, now_ms()), r;
    require(now_ms() < c->deadline, "scenario deadline");
    if (timeout < 0 || timeout > 2)
        timeout = 2;
    require(poll(&p, 1, timeout) >= 0, "session poll");
    r = sophia_ss_dispatch(&c->s, p.revents, 65536, now_ms());
    if (r)
        fprintf(stderr, "session ended: result=%d state=%d\n", r, sophia_ss_state(&c->s));
    require(!r, "session ended");
}

static int custodied(const struct scenario *c, uint64_t ticket)
{
    enum sophia_ss_outcome outcome;
    uint32_t error;
    if (!ticket)
        return 0;
    require(!sophia_ss_outcome(&c->s, ticket, &outcome, &error), "ticket");
    require(!error && (outcome == SOPHIA_SS_ADMITTED_LOCAL || outcome == SOPHIA_SS_IN_FLIGHT ||
                       outcome == SOPHIA_SS_SUBMITTED),
            "submission lost custody");
    return outcome == SOPHIA_SS_SUBMITTED;
}

static void submit(struct scenario *c, struct sophia_sf_record *record)
{
    int r = sophia_ss_submit(&c->s, record, 1, &c->ticket);
    require(!r, "local submit admission");
}

static void grant_fields(struct scenario *c, uint64_t *connection, uint64_t *content)
{
    *connection = c->epoch;
    *content = c->content_epoch;
}

static void handle_event(struct scenario *c, const struct sophia_sf_record *e)
{
    uint64_t connection = e->value.allocation_result.grant_connection_epoch;
    switch (e->header.kind) {
    case SOPHIA_SF_OBJECT_PUBLISHED:
        if (e->value.object_published.object_kind == SOPHIA_SF_OUTPUTS) {
            c->outputs_generation = e->value.object_published.generation;
            c->outputs_qid = e->value.object_published.qid;
            c->outputs_owed = 1;
        }
        break;
    case SOPHIA_SF_ALLOCATION_RESULT: {
        const struct sophia_sf_allocation_result *a = &e->value.allocation_result;
        require(connection == c->epoch && a->grant_content_epoch == c->content_epoch,
                "allocation result grant changed");
        require(a->allocation_request_id == 1 && a->status == 1 && !a->reason &&
                    a->output_id == c->output_id && a->output_generation == c->output_generation,
                "panel allocation was not granted");
        require(a->allocation_id && a->allocation_generation && a->scale_generation &&
                    a->scale_numerator == 1 && a->scale_denominator == 1 && !a->parent_id &&
                    !a->parent_generation,
                "panel allocation identity changed");
        require(a->pixel_x == 0 && a->pixel_y == 0 && a->pixel_width == c->surface_width &&
                    a->pixel_height == c->surface_height,
                "panel grant changed its physical placement");
        c->allocation_id = a->allocation_id;
        c->allocation_generation = a->allocation_generation;
        c->scale_generation = a->scale_generation;
        c->reservation = a->allowed_reservation_extent;
        c->granted = 1;
        break;
    }
    case SOPHIA_SF_RESOURCE_STATUS: {
        const struct sophia_sf_resource_status *r = &e->value.resource_status;
        require(r->resource_id == 1 && r->resource_generation == 1, "resource status identity");
        require(r->status == 1 || r->status == 2, "resource was not admitted and accepted");
        if (r->status == 1)
            c->admitted = 1;
        else
            c->accepted = 1;
        break;
    }
    case SOPHIA_SF_FRAME_PERMIT: {
        const struct sophia_sf_frame_permit *p = &e->value.frame_permit;
        require(p->output_id == c->output_id && p->output_generation == c->output_generation &&
                    p->demand_id == c->generation && p->permit_id && p->state == 1,
                "frame permit changed");
        c->permit = p->permit_id;
        c->have_permit = 1;
        break;
    }
    case SOPHIA_SF_CANDIDATE_OUTCOME: {
        const struct sophia_sf_candidate_outcome *o = &e->value.candidate_outcome;
        require(o->candidate_generation == c->generation && o->output_id == c->output_id &&
                    o->output_generation == c->output_generation,
                "candidate outcome identity changed");
        require((o->kind == 2) == (o->presentation_epoch != 0),
                "presentation epoch on the wrong outcome");
        c->outcome_kind = o->kind;
        c->outcome_reason = o->reason;
        c->have_outcome = 1;
        break;
    }
    case SOPHIA_SF_RESOURCE_RELEASED: {
        const struct sophia_sf_resource_released *r = &e->value.resource_released;
        require(r->resource_id == 1 && r->resource_generation == 1 && !r->reason,
                "resource release changed");
        c->released = 1;
        break;
    }
    default:
        fprintf(stderr, "unexpected event kind %u\n", e->header.kind);
        require(0, "unexpected event");
    }
}

static void pump(struct scenario *c)
{
    const struct sophia_sf_record *e;
    int r;
    service(c);
    if (!c->epoch && sophia_ss_state(&c->s) == SOPHIA_SS_READY) {
        c->epoch = sophia_ss_epoch(&c->s);
        require(c->epoch && sophia_ss_limits(&c->s), "negotiated epoch and limits");
        c->content_epoch = sophia_ss_limits(&c->s)->grant_content_epoch;
        require(c->content_epoch, "content grant epoch");
    }
    while (!sophia_ss_event(&c->s, &e)) {
        handle_event(c, e);
        require(!sophia_ss_consume(&c->s), "consume");
    }
    if (c->epoch && c->outputs_owed && !c->outputs_fetching &&
        sophia_ss_object(&c->s, SOPHIA_SF_OUTPUTS, c->outputs_generation, c->outputs_qid) == 0) {
        c->outputs_fetching = 1;
        c->outputs_owed = 0;
    }
    if (c->outputs_fetching) {
        const struct sophia_sf_record *o;
        r = sophia_ss_object_result(&c->s, &o);
        if (r != SOPHIA_9P_BUSY) {
            c->outputs_fetching = 0;
            if (r == SOPHIA_9P_AGAIN) {
                c->outputs_owed = 1;
            } else {
                const struct sophia_sf_outputs *v = &o->value.outputs;
                require(!r && o->header.kind == SOPHIA_SF_OUTPUTS, "outputs fetch");
                require(v->grant_connection_epoch == c->epoch &&
                            v->grant_content_epoch == c->content_epoch && v->output_count == 1 &&
                            v->outputs[0].scale_numerator == 1 &&
                            v->outputs[0].scale_denominator == 1,
                        "unexpected output facts");
                c->output_id = v->outputs[0].output_id;
                c->output_generation = v->outputs[0].output_generation;
                c->output_width = v->outputs[0].local_width;
                c->output_height = v->outputs[0].local_height;
                c->facts_generation = v->facts_generation;
                c->have_output = 1;
            }
        }
    }
    r = sophia_ss_ack(&c->s);
    require(!r || r == SOPHIA_9P_BUSY, "ack");
}

static void open_scenario(struct scenario *c, const char *path, void **storage)
{
    struct sophia_ss_config config = {{5, 6, CAP_DESCRIPTOR_SWITCHER | CAP_CONTENT_SURFACE},
                                      SOPHIA_SF_BAR, 16384, 8, 65536, NULL, 0};
    size_t bytes = sophia_ss_storage_bytes(config.msize, config.queue_bytes);
    int fd;
    c->deadline = now_ms() + DEADLINE_MS;
    *storage = malloc(bytes);
    require(*storage != NULL, "storage");
    fd = connect_admitted(path, c->deadline);
    require(fd >= 0, "connected fd");
    require(!sophia_ss_open_fd(&c->s, fd, &config, *storage, bytes), "session open");
    while (!(c->epoch && c->have_output))
        pump(c);
}

static void request_panel(struct scenario *c)
{
    struct sophia_sf_record record;
    struct sophia_sf_allocation_request *a = &record.value.allocation_request;
    memset(&record, 0, sizeof(record));
    record.header.kind = SOPHIA_SF_ALLOCATION_REQUEST;
    a->transaction = 3;
    grant_fields(c, &a->grant_connection_epoch, &a->grant_content_epoch);
    a->output_id = c->output_id;
    a->output_generation = c->output_generation;
    a->allocation_request_id = 1;
    a->operation = a->role = a->edge = 1;
    a->desired_width = c->surface_width;
    a->desired_height = c->surface_height;
#if PEER_MUTATION == 2
    a->desired_height -= 1;
#endif
    submit(c, &record);
}

static int upload(struct scenario *c)
{
    struct sophia_sf_resource_begin begin;
    int r;
    memset(&begin, 0, sizeof(begin));
    begin.transaction = 1;
    grant_fields(c, &begin.grant_connection_epoch, &begin.grant_content_epoch);
    begin.resource_id = begin.resource_generation = 1;
    begin.width_px = c->pixel_width;
    begin.height_px = c->pixel_height;
    begin.rendered_scale_numerator = begin.rendered_scale_denominator = 1;
    begin.pixel_format = 1;
    begin.chunk_count = 1;
    begin.total_bytes = c->pixel_bytes;
    r = sophia_ss_upload_begin(&c->s, begin, &c->ticket);
    require(!r || r == SOPHIA_9P_BUSY, "upload begin");
    return !r;
}

static void demand(struct scenario *c)
{
    struct sophia_sf_record record;
    struct sophia_sf_frame_demand *d = &record.value.frame_demand;
    memset(&record, 0, sizeof(record));
    record.header.kind = SOPHIA_SF_FRAME_DEMAND;
    d->transaction = 9;
    grant_fields(c, &d->grant_connection_epoch, &d->grant_content_epoch);
    d->output_id = c->output_id;
    d->output_generation = c->output_generation;
    d->demand_id = c->generation;
    d->reason = 1;
    c->have_permit = 0;
    submit(c, &record);
}

static void candidate(struct scenario *c)
{
    static struct sophia_sf_record record;
    struct sophia_sf_candidate *v = &record.value.candidate;
    memset(&record, 0, sizeof(record));
    record.header.kind = SOPHIA_SF_CANDIDATE;
    v->transaction = 10;
    grant_fields(c, &v->grant_connection_epoch, &v->grant_content_epoch);
    v->candidate_generation = c->generation;
    v->output_id = c->output_id;
    v->output_generation = c->output_generation;
    v->facts_generation = c->facts_generation;
    v->pacing_permit = c->permit;
    v->interaction_generation = 1;
    v->surface_count = v->placement_count = v->target_count = 1;
    v->surfaces[0].allocation_id = c->allocation_id;
    v->surfaces[0].allocation_generation = c->allocation_generation;
    v->surfaces[0].scale_generation = c->scale_generation;
    v->surfaces[0].role = v->surfaces[0].edge = 1;
    v->surfaces[0].reservation_extent =
        c->reservation < c->thickness ? c->reservation : c->thickness;
    v->surfaces[0].parent_surface_index = 65535;
    v->placements[0].resource_id = v->placements[0].resource_generation = 1;
    v->placements[0].destination_x_px = c->destination_x;
    v->placements[0].destination_y_px = c->destination_y;
    v->targets[0].action_kind = 1;
    v->targets[0].target_id = 1;
    v->targets[0].target_generation = c->generation;
    v->targets[0].action_id = 1;
    v->targets[0].bounds_x = c->destination_x;
    v->targets[0].bounds_y = c->destination_y;
    v->targets[0].bounds_width = c->pixel_width;
    v->targets[0].bounds_height = c->pixel_height;
    c->have_outcome = 0;
    submit(c, &record);
}

static void retire(struct scenario *c)
{
    struct sophia_sf_record record;
    struct sophia_sf_resource_retire *r = &record.value.resource_retire;
    memset(&record, 0, sizeof(record));
    record.header.kind = SOPHIA_SF_RESOURCE_RETIRE;
    r->transaction = 2;
    grant_fields(c, &r->grant_connection_epoch, &r->grant_content_epoch);
    r->resource_id = r->resource_generation = 1;
    submit(c, &record);
}

/* Stages shared by both scenarios, through the accepted resource. */
static void publish_resource(struct scenario *c)
{
    request_panel(c);
    while (!c->granted)
        pump(c);
    while (!upload(c))
        pump(c);
    while (!(c->admitted && custodied(c, c->ticket) && sophia_ss_upload_ready(&c->s)))
        pump(c);
    require(!sophia_ss_upload_chunk(&c->s, c->pixels, c->pixel_bytes), "upload chunk");
    while (!sophia_ss_upload_ready(&c->s))
        pump(c);
    for (;;) {
        int r = sophia_ss_upload_end(&c->s, 2, &c->ticket);
        require(!r || r == SOPHIA_9P_BUSY, "upload end");
        if (!r)
            break;
        pump(c);
    }
    while (!(c->accepted && custodied(c, c->ticket) && !sophia_ss_upload_pending(&c->s)))
        pump(c);
}

static void render(struct scenario *c)
{
    demand(c);
    while (!c->have_permit)
        pump(c);
    candidate(c);
    while (!c->have_outcome)
        pump(c);
}

static int content_proof(const char *path)
{
#if PEER_MUTATION == 1
    static const uint8_t pixels[8] = {0, 0, 255, 255, 0, 64, 0, 128};
#else
    static const uint8_t pixels[8] = {0, 0, 255, 255, 0, 128, 0, 128};
#endif
    static struct scenario c;
    void *storage;
    c.surface_width = 64;
    c.surface_height = 32;
    c.thickness = 24;
    c.pixels = pixels;
    c.pixel_bytes = sizeof(pixels);
    c.pixel_width = 2;
    c.pixel_height = 1;
    c.destination_x = 3;
    c.destination_y = 4;
    c.generation = 1;
    open_scenario(&c, path, &storage);
    require(c.output_width == 64 && c.output_height == 64, "unexpected output extent");
    publish_resource(&c);
    render(&c);
    /* The host has no native output; anything but its exact renderer
     * failure would be manufactured presentation evidence. */
    require(c.outcome_kind == 3 && c.outcome_reason == 9,
            "expected exact renderer failure, not simulated presentation");
#if PEER_MUTATION == 3
    sophia_ss_close(&c.s);
    return 0;
#endif
    retire(&c);
    while (!c.released)
        pump(&c);
    sophia_ss_close(&c.s);
    free(storage);
    puts("c_content_files schema=1 status=complete wire=9p2000.L protected_socket=true "
         "native_presentation=false");
    return 0;
}

static int content_serve(void)
{
    static struct scenario c;
    struct sophia_desktop_endpoint endpoint;
    const char *thickness = getenv("SOPHIA_SHELL_BAR_THICKNESS");
    uint8_t *raster;
    void *storage;
    size_t i;
    unsigned presented = 0;
    require(!sophia_desktop_shell_environment(&endpoint) && endpoint.wire == SOPHIA_DESKTOP_FILES,
            "requires exactly SOPHIA_SHELL_9P_SOCKET");
    require(thickness && atoi(thickness) > 0, "requires SOPHIA_SHELL_BAR_THICKNESS");
    c.thickness = (uint32_t)atoi(thickness);
    c.surface_height = c.thickness;
    c.generation = 1;
    /* The surface width follows the published output, so open first. */
    c.surface_width = 1;
    open_scenario(&c, endpoint.path, &storage);
    c.surface_width = c.output_width;
    c.pixel_width = c.surface_width;
    c.pixel_height = c.surface_height;
    c.pixel_bytes = (size_t)c.pixel_width * c.pixel_height * 4u;
    raster = malloc(c.pixel_bytes);
    require(raster != NULL, "raster");
    /* Opaque premultiplied BGRA whose pixels vary across the surface. */
    for (i = 0; i < c.pixel_bytes / 4u; i++) {
        raster[i * 4u] = (uint8_t)(i % c.pixel_width);
        raster[i * 4u + 1u] = (uint8_t)(i / c.pixel_width);
        raster[i * 4u + 2u] = (uint8_t)i;
        raster[i * 4u + 3u] = 255;
    }
    c.pixels = raster;
    publish_resource(&c);
    for (;;) {
        render(&c);
#if PEER_MUTATION == 3
        /* Red control: disconnect after the first outcome. */
        sophia_ss_close(&c.s);
        return 0;
#endif
        if (c.outcome_kind == 1) {
            /* Prepared precedes Presented for the same generation. */
            c.have_outcome = 0;
            while (!c.have_outcome)
                pump(&c);
            require(c.outcome_kind == 2, "Prepared was not followed by Presented");
        }
        if (c.outcome_kind == 2) {
            presented++;
            c.generation++;
            continue;
        }
        require(c.outcome_kind == 3 && c.outcome_reason == 9, "unexpected candidate outcome");
        break;
    }
    /* End state client-exits: retire, observe custody, exit. The renderer may
     * still hold the lease, so ResourceReleased is not awaited here. */
    retire(&c);
    while (!custodied(&c, c.ticket))
        pump(&c);
    sophia_ss_close(&c.s);
    free(raster);
    free(storage);
    printf("c_content_serve schema=1 status=complete wire=9p2000.L presented=%u "
           "renderer_failed=1\n",
           presented);
    return 0;
}

/* ---- Raw boundary controls ---------------------------------------------- */

#define TYPE_RLERROR 7u
#define TYPE_RREAD 117u
#define TYPE_RWRITE 119u
#define E_INVAL 22u
#define E_STALE 116u
#define E_ALREADY 114u

struct raw {
    struct sophia_9p_client c;
    uint64_t deadline, epoch, content_epoch, submission, sequence, events_offset;
    uint32_t root, api, events, submit, ack, limits;
    uint8_t type;
    uint32_t count, error;
    uint8_t data[16384];
    uint8_t pending[16384];
    size_t pending_used;
};

static void put(uint8_t *p, uint64_t v, unsigned bytes)
{
    unsigned i;
    for (i = 0; i < bytes; i++)
        p[i] = (uint8_t)(v >> (8u * i));
}

static uint64_t get(const uint8_t *p, unsigned bytes)
{
    uint64_t v = 0;
    unsigned i;
    for (i = 0; i < bytes; i++)
        v |= (uint64_t)p[i] << (8u * i);
    return v;
}

static void raw_wait(struct raw *r, struct sophia_9p_handle handle)
{
    for (;;) {
        struct sophia_9p_reply reply;
        struct pollfd p;
        require(now_ms() < r->deadline, "raw 9P deadline");
        require(sophia_9p_service(&r->c, 65536) >= 0, "raw 9P service");
        if (sophia_9p_peek(&r->c, &reply) == SOPHIA_9P_OK) {
            require(reply.handle.serial == handle.serial && reply.handle.slot == handle.slot,
                    "raw 9P reply order");
            r->type = reply.type;
            r->error = reply.type == TYPE_RLERROR ? reply.error : 0;
            r->count = reply.count;
            if (reply.type == TYPE_RREAD) {
                require(reply.count <= sizeof(r->data), "raw read size");
                memcpy(r->data, reply.data, reply.count);
            }
            require(!sophia_9p_consume(&r->c, reply.handle), "raw consume");
            return;
        }
        p.fd = r->c.fd;
        p.events = (short)(POLLIN | (sophia_9p_wants_write(&r->c) ? POLLOUT : 0));
        p.revents = 0;
        require(poll(&p, 1, 2) >= 0, "raw poll");
    }
}

static uint32_t raw_open(struct raw *r, const char *const *names, size_t count, uint32_t flags)
{
    struct sophia_9p_handle h;
    uint32_t fid;
    require(!sophia_9p_walk(&r->c, r->root, names, count, &h, &fid), "raw walk");
    raw_wait(r, h);
    require(r->type != TYPE_RLERROR, "raw walk refused");
    require(!sophia_9p_lopen(&r->c, fid, flags, &h), "raw lopen");
    raw_wait(r, h);
    require(r->type != TYPE_RLERROR, "raw lopen refused");
    return fid;
}

static void raw_clunk(struct raw *r, uint32_t fid)
{
    struct sophia_9p_handle h;
    require(!sophia_9p_clunk(&r->c, fid, &h), "raw clunk");
    raw_wait(r, h);
}

/* Returns 0 or the Rlerror errno; count holds the Rwrite count. */
static uint32_t raw_write(struct raw *r, uint32_t fid, uint64_t offset, const void *data,
                          size_t bytes)
{
    struct sophia_9p_handle h;
    require(!sophia_9p_write(&r->c, fid, offset, data, bytes, &h), "raw write request");
    raw_wait(r, h);
    if (r->type == TYPE_RLERROR)
        return r->error;
    require(r->type == TYPE_RWRITE && r->count == bytes, "raw write short");
    return 0;
}

static void raw_read(struct raw *r, uint32_t fid, uint64_t offset, uint32_t count)
{
    struct sophia_9p_handle h;
    require(!sophia_9p_read(&r->c, fid, offset, count, &h), "raw read request");
    raw_wait(r, h);
    require(r->type == TYPE_RREAD, "raw read refused");
}

static void header(uint8_t *p, uint32_t total, uint16_t kind, uint64_t epoch, uint64_t submission)
{
    memset(p, 0, 32);
    put(p, total, 4);
    put(p + 4, 1, 2);
    put(p + 6, kind, 2);
    put(p + 8, epoch, 8);
    put(p + 16, submission, 8);
}

static void submit_bytes(uint8_t p[24], uint64_t epoch, uint64_t submission, uint32_t bytes)
{
    memset(p, 0, 24);
    put(p, epoch, 8);
    put(p + 8, submission, 8);
    put(p + 16, bytes, 4);
}

/* Stage record bytes in a fresh transaction fid and submit them. Returns
 * the staging-write errno or else the submit errno (0 on custody). The
 * transaction fid is clunked, which drops a refused staging buffer. */
static uint32_t raw_stage(struct raw *r, const uint8_t *record, size_t bytes, const uint8_t *submit)
{
    static const char *const transaction[] = {"transaction"};
    uint32_t fid = raw_open(r, transaction, 1, 2), error;
    error = raw_write(r, fid, 0, record, bytes);
    if (!error)
        error = raw_write(r, r->submit, 0, submit, 24);
    raw_clunk(r, fid);
    return error;
}

static uint32_t raw_submit(struct raw *r, const uint8_t *record, size_t bytes)
{
    uint8_t s[24];
    submit_bytes(s, r->epoch, get(record + 16, 8), (uint32_t)bytes);
    return raw_stage(r, record, bytes, s);
}

/* The next whole event record of kind, skipping object announcements this
 * control peer never fetches; reads block at the journal tail. */
static const uint8_t *raw_event(struct raw *r, uint16_t kind)
{
    static uint8_t event[1024];
    size_t total;
next:
    while (r->pending_used < 4 || r->pending_used < get(r->pending, 4)) {
        raw_read(r, r->events, r->events_offset, 4096);
        require(r->count && r->pending_used + r->count <= sizeof(r->pending), "event read");
        memcpy(r->pending + r->pending_used, r->data, r->count);
        r->pending_used += r->count;
        r->events_offset += r->count;
    }
    total = (size_t)get(r->pending, 4);
    require(total >= 32 && total <= sizeof(event), "event size");
    memcpy(event, r->pending, total);
    memmove(r->pending, r->pending + total, r->pending_used - total);
    r->pending_used -= total;
    require(get(event + 4, 2) == 1 && get(event + 8, 8) == r->epoch, "event header");
    require(get(event + 24, 8) > r->sequence, "event sequence");
    r->sequence = get(event + 24, 8);
    if (get(event + 6, 2) == 19 && kind != 19)
        goto next;
    if (get(event + 6, 2) != kind)
        fprintf(stderr, "expected event %u, read %u\n", kind, (unsigned)get(event + 6, 2));
    require(get(event + 6, 2) == kind, "event kind");
    return event + 32;
}

static void raw_ack(struct raw *r)
{
    uint8_t a[16];
    put(a, r->epoch, 8);
    put(a + 8, r->sequence, 8);
    require(!raw_write(r, r->ack, 0, a, 16), "ack");
}

/* A valid submission: custody, then acknowledgement so the next
 * transaction may open. */
static void raw_accepted(struct raw *r, const uint8_t *record, size_t bytes)
{
    const uint8_t *s;
    require(!raw_submit(r, record, bytes), "valid independent record refused");
    s = raw_event(r, 18);
    /* Submitted names this id: no refused control was journaled first. */
    require(get(s, 8) == get(record + 16, 8) && get(s + 8, 2) == get(record + 6, 2),
            "Submitted names another submission");
    raw_ack(r);
}

static void expect(uint32_t actual, uint32_t expected, const char *control)
{
    if (actual != expected) {
        fprintf(stderr, "%s: errno %u, expected %u\n", control, actual, expected);
        require(0, "boundary control not refused as specified");
    }
    printf("control %s refused errno=%u\n", control, actual);
}

static void allocation_request(uint8_t p[160], struct raw *r, uint64_t submission)
{
    uint8_t *b = p + 32;
    header(p, 160, 257, r->epoch, submission);
    memset(b, 0, 128);
    put(b, 21, 8);
    put(b + 8, r->epoch, 8);
    put(b + 16, r->content_epoch, 8);
    put(b + 24, 2, 8);
    put(b + 32, 1, 8);
    put(b + 40, 7, 8);
    put(b + 48, 1, 2);
    put(b + 50, 1, 2);
    put(b + 52, 1, 2);
    put(b + 112, 64, 4);
    put(b + 116, 32, 4);
}

static void resource_record(uint8_t *p, struct raw *r, uint16_t kind, uint32_t bytes,
                            uint64_t submission)
{
    uint8_t *b = p + 32;
    header(p, bytes, kind, r->epoch, submission);
    memset(b, 0, bytes - 32u);
    put(b, 30 + kind, 8);
    if (kind == 258) {
        put(b + 16, r->epoch, 8);
        put(b + 24, r->content_epoch, 8);
        put(b + 32, 1, 8);
        put(b + 40, 1, 8);
        put(b + 48, 2, 4);
        put(b + 52, 1, 4);
        put(b + 56, 1, 4);
        put(b + 60, 1, 4);
        put(b + 64, 1, 2);
        put(b + 68, 1, 4);
        put(b + 72, 8, 8);
        return;
    }
    put(b + 8, r->epoch, 8);
    put(b + 16, r->content_epoch, 8);
    put(b + 24, 1, 8);
    put(b + 32, 1, 8);
    if (kind == 259) {
        put(b + 40, 8, 8);
        put(b + 48, 1, 4);
    }
}

static int content_malformed(const char *path)
{
    static struct raw r;
    static const char *const api[] = {"api"}, *const events[] = {"events"},
                             *const submit[] = {"submit"}, *const ack[] = {"ack"},
                             *const limits[] = {"limits"}, *const slot[] = {"upload", "0"};
    static const uint8_t pixels[8] = {0, 0, 255, 255, 0, 128, 0, 128};
    uint8_t record[160], big[256], s[24];
    const uint8_t *e;
    const char *field;
    struct sophia_9p_handle h;
    size_t storage_bytes = sophia_9p_storage_bytes(16384, 4);
    void *storage = malloc(storage_bytes);
    uint32_t upload;
    int fd;
    require(storage != NULL, "storage");
    r.deadline = now_ms() + DEADLINE_MS;
    fd = connect_admitted(path, r.deadline);
    require(!sophia_9p_init(&r.c, fd, 16384, 4, 16, storage, storage_bytes), "9P init");
    require(!sophia_9p_version(&r.c, &h), "version");
    raw_wait(&r, h);
    require(!sophia_9p_attach(&r.c, "", "", &h, &r.root), "attach");
    raw_wait(&r, h);
    require(r.type != TYPE_RLERROR, "attach refused");
    r.api = raw_open(&r, api, 1, 0);
    raw_read(&r, r.api, 0, 512);
    r.data[r.count < sizeof(r.data) ? r.count : sizeof(r.data) - 1] = 0;
    field = strstr((const char *)r.data, " epoch=");
    require(field != NULL, "api epoch");
    r.epoch = strtoull(field + 7, NULL, 10);
    require(r.epoch, "api epoch value");
    r.events = raw_open(&r, events, 1, 0);
    r.submit = raw_open(&r, submit, 1, 1);
    r.ack = raw_open(&r, ack, 1, 1);

    /* Negotiate: bar content without discrete input. */
    header(record, 48, 256, r.epoch, ++r.submission);
    put(record + 32, 5, 2);
    put(record + 34, 6, 2);
    put(record + 40, CAP_DESCRIPTOR_SWITCHER | CAP_CONTENT_SURFACE, 8);
    raw_accepted(&r, record, 48);
    e = raw_event(&r, 16);
    require(get(e, 2) == 6 && get(e + 4, 8) == r.epoch && get(e + 26, 2) == 1,
            "negotiated profile");
    r.limits = raw_open(&r, limits, 1, 0);
    raw_read(&r, r.limits, 0, 4096);
    require(r.count >= 48 && get(r.data + 6, 2) == 1 && get(r.data + 32, 8) == r.epoch,
            "limits object");
    r.content_epoch = get(r.data + 40, 8);
    require(r.content_epoch, "content epoch");
    raw_ack(&r);

    /* Boundary controls. None may journal Submitted or reach an owner. */
    allocation_request(record, &r, 10);
    put(record, 100, 4);
    expect(raw_submit(&r, record, 160), E_INVAL, "declared_length_below_written");
    allocation_request(record, &r, 10);
    put(record, 65537, 4);
    expect(raw_submit(&r, record, 160), E_INVAL, "declared_length_above_transaction_cap");
    allocation_request(record, &r, 10);
    submit_bytes(s, r.epoch, 10, 159);
    expect(raw_stage(&r, record, 160, s), E_INVAL, "submit_length_mismatch");
    allocation_request(record, &r, 10);
    submit_bytes(s, r.epoch, 10, 160);
    put(s + 20, 1, 4);
    expect(raw_stage(&r, record, 160, s), E_INVAL, "submit_reserved_nonzero");
    allocation_request(record, &r, 10);
    put(record + 4, 2, 2);
    expect(raw_submit(&r, record, 160), E_INVAL, "api_version_2");
    allocation_request(record, &r, 10);
    put(record + 6, 32, 2);
    expect(raw_submit(&r, record, 160), E_INVAL, "event_kind_as_candidate");
    allocation_request(record, &r, 10);
    put(record + 6, 300, 2);
    expect(raw_submit(&r, record, 160), E_INVAL, "unknown_kind");
    allocation_request(record, &r, 10);
    submit_bytes(s, r.epoch, 11, 160);
    expect(raw_stage(&r, record, 160, s), E_INVAL, "submission_id_mismatch");
    allocation_request(record, &r, 10);
    put(record + 8, r.epoch + 1, 8);
    expect(raw_submit(&r, record, 160), E_STALE, "header_epoch_stale");
    allocation_request(record, &r, 10);
    submit_bytes(s, r.epoch + 1, 10, 160);
    expect(raw_stage(&r, record, 160, s), E_STALE, "submit_epoch_stale");
    allocation_request(record, &r, 10);
    put(record + 32 + 54, 1, 2);
    expect(raw_submit(&r, record, 160), E_INVAL, "allocation_reserved_nonzero");
    allocation_request(record, &r, 10);
    put(record + 32 + 48, 4, 2);
    expect(raw_submit(&r, record, 160), E_INVAL, "allocation_operation_out_of_range");
    allocation_request(record, &r, 1);
    expect(raw_submit(&r, record, 160), E_ALREADY, "replayed_submission_id");

    /* The same attach still accepts independently encoded valid records. */
    allocation_request(record, &r, 10);
    raw_accepted(&r, record, 160);
    e = raw_event(&r, 32);
    require(get(e + 24, 8) == 7 && get(e + 32, 2) == 1 && get(e + 120, 4) == 64 &&
                get(e + 124, 4) == 32,
            "allocation was not granted exactly");
    raw_ack(&r);

    resource_record(big, &r, 258, 112, 11);
    raw_accepted(&r, big, 112);
    e = raw_event(&r, 33);
    require(get(e + 24, 8) == 1 && get(e + 40, 2) == 1, "resource not admitted");
    raw_ack(&r);
    upload = raw_open(&r, slot, 2, 1);
    expect(raw_write(&r, upload, 4, pixels + 4, 4), E_INVAL, "upload_gap");
    memcpy(big, pixels, 8);
    memset(big + 8, 0, 4);
    expect(raw_write(&r, upload, 0, big, 12), E_INVAL, "upload_past_declared_length");
    require(!raw_write(&r, upload, 0, pixels, 8), "canonical upload");
    resource_record(big, &r, 259, 88, 12);
    raw_accepted(&r, big, 88);
    e = raw_event(&r, 33);
    require(get(e + 24, 8) == 1 && get(e + 40, 2) == 2 && get(e + 48, 8) == 8,
            "resource not accepted");
    raw_ack(&r);
    raw_clunk(&r, upload);

    resource_record(big, &r, 261, 72, 13);
    raw_accepted(&r, big, 72);
    e = raw_event(&r, 34);
    require(get(e + 24, 8) == 1 && !get(e + 40, 2), "resource release");
    raw_ack(&r);
    close(fd);
    free(storage);
    puts("c_content_files_controls schema=1 status=complete wire=9p2000.L refused=15 "
         "accepted_after_refusal=true");
    return 0;
}

int main(int argc, char **argv)
{
    if (argc == 2 && !strcmp(argv[1], "content-serve")) {
        mode_name = argv[1];
        return content_serve();
    }
    require(argc == 4 && !strcmp(argv[2], "--socket"),
            "usage: content-proof|content-malformed --socket PATH, or content-serve");
    mode_name = argv[1];
    if (!strcmp(argv[1], "content-proof"))
        return content_proof(argv[3]);
    if (!strcmp(argv[1], "content-malformed"))
        return content_malformed(argv[3]);
    require(0, "unknown mode");
    return 2;
}

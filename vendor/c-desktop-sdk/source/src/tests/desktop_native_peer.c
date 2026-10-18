#define _POSIX_C_SOURCE 200809L
#include "../sophia_desktop_connection.h"
#include "../sophia_shell_native_session.h"
#include "../sophia_shell_image.h"
#include <fcntl.h>
#include <assert.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

/* Live r9 native launcher client: the real sophia_ns + sophia_ss + 9P stack
 * against Sophia's production file export and content owners. The control
 * channel only schedules scripted Session steps (publication, presentation,
 * input issue, close); it supplies no wire records. Only the public ns and
 * ss APIs are used. Pointer activation is not exercised here.
 *
 * Timing boundary: the fixture freezes its content clock and issues input at
 * a fixed fixture timestamp, which is not this process's CLOCK_MONOTONIC.
 * No deadline is exercised: no input-ack, action-ack, permit or presentation
 * expiry, and no production timing coverage is claimed. The ns still runs
 * its advisory local permit bound on the real clock.
 * argv: control socket, export socket. */
static const char *control;
static struct sophia_ss ss;
static struct sophia_ns ns;
static uint64_t deadline;
static unsigned edits;
static int ns_ready, ss_open, image_enabled, image_dir;
static struct sophia_ss_image image;
static unsigned char formats[65536];
static const char *export_path;
static void step(void);

static uint64_t now_ms(void)
{
    struct timespec t;
    assert(!clock_gettime(CLOCK_MONOTONIC, &t));
    return (uint64_t)t.tv_sec * 1000u + (uint64_t)t.tv_nsec / 1000000u;
}
static void phase(const char *name)
{
    struct sockaddr_un a;
    struct timeval timeout = {5, 0};
    char line[128], reply[128];
    size_t used = 0;
    ssize_t n;
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    assert(fd >= 0 && strlen(control) < sizeof(a.sun_path));
    memset(&a, 0, sizeof(a));
    a.sun_family = AF_UNIX;
    strcpy(a.sun_path, control);
    assert(!setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout)));
    assert(!setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout)));
    assert(!connect(fd, (struct sockaddr *)&a, sizeof(a)));
    n = snprintf(line, sizeof(line), "native %s\n", name);
    assert(n > 0 && (size_t)n < sizeof(line) && write(fd, line, (size_t)n) == n);
    /* Robustness: keep the session serviced (bounded by the deadline in
     * step) while the fixture holds a barrier. */
    for (;;) {
        struct pollfd p = {fd, POLLIN, 0};
        assert(poll(&p, 1, 0) >= 0);
        if (!p.revents) {
            if (ss_open)
                step();
            else
                assert(!poll(NULL, 0, 1) && now_ms() < deadline);
            continue;
        }
        n = read(fd, reply + used, sizeof(reply) - 1 - used);
        assert(n > 0);
        used += (size_t)n;
        assert(used < sizeof(reply));
        if (reply[used - 1] == '\n')
            break;
    }
    reply[used] = 0;
    if (strcmp(reply, "ok\n"))
        fprintf(stderr, "phase %s: %s", name, reply);
    assert(!strcmp(reply, "ok\n"));
    close(fd);
}
/* One bounded service pass of the session, then the ns and its acks. */
static void step(void)
{
    struct pollfd p[2];
    int timeout, r;
    assert(now_ms() < deadline);
    p[0].fd = sophia_ss_poll_fd(&ss);
    p[0].events = sophia_ss_poll_events(&ss);
    p[0].revents = 0;
    p[1].fd = image_enabled ? sophia_ss_image_poll_fd(&image) : -1;
    p[1].events = image_enabled ? sophia_ss_image_poll_events(&image) : 0;
    p[1].revents = 0;
    timeout = sophia_ss_timeout(&ss, now_ms());
    if (timeout < 0 || timeout > 2)
        timeout = 2;
    assert(poll(p, 2, timeout) >= 0);
    if (image_enabled)
        assert(sophia_ss_image_dispatch(&image, p[1].revents, now_ms()) >= 0);
    r = sophia_ss_dispatch(&ss, p[0].revents, 65536, now_ms());
    if (r)
        fprintf(stderr, "session ended: result=%d state=%d\n", r, sophia_ss_state(&ss));
    assert(!r);
    if (ns_ready)
        assert(!sophia_ns_service(&ns, now_ms()));
    r = sophia_ss_ack(&ss);
    assert(!r || r == SOPHIA_9P_BUSY);
}
#define RETRY(expr)                                                                                \
    do {                                                                                           \
        int retry_;                                                                                \
        while ((retry_ = (expr)) == SOPHIA_9P_BUSY)                                                \
            step();                                                                                \
        if (retry_)                                                                                \
            fprintf(stderr, "%s -> %d\n", #expr, retry_);                                          \
        assert(!retry_);                                                                           \
    } while (0)
/* The next ns event, which must be of this kind; it stays the head. */
static struct sophia_ns_event next(enum sophia_ns_event_kind kind)
{
    struct sophia_ns_event e;
    int r;
    memset(&e, 0, sizeof(e));
    while ((r = sophia_ns_next(&ns, now_ms(), &e)) == SOPHIA_9P_AGAIN)
        step();
    if (r || e.kind != kind)
        fprintf(stderr, "event r=%d kind=%d record=%u want %d\n", r, (int)e.kind,
                e.record ? e.record->header.kind : 0u, (int)kind);
    assert(!r && e.kind == kind);
    return e;
}
static void done(void) { assert(!sophia_ns_done(&ns)); }
/* Custody only: the server's Submitted for this ticket. */
static void custody(uint64_t ticket)
{
    enum sophia_ss_outcome o;
    for (;;) {
        assert(!sophia_ss_outcome(&ss, ticket, &o, NULL));
        if (o == SOPHIA_SS_SUBMITTED)
            return;
        assert(o == SOPHIA_SS_ADMITTED_LOCAL || o == SOPHIA_SS_IN_FLIGHT);
        step();
    }
}
/* An announced object: consume its announcement, fetch it, return it. */
static const struct sophia_sf_record *fetch(uint16_t kind)
{
    struct sophia_ns_event e = next(SOPHIA_NS_EV_OTHER);
    struct sophia_sf_object_published v;
    const struct sophia_sf_record *object;
    int r;
    assert(e.record->header.kind == SOPHIA_SF_OBJECT_PUBLISHED);
    v = e.record->value.object_published;
    assert(v.object_kind == kind);
    done();
    RETRY(sophia_ss_object(&ss, kind, v.generation, v.qid));
    while ((r = sophia_ss_object_result(&ss, &object)) == SOPHIA_9P_BUSY)
        step();
    assert(!r && object->header.kind == kind);
    return object;
}
static int edit(void *user, const struct sophia_sf_native_input *v)
{
    (void)user;
    assert(v->kind == 1 && v->text.size == 1 && v->text.data[0] == 'a');
    edits++;
    return 1;
}
static struct sophia_ns_scene scene(void)
{
    struct sophia_ns_scene s;
    memset(&s, 0, sizeof(s));
    s.placement_count = 1;
    s.placements[0].resource_id = 5;
    s.placements[0].resource_generation = 1;
    s.row_count = 1;
    s.rows[0] = 1;
    s.selected = 1;
    s.targets[0].target_id = 1;
    s.targets[0].target_generation = 1;
    s.targets[0].bounds_width = 2;
    s.targets[0].bounds_height = 1;
    return s;
}
/* Demand, permit, candidate, Prepared, then the scripted Presented and the
 * exact focus minted for it. Nothing is presented or focused before. */
static void frame(uint64_t revision)
{
    struct sophia_ns_scene s = scene();
    struct sophia_ns_obligations o;
    struct sophia_ns_event e;
    const struct sophia_sf_native_binding *f;
    uint64_t ticket, cticket, candidate;
    assert(sophia_ns_revision(&ns) == revision);
    RETRY(sophia_ns_demand(&ns, 1, now_ms(), &ticket));
    e = next(SOPHIA_NS_EV_PERMIT);
    assert(e.current && e.record->value.frame_permit.state == 1);
    done();
    custody(ticket);
    assert(!sophia_ns_obligations(&ns, &o) && o.permit_expires_ms);
    RETRY(sophia_ns_present(&ns, &s, now_ms(), &cticket));
    assert(sophia_ns_demand(&ns, 1, now_ms(), &ticket) == SOPHIA_9P_INVALID);
    /* The candidate is correlated from its own first outcome. */
    e = next(SOPHIA_NS_EV_CANDIDATE);
    assert(e.current && e.record->value.candidate_outcome.kind == 1);
    candidate = e.record->value.candidate_outcome.candidate_generation;
    done();
    custody(cticket);
    assert(!sophia_ns_presented(&ns) || sophia_ns_presented(&ns)->generation != candidate);
    phase("present");
    e = next(SOPHIA_NS_EV_CANDIDATE);
    assert(e.current && e.record->value.candidate_outcome.kind == 2 &&
           e.record->value.candidate_outcome.candidate_generation == candidate);
    done();
    assert(sophia_ns_presented(&ns)->generation == candidate);
    if (revision > 1) {
        e = next(SOPHIA_NS_EV_FOCUS_REVOKED);
        assert(e.current && !sophia_ns_focus(&ns));
        done();
    }
    e = next(SOPHIA_NS_EV_FOCUS);
    assert(e.current && e.held);
    done();
    f = sophia_ns_focus(&ns);
    assert(f && f->candidate_generation == candidate && f->state_revision == revision &&
           f->interaction_generation == SOPHIA_NS_INTERACTION_GENERATION);
}
static void launcher(void)
{
    const struct sophia_ns_config config = {1000, 5};
    const struct sophia_sf_record *object;
    struct sophia_ns_allocation_params p = {1, 64, 32, 0, 0, 0, 0};
    struct sophia_ic_metadata metadata;
    struct sophia_ip_offer_info info;
    struct sophia_ss_image_config image_config;
    const struct sophia_sf_limits *limits;
    char directory[4096], *slash;
    uint64_t offer;
    int source;
    void *mapped;
    size_t mapped_bytes;
    struct sophia_ss_reservation hold;
    struct sophia_ns_obligations o;
    struct sophia_ns_event e;
    static const uint8_t pixels[8] = {0};
    uint64_t ticket, first;
    while (!sophia_ss_limits(&ss))
        step();
    limits = sophia_ss_limits(&ss);
    assert(strlen(export_path) < sizeof(directory));
    strcpy(directory, export_path);
    slash = strrchr(directory, '/');
    assert(slash);
    slash[slash == directory ? 1 : 0] = 0;
    image_dir = open(directory, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    assert(image_dir >= 0);
    memset(&image_config, 0, sizeof(image_config));
    image_config.endpoint_dir = image_dir;
    image_config.formats = formats;
    image_config.formats_capacity = sizeof(formats);
    image_config.producer.bounds.max_resource_bytes = limits->max_resource_bytes;
    image_config.producer.bounds.max_width = limits->max_width_px;
    image_config.producer.bounds.max_height = limits->max_height_px;
    image_config.producer.bounds.max_outputs = limits->max_outputs;
    image_config.producer.bounds.transfer_slots = limits->max_open_transfers;
    image_config.producer.transfer_timeout_ms = limits->transfer_timeout_ms;
    image_config.producer.startup_deadline_ms = deadline;
    assert(!sophia_ss_image_init(&image, &image_config));
    assert(!sophia_ss_image_attach(&ss, &image));
    image_enabled = 1;
    RETRY(sophia_ns_init(&ns, &ss, &config));
    ns_ready = 1;
    phase("publish");
    object = fetch(SOPHIA_SF_CATALOG);
    assert(object->value.catalog.generation == 1);
    assert(!sophia_ns_catalog(&ns, 1));
    phase("outputs");
    object = fetch(SOPHIA_SF_OUTPUTS);
    assert(!sophia_ns_facts(&ns, object->value.outputs.facts_generation));
    phase("opening");
    e = next(SOPHIA_NS_EV_OPENING);
    assert(e.current && sophia_ns_opening(&ns)->opening == 7 && sophia_ns_revision(&ns) == 1);
    done();
    assert(!sophia_ns_obligations(&ns, &o) && !o.needs_catalog && !o.needs_facts);
    /* Allocation, correlated by request ID. */
    RETRY(sophia_ns_allocate(&ns, &p, now_ms(), &ticket));
    e = next(SOPHIA_NS_EV_ALLOCATION);
    assert(e.current && e.record->value.allocation_result.status == 1 &&
           e.record->value.allocation_result.allocation_request_id == 1);
    done();
    custody(ticket);
    /* Accepted status alone does not settle source custody. */
    while (sophia_ip_channel(&image.producer) != SOPHIA_IPC_READY)
        step();
    memset(&metadata, 0, sizeof(metadata));
    metadata.resource_id = 5;
    metadata.resource_generation = 1;
    metadata.admission_epoch = limits->grant_content_epoch;
    metadata.output = sophia_ns_allocation(&ns)->output_id;
    metadata.allocation = sophia_ns_allocation(&ns)->allocation_id;
    metadata.topology_generation = 3;
    metadata.source_kind = metadata.source_format = 1;
    metadata.width = 2;
    metadata.height = 1;
    metadata.stride = 8;
    metadata.plane_count = metadata.descriptor_count = 1;
    metadata.logical_bytes = sizeof(pixels);
    assert(!sophia_ip_isb_create(2, 1, &source, &mapped, &mapped_bytes));
    assert(mapped_bytes == sizeof(pixels));
    memcpy(mapped, pixels, mapped_bytes);
    assert(!sophia_ip_isb_finish(source, mapped, mapped_bytes));
    RETRY(sophia_ss_image_begin(&image, &metadata, source, now_ms(), &offer, &ticket));
    e = next(SOPHIA_NS_EV_RESOURCE);
    assert(e.record->value.resource_status.status == 2);
    done();
    custody(ticket);
    for (;;) {
        assert(!sophia_ip_offer(&image.producer, offer, &info));
        if (info.state == SOPHIA_IPO_SETTLED)
            break;
        step();
    }
    assert(info.status == SOPHIA_IPS_ACCEPTED && info.disposition == 1);
    assert(!sophia_ns_adopt_image(&ns, &image, offer));
    frame(1);
    /* Semantic input: no edit without reserved ack room; exactly one ack. */
    phase("text");
    e = next(SOPHIA_NS_EV_INPUT);
    assert(e.current && e.editable && !e.can_activate && sophia_ns_revision(&ns) == 2);
    RETRY(sophia_ss_reserve(&ss, 1, 256, &hold)); /* another owner holds the room */
    assert(sophia_ns_input_apply(&ns, 0, edit, NULL, &first) == SOPHIA_9P_BUSY && !edits);
    assert(!sophia_ss_cancel(&ss, &hold));
    RETRY(sophia_ns_input_apply(&ns, 0, edit, NULL, &first));
    assert(edits == 1);
    custody(first);
    phase("ack-consumed");
    assert(!sophia_ns_obligations(&ns, &o) && o.needs_candidate && !o.lost_acks);
    frame(2);
    /* Keyboard activation with its Accept, then the echoed outcome. */
    phase("accept");
    e = next(SOPHIA_NS_EV_INPUT);
    assert(e.current && e.can_activate && !e.editable);
    RETRY(sophia_ns_input_apply(&ns, 1, edit, NULL, &first));
    assert(edits == 1);
    e = next(SOPHIA_NS_EV_ACTIVATION);
    assert(e.current && e.record->value.native_activation_outcome.status == 1 &&
           !e.record->value.native_activation_outcome.reason &&
           e.record->value.native_activation_outcome.activation.cause == 1 &&
           e.record->value.native_activation_outcome.activation.slot == 1);
    done();
    custody(first);
    custody(first + 1);
    assert(!sophia_ns_focus(&ns)); /* Admitted disarms focus */
    phase("one-admission");
    /* Close ends the opening; the allocation is not used again. */
    phase("close");
    e = next(SOPHIA_NS_EV_FOCUS_REVOKED);
    assert(e.record->value.native_focus_revoked.reason == 12);
    done();
    e = next(SOPHIA_NS_EV_CLOSED);
    assert(e.current && e.record->value.native_closed.opening == 7);
    done();
    assert(!sophia_ns_opening(&ns) && !sophia_ns_focus(&ns) && !sophia_ns_presented(&ns) &&
           !sophia_ns_allocation(&ns));
    assert(!sophia_ns_obligations(&ns, &o) && o.awaiting_invalidation);
    /* Scripted: the old pixels are gone; the real owner invalidates. */
    phase("invalidate");
    e = next(SOPHIA_NS_EV_ALLOCATION);
    assert(e.current && e.record->value.allocation_result.status == 4 &&
           e.record->value.allocation_result.reason == 12);
    done();
    assert(!sophia_ns_obligations(&ns, &o) && !o.awaiting_invalidation);
    /* Resource settlement through the real store. */
    RETRY(sophia_ns_retire(&ns, 5, 1, &ticket));
    e = next(SOPHIA_NS_EV_RESOURCE_RELEASED);
    assert(e.current);
    done();
    custody(ticket);
    assert(!sophia_ns_obligations(&ns, &o) && !o.lost_acks && !o.next_ms);
}
/* The host supplies an afid verdict. This exercises the admitted-root API,
 * not the production launch authentication conversation. */
static void auth_reply(struct sophia_9p_client *wire, struct sophia_9p_handle request,
                       unsigned type)
{
    struct sophia_9p_reply reply;
    for (;;) {
        int result;
        struct pollfd p = {wire->fd, POLLIN, 0};
        assert(now_ms() < deadline);
        if (sophia_9p_wants_write(wire))
            p.events |= POLLOUT;
        assert(poll(&p, 1, 2) >= 0);
        assert(!sophia_9p_service(wire, 65536));
        result = sophia_9p_peek(wire, &reply);
        if (result == SOPHIA_9P_AGAIN)
            continue;
        assert(!result && reply.handle.serial == request.serial && reply.type == type && !reply.error);
        assert(!sophia_9p_consume(wire, request));
        return;
    }
}

int main(int argc, char **argv)
{
    struct sophia_desktop_connection connection = {-1, 0, 0, 0};
    struct sophia_ss_config config;
    struct sophia_ss_obligations o;
    size_t bytes;
    void *storage, *objects;
    int fd, result;
    struct sophia_9p_client wire;
    struct sophia_9p_handle request;
    uint32_t root, afid;
    size_t wire_bytes;
    void *wire_storage;
    assert(argc == 3);
    control = argv[1];
    export_path = argv[2];
    memset(&config, 0, sizeof(config));
    config.offer.minimum_revision = config.offer.maximum_revision = 9;
    config.offer.required_capabilities = 0x29a0;
    config.profile = SOPHIA_SF_LAUNCHER;
    config.msize = 8192;
    config.queue_slots = 8;
    config.queue_bytes = 65536;
    objects = malloc(SOPHIA_SF_MAX_RECORD);
    config.object_storage = objects;
    config.object_capacity = SOPHIA_SF_MAX_RECORD;
    bytes = sophia_ss_storage_bytes(config.msize, config.queue_bytes);
    storage = malloc(bytes);
    assert(bytes && storage && objects);
    assert(getchar() == 'G');
    deadline = now_ms() + 20000;
    phase("start");
    result = sophia_desktop_connection_begin(&connection, argv[2]);
    while (result > 0) {
        assert(now_ms() < deadline);
        if (result == SOPHIA_DESKTOP_CONNECT_RETRY) {
            assert(!poll(NULL, 0, SOPHIA_DESKTOP_CONNECT_RETRY_MS));
            result = sophia_desktop_connection_begin(&connection, argv[2]);
        } else {
            struct pollfd p = {connection.fd, sophia_desktop_connection_events(&connection), 0};
            assert(poll(&p, 1, 2) >= 0);
            result = sophia_desktop_connection_finish(&connection, p.revents);
        }
    }
    assert(result == SOPHIA_DESKTOP_CONNECTED);
    fd = sophia_desktop_connection_take(&connection);
    assert(fd >= 0);
    wire_bytes = sophia_9p_storage_bytes(config.msize, 8);
    wire_storage = malloc(wire_bytes);
    assert(wire_storage);
    assert(!sophia_9p_init(&wire, fd, config.msize, 8, 32, wire_storage, wire_bytes));
    assert(!sophia_9p_version(&wire, &request));
    auth_reply(&wire, request, 101);
    assert(!sophia_9p_auth(&wire, "", "", &request, &afid));
    auth_reply(&wire, request, 103);
    assert(!sophia_9p_attach_with_afid(&wire, afid, "", "", &request, &root));
    auth_reply(&wire, request, 105);
    assert(!sophia_9p_clunk(&wire, afid, &request));
    auth_reply(&wire, request, 121);
    assert(!sophia_ss_open_attached(&ss, &wire, root, &config, storage, bytes));
    ss_open = 1;
    while (sophia_ss_state(&ss) == SOPHIA_SS_NEGOTIATING)
        step();
    assert(sophia_ss_state(&ss) == SOPHIA_SS_READY);
    launcher();
    /* Every consumed event acknowledged before leaving. */
    for (;;) {
        assert(!sophia_ss_obligations(&ss, &o));
        if (o.acked == o.consumed && !o.objects)
            break;
        step();
    }
    assert(sophia_ns_state(&ns) == SOPHIA_NS_LIVE);
    sophia_ss_close(&ss);
    sophia_ip_close(&image.producer);
    close(image_dir);
    sophia_9p_close(&wire);
    close(fd);
    free(wire_storage);
    free(storage);
    free(objects);
    printf("sophia_c_native_sdk status=pass\n");
    return 0;
}

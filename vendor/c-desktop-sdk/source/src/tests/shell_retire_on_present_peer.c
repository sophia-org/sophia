#define _POSIX_C_SOURCE 200809L
#include "../sophia_shell_image.h"
#include <fcntl.h>
#include <poll.h>
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

#include "../sophia_shell_retire.h"

/* Production-export qualification lives in Sophia; this peer uses public C APIs only. */
static struct sophia_ss session;
#define client session.files
static struct sophia_ss_image image;
static uint8_t formats[65536];
static int image_dir, image_enabled;
static uint64_t now_ms(void) {
    struct timespec now;
    assert(!clock_gettime(CLOCK_MONOTONIC, &now));
    return (uint64_t)now.tv_sec * 1000 + (uint64_t)now.tv_nsec / 1000000;
}
static void auth_reply(struct sophia_9p_client *wire, struct sophia_9p_handle request, uint8_t type) {
    uint64_t deadline = now_ms() + 5000;
    for (;;) {
        struct sophia_9p_reply reply;
        struct pollfd p = {wire->fd, POLLIN, 0};
        int result;
        assert(now_ms() < deadline);
        if (sophia_9p_wants_write(wire)) p.events |= POLLOUT;
        assert(poll(&p, 1, 2) >= 0);
        assert(!sophia_9p_service(wire, 65536));
        result = sophia_9p_peek(wire, &reply);
        if (result == SOPHIA_9P_AGAIN) continue;
        assert(!result && reply.handle.serial == request.serial && reply.type == type && !reply.error);
        assert(!sophia_9p_consume(wire, request));
        return;
    }
}
static struct timespec started;
static int connect_path(const char *path)
{
    struct sockaddr_un a = {0};
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    struct timeval timeout = {5, 0};
    assert(fd >= 0 && strlen(path) < sizeof(a.sun_path));
    a.sun_family = AF_UNIX;
    strcpy(a.sun_path, path);
    assert(!setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout)));
    assert(!setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout)));
    assert(!connect(fd, (struct sockaddr *)&a, sizeof(a)));
    return fd;
}
static void step(void)
{
    struct timespec now, pause = {0, 100000};
    int status;
    struct pollfd p[2] = {{sophia_ss_poll_fd(&session), sophia_ss_poll_events(&session), 0},
        {image_enabled ? sophia_ss_image_poll_fd(&image) : -1,
         image_enabled ? sophia_ss_image_poll_events(&image) : 0, 0}};
    assert(poll(p, 2, 1) >= 0);
    status = sophia_ss_dispatch(&session, p[0].revents, 65536, now_ms());
    assert(!status);
    if (image_enabled) assert(sophia_ss_image_dispatch(&image, p[1].revents, now_ms()) >= 0);
    status = sophia_ss_ack(&session);
    assert(!status || status == SOPHIA_9P_BUSY);
    assert(!clock_gettime(CLOCK_MONOTONIC, &now));
    assert(now.tv_sec - started.tv_sec < 30);
    nanosleep(&pause, NULL);
}
static struct sophia_sf_record event(unsigned kind)
{
    const struct sophia_sf_record *r;
    struct sophia_sf_record out;
    while (sophia_ss_event(&session, &r))
        step();
    if (r->header.kind != kind)
        fprintf(stderr, "event %u want %u\n", r->header.kind, kind);
    assert(r->header.kind == kind);
    out = *r;
    assert(!sophia_ss_consume(&session));
    return out;
}
static void submit(struct sophia_sf_record *r)
{
    uint64_t ticket;
    enum sophia_ss_outcome outcome;
    int result;
    do {
        result = sophia_ss_submit(&session, r, 1, &ticket);
        if (result == SOPHIA_9P_BUSY) step();
    } while (result == SOPHIA_9P_BUSY);
    assert(!result);
    do {
        step();
        assert(!sophia_ss_outcome(&session, ticket, &outcome, NULL));
    } while (outcome < SOPHIA_SS_SUBMITTED);
    assert(outcome == SOPHIA_SS_SUBMITTED);
}
static void allocate(void)
{
    struct sophia_sf_record r = {0}, result;
    result = event(SOPHIA_SF_OBJECT_PUBLISHED);
    assert(!sophia_ss_object(&session, SOPHIA_SF_OUTPUTS,
        result.value.object_published.generation, result.value.object_published.qid));
    {
        const struct sophia_sf_record *outputs;
        while (!client.object_ready) step();
        assert(!sophia_ss_object_result(&session, &outputs));
        assert(outputs->header.kind == SOPHIA_SF_OUTPUTS);
    }
    {
        struct sophia_sf_allocation_request *a = &r.value.allocation_request;
        r.header.kind = SOPHIA_SF_ALLOCATION_REQUEST;
        a->transaction = 40;
        a->grant_connection_epoch = client.epoch;
        a->grant_content_epoch = 1;
        a->output_id = 2;
        a->output_generation = 1;
        a->allocation_request_id = 1;
        a->operation = 1;
        a->role = 1;
        a->edge = 1;
        a->desired_width = 64;
        a->desired_height = 32;
    }
    submit(&r);
    result = event(SOPHIA_SF_ALLOCATION_RESULT);
    assert(result.value.allocation_result.status == 1 &&
           result.value.allocation_result.allocation_request_id == 1);
}
static void upload(uint64_t id)
{
    struct sophia_ss_ring_image m = {0};
    struct sophia_ss_ring_offer_info info;
    struct sophia_sf_record status;
    uint64_t offer;
    uint8_t *pixels;
    int result;
    while (sophia_ss_image_ring_state(&image, NULL) < SOPHIA_SS_RING_READY)
        step();
    assert(sophia_ss_image_ring_state(&image, NULL) == SOPHIA_SS_RING_READY);
    /* Direct fill: the pixels are written straight into the ring slot. */
    pixels = sophia_ss_image_ring_pixels(&image, 0);
    assert(pixels);
    memset(pixels, 0, 8);
    m.resource_id = id; m.resource_generation = 1;
    m.allocation_id = 1; m.allocation_generation = 1;
    m.output_id = 2; m.output_generation = 1; m.topology_generation = 3;
    m.width = 2; m.height = 1;
    do {
        result = sophia_ss_image_ring_offer(&image, 0, &m, now_ms(), &offer);
        if (result == SOPHIA_IPR_AGAIN) step();
    } while (result == SOPHIA_IPR_AGAIN);
    assert(!result);
    status = event(SOPHIA_SF_RESOURCE_STATUS);
    assert(status.value.resource_status.status == 2);
    /* Settled only after the final status and the exact SlotReleased. */
    do {
        step();
        assert(!sophia_ss_image_ring_info(&image, offer, &info));
    } while (info.state == SOPHIA_SS_SLOT_OFFERING || info.state == SOPHIA_SS_SLOT_OFFERED);
    assert(info.state == SOPHIA_SS_SLOT_RELEASED && info.status == SOPHIA_IPS_ACCEPTED &&
           !info.reason && !info.changed_generation && info.adoptable);
    assert(!sophia_ss_image_ring_forget(&image, offer));
    assert(sophia_ss_image_ring_pixels(&image, 0));
}
static uint64_t permit(uint64_t demand)
{
    struct sophia_sf_record r = {0}, e;
    struct sophia_sf_frame_demand *v = &r.value.frame_demand;
    r.header.kind = SOPHIA_SF_FRAME_DEMAND;
    v->transaction = 80 + demand;
    v->grant_connection_epoch = client.epoch;
    v->grant_content_epoch = 1;
    v->output_id = 2;
    v->output_generation = 1;
    v->allocation_id = 1;
    v->allocation_generation = 1;
    v->demand_id = demand;
    v->reason = 1;
    submit(&r);
    e = event(SOPHIA_SF_FRAME_PERMIT);
    assert(e.value.frame_permit.state == 1 && e.value.frame_permit.demand_id == demand);
    return e.value.frame_permit.permit_id;
}
static void seen(const char *kind, uint64_t id)
{
    printf("%s %llu\n", kind, (unsigned long long)id);
    assert(!fflush(stdout));
}
static void frames(void)
{
    struct sophia_sr tracker;
    struct sophia_sr_entry entries[2];
    uint64_t generation;
    allocate();
    assert(!sophia_sr_init(&tracker, client.epoch, 1, entries, 2));
    for (generation = 1; generation <= 5; ++generation) {
        struct sophia_sf_record r = {0}, outcome;
        struct sophia_sf_candidate *c = &r.value.candidate;
        struct sophia_sf_candidate_retire retire = {40 + generation,
            generation == 5 ? 3 : generation - 1, 1};
        struct sophia_sr_progress progress;
        uint64_t ticket;
        int result;
        if (generation <= 4) {
            upload(generation);
            seen("uploaded", generation);
        }
        r.header.kind = SOPHIA_SF_CANDIDATE;
        c->transaction = 20 + generation;
        c->grant_connection_epoch = client.epoch;
        c->grant_content_epoch = 1;
        c->candidate_generation = generation;
        c->output_id = 2; c->output_generation = 1;
        c->facts_generation = 3; c->interaction_generation = 4;
        c->pacing_permit = permit(generation);
        c->surface_count = c->placement_count = 1;
        c->surfaces[0].allocation_id = c->surfaces[0].allocation_generation = 1;
        c->surfaces[0].scale_generation = 5;
        c->surfaces[0].role = c->surfaces[0].edge = 1;
        c->surfaces[0].reservation_extent = 24;
        c->surfaces[0].parent_surface_index = 65535;
        c->placements[0].resource_id = generation == 5 ? 4 : generation;
        c->placements[0].resource_generation = 1;
        if (generation == 1) submit(&r);
        else {
            do {
                result = sophia_ss_submit_retiring_candidate(&session, &r, &retire, &ticket);
                if (result == SOPHIA_9P_BUSY) step();
            } while (result == SOPHIA_9P_BUSY);
            assert(!result);
            assert(!sophia_sr_name(&tracker, 2, 1, generation, &retire));
        }
        do {
            outcome = event(SOPHIA_SF_CANDIDATE_OUTCOME);
            assert(outcome.value.candidate_outcome.candidate_generation == generation);
            assert(!sophia_sr_observe(&tracker, &outcome, &progress));
            if (outcome.value.candidate_outcome.kind == 1)
                assert(progress.kind == SOPHIA_SR_UNRELATED);
        } while (outcome.value.candidate_outcome.kind == 1);
        assert(outcome.value.candidate_outcome.kind == (generation == 4 ? 3 : 2));
        assert(outcome.value.candidate_outcome.reason == (generation == 4 ? 9 : 0));
        if (generation == 1) {
            assert(progress.kind == SOPHIA_SR_UNRELATED);
            continue;
        }
        assert(progress.resource_id == retire.resource_id && progress.resource_generation == 1);
        if (generation == 4) {
            assert(progress.kind == SOPHIA_SR_KEPT);
            assert(!sophia_sr_holds(&tracker, retire.resource_id, 1));
            seen("kept", retire.resource_id);
            continue;
        }
        assert(progress.kind == SOPHIA_SR_RETIRING);
        assert(sophia_sr_holds(&tracker, retire.resource_id, 1));
        seen("retiring", retire.resource_id);
        if (generation == 5) {
            /* The owner disconnects while this predecessor is still leased. */
            uint64_t deadline = now_ms() + 5000;
            do {
                struct pollfd p = {sophia_ss_poll_fd(&session), sophia_ss_poll_events(&session), 0};
                const struct sophia_sf_record *unexpected;
                assert(now_ms() < deadline);
                assert(poll(&p, 1, 2) >= 0);
                result = sophia_ss_dispatch(&session, p.revents, 65536, now_ms());
                assert(sophia_ss_event(&session, &unexpected) != 0);
            } while (!result);
            assert(sophia_ss_state(&session) == SOPHIA_SS_CLOSED ||
                sophia_ss_state(&session) == SOPHIA_SS_STALE);
            sophia_sr_revoke(&tracker);
            assert(!sophia_sr_holds(&tracker, retire.resource_id, 1));
            seen("closed", 0);
            return;
        }
        outcome = event(SOPHIA_SF_RESOURCE_RELEASED);
        assert(!sophia_sr_observe(&tracker, &outcome, &progress));
        assert(progress.kind == SOPHIA_SR_RELEASED && progress.resource_id == retire.resource_id);
        assert(!sophia_sr_holds(&tracker, retire.resource_id, 1));
        seen("released", retire.resource_id);
    }
}
int main(int argc, char **argv)
{
    struct sophia_9p_client wire;
    struct sophia_sf_negotiate offer;
    struct sophia_ss_config config = {0};
    struct sophia_ss_image_config image_config = {0};
    struct sophia_9p_handle request;
    uint32_t afid, attached_root;
    void *session_storage;
    size_t session_bytes;
    char directory[256], *slash;
    const char *endpoint;
    size_t bytes = sophia_9p_storage_bytes(4096, 8);
    void *memory = malloc(bytes), *objects = malloc(SOPHIA_SF_MAX_RECORD);
    int fd;
    assert(argc == 2 && memory && objects);
    endpoint = argv[1];
    assert(getchar() == 'G');
    assert(!clock_gettime(CLOCK_MONOTONIC, &started));
    fd = connect_path(endpoint);
    assert(!sophia_9p_init(&wire, fd, 4096, 8, 32, memory, bytes));
    offer.minimum_revision = offer.maximum_revision = 12;
    offer.required_capabilities = UINT64_C(0x8081) | SOPHIA_SF_CAP_CONTENT_RETIRE_ON_PRESENT;
    assert(!sophia_9p_version(&wire, &request)); auth_reply(&wire, request, 101);
    assert(!sophia_9p_auth(&wire, "", "", &request, &afid)); auth_reply(&wire, request, 103);
    assert(!sophia_9p_attach_with_afid(&wire, afid, "", "", &request, &attached_root)); auth_reply(&wire, request, 105);
    assert(!sophia_9p_clunk(&wire, afid, &request)); auth_reply(&wire, request, 121);
    config.offer = offer; config.profile = SOPHIA_SF_BAR; config.msize = 4096;
    config.queue_slots = 8; config.queue_bytes = 65536;
    config.object_storage = objects;
    config.object_capacity = SOPHIA_SF_MAX_RECORD;
    memset(objects, 0xa5, SOPHIA_SF_MAX_RECORD);
    session_bytes = sophia_ss_storage_bytes(config.msize, config.queue_bytes);
    session_storage = malloc(session_bytes); assert(session_storage);
    assert(!sophia_ss_open_attached(&session, &wire, attached_root, &config, session_storage, session_bytes));
    strcpy(directory, endpoint); slash = strrchr(directory, '/');
    assert(slash); slash[slash == directory ? 1 : 0] = 0;
    image_dir = open(directory, O_RDONLY | O_DIRECTORY | O_CLOEXEC); assert(image_dir >= 0);
    image_config.endpoint_dir = image_dir; image_config.formats = formats;
    image_config.formats_capacity = sizeof formats;
    image_config.producer.bounds.max_resource_bytes = 4194304;
    image_config.producer.bounds.max_width = image_config.producer.bounds.max_height = 8192;
    image_config.producer.bounds.max_outputs = 16; image_config.producer.bounds.transfer_slots = 4;
    image_config.producer.transfer_timeout_ms = 5000;
    image_config.producer.startup_deadline_ms = now_ms() + 5000;
    image_config.ring_slots = 2; image_config.ring_capacity = 4096;
    assert(!sophia_ss_image_init(&image, &image_config));
    assert(!sophia_ss_image_attach(&session, &image)); image_enabled = 1;
    while (sophia_ss_state(&session) == SOPHIA_SS_NEGOTIATING) step();
    assert(sophia_ss_state(&session) == SOPHIA_SS_READY);
    assert(sophia_ss_welcome(&session)->capabilities == (offer.required_capabilities | 2));
    frames();
    sophia_ss_close(&session);
    sophia_ss_image_close(&image);
    close(image_dir);
    sophia_9p_close(&wire);
    free(session_storage);
    close(fd);
    free(objects);
    free(memory);
    return 0;
}

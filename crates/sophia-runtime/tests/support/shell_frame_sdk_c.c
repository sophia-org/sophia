/* Independent C SDK against the shared real file owner/copy worker fixture.
 * Authentication verdicts and renderer completion are supplied by the Rust owner. */
#define _POSIX_C_SOURCE 200809L
#include "sophia_shell_frame_stream.h"
#include "sophia_shell_image.h"
#include <assert.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

static struct sophia_9p_client wire;
static struct sophia_ss session;
static struct sophia_ss_image image;
static struct sophia_ss_frame_stream stream;
static uint64_t deadline;
static int fd;
static int image_enabled;
static const struct sophia_sft_key key = {7, 9, 2, 3, 2, 1, 6, 1};

static uint64_t now_ms(void)
{
    struct timespec t;
    assert(!clock_gettime(CLOCK_MONOTONIC, &t));
    return (uint64_t)t.tv_sec * 1000 + (uint64_t)t.tv_nsec / 1000000;
}

static void auth_reply(struct sophia_9p_handle request, uint8_t type)
{
    for (;;) {
        struct sophia_9p_reply reply;
        struct pollfd p = {fd, POLLIN, 0};
        int r;
        assert(now_ms() < deadline);
        if (sophia_9p_wants_write(&wire)) p.events |= POLLOUT;
        assert(poll(&p, 1, 2) >= 0);
        assert(!sophia_9p_service(&wire, 65536));
        r = sophia_9p_peek(&wire, &reply);
        if (r == SOPHIA_9P_AGAIN) continue;
        assert(!r && reply.handle.serial == request.serial && reply.type == type && !reply.error);
        assert(!sophia_9p_consume(&wire, request));
        return;
    }
}

static void step(void)
{
    struct pollfd p[2] = {
        {sophia_ss_poll_fd(&session), sophia_ss_poll_events(&session), 0},
        {image_enabled ? sophia_ss_image_poll_fd(&image) : -1,
         image_enabled ? sophia_ss_image_poll_events(&image) : 0, 0}};
    int r;
    assert(now_ms() < deadline);
    assert(poll(p, 2, 2) >= 0);
    if (image_enabled)
        assert(sophia_ss_image_dispatch(&image, p[1].revents, now_ms()) >= 0);
    r = sophia_ss_dispatch(&session, p[0].revents, 65536, now_ms());
    if (r) fprintf(stderr, "C SFT session result=%d state=%d\n", r, sophia_ss_state(&session));
    assert(!r);
    r = sophia_ss_ack(&session);
    assert(!r || r == SOPHIA_9P_BUSY);
}

/* Image status remains an application event; fetch each announced object
 * before acknowledging it. Neither is a frame credit or a source release. */
static void auxiliary(void)
{
    const struct sophia_sf_record *e;
    int r;
    while (!(r = sophia_ss_event(&session, &e))) {
        if (e->header.kind == SOPHIA_SF_RESOURCE_STATUS) {
            assert(e->value.resource_status.status == 2);
            assert(!sophia_ss_consume(&session));
        } else if (e->header.kind == SOPHIA_SF_OBJECT_PUBLISHED) {
            struct sophia_sf_object_published v = e->value.object_published;
            assert(!sophia_ss_consume(&session));
            while ((r = sophia_ss_object(&session, v.object_kind, v.generation, v.qid)) == SOPHIA_9P_BUSY)
                step();
            assert(!r);
            while ((r = sophia_ss_object_result(&session, &e)) == SOPHIA_9P_BUSY) step();
            assert(!r && e->header.kind == v.object_kind);
        } else return;
    }
    assert(r == SOPHIA_9P_AGAIN);
}

static struct sophia_sf_record next(uint16_t kind)
{
    for (;;) {
        const struct sophia_sf_record *e;
        struct sophia_sf_record copy;
        int r;
        auxiliary();
        r = sophia_ss_event(&session, &e);
        if (r == SOPHIA_9P_AGAIN) { step(); continue; }
        assert(!r);
        if (e->header.kind != kind)
            fprintf(stderr, "C SFT expected kind=%u received=%u\n", kind, e->header.kind);
        assert(e->header.kind == kind);
        copy = *e;
        if (kind >= SOPHIA_SFT_OPENED && kind <= SOPHIA_SFT_OUTCOME)
            assert(!sophia_ss_frame_observe(&stream, e));
        assert(!sophia_ss_consume(&session));
        return copy;
    }
}

static struct sophia_ss_ring_image metadata(uint64_t resource)
{
    const struct sophia_ss_ring_image m = {resource, 1, 2, 1, 2, 3, 6, 2, 1};
    return m;
}

static void settle(uint64_t offer)
{
    for (;;) {
        struct sophia_ss_ring_offer_info info;
        auxiliary();
        assert(!sophia_ss_image_ring_info(&image, offer, &info));
        if (info.state == SOPHIA_SS_SLOT_RELEASED) {
            assert(info.status == SOPHIA_IPS_ACCEPTED && info.adoptable);
            assert(!info.reason && !info.changed_generation);
            assert(!sophia_ss_image_ring_forget(&image, offer));
            assert(sophia_ss_image_ring_pixels(&image, 0));
            return;
        }
        assert(info.state == SOPHIA_SS_SLOT_OFFERING || info.state == SOPHIA_SS_SLOT_OFFERED);
        step();
    }
}

static struct sophia_sf_record scene(void)
{
    struct sophia_sf_record r = {0};
    struct sophia_sf_candidate *c = &r.value.frame.commit.candidate;
    r.header.kind = SOPHIA_SFT_COMMIT;
    c->transaction = 101;
    c->grant_connection_epoch = 7; c->grant_content_epoch = 9;
    c->candidate_generation = 1; c->output_id = 2; c->output_generation = 3;
    c->facts_generation = 6; c->pacing_permit = 1; c->interaction_generation = 8;
    c->surface_count = 1; c->placement_count = 2;
    c->surfaces[0].allocation_id = 2; c->surfaces[0].allocation_generation = 1;
    c->surfaces[0].scale_generation = 4;
    c->surfaces[0].role = c->surfaces[0].edge = 1;
    c->surfaces[0].reservation_extent = 32;
    c->surfaces[0].parent_surface_index = UINT16_MAX;
    c->placements[0] = (struct sophia_sf_content_placement){10, 1, 0, 0, 0};
    c->placements[1] = (struct sophia_sf_content_placement){6, 1, 0, 2, 0};
    return r;
}

static void legacy(struct sophia_sf_candidate candidate)
{
    struct sophia_sf_record demand = {0}, permit, r = {0}, outcome;
    uint64_t ticket;
    demand.header.kind = SOPHIA_SF_FRAME_DEMAND;
    demand.value.frame_demand = (struct sophia_sf_frame_demand){110, 7, 9, 2, 3, 2, 1, 1, 1};
    assert(!sophia_ss_submit(&session, &demand, 1, &ticket));
    permit = next(SOPHIA_SF_FRAME_PERMIT);
    assert(permit.value.frame_permit.state == 1);
    candidate.transaction = 111; candidate.candidate_generation = 2;
    candidate.pacing_permit = permit.value.frame_permit.permit_id;
    candidate.placements[0].resource_id = 11;
    r.header.kind = SOPHIA_SF_CANDIDATE; r.value.candidate = candidate;
    assert(!sophia_ss_submit(&session, &r, 1, &ticket));
    outcome = next(SOPHIA_SF_CANDIDATE_OUTCOME);
    assert(outcome.value.candidate_outcome.kind == 1 && outcome.value.candidate_outcome.candidate_generation == 2);
    outcome = next(SOPHIA_SF_CANDIDATE_OUTCOME);
    assert(outcome.value.candidate_outcome.kind == 2 && outcome.value.candidate_outcome.candidate_generation == 2);
    assert(sophia_ss_welcome(&session)->selected_revision == 13);
}

int main(int argc, char **argv)
{
    static uint8_t wire_storage[1u << 20], storage[1u << 20], formats[4096];
    struct sophia_ss_config config = {{13, 13, (1u << 18) | (1u << 15) | (1u << 7) | 1u},
                                     SOPHIA_SF_BAR, 8192, 16, 65536, NULL, 0};
    struct sophia_ss_image_config ic = {0};
    struct sophia_9p_handle request;
    struct sophia_sf_record r = scene(), outcome;
    struct sophia_ss_ring_image m;
    uint32_t afid, root;
    uint64_t ticket, offer, resource;
    int directory, complete;
    struct sockaddr_un address = {0};
    assert(argc == 3);
    assert(strlen(argv[2]) < sizeof address.sun_path);
    address.sun_family = AF_UNIX; strcpy(address.sun_path, argv[2]);
    fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    assert(fd >= 0 && !connect(fd, (struct sockaddr *)&address, sizeof address));
    deadline = now_ms() + 10000;
    assert(!sophia_9p_init(&wire, fd, 8192, 16, 32, wire_storage, sizeof wire_storage));
    assert(!sophia_9p_version(&wire, &request)); auth_reply(request, 101);
    assert(!sophia_9p_auth(&wire, "", "", &request, &afid)); auth_reply(request, 103);
    assert(!sophia_9p_attach_with_afid(&wire, afid, "", "", &request, &root)); auth_reply(request, 105);
    assert(!sophia_9p_clunk(&wire, afid, &request)); auth_reply(request, 121);
    assert(!sophia_ss_open_attached(&session, &wire, root, &config, storage, sizeof storage));
    while (!sophia_ss_limits(&session)) { step(); auxiliary(); }
    assert(sophia_ss_welcome(&session)->selected_revision == 13);
    directory = open(argv[1], O_RDONLY | O_DIRECTORY | O_CLOEXEC); assert(directory >= 0);
    ic.endpoint_dir = directory; ic.formats = formats; ic.formats_capacity = sizeof formats;
    ic.ring_slots = 2; ic.ring_capacity = 8;
    ic.producer.bounds.max_resource_bytes = 1u << 20;
    ic.producer.bounds.max_width = 1920; ic.producer.bounds.max_height = 1024;
    ic.producer.bounds.max_outputs = 1; ic.producer.bounds.transfer_slots = 4;
    ic.producer.transfer_timeout_ms = 2000; ic.producer.startup_deadline_ms = deadline;
    assert(!sophia_ss_image_init(&image, &ic));
    assert(!sophia_ss_image_attach(&session, &image)); image_enabled = 1;
    while (sophia_ss_image_ring_state(&image, NULL) < SOPHIA_SS_RING_READY) { step(); auxiliary(); }
    assert(sophia_ss_image_ring_state(&image, NULL) == SOPHIA_SS_RING_READY);
    for (resource = 4; resource < 7; ++resource) {
        void *pixels = sophia_ss_image_ring_pixels(&image, 0); assert(pixels);
        memset(pixels, (int)resource, 8); m = metadata(resource);
        assert(!sophia_ss_frame_initial_ring_offer(&image, &stream, &key, 0, &m, now_ms(), &offer));
        settle(offer);
    }
    assert(!sophia_ss_frame_has_credit(&stream));
    assert(!sophia_ss_frame_open(&session, &stream, 100, &key, &ticket));
    assert(!sophia_ss_frame_has_credit(&stream));
    next(SOPHIA_SFT_OPENED); assert(sophia_ss_frame_has_credit(&stream));
    memset(sophia_ss_image_ring_pixels(&image, 0), 41, 8); m = metadata(10);
    assert(!sophia_ss_frame_submit_ring(&image, &stream, 0, &m, &r, now_ms(), &ticket, &offer));
    assert(!sophia_ss_frame_has_credit(&stream) && !sophia_ss_image_ring_pixels(&image, 0));
    outcome = next(SOPHIA_SFT_OUTCOME);
    assert(outcome.value.frame.outcome.candidate.kind == 1 && !sophia_ss_frame_has_credit(&stream));
    outcome = next(SOPHIA_SFT_OUTCOME);
    assert(outcome.value.frame.outcome.candidate.kind == 2 && sophia_ss_frame_has_credit(&stream));
    assert(!sophia_ss_frame_close(&session, &stream, 102, &ticket));
    next(SOPHIA_SFT_CLOSED); assert(sophia_ss_frame_drained(&stream));
    settle(offer);
    memset(sophia_ss_image_ring_pixels(&image, 0), 73, 8); m = metadata(11);
    assert(!sophia_ss_frame_legacy_ring_offer(&image, &stream, 0, &m, now_ms(), &offer));
    settle(offer);
    legacy(r.value.frame.commit.candidate);
    complete = openat(directory, "c-complete", O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    assert(complete >= 0); close(complete);
    /* Keep the authenticated process alive until the owner has checked leases. */
    while (faccessat(directory, "c-release", F_OK, 0)) {
        assert(now_ms() < deadline);
        assert(!poll(NULL, 0, 1));
    }
    sophia_ss_image_close(&image); sophia_ss_close(&session); close(directory);
    return 0;
}

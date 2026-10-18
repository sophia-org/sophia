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

/* This peer drives the public C file-session API. The test control channel
 * schedules real server owners; it supplies no wire records or codec bytes. */
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
static const char *root, *fixture;
static struct timespec started;
static uint8_t input_text[256];
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
static void phase(const char *name)
{
    char path[512], line[256], reply[256];
    size_t used = 0;
    ssize_t n;
    int fd;
    assert(snprintf(path, sizeof(path), "%s/control.sock", root) > 0);
    fd = connect_path(path);
    n = snprintf(line, sizeof(line), "%s %s\n", fixture, name);
    assert(n > 0 && (size_t)n < sizeof(line));
    assert(write(fd, line, (size_t)n) == n);
    do {
        n = read(fd, reply + used, sizeof(reply) - 1 - used);
        assert(n > 0);
        used += (size_t)n;
        assert(used < sizeof(reply));
    } while (reply[used - 1] != '\n');
    reply[used] = 0;
    if (strcmp(reply, "ok\n"))
        fprintf(stderr, "phase %s: %s", name, reply);
    assert(!strcmp(reply, "ok\n"));
    close(fd);
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
    if (kind == SOPHIA_SF_NATIVE_INPUT) {
        memcpy(input_text, r->value.native_input.text.data, r->value.native_input.text.size);
        out.value.native_input.text.data = input_text;
    }
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
static void echoed(const struct sophia_sf_record *request, const struct sophia_sf_record *outcome,
                   size_t body_size)
{
    struct sophia_sf_record value = *request;
    uint8_t expected[512], actual[512];
    size_t a, b;
    value.header.epoch = client.epoch;
    value.header.submission = 1;
    value.header.sequence = 0;
    assert(!sophia_sf_encode(expected, sizeof(expected), &value, &a));
    assert(!sophia_sf_encode(actual, sizeof(actual), outcome, &b));
    assert(a == 32 + body_size && b >= a && !memcmp(expected + 32, actual + 32, body_size));
}
static void snapshot(uint64_t generation, unsigned count, int identities)
{
    struct sophia_sf_record published = event(SOPHIA_SF_OBJECT_PUBLISHED);
    const struct sophia_sf_record *r;
    struct sophia_sf_catalog_entry row;
    assert(published.value.object_published.object_kind == SOPHIA_SF_CATALOG);
    assert(published.value.object_published.generation == generation);
    assert(!sophia_ss_object(&session, SOPHIA_SF_CATALOG, generation,
                                    published.value.object_published.qid));
    while (!client.object_ready)
        step();
    assert(!sophia_ss_object_result(&session, &r));
    assert(r->value.catalog.generation == generation && r->value.catalog.entry_count == count &&
           r->value.catalog.identities_present == identities);
    assert(!sophia_sf_catalog_entry_at(&r->value.catalog, count - 1, &row));
    assert(row.slot == count && (row.identity.size != 0) == identities);
}
static void allocate(int native)
{
    struct sophia_sf_record r = {0}, result;
    phase("outputs");
    result = event(SOPHIA_SF_OBJECT_PUBLISHED);
    assert(!sophia_ss_object(&session, SOPHIA_SF_OUTPUTS,
        result.value.object_published.generation, result.value.object_published.qid));
    {
        const struct sophia_sf_record *outputs;
        while (!client.object_ready) step();
        assert(!sophia_ss_object_result(&session, &outputs));
        assert(outputs->header.kind == SOPHIA_SF_OUTPUTS);
    }
    if (native) {
        struct sophia_sf_native_allocation_request *a = &r.value.native_allocation_request;
        r.header.kind = SOPHIA_SF_NATIVE_ALLOCATION_REQUEST;
        a->transaction = 40;
        a->grant_connection_epoch = client.epoch;
        a->grant_content_epoch = 1;
        a->opening = 7;
        a->output_id = 2;
        a->output_generation = 1;
        a->request_id = 1;
        a->operation = 1;
        a->edge = 1;
        a->desired_width = 64;
        a->desired_height = 32;
    } else {
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
static void upload(void)
{
    struct sophia_ic_metadata metadata = {0};
    struct sophia_ip_offer_info info;
    struct sophia_sf_record status;
    uint64_t offer, ticket;
    int source, result;
    void *pixels;
    size_t bytes;
    assert(!sophia_ip_isb_create(2, 1, &source, &pixels, &bytes));
    memset(pixels, 0, bytes);
    assert(!sophia_ip_isb_finish(source, pixels, bytes));
    metadata.resource_id = 5; metadata.resource_generation = 1;
    metadata.admission_epoch = 1;
    metadata.output = 2; metadata.allocation = 1; metadata.topology_generation = 3;
    metadata.source_kind = metadata.source_format = 1;
    metadata.width = 2; metadata.height = 1; metadata.stride = 8;
    metadata.plane_count = metadata.descriptor_count = 1; metadata.logical_bytes = 8;
    do {
        result = sophia_ss_image_begin(&image, &metadata, source, now_ms(), &offer, &ticket);
        if (result == SOPHIA_IPR_BUSY || result == SOPHIA_IPR_AGAIN) step();
    } while (result == SOPHIA_IPR_BUSY || result == SOPHIA_IPR_AGAIN);
    assert(!result);
    status = event(SOPHIA_SF_RESOURCE_STATUS);
    assert(status.value.resource_status.status == 2);
    do {
        step();
        assert(!sophia_ip_offer(&image.producer, offer, &info));
    } while (info.state != SOPHIA_IPO_SETTLED);
    assert(info.status == SOPHIA_IPS_ACCEPTED && info.disposition == 1);
    assert(!sophia_ip_forget(&image.producer, offer));
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
static struct sophia_sf_record candidate(int native, uint64_t pacing, uint64_t generation,
                                         uint64_t catalog, uint64_t state)
{
    struct sophia_sf_record r = {0};
    struct sophia_sf_role_candidate *v = &r.value.role_candidate;
    struct sophia_sf_candidate *c = &v->content;
    r.header.kind = native ? SOPHIA_SF_NATIVE_CANDIDATE : SOPHIA_SF_CATALOG_CANDIDATE;
    c->transaction = 100 + generation;
    c->grant_connection_epoch = client.epoch;
    c->grant_content_epoch = 1;
    c->candidate_generation = generation;
    c->output_id = 2;
    c->output_generation = 1;
    c->facts_generation = 3;
    c->pacing_permit = pacing;
    c->interaction_generation = 4;
    c->surface_count = c->placement_count = c->target_count = 1;
    c->surfaces[0].allocation_id = 1;
    c->surfaces[0].allocation_generation = 1;
    c->surfaces[0].scale_generation = 5;
    c->surfaces[0].role = native ? 3 : 1;
    c->surfaces[0].edge = 1;
    c->surfaces[0].parent_surface_index = 65535;
    c->placements[0].resource_id = 5;
    c->placements[0].resource_generation = 1;
    c->targets[0].action_kind = native ? 2 : 3;
    c->targets[0].target_id = 1;
    c->targets[0].target_generation = 1;
    c->targets[0].action_id = 1;
    c->targets[0].bounds_width = 2;
    c->targets[0].bounds_height = 1;
    v->catalog_generation = catalog;
    if (native) {
        v->opening = 7;
        v->state_revision = state;
        v->selected = 1;
        v->row_count = 1;
        v->rows[0] = 1;
    }
    return r;
}
static void outcome(uint64_t generation, unsigned kind)
{
    struct sophia_sf_record e = event(SOPHIA_SF_CANDIDATE_OUTCOME);
    assert(e.value.candidate_outcome.candidate_generation == generation &&
           e.value.candidate_outcome.kind == kind && e.value.candidate_outcome.reason == 0);
}
static void acknowledge(struct sophia_sf_native_input input, uint64_t transaction)
{
    struct sophia_sf_record r = {0};
    r.header.kind = SOPHIA_SF_NATIVE_INPUT_ACK;
    r.value.native_input_ack.binding = input.binding;
    r.value.native_input_ack.binding.transaction = transaction;
    r.value.native_input_ack.event_id = input.event_id;
    r.value.native_input_ack.state_revision = input.state_revision;
    r.value.native_input_ack.disposition = 1;
    submit(&r);
}
static void launcher(void)
{
    struct sophia_sf_record r, e;
    struct sophia_sf_native_binding focus, fresh;
    struct sophia_sf_native_input text, accept;
    phase("publish");
    snapshot(3, 3, 0);
    phase("opening");
    e = event(SOPHIA_SF_NATIVE_OPENING);
    assert(e.value.native_opening.opening == 7 && e.value.native_opening.state_revision == 1);
    allocate(1);
    upload();
    r = candidate(1, permit(1), 1, 3, 1);
    {
        uint64_t next = client.next_submission;
        struct sophia_sf_record invalid = r;
        invalid.value.role_candidate.selected = 2;
        while (client.submit_stage)
            step();
        uint64_t ticket = 0;
        assert(sophia_ss_submit(&session, &invalid, 1, &ticket) == SOPHIA_9P_INVALID);
        assert(!ticket);
        assert(client.next_submission == next);
    }
    submit(&r);
    outcome(1, 1);
    phase("present");
    outcome(1, 2);
    e = event(SOPHIA_SF_NATIVE_FOCUS);
    focus = e.value.native_focus;
    assert(focus.candidate_generation == 1 && focus.state_revision == 1 && focus.focus_lease);
    phase("text");
    e = event(SOPHIA_SF_NATIVE_INPUT);
    text = e.value.native_input;
    assert(text.kind == 1 && text.text.size == 1 && text.text.data[0] == 'a');
    acknowledge(text, 700);
    phase("ack-consumed");
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_NATIVE_ACTIVATE;
    r.value.native_activate.binding = focus;
    r.value.native_activate.binding.transaction = 701;
    r.value.native_activate.event_id = text.event_id;
    r.value.native_activate.state_revision = 1;
    r.value.native_activate.cause = 2;
    r.value.native_activate.slot = 1;
    submit(&r);
    e = event(SOPHIA_SF_NATIVE_ACTIVATION_OUTCOME);
    echoed(&r, &e, 132);
    assert(e.value.native_activation_outcome.status == 2 &&
           e.value.native_activation_outcome.reason == 1);
    r = candidate(1, permit(2), 2, 3, text.state_revision);
    submit(&r);
    outcome(2, 1);
    phase("present");
    outcome(2, 2);
    event(SOPHIA_SF_NATIVE_FOCUS_REVOKED);
    e = event(SOPHIA_SF_NATIVE_FOCUS);
    fresh = e.value.native_focus;
    assert(fresh.focus_lease != focus.focus_lease && fresh.state_revision == text.state_revision);
    phase("accept");
    e = event(SOPHIA_SF_NATIVE_INPUT);
    accept = e.value.native_input;
    assert(accept.kind == 17 && !accept.text.size && accept.state_revision == fresh.state_revision);
    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_SF_NATIVE_ACTIVATE;
    r.value.native_activate.binding = fresh;
    r.value.native_activate.binding.transaction = 702;
    r.value.native_activate.event_id = accept.event_id;
    r.value.native_activate.state_revision = accept.state_revision;
    r.value.native_activate.cause = 1;
    r.value.native_activate.slot = 1;
    submit(&r);
    e = event(SOPHIA_SF_NATIVE_ACTIVATION_OUTCOME);
    echoed(&r, &e, 132);
    assert(e.value.native_activation_outcome.status == 1 &&
           !e.value.native_activation_outcome.reason &&
           e.value.native_activation_outcome.activation.event_id == accept.event_id);
    acknowledge(accept, 703);
    acknowledge(text, 704);
    phase("stale-ack-consumed");
    phase("one-admission");
    phase("close");
    e = event(SOPHIA_SF_NATIVE_FOCUS_REVOKED);
    assert(e.value.native_focus_revoked.reason == 12);
    e = event(SOPHIA_SF_NATIVE_CLOSED);
    assert(e.value.native_closed.opening == 7 && e.value.native_closed.reason == 12);
}
static void dock(void)
{
    struct sophia_sf_record r, e;
    unsigned i;
    phase("publish");
    snapshot(3, 3, 1);
    phase("maximum");
    snapshot(5, 4096, 1);
    phase("compact");
    snapshot(6, 3, 1);
    allocate(0);
    upload();
    r = candidate(0, permit(1), 1, 6, 1);
    submit(&r);
    outcome(1, 1);
    phase("present");
    outcome(1, 2);
    for (i = 0; i < 3; i++) {
        phase("action");
        e = event(SOPHIA_SF_ACTION);
        memset(&r, 0, sizeof(r));
        r.header.kind = SOPHIA_SF_CATALOG_ACTIVATE;
        r.value.catalog_activate.action = e.value.action;
        r.value.catalog_activate.action.transaction = 800 + i;
        r.value.catalog_activate.catalog_generation = i == 1 ? 3 : 6;
        if (i == 2)
            r.value.catalog_activate.action.action_id = 4096;
        submit(&r);
        e = event(SOPHIA_SF_CATALOG_ACTIVATION_OUTCOME);
        echoed(&r, &e, 128);
        assert(e.value.catalog_activation_outcome.status == (i ? 2 : 1) &&
               !e.value.catalog_activation_outcome.reason &&
               e.value.catalog_activation_outcome.activation.action.transaction == 800 + i);
    }
    phase("one-admission");
}
static void insufficient_storage(void)
{
    struct sophia_sf_record publication;
    const struct sophia_sf_record *value;
    phase("publish");
    publication = event(SOPHIA_SF_OBJECT_PUBLISHED);
    assert(!sophia_ss_object(&session, SOPHIA_SF_CATALOG, 3,
                                    publication.value.object_published.qid));
    while (!client.object_ready)
        step();
    assert(sophia_ss_object_result(&session, &value) == SOPHIA_9P_INVALID);
    assert(!client.terminal && !client.remote_error && !client.object_stage);
    phase("outputs");
    publication = event(SOPHIA_SF_OBJECT_PUBLISHED);
    assert(!sophia_ss_object(&session, SOPHIA_SF_OUTPUTS, 3,
                                    publication.value.object_published.qid));
    while (!client.object_ready)
        step();
    assert(!sophia_ss_object_result(&session, &value));
    assert(value->header.kind == SOPHIA_SF_OUTPUTS);
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
    enum sophia_sf_profile profile;
    char path[512], endpoint[256];
    FILE *file;
    size_t n, bytes = sophia_9p_storage_bytes(4096, 8);
    void *memory = malloc(bytes), *objects = malloc(SOPHIA_SF_MAX_RECORD);
    int fd;
    assert(argc == 3 && memory && objects);
    root = argv[1];
    fixture = argv[2];
    profile = !strcmp(fixture, "launcher") ? SOPHIA_SF_LAUNCHER : SOPHIA_SF_DOCK;
    assert(getchar() == 'G');
    assert(!clock_gettime(CLOCK_MONOTONIC, &started));
    phase("start");
    snprintf(path, sizeof(path), "%s/%s.endpoint", root, fixture);
    file = fopen(path, "r");
    assert(file);
    n = fread(endpoint, 1, sizeof(endpoint) - 1, file);
    assert(n && !ferror(file));
    fclose(file);
    endpoint[n] = 0;
    fd = connect_path(endpoint);
    assert(!sophia_9p_init(&wire, fd, 4096, 8, 32, memory, bytes));
    offer.minimum_revision = offer.maximum_revision = 9;
    offer.required_capabilities = profile == SOPHIA_SF_LAUNCHER ? 0x29a0 : 0x31a2;
    assert(!sophia_9p_version(&wire, &request)); auth_reply(&wire, request, 101);
    assert(!sophia_9p_auth(&wire, "", "", &request, &afid)); auth_reply(&wire, request, 103);
    assert(!sophia_9p_attach_with_afid(&wire, afid, "", "", &request, &attached_root)); auth_reply(&wire, request, 105);
    assert(!sophia_9p_clunk(&wire, afid, &request)); auth_reply(&wire, request, 121);
    config.offer = offer; config.profile = profile; config.msize = 4096;
    config.queue_slots = 8; config.queue_bytes = 65536;
    config.object_storage = objects;
    config.object_capacity = !strcmp(fixture, "dock-small") ? 1024 : SOPHIA_SF_MAX_RECORD;
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
    assert(!sophia_ss_image_init(&image, &image_config));
    assert(!sophia_ss_image_attach(&session, &image)); image_enabled = 1;
    while (sophia_ss_state(&session) == SOPHIA_SS_NEGOTIATING) step();
    assert(sophia_ss_state(&session) == SOPHIA_SS_READY);
    assert(sophia_ss_welcome(&session)->capabilities == offer.required_capabilities);
    if (!strcmp(fixture, "dock-small")) {
        insufficient_storage();
        assert(((uint8_t *)objects)[1024] == 0xa5);
    } else if (profile == SOPHIA_SF_LAUNCHER)
        launcher();
    else
        dock();
    if (strcmp(fixture, "dock-small")) {
        while (client.submit_stage || client.ack_op.active || client.acked_sequence != client.consumed_sequence) step();
    }
    sophia_ss_close(&session);
    sophia_ip_close(&image.producer);
    close(image_dir);
    sophia_9p_close(&wire);
    free(session_storage);
    close(fd);
    free(objects);
    free(memory);
    printf("sophia_c_role_files role=%s status=pass\n", fixture);
    return 0;
}

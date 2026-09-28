/* Independent C SDK descriptor client for the Session presentation boundary.
 * No Rust encoders, IPC framing, application process or physical display. */
#define _POSIX_C_SOURCE 200809L
#include "sophia_shell_session.h"
#include <assert.h>
#include <errno.h>
#include <poll.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

static struct sophia_ss session;
static uint8_t objects[65536];
static uint64_t deadline, last_presentation;

static uint64_t now(void)
{
    struct timespec value;
    assert(!clock_gettime(CLOCK_MONOTONIC, &value));
    return (uint64_t)value.tv_sec * 1000 + (uint64_t)value.tv_nsec / 1000000;
}

static void tick(void)
{
    assert(now() < deadline);
    struct pollfd p = {sophia_ss_poll_fd(&session), sophia_ss_poll_events(&session), 0};
    int status = poll(&p, 1, 1);
    assert(status >= 0 || errno == EINTR);
    assert(!sophia_ss_dispatch(&session, p.revents, 65536, now()));
    status = sophia_ss_ack(&session);
    assert(!status || status == SOPHIA_9P_BUSY);
}

static const struct sophia_sf_record *event(unsigned kind)
{
    const struct sophia_sf_record *r = NULL;
    int status;
    while ((status = sophia_ss_event(&session, &r)) == SOPHIA_9P_AGAIN)
        tick();
    assert(!status && r && r->header.kind == kind && r->header.epoch == 1);
    return r;
}

static void outcome(uint64_t generation, unsigned kind)
{
    const struct sophia_sf_record *r = event(SOPHIA_SF_DESCRIPTOR_OUTCOME);
    const struct sophia_sf_descriptor_outcome *v = &r->value.descriptor_outcome;
    assert(v->transaction == generation && v->candidate_generation == generation);
    assert(v->connection_epoch == 1 && v->kind == kind);
    if (kind == 2) {
        assert(v->presentation_epoch > last_presentation);
        last_presentation = v->presentation_epoch;
    } else {
        assert(v->presentation_epoch == 0);
    }
    assert(!sophia_ss_consume(&session));
}

static void candidate(uint64_t generation)
{
    const struct sophia_sf_record *r = event(SOPHIA_SF_OBJECT_PUBLISHED);
    struct sophia_sf_object_published p = r->value.object_published;
    assert(p.object_kind == SOPHIA_SF_DESCRIPTORS && p.generation == generation);
    assert(!sophia_ss_consume(&session));
    assert(!sophia_ss_object(&session, p.object_kind, p.generation, p.qid));
    int status;
    while ((status = sophia_ss_object_result(&session, &r)) == SOPHIA_9P_BUSY)
        tick();
    assert(!status && r && r->header.kind == SOPHIA_SF_DESCRIPTORS);
    const struct sophia_sf_descriptors *d = &r->value.descriptors;
    assert(d->transaction == generation && d->snapshot_generation == generation);
    assert(d->connection_epoch == 1 && d->output_id == 1 && d->descriptor_count == 1);
    struct sophia_sf_descriptor_entry entry;
    assert(!sophia_sf_descriptor_entry_at(d, 0, &entry));
    assert(entry.slot == 1 && entry.generation == 1 && entry.action_token == 7);
    assert(entry.action_recipient_epoch == 1);
    unsigned visible = generation <= 3;
    struct sophia_sf_record record = {
        .header.kind = SOPHIA_SF_DESCRIPTOR_CANDIDATE,
        .value.descriptor_candidate = {
            generation, 1, generation, generation, 1, (uint16_t)visible,
            (uint16_t)(visible ? 1 : 0),
            (uint16_t)(visible ? (generation == 1 ? 24 : 32) : 0),
            (uint16_t)visible, (uint16_t)visible, {{1, 1}}}};
    uint64_t ticket;
    assert(!sophia_ss_submit(&session, &record, 1, &ticket));
    enum sophia_ss_outcome custody;
    uint32_t error;
    do {
        tick();
        assert(!sophia_ss_outcome(&session, ticket, &custody, &error));
    } while (custody == SOPHIA_SS_IN_FLIGHT || custody == SOPHIA_SS_ADMITTED_LOCAL);
    assert(custody == SOPHIA_SS_SUBMITTED);
}

int main(int argc, char **argv)
{
    assert(argc == 2 && !strcmp(argv[1], "--serve"));
    assert(!getenv("SOPHIA_SHELL_SOCKET") && !getenv("DISPLAY") && !getenv("WAYLAND_DISPLAY"));
    assert(access("/dev/dri", F_OK) && errno == ENOENT);
    const char *path = getenv("SOPHIA_SHELL_9P_SOCKET");
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    assert(path && strlen(path) < sizeof(address.sun_path));
    memcpy(address.sun_path, path, strlen(path) + 1);
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    assert(fd >= 0 && !connect(fd, (struct sockaddr *)&address, sizeof(address)));
    struct sophia_ss_config config = {
        {1, 8, 5}, SOPHIA_SF_DESCRIPTOR, 4096, 4, 8192, objects, sizeof(objects)};
    size_t size = sophia_ss_storage_bytes(config.msize, config.queue_bytes);
    void *storage = malloc(size);
    assert(storage);
    deadline = now() + 30000;
    assert(!sophia_ss_open_fd(&session, fd, &config, storage, size));
    while (sophia_ss_state(&session) == SOPHIA_SS_NEGOTIATING)
        tick();
    assert(sophia_ss_state(&session) == SOPHIA_SS_READY && sophia_ss_epoch(&session) == 1);
    assert(sophia_ss_welcome(&session)->capabilities == 7 && !sophia_ss_limits(&session));
    for (uint64_t generation = 1; generation <= 5; generation++) {
        candidate(generation);
        outcome(generation, 1);
        outcome(generation, generation == 2 || generation == 5 ? 3 : 2);
    }
    sophia_ss_close(&session);
    close(fd);
    free(storage);
    return 0;
}

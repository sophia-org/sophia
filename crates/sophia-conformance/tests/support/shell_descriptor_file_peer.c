/* Contract peer for protected descriptor hosts. The only codecs are the
 * standalone C SDK: no IPC library, Rust encoder, UI or product policy. */
#define _POSIX_C_SOURCE 200809L
#include "sophia_shell_session.h"
#include <assert.h>
#include <errno.h>
#include <poll.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

static struct sophia_ss session;
static uint8_t objects[4 * 1024 * 1024], transaction[65536], rows[256 * 204];
static void *storage;
static int fd, serve, bar;
static const char *fault;
static volatile sig_atomic_t stopping;
static uint64_t deadline, epoch, generation, descriptor_generation, tab_generation;
static uint64_t descriptor_presentation, tab_presentation, last_activation, last_tab_activation;
static uint64_t reference_catalog, reference_presentation;
static uint16_t reference_count, page, pages = 1;
static unsigned descriptor_snapshots;
static uint64_t launcher_catalog, launcher_generation, launcher_request, launcher_presentation;
static uint64_t launched_generation;
static uint16_t launcher_slots[32], launcher_count;

static uint64_t now(void)
{
    struct timespec t;
    assert(!clock_gettime(CLOCK_MONOTONIC, &t));
    return (uint64_t)t.tv_sec * 1000 + (uint64_t)t.tv_nsec / 1000000;
}
static void stop(int signal)
{
    (void)signal;
    stopping = 1;
}
static void done(void)
{
    sophia_ss_close(&session);
    free(storage);
    close(fd);
    exit(0);
}
static void ack(void)
{
    int result = sophia_ss_ack(&session);
    assert(!result || result == SOPHIA_9P_BUSY);
}
static void tick(void)
{
    if (stopping)
        done();
    assert(now() < deadline);
    struct pollfd p = {sophia_ss_poll_fd(&session), sophia_ss_poll_events(&session), 0};
    int result = poll(&p, 1, 10);
    assert(result >= 0 || errno == EINTR);
    if (stopping)
        done();
    assert(!sophia_ss_dispatch(&session, p.revents, 65536, now()));
    ack();
}
static struct sophia_sf_record next(void)
{
    const struct sophia_sf_record *value;
    int result;
    while ((result = sophia_ss_event(&session, &value)) == SOPHIA_9P_AGAIN)
        tick();
    assert(!result && value->header.epoch == epoch);
    struct sophia_sf_record copy = *value;
    assert(!sophia_ss_consume(&session));
    if (copy.header.kind == SOPHIA_SF_OBJECT_PUBLISHED) {
        const struct sophia_sf_object_published *p = &copy.value.object_published;
        assert(!sophia_ss_object(&session, p->object_kind, p->generation, p->qid));
        while ((result = sophia_ss_object_result(&session, &value)) == SOPHIA_9P_BUSY)
            tick();
        assert(!result && value->header.kind == p->object_kind);
        copy = *value;
    }
    ack();
    return copy;
}
static void submit(struct sophia_sf_record *record)
{
    uint64_t ticket;
    enum sophia_ss_outcome outcome;
    uint32_t error;
    assert(!sophia_ss_submit(&session, record, 1, &ticket));
    do {
        tick();
        assert(!sophia_ss_outcome(&session, ticket, &outcome, &error));
    } while (outcome == SOPHIA_SS_ADMITTED_LOCAL || outcome == SOPHIA_SS_IN_FLIGHT);
    assert(outcome == SOPHIA_SS_SUBMITTED);
}
static void descriptors(const struct sophia_sf_descriptors *snapshot)
{
    struct sophia_sf_record reply = {.header.kind = SOPHIA_SF_DESCRIPTOR_CANDIDATE};
    struct sophia_sf_descriptor_candidate *c = &reply.value.descriptor_candidate;
    descriptor_snapshots++;
    c->transaction = snapshot->transaction;
    c->connection_epoch = epoch;
    c->snapshot_generation = snapshot->snapshot_generation;
    c->candidate_generation = descriptor_generation = ++generation;
    c->output_id = snapshot->output_id;
    c->visible = descriptor_snapshots == 1 && snapshot->descriptor_count != 0;
    c->entry_count = c->visible ? snapshot->descriptor_count : 0;
    assert(c->entry_count <= 16);
    for (unsigned i = 0; i < c->entry_count; i++) {
        struct sophia_sf_descriptor_entry entry;
        assert(!sophia_sf_descriptor_entry_at(snapshot, i, &entry));
        c->entries[i] = (struct sophia_sf_descriptor_candidate_entry){entry.slot, entry.generation};
    }
    if (c->visible) {
        c->selected_slot = c->entries[0].slot;
        if (bar) {
            c->reservation_edge = 2;
            c->reservation_thickness = 28;
        }
    }
    submit(&reply);
}
static void tabs(const struct sophia_sf_tabs *snapshot)
{
    struct sophia_sf_record reply = {.header.kind = SOPHIA_SF_TABS_CANDIDATE};
    struct sophia_sf_tabs_candidate *c = &reply.value.tabs_candidate;
    assert(snapshot->group_count <= sizeof(rows) / 8);
    c->transaction = snapshot->transaction;
    c->connection_epoch = epoch;
    c->snapshot_generation = snapshot->generation;
    c->candidate_generation = tab_generation = ++generation;
    c->group_count = snapshot->group_count;
    c->rows = rows;
    c->rows_bytes = (size_t)c->group_count * 8;
    for (unsigned i = 0; i < c->group_count; i++) {
        struct sophia_sf_tab_group group;
        assert(!sophia_sf_tab_group_at(snapshot, i, &group));
        assert(!sophia_sf_tab_order_encode(rows + i * 8, group.group_slot));
    }
    submit(&reply);
}
static void activation(const struct sophia_sf_descriptor_activation *a)
{
    int tab = a->candidate_generation == tab_generation;
    uint64_t presentation = tab ? tab_presentation : descriptor_presentation;
    uint64_t *last = tab ? &last_tab_activation : &last_activation;
    struct sophia_sf_record reply = {.header.kind = SOPHIA_SF_DESCRIPTOR_ACTIVATION_ACK};
    struct sophia_sf_descriptor_activation_ack *v = &reply.value.descriptor_activation_ack;
    v->transaction = a->transaction;
    v->connection_epoch = epoch;
    v->activation = a->activation;
    v->disposition = a->presentation_epoch == presentation && a->activation > *last ? 1 : 2;
    if (v->disposition == 1)
        *last = a->activation;
    if (tab && fault) {
        if (!strcmp(fault, "ack-activation"))
            v->activation++;
        else if (!strcmp(fault, "ack-transaction"))
            v->transaction++;
        else if (!strcmp(fault, "ack-disposition"))
            v->disposition = 2;
        else
            abort();
    }
    submit(&reply);
}
static void shortcuts(const struct sophia_sf_shortcuts *catalog)
{
    reference_catalog = catalog->generation;
    reference_count = catalog->entry_count;
    assert(reference_count <= 256);
    for (unsigned i = 0; i < reference_count; i++) {
        struct sophia_sf_shortcut_entry entry;
        assert(!sophia_sf_shortcut_entry_at(catalog, i, &entry));
        struct sophia_sf_reference_entry row = {entry.slot, entry.chord, entry.action};
        assert(!sophia_sf_reference_entry_encode(rows + i * 204, &row));
    }
}
static void reference(const struct sophia_sf_reference_request *request)
{
    assert(request->catalog_generation == reference_catalog);
    assert(request->presentation_epoch == reference_presentation);
    struct sophia_sf_record reply = {.header.kind = SOPHIA_SF_REFERENCE_CANDIDATE};
    struct sophia_sf_reference_candidate *c = &reply.value.reference_candidate;
    c->transaction = request->transaction;
    c->connection_epoch = epoch;
    c->catalog_generation = reference_catalog;
    c->request_generation = request->request_generation;
    c->candidate_generation = ++generation;
    c->output_id = request->output_id;
    c->visible = request->operation != 4;
    c->page = request->operation == 2 ? (uint16_t)((page + 1) % pages) :
        request->operation == 3 ? (uint16_t)((page + pages - 1) % pages) : page;
    c->entry_count = reference_count;
    c->rows = rows;
    c->rows_bytes = (size_t)reference_count * 204;
    c->style = (struct sophia_sf_reference_style){14, 18, 8, 4, 8, 16, 1, 8, 2,
        {0xff202020, 0xffffffff, 0xffeeeeee, 0xffdddddd, 0xffcccccc, 0xffbbbbbb},
        {(const uint8_t *)"Contract fixture", 16}};
    submit(&reply);
}
static void applications(const struct sophia_sf_catalog *catalog)
{
    assert(catalog->entry_count == 4096);
    launcher_catalog = catalog->generation;
    launcher_count = 0;
    for (unsigned i = 0; i < catalog->entry_count; i++) {
        struct sophia_sf_catalog_entry entry;
        assert(!sophia_sf_catalog_entry_at(catalog, i, &entry));
        if (launcher_count < 32)
            launcher_slots[launcher_count++] = entry.slot;
    }
}
static void launcher(const struct sophia_sf_descriptor_launcher_request *request)
{
    struct sophia_sf_record reply = {.header.kind = SOPHIA_SF_DESCRIPTOR_LAUNCHER_CANDIDATE};
    struct sophia_sf_descriptor_launcher_candidate *c = &reply.value.descriptor_launcher_candidate;
    assert(request->request.catalog_generation == launcher_catalog);
    c->transaction = request->request.transaction;
    c->connection_epoch = epoch;
    c->catalog_generation = launcher_catalog;
    c->request_generation = launcher_request = request->request.request_generation;
    c->candidate_generation = launcher_generation = ++generation;
    c->output_id = request->request.output_id;
    c->visible = request->request.operation != 4;
    c->selected = launcher_slots[0];
    c->entry_count = launcher_count;
    c->font_size = 14;
    for (unsigned i = 0; i < 4; i++)
        c->colors[i] = 0xff202020;
    memcpy(c->entries, launcher_slots, launcher_count * sizeof(launcher_slots[0]));
    launcher_presentation = 0;
    submit(&reply);
}
static void launcher_activation(const struct sophia_sf_descriptor_launcher_activation *a)
{
    struct sophia_sf_record reply = {.header.kind = SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION_ACK};
    struct sophia_sf_descriptor_launcher_activation_ack *v =
        &reply.value.descriptor_launcher_activation_ack;
    v->grant = *a;
    v->consumed = a->presentation_epoch == launcher_presentation && launcher_presentation &&
        a->request_generation == launcher_request && a->candidate_generation == launcher_generation &&
        a->candidate_generation != launched_generation && a->activation > last_activation;
    if (v->consumed) {
        last_activation = a->activation;
        launched_generation = a->candidate_generation;
    }
    submit(&reply);
}
int main(int argc, char **argv)
{
    assert(argc == 2 || argc == 3);
    serve = !strcmp(argv[1], "--serve");
    bar = !strcmp(argv[1], "--bar-proof");
    assert(serve || bar || !strcmp(argv[1], "--proof"));
    if (argc == 3) {
        assert(serve && !strncmp(argv[2], "--fault=", 8));
        fault = argv[2] + 8;
    }
    assert(!getenv("SOPHIA_SHELL_SOCKET"));
    const char *path = getenv("SOPHIA_SHELL_9P_SOCKET");
    assert(path && path[0] == '/');
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    assert(strlen(path) < sizeof(address.sun_path));
    memcpy(address.sun_path, path, strlen(path) + 1);
    fd = socket(AF_UNIX, SOCK_STREAM, 0);
    assert(fd >= 0 && !connect(fd, (struct sockaddr *)&address, sizeof(address)));
    struct sigaction action = {0};
    action.sa_handler = stop;
    assert(!sigemptyset(&action.sa_mask) && !sigaction(SIGTERM, &action, NULL));
    struct sophia_ss_config config = {
        .offer = {8, 8, serve ? 125 : bar ? 3 : 1},
        .profile = SOPHIA_SF_DESCRIPTOR, .msize = 65536,
        .queue_slots = 64, .queue_bytes = 512 * 1024,
        .object_storage = objects, .object_capacity = sizeof(objects),
    };
    size_t bytes = sophia_ss_storage_bytes(config.msize, config.queue_bytes);
    storage = calloc(1, bytes);
    assert(storage && !sophia_ss_open_fd_staging(&session, fd, &config, storage, bytes,
                                               transaction, sizeof(transaction)));
    deadline = now() + 30000;
    while (sophia_ss_state(&session) == SOPHIA_SS_NEGOTIATING)
        tick();
    assert(sophia_ss_state(&session) == SOPHIA_SS_READY);
    epoch = sophia_ss_welcome(&session)->connection_epoch;
    for (;;) {
        struct sophia_sf_record value = next();
        switch (value.header.kind) {
        case SOPHIA_SF_CATALOG:
            applications(&value.value.catalog);
            break;
        case SOPHIA_SF_DESCRIPTOR_LAUNCHER_REQUEST:
            launcher(&value.value.descriptor_launcher_request);
            break;
        case SOPHIA_SF_DESCRIPTOR_LAUNCHER_OUTCOME:
            if (value.value.descriptor_launcher_outcome.kind == 2)
                launcher_presentation = value.value.descriptor_launcher_outcome.presentation_epoch;
            break;
        case SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION:
            launcher_activation(&value.value.descriptor_launcher_activation);
            break;
        case SOPHIA_SF_DESCRIPTOR_LAUNCH_OUTCOME:
            assert(value.value.descriptor_launch_outcome.grant.candidate_generation == launched_generation);
            break;
        case SOPHIA_SF_DESCRIPTORS:
            descriptors(&value.value.descriptors);
            break;
        case SOPHIA_SF_TABS:
            tabs(&value.value.tabs);
            break;
        case SOPHIA_SF_SHORTCUTS:
            shortcuts(&value.value.shortcuts);
            break;
        case SOPHIA_SF_DESCRIPTOR_ACTIVATION:
            activation(&value.value.descriptor_activation);
            break;
        case SOPHIA_SF_DESCRIPTOR_OUTCOME: {
            const struct sophia_sf_descriptor_outcome *o = &value.value.descriptor_outcome;
            if (o->kind == 2) {
                if (o->candidate_generation == tab_generation)
                    tab_presentation = o->presentation_epoch;
                else {
                    assert(o->candidate_generation == descriptor_generation);
                    descriptor_presentation = o->presentation_epoch;
                    if (!serve && descriptor_snapshots == 2)
                        done();
                }
            }
            break;
        }
        case SOPHIA_SF_REFERENCE_REQUEST:
            reference(&value.value.reference_request);
            break;
        case SOPHIA_SF_REFERENCE_OUTCOME:
            if (value.value.reference_outcome.kind == 2) {
                page = value.value.reference_outcome.page;
                pages = value.value.reference_outcome.pages;
                reference_presentation = value.value.reference_outcome.presentation_epoch;
            }
            break;
        default:
            abort();
        }
    }
}

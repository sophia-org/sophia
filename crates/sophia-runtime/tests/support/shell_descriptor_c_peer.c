/* Independent native file client for Sophia's production descriptor export.
 * Wire values come from the pinned C SDK and proposed descriptor contract.
 * Control messages synchronize test owners; they carry no protocol payload. */
#define _POSIX_C_SOURCE 200809L
#include "sophia_shell_session.h"
#include <assert.h>
#include <errno.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

#define EPOCH 41u
static struct sophia_ss session;
static uint8_t objects[1048576], transaction[65536], rows[256 * 204];
static int control;
static uint64_t deadline;
static uint64_t now(void)
{
    struct timespec t;
    assert(!clock_gettime(CLOCK_MONOTONIC, &t));
    return (uint64_t)t.tv_sec * 1000 + (uint64_t)t.tv_nsec / 1000000;
}
static int connect_to(const char *path)
{
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    assert(strlen(path) < sizeof(address.sun_path));
    memcpy(address.sun_path, path, strlen(path) + 1);
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    assert(fd >= 0 && !connect(fd, (struct sockaddr *)&address, sizeof(address)));
    return fd;
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
static void phase(const char *name)
{
    char reply;
    assert(write(control, name, strlen(name)) == (ssize_t)strlen(name));
    assert(write(control, "\n", 1) == 1);
    struct pollfd p = {control, POLLIN, 0};
    while (poll(&p, 1, 0) == 0)
        tick();
    assert(read(control, &reply, 1) == 1 && reply == 'G');
}
static const struct sophia_sf_record *event(unsigned kind)
{
    const struct sophia_sf_record *r = NULL;
    int status;
    while ((status = sophia_ss_event(&session, &r)) == SOPHIA_9P_AGAIN)
        tick();
    assert(!status && r && r->header.kind == kind && r->header.epoch == EPOCH);
    return r;
}
static void consume(void)
{
    assert(!sophia_ss_consume(&session));
}
static const struct sophia_sf_record *fetch(unsigned kind)
{
    const struct sophia_sf_record *r = event(SOPHIA_SF_OBJECT_PUBLISHED);
    struct sophia_sf_object_published publication = r->value.object_published;
    uint64_t sequence = r->header.sequence;
    struct sophia_ss_obligations owed;
    assert(publication.object_kind == kind);
    consume();
    assert(!sophia_ss_obligations(&session, &owed));
    assert(owed.blocked && (owed.objects & (1u << (kind - 1))) && owed.ack_limit < sequence);
    assert(!sophia_ss_object(&session, kind, publication.generation, publication.qid));
    int result;
    while ((result = sophia_ss_object_result(&session, &r)) == SOPHIA_9P_BUSY)
        tick();
    assert(!result && r && r->header.kind == kind);
    assert(!sophia_ss_obligations(&session, &owed));
    assert(!(owed.objects & (1u << (kind - 1))) && owed.ack_limit >= sequence);
    return r;
}
static void submit(struct sophia_sf_record *records, size_t count)
{
    uint64_t ticket;
    assert(!sophia_ss_submit(&session, records, count, &ticket));
    for (size_t i = 0; i < count; i++) {
        enum sophia_ss_outcome outcome;
        uint32_t error;
        do {
            tick();
            assert(!sophia_ss_outcome(&session, ticket + i, &outcome, &error));
        } while (outcome == SOPHIA_SS_IN_FLIGHT || outcome == SOPHIA_SS_ADMITTED_LOCAL);
        if (outcome != SOPHIA_SS_SUBMITTED)
            fprintf(stderr, "candidate custody outcome=%d remote_error=%u\n", outcome, error);
        assert(outcome == SOPHIA_SS_SUBMITTED);
    }
}
static void descriptor_outcome(uint64_t tx, uint64_t generation, unsigned kind)
{
    const struct sophia_sf_record *r = event(SOPHIA_SF_DESCRIPTOR_OUTCOME);
    const struct sophia_sf_descriptor_outcome *v = &r->value.descriptor_outcome;
    assert(v->transaction == tx && v->candidate_generation == generation && v->kind == kind);
    assert(v->connection_epoch == EPOCH && v->presentation_epoch == (kind == 2 ? 100 : 0));
    consume();
}
static void descriptors(void)
{
    const struct sophia_sf_record *r = fetch(SOPHIA_SF_DESCRIPTORS);
    assert(r->value.descriptors.descriptor_count == 16 && r->value.descriptors.transaction == 10);
    assert(r->value.descriptors.snapshot_generation == 10 && r->value.descriptors.output_id == 8);
    struct sophia_sf_record candidate = {
        .header.kind = SOPHIA_SF_DESCRIPTOR_CANDIDATE,
        .value.descriptor_candidate = {10, EPOCH, 10, 11, 8, 1, 1, 24, 1, 16, {{0}}}};
    for (unsigned i = 0; i < 16; i++) {
        struct sophia_sf_descriptor_entry entry;
        assert(!sophia_sf_descriptor_entry_at(&r->value.descriptors, i, &entry));
        assert(entry.slot == i + 1 && entry.generation == 6 && entry.action_token == i + 3);
        assert(entry.action_recipient_epoch == EPOCH && entry.action_target_slot == i + 1);
        assert(entry.label_present && entry.label.size == 4 && !memcmp(entry.label.data, "Item", 4));
        candidate.value.descriptor_candidate.entries[i] =
            (struct sophia_sf_descriptor_candidate_entry){(uint16_t)(i + 1), 6};
    }
    assert(sophia_ss_record_bytes(&candidate) == 276);
    submit(&candidate, 1);
    phase("descriptor-candidate");
    descriptor_outcome(10, 11, 1);
    phase("descriptor-prepared");
    descriptor_outcome(10, 11, 2);
    phase("descriptor-presented");
    r = event(SOPHIA_SF_DESCRIPTOR_ACTIVATION);
    const struct sophia_sf_descriptor_activation *a = &r->value.descriptor_activation;
    assert(a->transaction == 20 && a->candidate_generation == 11 && a->presentation_epoch == 100);
    assert(a->activation == 21 && a->action_token == 3 && a->action_target_slot == 1 &&
           a->action_target_generation == 6 && a->action_issuer_epoch == 4 &&
           a->action_issuer_revocation_epoch == 5 && a->action_recipient_epoch == EPOCH);
    consume();
    struct sophia_sf_record acks[2] = {
        {.header.kind = SOPHIA_SF_DESCRIPTOR_ACTIVATION_ACK,
         .value.descriptor_activation_ack = {19, EPOCH, 21, 1}},
        {.header.kind = SOPHIA_SF_DESCRIPTOR_ACTIVATION_ACK,
         .value.descriptor_activation_ack = {20, EPOCH, 21, 1}}};
    submit(acks, 2);
    phase("descriptor-ack");
}
static void tabs(void)
{
    const struct sophia_sf_record *r = fetch(SOPHIA_SF_TABS);
    const struct sophia_sf_tabs *tabs = &r->value.tabs;
    assert(tabs->transaction == 25 && tabs->generation == 25 && tabs->group_count == 1024 &&
           tabs->entry_count == 2048);
    for (unsigned i = 0; i < 1024; i++) {
        struct sophia_sf_tab_group group;
        struct sophia_sf_descriptor_entry entry;
        assert(!sophia_sf_tab_group_at(tabs, i, &group));
        assert(group.group_slot == i + 1 && group.entry_count == 2 &&
               group.selected_slot == 2 * i + 1 && group.output_id == 8);
        assert(!sophia_sf_tab_entry_at(tabs, 2 * i + 1, &entry));
        assert(entry.slot == 2 * i + 2 && entry.action_recipient_epoch == EPOCH);
        assert(!sophia_sf_tab_order_encode(rows + i * 8, i + 1));
    }
    struct sophia_sf_record candidate = {
        .header.kind = SOPHIA_SF_TABS_CANDIDATE,
        .value.tabs_candidate = {25, EPOCH, 25, 26, 1024, rows, 8192}};
    assert(sophia_ss_record_bytes(&candidate) == 8260);
    submit(&candidate, 1);
    phase("tabs-candidate");
    descriptor_outcome(25, 26, 1);
    descriptor_outcome(25, 26, 2);
    phase("tabs-done");
}
static void reference(void)
{
    const struct sophia_sf_record *r = fetch(SOPHIA_SF_SHORTCUTS);
    assert(r->value.shortcuts.entry_count == 256 && r->value.shortcuts.generation == 30);
    for (unsigned i = 0; i < 256; i++) {
        struct sophia_sf_shortcut_entry shortcut;
        assert(!sophia_sf_shortcut_entry_at(&r->value.shortcuts, i, &shortcut));
        assert(shortcut.slot == i + 1 && shortcut.chord.size == 7 &&
               !memcmp(shortcut.chord.data, "Super+q", 7));
        struct sophia_sf_reference_entry entry = {
            (uint16_t)(i + 1), {(const uint8_t *)"Super+q", 7}, {(const uint8_t *)"Action", 6}};
        assert(!sophia_sf_reference_entry_encode(rows + i * 204, &entry));
    }
    r = event(SOPHIA_SF_REFERENCE_REQUEST);
    assert(r->value.reference_request.transaction == 30 && r->value.reference_request.operation == 1);
    assert(r->value.reference_request.catalog_generation == 30 &&
           r->value.reference_request.request_generation == 30 &&
           r->value.reference_request.output_id == 8);
    consume();
    struct sophia_sf_record candidate = {
        .header.kind = SOPHIA_SF_REFERENCE_CANDIDATE,
        .value.reference_candidate = {
            30, EPOCH, 30, 30, 31, 8, 1, 0, 256,
            {12, 16, 4, 2, 4, 4, 1, 4, 1,
             {0xff000000, 0xff000000, 0xff000000, 0xff000000, 0xff000000, 0xff000000},
             {(const uint8_t *)"Shortcuts", 9}}, rows, sizeof(rows)}};
    assert(sophia_ss_record_bytes(&candidate) == 52488);
    submit(&candidate, 1);
    phase("reference-candidate");
    for (unsigned kind = 1; kind <= 2; kind++) {
        r = event(SOPHIA_SF_REFERENCE_OUTCOME);
        const struct sophia_sf_reference_outcome *v = &r->value.reference_outcome;
        assert(v->transaction == 30 && v->catalog_generation == 30 && v->request_generation == 30 &&
               v->candidate_generation == 31 && v->page == 0 && v->pages == 1 && v->kind == kind &&
               v->presentation_epoch == (kind == 2 ? 100 : 0));
        consume();
    }
    phase("reference-done");
}
static void launcher(void)
{
    const struct sophia_sf_record *r = fetch(SOPHIA_SF_CATALOG);
    assert(r->value.catalog.entry_count == 32 && r->value.catalog.generation == 40 &&
           !r->value.catalog.identities_present);
    r = event(SOPHIA_SF_DESCRIPTOR_LAUNCHER_REQUEST);
    assert(r->value.descriptor_launcher_request.request.transaction == 40 &&
           r->value.descriptor_launcher_request.request.operation == 0 &&
           r->value.descriptor_launcher_request.query.size == 0);
    consume();
    struct sophia_sf_record candidate = {
        .header.kind = SOPHIA_SF_DESCRIPTOR_LAUNCHER_CANDIDATE,
        .value.descriptor_launcher_candidate = {
            40, EPOCH, 40, 40, 41, 8, 1, 2, 32, 12,
            {0xff000000, 0xff000000, 0xff000000, 0xff000000}, {0}}};
    for (unsigned i = 0; i < 32; i++)
        candidate.value.descriptor_launcher_candidate.entries[i] = (uint16_t)(i + 1);
    assert(sophia_ss_record_bytes(&candidate) == 168);
    submit(&candidate, 1);
    phase("launcher-candidate");
    for (unsigned kind = 1; kind <= 2; kind++) {
        r = event(SOPHIA_SF_DESCRIPTOR_LAUNCHER_OUTCOME);
        const struct sophia_sf_descriptor_launcher_outcome *v = &r->value.descriptor_launcher_outcome;
        assert(v->transaction == 40 && v->request_generation == 40 && v->candidate_generation == 41 &&
               v->kind == kind && v->presentation_epoch == (kind == 2 ? 100 : 0));
        consume();
    }
    phase("launcher-presented");
    r = event(SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION);
    struct sophia_sf_descriptor_launcher_activation activation = r->value.descriptor_launcher_activation;
    assert(activation.transaction == 50 && activation.catalog_generation == 40 &&
           activation.request_generation == 40 && activation.candidate_generation == 41 &&
           activation.presentation_epoch == 100 && activation.activation == 51 && activation.slot == 2);
    consume();
    struct sophia_sf_record ack = {
        .header.kind = SOPHIA_SF_DESCRIPTOR_LAUNCHER_ACTIVATION_ACK,
        .value.descriptor_launcher_activation_ack = {activation, 1}};
    submit(&ack, 1);
    phase("launcher-ack");
    r = event(SOPHIA_SF_DESCRIPTOR_LAUNCH_OUTCOME);
    assert(r->value.descriptor_launch_outcome.grant.transaction == 50 &&
           r->value.descriptor_launch_outcome.grant.activation == 51 &&
           r->value.descriptor_launch_outcome.status == 1);
    consume();
}
int main(int argc, char **argv)
{
    assert(argc == 4);
    char go;
    assert(read(STDIN_FILENO, &go, 1) == 1 && go == 'G');
    int combined = !strcmp(argv[3], "combined");
    assert(combined || !strcmp(argv[3], "metadata"));
    struct sophia_ss_config config = {
        {1, 8, combined ? 2045 : 1661}, SOPHIA_SF_DESCRIPTOR, 4096, 4, 262144,
        objects, sizeof(objects)};
    size_t size = sophia_ss_storage_bytes(config.msize, config.queue_bytes);
    void *storage = malloc(size);
    assert(storage);
    int fd = connect_to(argv[1]);
    control = connect_to(argv[2]);
    deadline = now() + 30000;
    assert(!sophia_ss_open_fd_staging(&session, fd, &config, storage, size,
                                     transaction, sizeof(transaction)));
    while (sophia_ss_state(&session) == SOPHIA_SS_NEGOTIATING)
        tick();
    assert(sophia_ss_state(&session) == SOPHIA_SS_READY && sophia_ss_epoch(&session) == EPOCH);
    assert(sophia_ss_welcome(&session)->capabilities == (combined ? 2047u : 1663u));
    assert(sophia_ss_welcome(&session)->selected_revision == 8);
    assert(!!sophia_ss_limits(&session) == combined);
    if (!combined)
        assert(sophia_ss_object(&session, SOPHIA_SF_OUTPUTS, 1, 1) == SOPHIA_9P_ARGUMENT);
    phase("ready");
    descriptors();
    tabs();
    reference();
    launcher();
    phase("done");
    sophia_ss_close(&session);
    close(fd);
    close(control);
    free(storage);
    printf("descriptor_c status=pass profile=%s snapshots=4 candidates=4 activations=2\n", argv[3]);
    return 0;
}

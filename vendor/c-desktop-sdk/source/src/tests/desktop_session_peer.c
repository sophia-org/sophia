#define _POSIX_C_SOURCE 200809L
#include "../sophia_desktop_connection.h"
#include "../sophia_shell_session.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>

/* Sophia's integration harness admits this process and drives real owners.
 * Use only the public session API; local admission is checked separately from
 * Submitted custody and resource/allocation outcomes. */
static uint64_t now_ms(void)
{
    struct timespec t;
    assert(!clock_gettime(CLOCK_MONOTONIC, &t));
    return (uint64_t)t.tv_sec * 1000u + (uint64_t)t.tv_nsec / 1000000u;
}
static void service(struct sophia_ss *s, uint64_t deadline)
{
    struct pollfd p = {sophia_ss_poll_fd(s), sophia_ss_poll_events(s), 0};
    int timeout = sophia_ss_timeout(s, now_ms());
    int r;
    assert(now_ms() < deadline);
    if (timeout < 0 || timeout > 2)
        timeout = 2;
    assert(poll(&p, 1, timeout) >= 0);
    r = sophia_ss_dispatch(s, p.revents, 65536, now_ms());
    if (r)
        fprintf(stderr, "session ended: result=%d state=%d\n", r, sophia_ss_state(s));
    assert(!r);
}
static int custodied(const struct sophia_ss *s, uint64_t ticket)
{
    enum sophia_ss_outcome outcome;
    uint32_t error;
    if (!ticket)
        return 0;
    assert(!sophia_ss_outcome(s, ticket, &outcome, &error));
    assert(!error);
    assert(outcome == SOPHIA_SS_ADMITTED_LOCAL || outcome == SOPHIA_SS_IN_FLIGHT ||
           outcome == SOPHIA_SS_SUBMITTED);
    return outcome == SOPHIA_SS_SUBMITTED;
}
int main(int argc, char **argv)
{
    struct sophia_desktop_endpoint endpoint;
    struct sophia_desktop_connection connection = {.fd = -1};
    struct sophia_ss session;
    struct sophia_ss_config config = {{5, 6, 0}, SOPHIA_SF_BAR, 4096, 8, 65536, NULL, 0};
    struct sophia_sf_record request = {0};
    struct sophia_sf_resource_begin begin = {0};
    const struct sophia_sf_record *event;
    struct sophia_ss_obligations obligations;
    uint8_t pixels[65536] = {0};
    uint64_t deadline, ticket = 0, first_ticket = 0;
    size_t bytes = sophia_ss_storage_bytes(config.msize, config.queue_bytes);
    void *storage = malloc(bytes);
    int fd, result, stage = 0, rejected = 0, accepted = 0, cancelled = 0;
    assert(argc == 3 && storage);
    config.offer.required_capabilities = strtoull(argv[2], NULL, 10);
    assert(getchar() == 'G');
    deadline = now_ms() + 8000;
    assert(!sophia_desktop_select_shell(argv[1], NULL, &endpoint));
    result = sophia_desktop_connection_begin(&connection, endpoint.path);
    while (result > 0) {
        assert(now_ms() < deadline);
        if (result == SOPHIA_DESKTOP_CONNECT_RETRY) {
            assert(!poll(NULL, 0, SOPHIA_DESKTOP_CONNECT_RETRY_MS));
            result = sophia_desktop_connection_begin(&connection, endpoint.path);
        } else {
            struct pollfd p = {connection.fd, sophia_desktop_connection_events(&connection), 0};
            assert(poll(&p, 1, 2) >= 0);
            result = sophia_desktop_connection_finish(&connection, p.revents);
        }
    }
    assert(result == SOPHIA_DESKTOP_CONNECTED);
    fd = sophia_desktop_connection_take(&connection);
    assert(fd >= 0);
    assert(!sophia_ss_open_fd(&session, fd, &config, storage, bytes));
    while (stage != 7) {
        service(&session, deadline);
        while (!sophia_ss_event(&session, &event)) {
            if (event->header.kind == SOPHIA_SF_ALLOCATION_RESULT) {
                assert(event->value.allocation_result.allocation_request_id == 1);
                assert(event->value.allocation_result.status == 2);
                rejected = 1;
            } else if (event->header.kind == SOPHIA_SF_RESOURCE_STATUS) {
                if (event->value.resource_status.status == 2)
                    accepted = 1;
                if (event->value.resource_status.status == 4)
                    cancelled = 1;
            } else {
                fprintf(stderr, "unexpected event kind %u\n", event->header.kind);
                abort();
            }
            assert(!sophia_ss_consume(&session));
        }
        result = sophia_ss_ack(&session);
        assert(!result || result == SOPHIA_9P_BUSY);
        if (!stage && sophia_ss_state(&session) == SOPHIA_SS_READY) {
            struct sophia_sf_allocation_request *a = &request.value.allocation_request;
            const struct sophia_sf_limits *limits = sophia_ss_limits(&session);
            assert(limits && sophia_ss_epoch(&session));
            request.header.kind = SOPHIA_SF_ALLOCATION_REQUEST;
            a->transaction = 40;
            a->grant_connection_epoch = sophia_ss_epoch(&session);
            a->grant_content_epoch = limits->grant_content_epoch;
            a->output_id = 99;
            a->output_generation = a->allocation_request_id = 1;
            a->operation = a->role = a->edge = 1;
            a->desired_width = 64;
            a->desired_height = 32;
            assert(!sophia_ss_submit(&session, &request, 1, &ticket));
            first_ticket = ticket;
            begin.grant_connection_epoch = a->grant_connection_epoch;
            begin.grant_content_epoch = a->grant_content_epoch;
            begin.resource_id = begin.resource_generation = 1;
            begin.width_px = 2048;
            begin.height_px = 8;
            begin.rendered_scale_numerator = begin.rendered_scale_denominator = begin.pixel_format = 1;
            begin.chunk_count = 2;
            begin.total_bytes = sizeof(pixels);
            stage = 1;
        }
        if (stage == 1 && rejected && custodied(&session, ticket)) {
            begin.transaction = 70;
            result = sophia_ss_upload_begin(&session, begin, &ticket);
            assert(!result || result == SOPHIA_9P_BUSY);
            if (!result)
                stage = 2;
        }
        if (stage == 2 && custodied(&session, ticket) && sophia_ss_upload_ready(&session)) {
            assert(!sophia_ss_upload_chunk(&session, pixels, sizeof(pixels)));
            stage = 3;
        }
        if (stage == 3 && sophia_ss_upload_ready(&session)) {
            result = sophia_ss_upload_end(&session, 71, &ticket);
            assert(!result || result == SOPHIA_9P_BUSY);
            if (!result)
                stage = 4;
        }
        if (stage == 4 && accepted && custodied(&session, ticket) &&
            !sophia_ss_upload_pending(&session)) {
            begin.transaction = 72;
            begin.resource_id = 2;
            result = sophia_ss_upload_begin(&session, begin, &ticket);
            assert(!result || result == SOPHIA_9P_BUSY);
            if (!result)
                stage = 5;
        }
        if (stage == 5 && custodied(&session, ticket) && sophia_ss_upload_ready(&session)) {
            result = sophia_ss_upload_cancel(&session, 73, &ticket);
            assert(!result || result == SOPHIA_9P_BUSY);
            if (!result)
                stage = 6;
        }
        if (stage == 6 && cancelled && custodied(&session, ticket) &&
            !sophia_ss_upload_pending(&session)) {
            assert(!sophia_ss_obligations(&session, &obligations));
            if (obligations.acked == obligations.consumed && !obligations.objects)
                stage = 7;
        }
    }
    assert(ticket == first_ticket + 4);
    while (first_ticket <= ticket)
        assert(custodied(&session, first_ticket++));
    sophia_ss_close(&session);
    close(fd);
    free(storage);
    puts("C desktop session: custody, allocation, upload, cancel, ack passed");
    return 0;
}

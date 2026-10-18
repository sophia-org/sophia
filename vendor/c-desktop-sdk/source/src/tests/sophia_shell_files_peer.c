#define _POSIX_C_SOURCE 200809L
#include "../sophia_shell_files_client.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

/* Independent C peer: the Rust harness supplies only an admitted endpoint and
 * production server owners. Every record below is built by this C binding. */
int main(int argc, char **argv)
{
    struct sophia_9p_client wire;
    struct sophia_sf_client c;
    struct sockaddr_un address;
    struct sophia_sf_negotiate offer = {5, 6, 0};
    struct sophia_sf_record request = {0};
    const struct sophia_sf_record *r;
    struct timespec start, now, pause = {0, 100000};
    size_t bytes = sophia_9p_storage_bytes(4096, 8);
    void *memory = malloc(bytes);
    int fd, stage = 0, status, rejected = 0;
    assert(argc == 3 && memory);
    offer.required_capabilities = strtoull(argv[2], NULL, 10);
    assert(getchar() == 'G');
    memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
    assert(strlen(argv[1]) < sizeof(address.sun_path));
    strcpy(address.sun_path, argv[1]);
    fd = socket(AF_UNIX, SOCK_STREAM, 0);
    assert(fd >= 0);
    assert(!connect(fd, (struct sockaddr *)&address, sizeof(address)));
    assert(!sophia_9p_init(&wire, fd, 4096, 8, 32, memory, bytes));
    assert(!sophia_sf_client_init(&c, &wire, offer));
    assert(!clock_gettime(CLOCK_MONOTONIC, &start));
    while (stage != 9) {
        /* EAGAIN deferred a submit in an earlier pass; the pause is the backoff. */
        if (c.submit_wait)
            assert(!sophia_sf_client_submit_retry(&c));
        status = sophia_sf_client_service(&c, 65536);
        if (status)
            fprintf(stderr, "service=%d remote=%u boot=%u submit=%u stage=%d\n", status,
                    c.remote_error, c.bootstrap, c.submit_stage, stage);
        assert(!status);
        while (!sophia_sf_client_event(&c, &r)) {
            if (r->header.kind == SOPHIA_SF_ALLOCATION_RESULT) {
                assert(r->value.allocation_result.allocation_request_id == 1);
                assert(r->value.allocation_result.status == 2);
                rejected = 1;
            }
            assert(r->header.kind != SOPHIA_SF_REFUSED);
            assert(!sophia_sf_client_event_consume(&c));
        }
        status = sophia_sf_client_ack(&c);
        assert(status == 0 || status == SOPHIA_9P_BUSY);
        if (c.object_ready) {
            assert(!sophia_sf_client_object_result(&c, &r));
            assert(r->header.kind == SOPHIA_SF_LIMITS);
        }
        if (stage == 0 && sophia_sf_client_ready(&c) && !c.submit_stage) {
            struct sophia_sf_allocation_request *a = &request.value.allocation_request;
            request.header.kind = SOPHIA_SF_ALLOCATION_REQUEST;
            a->transaction = 40;
            a->grant_connection_epoch = c.epoch;
            a->grant_content_epoch = 1;
            a->output_id = 99;
            a->output_generation = 1;
            a->allocation_request_id = 1;
            a->operation = 1;
            a->role = 1;
            a->edge = 1;
            a->desired_width = 64;
            a->desired_height = 32;
            assert(!sophia_sf_client_submit(&c, &request));
            stage = 1;
        }
        if (stage == 1 && rejected && !c.submit_stage) {
            uint64_t next = c.next_submission;
            request.header.kind = SOPHIA_SF_RESOURCE_BEGIN;
            assert(sophia_sf_client_submit(&c, &request) == SOPHIA_9P_ARGUMENT);
            request.header.kind = SOPHIA_SF_RESOURCE_END;
            assert(sophia_sf_client_submit(&c, &request) == SOPHIA_9P_ARGUMENT);
            assert(c.next_submission == next);
            stage = 8;
        }
        if (stage == 8 && c.acked_sequence == c.consumed_sequence && !c.ack_op.active)
            stage = 9;
        assert(!clock_gettime(CLOCK_MONOTONIC, &now));
        if (now.tv_sec - start.tv_sec > 8) {
            fprintf(stderr, "timeout stage=%d boot=%u submit=%u seq=%llu\n", stage,
                    c.bootstrap, c.submit_stage, (unsigned long long)c.sequence);
            abort();
        }
        nanosleep(&pause, NULL);
    }
    close(fd);
    free(memory);
    puts("C native file session: negotiation, allocation, byte refusal, ack passed");
    return 0;
}

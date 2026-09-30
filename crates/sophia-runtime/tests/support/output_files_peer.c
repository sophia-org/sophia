/* Generic public C SDK peer for the production output file export. */
#define _POSIX_C_SOURCE 200809L
#include "sophia_output_session.h"
#include <assert.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

static uint64_t now_ms(void) {
    struct timespec now;
    assert(clock_gettime(CLOCK_MONOTONIC, &now) == 0);
    return (uint64_t)now.tv_sec * 1000 + (uint64_t)now.tv_nsec / 1000000;
}

int main(int argc, char **argv) {
    struct sockaddr_un address;
    struct sophia_os_config config;
    struct sophia_os *session;
    void *storage;
    uint64_t deadline, ticket = 0;
    int fd, offered = 0, terminal = 0;
    char admitted;
    assert(argc == 2);
    /* The parent authorizes this exact PID before releasing the pipe gate. */
    assert(read(STDIN_FILENO, &admitted, 1) == 1 && admitted == 'G');
    memset(&address, 0, sizeof address);
    address.sun_family = AF_UNIX;
    assert(strlen(argv[1]) < sizeof address.sun_path);
    strcpy(address.sun_path, argv[1]);
    fd = socket(AF_UNIX, SOCK_STREAM, 0);
    assert(fd >= 0 && connect(fd, (struct sockaddr *)&address, sizeof address) == 0);
    assert(fcntl(fd, F_SETFL, O_NONBLOCK) == 0);
    memset(&config, 0, sizeof config);
    config.msize = 4096; /* Force topology/event reads through smaller buffers. */
    config.offer.minimum_revision = config.offer.maximum_revision = 1;
    config.offer.capabilities = SOPHIA_OF_CAP_OBSERVE | SOPHIA_OF_CAP_CONFIGURE;
    deadline = now_ms() + 5000;
    config.bootstrap_deadline_ms = deadline;
    session = malloc(sophia_os_state_bytes());
    storage = malloc(sophia_os_storage_bytes(config.msize));
    assert(session && storage);
    assert(!sophia_os_open_fd(session, fd, &config, storage,
        sophia_os_storage_bytes(config.msize), now_ms()));
    while (now_ms() < deadline) {
        const struct sophia_of_record *event;
        struct sophia_os_obligations obligations;
        struct pollfd pollfd;
        int timeout, result;
        pollfd.fd = sophia_os_poll_fd(session);
        pollfd.events = sophia_os_poll_events(session);
        pollfd.revents = 0;
        timeout = sophia_os_timeout(session, now_ms());
        if (timeout < 0 || timeout > 20) timeout = 20;
        assert(poll(&pollfd, 1, timeout) >= 0);
        result = sophia_os_dispatch(session, pollfd.revents, 65536, now_ms());
        if (result < 0) {
            fprintf(stderr, "output C dispatch=%d state=%d remote=%u\n", result,
                sophia_os_state(session), sophia_os_remote_error(session));
            abort();
        }
        if (!sophia_os_event(session, &event)) {
            if (event->header.kind == SOPHIA_OF_OBJECT_PUBLISHED) {
                const struct sophia_of_topology *topology = sophia_os_topology(session);
                struct sophia_of_proposal proposal;
                assert(topology && topology->topology_epoch == 4);
                assert(topology->head_count == 1 && topology->group_count == 1);
                assert(!offered);
                memset(&proposal, 0, sizeof proposal);
                assert(!sophia_os_next_transaction(session, &proposal.transaction));
                proposal.base_topology_epoch = topology->topology_epoch;
                proposal.intent = SOPHIA_OF_VALIDATE_ONLY;
                proposal.head_count = proposal.group_count = 1;
                proposal.heads[0].head = topology->heads[0].head;
                proposal.heads[0].generation = topology->heads[0].generation;
                proposal.heads[0].mode = topology->heads[0].current_mode;
                /* The fixture supplies these target values explicitly;
                 * revision 1 does not report their current values. */
                proposal.heads[0].transform = SOPHIA_OF_NORMAL;
                proposal.heads[0].vrr = SOPHIA_OF_VRR_DISABLED;
                proposal.groups[0].output = topology->groups[0].output;
                proposal.groups[0].x = topology->groups[0].x;
                proposal.groups[0].y = topology->groups[0].y;
                proposal.groups[0].width = topology->groups[0].width;
                proposal.groups[0].height = topology->groups[0].height;
                proposal.groups[0].member_count = 1;
                proposal.groups[0].members[0] = topology->groups[0].members[0];
                assert(!sophia_os_consume(session));
                assert(!sophia_os_submit(session, &proposal, deadline, &ticket));
                offered = 1;
            } else {
                enum sophia_os_custody custody;
                uint32_t error;
                assert(event->header.kind == SOPHIA_OF_OUTCOME);
                assert(event->value.outcome.outcome == SOPHIA_OF_VALIDATED);
                assert(!sophia_os_outcome(session, ticket, &custody, &error));
                assert(custody == SOPHIA_OS_SUBMITTED && error == 0);
                assert(!sophia_os_consume(session));
                terminal = 1;
            }
        }
        assert(!sophia_os_obligations(session, &obligations));
        if (terminal && obligations.acked >= 5) break;
    }
    assert(offered && terminal);
    sophia_os_close(session);
    close(fd);
    free(storage);
    free(session);
    puts("output C SDK: negotiated, exact topology read, proposal, terminal ack");
    return 0;
}

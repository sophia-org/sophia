#include "../sophia_shell_wire.h"
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <sys/socket.h>
#include <unistd.h>

static unsigned reads, writes;

ssize_t __wrap_recv(int fd, void *dst, size_t count, int flags)
{
    (void)fd; (void)dst;
    assert(count && flags == MSG_DONTWAIT);
    ++reads;
    assert(reads <= 32);
    errno = EINTR;
    return -1;
}

ssize_t __wrap_send(int fd, const void *src, size_t count, int flags)
{
    (void)fd; (void)src;
    assert(count && flags == (MSG_DONTWAIT | MSG_NOSIGNAL));
    ++writes;
    assert(writes <= 32);
    errno = EINTR;
    return -1;
}

int main(void)
{
    alarm(10);
    uint8_t rx[128], tx[128];
    struct sophia_shell_wire wire;
    assert(sophia_shell_wire_init(&wire, 1, rx, sizeof(rx), rx, sizeof(rx)) == SOPHIA_SHELL_ARGUMENT);
    assert(sophia_shell_wire_init(&wire, 1, rx, 23, tx, sizeof(tx)) == SOPHIA_SHELL_ARGUMENT);
    assert(sophia_shell_wire_init(&wire, 1, rx, sizeof(rx), tx, sizeof(tx)) == SOPHIA_SHELL_OK);
    assert(sophia_shell_wire_queue(&wire, 102, 1, NULL, 0) == SOPHIA_SHELL_OK);
    struct sophia_shell_frame frame;
    assert(sophia_shell_wire_receive(&wire, 0, &frame) == SOPHIA_SHELL_AGAIN && reads == 0);
    assert(sophia_shell_wire_flush(&wire, 0) == SOPHIA_SHELL_AGAIN && writes == 0);
    assert(sophia_shell_wire_receive(&wire, 128, &frame) == SOPHIA_SHELL_AGAIN && reads == 32);
    assert(sophia_shell_wire_flush(&wire, 128) == SOPHIA_SHELL_AGAIN && writes == 32);
    assert(wire.rx_used == 0 && wire.tx_used == 24 && wire.tx_sent == 0 && wire.terminal == 0);
    alarm(0);
    puts("sophia_shell_wire interrupted_syscalls=bounded storage_overlap=refused mocked_io=true");
    return 0;
}

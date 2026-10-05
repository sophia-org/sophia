/* Controls for the stand-in's service pass (tools/qemu_lock_provider_service.h)
 * against real peers on a socketpair: how each way a peer can end the
 * connection is recorded. Each control starts a fresh lock client, whose
 * first pass sends Tversion, and prints the stand-in's failure line for the
 * pass that failed. tools/tests/qemu_lock_provider_service_test.py checks
 * the lines. */
#define _GNU_SOURCE
#include "qemu_lock_provider_service.h"
#include <fcntl.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static struct sophia_9p_client wire;
static struct sophia_lc_client client;

/* A fresh client on one end of a socketpair; the other end is returned. */
static int start(void) {
  static uint8_t *storage;
  size_t bytes = sophia_9p_storage_bytes(65536, 16);
  struct sophia_lf_negotiate offer;
  int fd[2];
  if (!storage && !(storage = malloc(bytes)))
    exit(2);
  if (socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, fd) ||
      fcntl(fd[0], F_SETFL, O_NONBLOCK) ||
      sophia_9p_init(&wire, fd[0], 65536, 16, 32, storage, bytes))
    exit(2);
  memset(&offer, 0, sizeof offer);
  offer.minimum_revision = offer.maximum_revision = 1;
  offer.capabilities = SOPHIA_LF_CAP_PRESENT;
  if (sophia_lc_init(&client, &wire, &offer))
    exit(2);
  return fd[1];
}

/* Passes until one fails, at most `limit`; prints that pass's line. */
static void finish(const char *name, int limit) {
  struct standin_service pass = {0};
  char line[256];
  int i;
  for (i = 0; i < limit; ++i)
    if (standin_service(&client, &wire, 65536, &pass))
      break;
  if (i == limit) {
    printf("control=%s no failure\n", name);
  } else {
    standin_failure(line, sizeof line, name, "service", &client, &pass);
    fputs(line, stdout);
  }
  close(wire.fd);
}

static void take_request(int peer) {
  uint8_t b[256];
  if (read(peer, b, sizeof b) <= 0)
    exit(3);
}

int main(void) {
  int peer;
  signal(SIGPIPE, SIG_IGN);

  /* The peer reads Tversion and closes: a clean end of stream. */
  peer = start();
  if (standin_service(&client, &wire, 65536, &(struct standin_service){0}))
    exit(4);
  take_request(peer);
  close(peer);
  finish("orderly_eof", 4);

  /* The peer closes before the queued Tversion is sent: the send fails. */
  peer = start();
  close(peer);
  finish("closed_with_request_queued", 4);

  /* The peer closes with the sent Tversion still unread. */
  peer = start();
  if (standin_service(&client, &wire, 65536, &(struct standin_service){0}))
    exit(4);
  close(peer);
  finish("closed_with_request_unread", 4);

  /* A frame shorter than any 9P message. */
  peer = start();
  if (standin_service(&client, &wire, 65536, &(struct standin_service){0}))
    exit(4);
  take_request(peer);
  if (write(peer, "\3\0\0\0\0\0\0", 7) != 7)
    exit(3);
  finish("malformed_reply", 4);
  close(peer);

  /* Half an Rversion, then end of stream. */
  peer = start();
  if (standin_service(&client, &wire, 65536, &(struct standin_service){0}))
    exit(4);
  take_request(peer);
  if (write(peer, "\23\0\0\0\145\377\377", 7) != 7)
    exit(3);
  close(peer);
  finish("eof_mid_reply", 4);
  return 0;
}

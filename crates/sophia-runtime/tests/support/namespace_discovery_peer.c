/* Generic independent SDK reader of a production-published namespace. This
 * proves naming/connection identity only; no authentication is supplied. */
#include "sophia_namespace.h"
#include <assert.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    struct sophia_ns_discovery *view = malloc(sizeof *view);
    const struct sophia_ns_endpoint *endpoint;
    int directory, stream;
    assert(argc == 5 && view);
    directory = sophia_ns_open(argv[1], view);
    assert(directory >= 0);
    endpoint = sophia_ns_find(view, argv[2]);
    assert(endpoint && endpoint->service == strtoull(argv[3], NULL, 10));
    assert(endpoint->protocol == SOPHIA_NS_9P2000L);
    assert(!strcmp(endpoint->interface, "inspection") && endpoint->version == 1);
    stream = sophia_ns_connect(directory, view, argv[2]);
    assert(stream >= 0);
    assert(write(stream, argv[4], strlen(argv[4])) == (ssize_t)strlen(argv[4]));
    close(stream);
    close(directory);
    free(view);
    return 0;
}

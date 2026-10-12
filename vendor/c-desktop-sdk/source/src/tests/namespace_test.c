#define _GNU_SOURCE
#include "sophia_namespace.h"
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef __linux__
#include <fcntl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>
#endif

static const char record[] =
    "sophia-namespace\t1\t12\t2\n"
    "endpoint\troles/inspection\t9p2000.L\tinspection\t1\t31\t2\tavailable\n"
    "endpoint\troles/status\t9p2000.L\tinspection\t1\t31\t2\twaiting\n";
static void parser(void) {
    struct sophia_ns_discovery *v = malloc(sizeof *v), *before = malloc(sizeof *before);
    const char *bad[] = {"/absolute", "../outside", "roles//x", "roles/./x", "discovery", "discovery/x", "x\r", "x\302\205"};
    char text[16384], name[257];
    size_t i, offset;
    int n;
    assert(v && before);
    assert(!sophia_ns_parse(v, record, sizeof record - 1));
    assert(v->revision == 12 && v->count == 2);
    assert(v->endpoints[0].service == v->endpoints[1].service);
    assert(v->endpoints[0].availability != v->endpoints[1].availability);
    assert(sophia_ns_find(v, "roles/status") == &v->endpoints[1]);
    assert(!sophia_ns_find(v, "admin"));
    *before = *v;
    for (i = 0; i < sizeof record - 1; ++i) {
        assert(sophia_ns_parse(v, record, i) == -1);
        assert(!memcmp(v, before, sizeof *v));
    }
    for (i = 0; i < sizeof bad / sizeof bad[0]; ++i) {
        n = snprintf(text, sizeof text, "sophia-namespace\t1\t1\t1\nendpoint\t%s\tx11\tx11\t1\t1\t1\tavailable\n", bad[i]);
        assert(n > 0 && sophia_ns_parse(v, text, (size_t)n) == -1);
    }
    strcpy(text, "sophia-namespace\t1\t01\t0\n");
    assert(sophia_ns_parse(v, text, strlen(text)) == -1);
    memset(name, 'x', 256); name[256] = 0;
    n = snprintf(text, sizeof text, "sophia-namespace\t1\t1\t1\nendpoint\t%s\tx11\tx11\t1\t1\t1\tavailable\n", name);
    assert(n > 0 && sophia_ns_parse(v, text, (size_t)n) == -1);
    name[255] = 0;
    n = snprintf(text, sizeof text, "sophia-namespace\t1\t1\t1\nendpoint\t%s\tx11\tx11\t1\t1\t1\tavailable\n", name);
    assert(n > 0 && !sophia_ns_parse(v, text, (size_t)n));
    offset = (size_t)snprintf(text, sizeof text, "sophia-namespace\t1\t1\t64\n");
    for (i = 0; i < 64; ++i) {
        n = snprintf(text + offset, sizeof text - offset, "endpoint\tp%zu\tx11\tx11\t1\t1\t1\tavailable\n", i);
        assert(n > 0 && (size_t)n < sizeof text - offset); offset += (size_t)n;
    }
    assert(!sophia_ns_parse(v, text, offset));
    text[strlen("sophia-namespace\t1\t1\t6")] = '5';
    assert(sophia_ns_parse(v, text, offset) == -1);
    free(before); free(v);
}

#ifdef __linux__
static void discovery(int dir, const char *endpoint, unsigned service) {
    char bytes[2048];
    int fd, n;
    n = snprintf(bytes, sizeof bytes, "sophia-namespace\t1\t1\t1\nendpoint\t%s\t9p2000.L\tinspection\t1\t%u\t1\tavailable\n", endpoint, service);
    assert(n > 0 && (size_t)n < sizeof bytes);
    fd = openat(dir, "discovery", O_CREAT | O_WRONLY | O_EXCL, 0400);
    assert(fd >= 0 && write(fd, bytes, (size_t)n) == n); close(fd);
}
static int listener(int dir, const char *target) {
    int fd, n;
    struct sockaddr_un a;
    memset(&a, 0, sizeof a); a.sun_family = AF_UNIX;
    n = snprintf(a.sun_path, sizeof a.sun_path, "/proc/self/fd/%d/temporary", dir);
    assert(n > 0 && (size_t)n < sizeof a.sun_path);
    fd = socket(AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0);
    assert(fd >= 0 && !bind(fd, (struct sockaddr *)&a, (socklen_t)(offsetof(struct sockaddr_un, sun_path) + (size_t)n + 1)));
    assert(!listen(fd, 8) && !renameat(dir, "temporary", dir, target));
    return fd;
}
static void directories(void) {
    char root[] = "/tmp/sophia-ns-c-XXXXXX", path[512], retained[512], cwd[4096], after[4096];
    char first[201], second[201], name[403];
    struct sophia_ns_discovery *v = malloc(sizeof *v);
    int parent, old, fresh, a, b, pinned, stream, peer;
    assert(v && mkdtemp(root));
    parent = open(root, O_RDONLY | O_DIRECTORY | O_CLOEXEC); assert(parent >= 0);
    assert(!mkdirat(parent, "view", 0700));
    old = openat(parent, "view", O_RDONLY | O_DIRECTORY | O_CLOEXEC); assert(old >= 0);
    memset(first, 'x', 200); first[200] = 0; memset(second, 'y', 200); second[200] = 0;
    assert(snprintf(name, sizeof name, "%s/%s", first, second) == 401);
    assert(!mkdirat(old, first, 0700)); a = listener(old, name); discovery(old, name, 31);
    assert(snprintf(path, sizeof path, "%s/view", root) > 0);
    pinned = sophia_ns_open(path, v); assert(pinned >= 0 && v->endpoints[0].service == 31);
    assert(getcwd(cwd, sizeof cwd));
    assert(!renameat(parent, "view", parent, "retained") && !mkdirat(parent, "view", 0700));
    fresh = openat(parent, "view", O_RDONLY | O_DIRECTORY | O_CLOEXEC); assert(fresh >= 0);
    assert(!mkdirat(fresh, first, 0700)); b = listener(fresh, name); discovery(fresh, name, 32);
    stream = sophia_ns_connect(pinned, v, name); assert(stream >= 0);
    assert((fcntl(stream, F_GETFD) & FD_CLOEXEC) && (fcntl(stream, F_GETFL) & O_NONBLOCK));
    peer = accept(a, NULL, NULL); assert(peer >= 0); close(peer);
    assert(accept(b, NULL, NULL) < 0 && errno == EAGAIN);
    assert(getcwd(after, sizeof after) && !strcmp(cwd, after));
    assert(sophia_ns_connect(pinned, v, "undeclared") < 0); close(stream);
    assert(!unlinkat(old, name, 0) && !symlinkat("/tmp", old, name));
    assert(sophia_ns_connect(pinned, v, name) < 0);
    assert(!unlinkat(old, name, 0) && !unlinkat(old, first, AT_REMOVEDIR));
    assert(!unlinkat(old, "discovery", 0) && !symlinkat("/tmp", old, "discovery"));
    assert(snprintf(retained, sizeof retained, "%s/retained", root) > 0);
    assert(sophia_ns_open(retained, v) < 0); assert(!unlinkat(old, "discovery", 0));
    assert(!symlinkat("view", parent, "alias"));
    assert(snprintf(path, sizeof path, "%s/alias", root) > 0);
    assert(sophia_ns_open(path, v) < 0); assert(!unlinkat(parent, "alias", 0));
    assert(!unlinkat(fresh, name, 0) && !unlinkat(fresh, first, AT_REMOVEDIR));
    assert(!unlinkat(fresh, "discovery", 0));
    close(a); close(b); close(pinned); close(old); close(fresh);
    assert(!unlinkat(parent, "view", AT_REMOVEDIR) && !unlinkat(parent, "retained", AT_REMOVEDIR));
    close(parent); assert(!rmdir(root)); free(v);
}
#endif
int main(void) {
    parser();
#ifdef __linux__
    directories();
#endif
    puts("namespace discovery and pinned lookup: ok");
    return 0;
}

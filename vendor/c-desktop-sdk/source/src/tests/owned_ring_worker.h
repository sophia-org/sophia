/* Caller-owned worker lifetime over the public C API. Only the owner touches
 * the adapter; the worker receives a bounded payload and must join before close. */
#include <pthread.h>

struct ring_worker {
    uint8_t *pixels;
    size_t bytes;
    int started[2], resume[2];
    pthread_t thread;
};

static void *ring_worker_fill(void *arg)
{
    struct ring_worker *w = arg;
    char byte = 1;
    memset(w->pixels, 0x31, w->bytes / 2);
    assert(write(w->started[1], &byte, 1) == 1);
    assert(read(w->resume[0], &byte, 1) == 1);
    memset(w->pixels + w->bytes / 2, 0x73, w->bytes - w->bytes / 2);
    return NULL;
}

static void ring_worker_start(struct ring_worker *w, struct image_rig *t)
{
    uint64_t capacity = sophia_ss_image_ring_capacity(&t->img, NULL);
    memset(w, 0, sizeof(*w));
    w->bytes = 16 * 16 * 4;
    assert(w->bytes < capacity);
    w->pixels = sophia_ss_image_ring_pixels(&t->img, 0);
    assert(w->pixels);
    /* The worker never sees the remaining slot capacity or its header. */
    w->pixels[w->bytes] = 0xa5;
    assert(pipe2(w->started, O_CLOEXEC) == 0);
    assert(pipe2(w->resume, O_CLOEXEC) == 0);
    assert(pthread_create(&w->thread, NULL, ring_worker_fill, w) == 0);
    {
        char byte;
        assert(read(w->started[0], &byte, 1) == 1);
    }
}

static void ring_worker_check(const struct ring_worker *w, int source_fd,
                              uint64_t expected_generation)
{
    uint8_t observed[16 * 16 * 4 + 1];
    uint64_t generation = UINT64_MAX;
    size_t i;
    /* Read through the independently transferred FD, not the worker pointer. */
    assert(pread(source_fd, observed, sizeof(observed), SOPHIA_OR_SLOT_HEADER_BYTES) ==
           (ssize_t)sizeof(observed));
    for (i = 0; i < w->bytes; i++)
        assert(observed[i] == (i < w->bytes / 2 ? 0x31 : 0x73));
    assert(observed[w->bytes] == 0xa5);
    assert(pread(source_fd, &generation, sizeof(generation), 0) == sizeof(generation));
    assert(generation == expected_generation);
}

static void ring_worker_join(struct ring_worker *w, int source_fd, uint64_t expected_generation)
{
    char byte = 1;
    assert(write(w->resume[1], &byte, 1) == 1);
    assert(pthread_join(w->thread, NULL) == 0);
    close(w->started[0]);
    close(w->started[1]);
    close(w->resume[0]);
    close(w->resume[1]);
    ring_worker_check(w, source_fd, expected_generation);
}

static void ring_worker_commit_and_release(void)
{
    struct image_rig t;
    struct ring_worker w;
    struct sophia_ss_ring_image m = ring_image(41, 16, 16);
    uint64_t offer;
    int fd;
    ring_start(&t, 2, 0);
    fd = ring_ready(&t);
    ring_worker_start(&w, &t);
    ring_steps(&t, 8);
    assert(t.r.p.ring_writes == 1);
    ring_worker_join(&w, fd, 0);
    assert(sophia_ss_image_ring_pixels(&t.img, 0) == w.pixels);
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_OK);
    assert(offer == 1);
    ring_steps(&t, 4);
    assert(ring_info(&t, offer).offer.slot_generation == 2);
    assert(!sophia_ss_image_ring_pixels(&t.img, 0));
    ring_status(&t, offer, 41, 2, 0);
    assert(!sophia_ss_image_ring_pixels(&t.img, 0));
    assert(sophia_ss_image_ring_forget(&t.img, offer) != SOPHIA_IPR_OK);
    ring_event(&t.r.p, SOPHIA_OR_SLOT_RELEASED, offer, 0, 2, 41, 0, 0);
    ring_steps(&t, 4);
    assert(sophia_ss_image_ring_forget(&t.img, offer) == SOPHIA_IPR_OK);
    assert(sophia_ss_image_ring_pixels(&t.img, 0) == w.pixels);
    ring_worker_start(&w, &t);
    ring_worker_join(&w, fd, 2);
    m.resource_id = 42;
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_OK);
    assert(offer == 2 && ring_info(&t, offer).offer.slot_generation == 4);
    ring_close(&t, fd);
}

static void ring_worker_abort_and_retry(void)
{
    struct image_rig t;
    struct ring_worker w;
    struct sophia_ss_ring_image m = ring_image(41, 16, 16);
    uint64_t offer = UINT64_MAX;
    int fd;
    ring_start(&t, 2, 0);
    fd = ring_ready(&t);
    ring_worker_start(&w, &t);
    ring_steps(&t, 4);
    ring_worker_join(&w, fd, 0);
    /* A stale or cancelled render is discarded locally without an Offer. */
    assert(t.r.p.ring_writes == 1);
    ring_worker_start(&w, &t);
    ring_worker_join(&w, fd, 0);
    m.width = 0;
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_REFUSED);
    assert(offer == UINT64_MAX && t.r.p.ring_writes == 1);
    ring_worker_check(&w, fd, 0);
    m.width = 16;
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_OK);
    assert(offer == 1 && ring_info(&t, offer).offer.slot_generation == 2);
    ring_close(&t, fd);
}

static void ring_worker_backpressure(void)
{
    struct image_rig t;
    struct ring_worker w;
    struct sophia_ss_ring_image prior = ring_image(40, 16, 16);
    struct sophia_ss_ring_image m = ring_image(41, 16, 16);
    uint64_t offer, sentinel = UINT64_MAX;
    unsigned writes;
    int fd;
    ring_start(&t, 2, 0);
    fd = ring_ready(&t);
    /* A prior Offer may still be awaiting its control write when filling starts. */
    assert(sophia_ss_image_ring_offer(&t.img, 1, &prior, t.r.now, &offer) == SOPHIA_IPR_OK);
    assert(offer == 1);
    ring_worker_start(&w, &t);
    ring_worker_join(&w, fd, 0);
    writes = t.r.p.ring_writes;
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &sentinel) == SOPHIA_IPR_AGAIN);
    assert(sentinel == UINT64_MAX && t.r.p.ring_writes == writes);
    ring_worker_check(&w, fd, 0);
    ring_steps(&t, 4);
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_OK);
    assert(offer == 2 && ring_info(&t, offer).offer.slot_generation == 2);
    ring_close(&t, fd);
}

static void ring_worker_disconnect(void)
{
    struct image_rig t;
    struct ring_worker w;
    struct sophia_ss_ring_image m = ring_image(41, 16, 16);
    uint64_t offer = UINT64_MAX;
    unsigned loss;
    for (loss = 0; loss < 2; loss++) {
        int fd;
        ring_start(&t, 2, 0);
        fd = ring_ready(&t);
        ring_worker_start(&w, &t);
        if (loss == 0)
            sophia_ss_close(&t.r.s);
        else
            peer_channel(&t.r.p, SOPHIA_IC_CHANNEL_CLOSED, SOPHIA_IC_CLOSED_PEER);
        ring_steps(&t, 4);
        assert(sophia_ss_image_ring_state(&t.img, NULL) == SOPHIA_SS_RING_FAILED);
        assert(!sophia_ss_image_ring_pixels(&t.img, 0));
        /* Disconnect does not reclaim the worker's mapping. Explicit image close
         * is deferred until join, including timeout/cancellation/error paths. */
        ring_worker_join(&w, fd, 0);
        assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_CLOSED);
        assert(offer == UINT64_MAX && t.r.p.ring_writes == 1);
        ring_close(&t, fd);
    }
}

static void owned_ring_worker(void)
{
    ring_worker_commit_and_release();
    ring_worker_abort_and_retry();
    ring_worker_backpressure();
    ring_worker_disconnect();
    puts("owned_ring_worker: 5 scenarios passed (caller-managed lifetime)");
}

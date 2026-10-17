#include "shell_session_peer.h"

/* A multi-chunk buffer, not four separate calls: each call borrows storage
 * until its entire window has settled. The peer withholds every pixel reply. */
static void start_budget(struct rig *r, const uint8_t *pixels, size_t size, uint32_t iounit, size_t budget)
{
    struct sophia_sf_resource_begin b = {0};
    const struct sophia_sf_record *e;
    uint64_t ticket;
    unsigned i;
    rig_ready(r, &rig_config);
    r->p.upload_iounit = iounit;
    r->s.files.limits.max_height_px = 16;
    r->s.files.limits.max_resource_bytes = 64;
    b.transaction = 77;
    b.grant_connection_epoch = PEER_EPOCH;
    b.grant_content_epoch = r->s.files.limits.grant_content_epoch;
    b.resource_id = b.resource_generation = 1;
    b.width_px = 1;
    b.height_px = (uint32_t)(size / 4);
    b.rendered_scale_numerator = b.rendered_scale_denominator = b.pixel_format = 1;
    b.total_bytes = size;
    b.chunk_count = b.height_px;
    assert(r->s.files.limits.max_chunk_bytes == 4);
    assert(!sophia_ss_upload_begin(&r->s, b, &ticket));
    assert(rig_until(r, ticket, 32) == SOPHIA_SS_SUBMITTED);
    peer_resource_status(&r->p, &b, 1);
    rig_run(r, 8);
    assert(!sophia_ss_event(&r->s, &e));
    assert(e->header.kind == SOPHIA_SF_RESOURCE_STATUS);
    assert(!sophia_ss_consume(&r->s));
    rig_ack(r);
    rig_run(r, 8);
    assert(sophia_ss_upload_ready(&r->s));
    r->p.hold_upload = 1;
    r->service_budget = budget;
    r->partial_send_seen = 0;
    assert(!sophia_ss_upload_chunk(&r->s, pixels, size));
    for (i = 0; i < 4096 && r->p.upload_held < (size >= 16 ? 4 : size / 4); i++)
        rig_step(r);
    assert(r->p.upload_held == (size >= 16 ? 4 : size / 4));
}

static void start_cap(struct rig *r, const uint8_t *pixels, size_t size, uint32_t iounit)
{
    start_budget(r, pixels, size, iounit, 0);
}

static void start(struct rig *r, const uint8_t *pixels, size_t size)
{
    start_cap(r, pixels, size, 0);
}

static void iounit_fragments_keep_order(void)
{
    struct rig r;
    uint8_t pixels[16] = {0};
    unsigned i;
    start_cap(&r, pixels, sizeof(pixels), 2);
    for (i = 0; i < 4; i++) {
        assert(r.p.upload_counts[i] == 2 && r.p.upload_offsets[i] == i * 2);
        peer_count(&r.p, r.p.upload_tags[i], 2);
    }
    rig_run(&r, 4);
    assert(r.p.upload_held == 8);
    for (i = 4; i < 8; i++) {
        assert(r.p.upload_counts[i] == 2 && r.p.upload_offsets[i] == i * 2);
        peer_count(&r.p, r.p.upload_tags[i], 2);
    }
    rig_run(&r, 4);
    assert(sophia_ss_upload_ready(&r.s) && r.s.files.upload_offset == 16);
    rig_close(&r);
}

static void ordered_success(void)
{
    struct rig r;
    uint8_t pixels[24] = {0};
    unsigned i;
    start(&r, pixels, sizeof(pixels));
    for (i = 0; i < 4; i++) {
        assert(r.p.upload_counts[i] == 4);
        assert(r.p.upload_offsets[i] == i * 4);
    }
    peer_count(&r.p, r.p.upload_tags[3], 4);
    rig_run(&r, 4);
    assert(!r.s.files.upload_offset && r.p.upload_held == 4);
    peer_count(&r.p, r.p.upload_tags[0], 4);
    rig_run(&r, 4);
    assert(r.s.files.upload_offset == 4 && r.p.upload_held == 5);
    peer_count(&r.p, r.p.upload_tags[2], 4);
    peer_count(&r.p, r.p.upload_tags[1], 4);
    rig_run(&r, 4);
    assert(r.s.files.upload_offset == 16 && r.p.upload_held == 6);
    peer_count(&r.p, r.p.upload_tags[5], 4);
    peer_count(&r.p, r.p.upload_tags[4], 4);
    rig_run(&r, 4);
    assert(r.s.files.upload_offset == 24 && sophia_ss_upload_ready(&r.s));
    rig_close(&r);
}

static void recover(int backpressure)
{
    struct rig r;
    uint8_t pixels[16] = {0};
    unsigned i;
    start(&r, pixels, sizeof(pixels));
    /* Deliver invalidated tails before the head: none may settle early. */
    for (i = 3; i > 0; i--)
        peer_error(&r.p, r.p.upload_tags[i], 22);
    rig_run(&r, 4);
    assert(!r.s.files.upload_offset && r.p.upload_held == 4);
    if (backpressure)
        peer_error(&r.p, r.p.upload_tags[0], 11);
    else
        peer_count(&r.p, r.p.upload_tags[0], 2);
    rig_run(&r, 4);
    if (backpressure) {
        assert(r.s.upload_waiting && r.p.upload_held == 4);
        assert(sophia_ss_timeout(&r.s, r.now) == 4);
        r.now += 3;
        rig_run(&r, 4);
        assert(r.p.upload_held == 4 && sophia_ss_timeout(&r.s, r.now) == 1);
        r.now++;
        rig_run(&r, 4);
    }
    assert(r.p.upload_held == 8);
    assert(r.p.upload_offsets[4] == (backpressure ? 0u : 2u));
    assert(r.p.upload_counts[4] == (backpressure ? 4u : 2u));
    for (i = 4; i < 8; i++)
        peer_count(&r.p, r.p.upload_tags[i], r.p.upload_counts[i]);
    rig_run(&r, 4);
    assert(r.s.files.upload_offset == 16 && sophia_ss_upload_ready(&r.s));
    assert(sophia_ss_timeout(&r.s, r.now) == -1);
    rig_close(&r);
}

static void bad_tail_and_fence(int fence)
{
    struct rig r;
    uint8_t pixels[16] = {0};
    unsigned i;
    start(&r, pixels, sizeof(pixels));
    peer_error(&r.p, r.p.upload_tags[0], 11);
    if (fence)
        peer_error(&r.p, r.p.upload_tags[1], 116);
    else
        peer_count(&r.p, r.p.upload_tags[1], 4);
    rig_run(&r, 4);
    if (fence) {
        assert(r.s.files.upload_data == pixels && r.s.files.upload_count == 2);
        for (i = 2; i < 4; i++)
            peer_error(&r.p, r.p.upload_tags[i], 116);
        rig_run(&r, 8);
        assert(!sophia_ss_upload_pending(&r.s) && r.s.state == SOPHIA_SS_READY);
        assert(!r.s.files.upload_offset && r.p.upload_held == 4);
    } else
        assert(r.s.state == SOPHIA_SS_FAILED);
    rig_close(&r);
}

static void explicit_retry_and_backoff(void)
{
    struct rig r;
    uint8_t pixels[4] = {0};
    unsigned i;
    start(&r, pixels, sizeof(pixels));
    assert(sophia_sf_client_upload_retry(&r.s.files) == SOPHIA_9P_ARGUMENT);
    peer_error(&r.p, r.p.upload_tags[0], 11);
    /* The direct API has no clock and cannot retry merely by being serviced. */
    for (i = 0; i < 8; i++) {
        assert(!sophia_sf_client_service(&r.s.files, 65536));
        peer_pump(&r.p);
    }
    assert(r.s.files.upload_wait && r.p.upload_held == 1);
    assert(!sophia_sf_client_upload_retry(&r.s.files));
    rig_run(&r, 4);
    assert(r.p.upload_held == 2);
    peer_error(&r.p, r.p.upload_tags[1], 11);
    rig_run(&r, 4);
    assert(sophia_ss_timeout(&r.s, r.now) == 4);
    r.now += 4;
    rig_run(&r, 4);
    assert(r.p.upload_held == 3);
    peer_error(&r.p, r.p.upload_tags[2], 11);
    rig_run(&r, 4);
    assert(sophia_ss_timeout(&r.s, r.now) == 8);
    /* A different retry timer must not hide the upload's earlier wake. */
    r.s.waiting = 1;
    r.s.progress = 0;
    r.s.retry_at = r.now + 16;
    assert(sophia_ss_timeout(&r.s, r.now) == 8);
    r.s.retry_at = r.now + 2;
    assert(sophia_ss_timeout(&r.s, r.now) == 2);
    r.s.waiting = 0;
    r.now += 8;
    rig_run(&r, 4);
    peer_count(&r.p, r.p.upload_tags[3], 4);
    rig_run(&r, 4);
    assert(sophia_ss_upload_ready(&r.s) && !r.s.upload_retry_delay);
    rig_close(&r);
}

static void invalid_write_counts(void)
{
    unsigned counts[] = {0, 5};
    unsigned i;
    for (i = 0; i < 2; i++) {
        struct rig r;
        uint8_t pixels[16] = {0};
        start(&r, pixels, sizeof(pixels));
        peer_count(&r.p, r.p.upload_tags[0], counts[i]);
        rig_run(&r, 4);
        assert(r.s.state == SOPHIA_SS_FAILED);
        assert(r.p.upload_held == 4 && !r.s.files.upload_offset);
        rig_close(&r);
    }
}

static void terminal_window(int stale_head)
{
    struct rig r;
    uint8_t pixels[16] = {0};
    const struct sophia_sf_record *event;
    uint64_t ticket;
    unsigned i;
    start(&r, pixels, sizeof(pixels));
    assert(sophia_ss_upload_cancel(&r.s, 78, &ticket) == SOPHIA_9P_BUSY);
    if (stale_head)
        peer_error(&r.p, r.p.upload_tags[0], 116);
    else
        peer_resource_status(&r.p, &r.s.files.resource, 4);
    rig_run(&r, 4);
    assert(!r.p.upload_clunks && r.s.files.upload_data == pixels);
    if (!stale_head) {
        assert(!sophia_ss_event(&r.s, &event));
        assert(event->header.kind == SOPHIA_SF_RESOURCE_STATUS);
        assert(!sophia_ss_consume(&r.s));
    }
    for (i = 4; i > (unsigned)stale_head; i--) {
        if (!stale_head && i == 1)
            peer_count(&r.p, r.p.upload_tags[i - 1], 4);
        else
            peer_error(&r.p, r.p.upload_tags[i - 1], stale_head ? 116 : 22);
    }
    rig_run(&r, 12);
    assert(r.p.upload_clunks == 1 && !r.s.files.upload_data);
    assert(!sophia_ss_upload_pending(&r.s) && r.s.state == SOPHIA_SS_READY);
    assert(r.p.upload_held == 4);
    rig_close(&r);
}

static void partial_transport_window(void)
{
    struct rig r;
    uint8_t pixels[24] = {0};
    unsigned i;
    start_budget(&r, pixels, sizeof(pixels), 0, 1);
    assert(r.partial_send_seen);
    for (i = 0; i < 4; i++)
        peer_count(&r.p, r.p.upload_tags[i], 4);
    for (i = 0; i < 4096 && r.p.upload_held < 6; i++)
        rig_step(&r);
    assert(r.p.upload_held == 6);
    for (i = 0; i < 6; i++)
        assert(r.p.upload_offsets[i] == i * 4 && r.p.upload_counts[i] == 4);
    for (i = 4; i < 6; i++)
        peer_count(&r.p, r.p.upload_tags[i], 4);
    for (i = 0; i < 4096 && !sophia_ss_upload_ready(&r.s); i++)
        rig_step(&r);
    assert(sophia_ss_upload_ready(&r.s) && r.s.files.upload_offset == sizeof(pixels));
    rig_close(&r);
}

int main(void)
{
    ordered_success();
    iounit_fragments_keep_order();
    recover(0);
    recover(1);
    bad_tail_and_fence(0);
    bad_tail_and_fence(1);
    explicit_retry_and_backoff();
    invalid_write_counts();
    terminal_window(0);
    terminal_window(1);
    partial_transport_window();
    puts("shell upload window: ordered credit, short recovery, idle wake and fencing: ok");
    return 0;
}

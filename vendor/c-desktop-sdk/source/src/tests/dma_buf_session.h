/* Shell r10 DMA-BUF through the image adapter, over the scripted 9P peer
 * and a real SEQPACKET endpoint: discovery mask 6, ChannelReady 6, the
 * eager ring registration beside it, ImageBegin/Submitted, ImageAdmitted,
 * a one-fd Source, Accepted alone not settled, and the exact
 * SourceReleased(Captured) settling the offer. The source fd is a plain
 * memfd: this fixture proves wire shape and custody only, never device
 * or GPU completion evidence. Included by shell_image_test.c after
 * owned_ring_session.h. */
#include <sys/mman.h>

#define DMA_BIT_CAP (UINT64_C(1) << 14)
#define RING_BIT_CAP (UINT64_C(1) << 15)

static uint8_t dma_formats_obj[SOPHIA_IC_FORMATS_HEADER_BYTES + SOPHIA_IC_FORMAT_ROW_BYTES];
static size_t dma_formats_obj_bytes;

static void dma_start(struct image_rig *t)
{
    struct sophia_ss_image_config c;
    struct sophia_ss_config config = image_config;
    struct sophia_ic_header h = {SOPHIA_IC_IMAGE_FORMATS, PEER_EPOCH, 0, 0};
    struct sophia_ic_formats f;
    static const struct sophia_ic_format_row row = {7, 226, 0, 0x34325258u,
                                                    SOPHIA_IC_FORMAT_OPAQUE, 0, 512, 512};
    config.profile = SOPHIA_SF_BAR;
    config.offer.maximum_revision = 10;
    config.offer.required_capabilities = RING_BIT_CAP | DMA_BIT_CAP | CONTENT;
    memset(t, 0, sizeof(*t));
    listen_dir(t);
    make_set(&t->set, SOPHIA_IC_KIND_BIT_OWNED_RING | SOPHIA_IC_KIND_BIT_DMABUF);
    rig_start(&t->r, &config);
    t->welcome.selected_revision = 10;
    t->welcome.connection_epoch = PEER_EPOCH;
    t->welcome.capabilities = config.offer.required_capabilities;
    t->welcome.max_descriptors = 16;
    t->welcome.max_label_bytes = 128;
    t->welcome.max_pending_activations = 16;
    t->welcome.limits_published = 1;
    t->r.p.welcome_override = &t->welcome;
    t->r.p.revision = 10;
    t->r.p.image_data[0] = (const uint8_t *)t->set.text;
    t->r.p.image_bytes[0] = t->set.text_bytes;
    t->r.p.image_data[1] = t->set.limits;
    t->r.p.image_bytes[1] = t->set.limits_bytes;
    memset(&f, 0, sizeof(f));
    f.channel_generation = GEN;
    f.formats_generation = 5;
    f.limits_generation = 6;
    f.row_count = 1;
    assert(sophia_ic_formats_encode(dma_formats_obj, sizeof(dma_formats_obj), &h, &f, &row,
                                    &dma_formats_obj_bytes) == SOPHIA_IC_OK);
    t->r.p.image_data[2] = dma_formats_obj;
    t->r.p.image_bytes[2] = dma_formats_obj_bytes;
    {
        struct sophia_sf_record record;
        size_t bytes;
        assert(!sophia_sf_decode(vector_Limits, sizeof(vector_Limits), &record));
        record.header.epoch = PEER_EPOCH;
        record.value.limits.grant_connection_epoch = PEER_EPOCH;
        record.value.limits.grant_content_epoch = 3;
        record.value.limits.max_resource_bytes = 1u << 20;
        record.value.limits.max_staging_bytes = 1u << 20;
        record.value.limits.max_resident_bytes = 1u << 20;
        record.value.limits.max_retiring_bytes = 1u << 20;
        record.value.limits.max_session_retiring_bytes = 3u << 20;
        record.value.limits.max_width_px = record.value.limits.max_height_px = 512;
        record.value.limits.max_outputs = 2;
        record.value.limits.max_open_transfers = 4;
        record.value.limits.max_chunk_bytes = 2048;
        record.value.limits.max_frame_payload = 4096;
        record.value.limits.max_input_queue_bytes = 8192;
        record.value.limits.transfer_timeout_ms = 2000;
        assert(!sophia_sf_encode(t->content_limits, sizeof(t->content_limits), &record, &bytes));
        t->r.p.object_data[SOPHIA_SF_LIMITS] = t->content_limits;
        t->r.p.object_bytes[SOPHIA_SF_LIMITS] = bytes;
    }
    c = adapter_config(t);
    c.dma_buf = 1;
    c.ring_slots = 1;
    c.ring_capacity = RING_CAPACITY;
    assert(sophia_ss_image_init(&t->img, &c) == SOPHIA_IPR_OK);
    assert(sophia_ss_image_attach(&t->r.s, &t->img) == SOPHIA_IPR_OK);
}

/* Hello and ChannelReady 6. The DMA opt-in selects the DMA producer
 * lifecycle: no ring is registered or driven on this connection. */
static void dma_ready(struct image_rig *t)
{
    struct sophia_ip *p;
    uint8_t b[256];
    unsigned i;
    int fd;
    rig_settle(&t->r);
    assert(sophia_ss_state(&t->r.s) == SOPHIA_SS_READY);
    for (i = 0; i < 32 && sophia_ss_image_poll_fd(&t->img) < 0; i++)
        image_step(t);
    p = sophia_ss_image_producer(&t->img);
    for (i = 0; i < 4 && sophia_ip_channel(p) != SOPHIA_IPC_HELLO_SENT; i++)
        image_step(t);
    assert(owner_receive(t, b, sizeof(b), &fd) == SOPHIA_IC_HELLO_BYTES && fd < 0);
    /* Advertised 6, granted 6, both protocol bits negotiated. */
    peer_channel(&t->r.p, SOPHIA_IC_CHANNEL_READY,
                 SOPHIA_IC_KIND_BIT_OWNED_RING | SOPHIA_IC_KIND_BIT_DMABUF);
    ring_steps(t, 6);
    assert(sophia_ip_channel(p) == SOPHIA_IPC_READY);
    /* No ring registration was written and none is driven. */
    assert(sophia_ss_image_ring_state(&t->img, NULL) == SOPHIA_SS_RING_IDLE);
    assert(t->r.p.ring_writes == 0);
    ring_steps(t, 4);
    assert(sophia_ss_image_ring_state(&t->img, NULL) == SOPHIA_SS_RING_IDLE);
    assert(t->r.p.ring_writes == 0);
}

static struct sophia_ic_metadata dma_image(uint64_t resource)
{
    struct sophia_ic_metadata m = image(resource);
    m.source_kind = SOPHIA_IC_SOURCE_DMABUF;
    m.source_format = 0x34325258u;
    m.modifier = 0;
    m.width = 10;
    m.height = 5;
    m.stride = 0;
    m.logical_bytes = 200;
    return m;
}

static int dma_memfd(off_t bytes)
{
    int fd = memfd_create("sophia-dma-test", MFD_CLOEXEC);
    assert(fd >= 0 && !ftruncate(fd, bytes));
    return fd;
}

static void dma_buf_end_to_end(void)
{
    struct image_rig t;
    struct sophia_ic_metadata m = dma_image(61);
    struct sophia_ic_plane plane = {0, 40, 0};
    struct sophia_ip_offer_info info;
    struct sophia_ic_record r;
    uint64_t offer, ticket;
    uint8_t packet[256];
    unsigned i;
    int source_fd, fd = -1;
    dma_start(&t);
    dma_ready(&t);
    ack_all(&t);

    /* A format row mismatch refuses before the producer takes the fd. */
    {
        struct sophia_ic_metadata bad = m;
        int bad_fd = dma_memfd(200);
        bad.output = 9;
        assert(sophia_ss_image_begin_dma_buf(&t.img, &bad, &plane, bad_fd, 0, t.r.now,
                                             &offer, &ticket) == SOPHIA_IPR_REFUSED);
        close(bad_fd);
    }
    /* An fd smaller than the declared plane span is refused the same way. */
    {
        int small_fd = dma_memfd(100);
        assert(sophia_ss_image_begin_dma_buf(&t.img, &m, &plane, small_fd, 0, t.r.now,
                                             &offer, &ticket) == SOPHIA_IPR_REFUSED);
        close(small_fd);
    }

    source_fd = dma_memfd(200);
    assert(sophia_ss_image_begin_dma_buf(&t.img, &m, &plane, source_fd, 0, t.r.now,
                                         &offer, &ticket) == SOPHIA_IPR_OK);
    close(source_fd); /* borrowed: the producer holds its own dup */
    for (i = 0; i < 16 && rig_outcome(&t.r, ticket) != SOPHIA_SS_SUBMITTED; i++)
        image_step(&t);
    assert(rig_outcome(&t.r, ticket) == SOPHIA_SS_SUBMITTED);

    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_IC_IMAGE_ADMITTED;
    r.value.image = m;
    r.value.image.offer = offer;
    peer_image(&t.r.p, &r);
    for (i = 0; i < 16 && owner_receive(&t, packet, sizeof(packet), &fd) < 0; i++)
        image_step(&t);
    /* The Source packet carries exactly one fd, which stays open here. */
    assert(fd >= 0);
    close(fd);
    assert(!sophia_ip_offer(&t.img.producer, offer, &info));
    assert(info.state == SOPHIA_IPO_SENT);

    /* Accepted alone does not settle: no adoption, no slot reuse yet. */
    peer_status(&t.r.p, offer, 61, 2, 0);
    for (i = 0; i < 8; i++)
        image_step(&t);
    assert(!sophia_ip_offer(&t.img.producer, offer, &info));
    assert(info.state != SOPHIA_IPO_SETTLED && info.status == SOPHIA_IPS_ACCEPTED);
    assert(sophia_ip_forget(&t.img.producer, offer) == SOPHIA_IPR_REFUSED);
    /* Consume the application status head so the journal can deliver
     * SourceReleased next. */
    {
        uint16_t kinds[4];
        assert(app_events(&t, kinds, 4) == 1 && kinds[0] == SOPHIA_SF_RESOURCE_STATUS);
    }
    ack_all(&t);

    memset(&r, 0, sizeof(r));
    r.header.kind = SOPHIA_IC_SOURCE_RELEASED;
    r.value.released.offer = offer;
    r.value.released.resource_id = 61;
    r.value.released.resource_generation = 1;
    r.value.released.disposition = SOPHIA_IC_CAPTURED;
    peer_image(&t.r.p, &r);
    for (i = 0; i < 8; i++)
        image_step(&t);
    assert(!sophia_ip_offer(&t.img.producer, offer, &info));
    assert(info.state == SOPHIA_IPO_SETTLED && info.disposition == SOPHIA_IC_CAPTURED);
    assert(sophia_ip_forget(&t.img.producer, offer) == SOPHIA_IPR_OK);

    sophia_ss_image_close(&t.img);
    image_close(&t);
}

/* Default ring client: discovery advertises 6 but ChannelReady grants
 * only 4. The ring works unchanged and DMA stays unavailable. */
static void dma_discovery6_granted4_ring_only(void)
{
    struct image_rig t;
    struct sophia_ss_image_config c;
    struct sophia_ss_config config = image_config;
    struct sophia_ic_metadata m = dma_image(63);
    struct sophia_ic_plane plane = {0, 40, 0};
    uint64_t offer, ticket;
    int ring_fd, fd;
    config.profile = SOPHIA_SF_BAR;
    config.offer.maximum_revision = 10;
    config.offer.required_capabilities = RING_BIT_CAP | CONTENT;
    memset(&t, 0, sizeof(t));
    listen_dir(&t);
    make_set(&t.set, SOPHIA_IC_KIND_BIT_OWNED_RING | SOPHIA_IC_KIND_BIT_DMABUF);
    rig_start(&t.r, &config);
    t.welcome.selected_revision = 10;
    t.welcome.connection_epoch = PEER_EPOCH;
    t.welcome.capabilities = config.offer.required_capabilities;
    t.welcome.max_descriptors = 16;
    t.welcome.max_label_bytes = 128;
    t.welcome.max_pending_activations = 16;
    t.welcome.limits_published = 1;
    t.r.p.welcome_override = &t.welcome;
    t.r.p.revision = 10;
    t.r.p.image_data[0] = (const uint8_t *)t.set.text;
    t.r.p.image_bytes[0] = t.set.text_bytes;
    t.r.p.image_data[1] = t.set.limits;
    t.r.p.image_bytes[1] = t.set.limits_bytes;
    t.r.p.image_data[2] = t.set.formats;
    t.r.p.image_bytes[2] = t.set.formats_bytes;
    {
        struct sophia_sf_record record;
        size_t bytes;
        assert(!sophia_sf_decode(vector_Limits, sizeof(vector_Limits), &record));
        record.header.epoch = PEER_EPOCH;
        record.value.limits.grant_connection_epoch = PEER_EPOCH;
        record.value.limits.grant_content_epoch = 3;
        record.value.limits.max_resource_bytes = 1u << 20;
        record.value.limits.max_staging_bytes = 1u << 20;
        record.value.limits.max_resident_bytes = 1u << 20;
        record.value.limits.max_retiring_bytes = 1u << 20;
        record.value.limits.max_session_retiring_bytes = 3u << 20;
        record.value.limits.max_width_px = record.value.limits.max_height_px = 512;
        record.value.limits.max_outputs = 2;
        record.value.limits.max_open_transfers = 4;
        record.value.limits.max_chunk_bytes = 2048;
        record.value.limits.max_frame_payload = 4096;
        record.value.limits.max_input_queue_bytes = 8192;
        record.value.limits.transfer_timeout_ms = 2000;
        assert(!sophia_sf_encode(t.content_limits, sizeof(t.content_limits), &record, &bytes));
        t.r.p.object_data[SOPHIA_SF_LIMITS] = t.content_limits;
        t.r.p.object_bytes[SOPHIA_SF_LIMITS] = bytes;
    }
    c = adapter_config(&t);
    c.ring_slots = 1;
    c.ring_capacity = RING_CAPACITY;
    assert(sophia_ss_image_init(&t.img, &c) == SOPHIA_IPR_OK);
    assert(sophia_ss_image_attach(&t.r.s, &t.img) == SOPHIA_IPR_OK);
    ring_fd = ring_ready(&t); /* ChannelReady grants only the ring (4) */
    assert(sophia_ss_image_ring_state(&t.img, NULL) == SOPHIA_SS_RING_READY);
    assert(t.img.producer.expect.source_kinds == SOPHIA_IC_KIND_BIT_OWNED_RING);
    fd = dma_memfd(200);
    assert(sophia_ss_image_begin_dma_buf(&t.img, &m, &plane, fd, 0, t.r.now,
                                         &offer, &ticket) == SOPHIA_IPR_REFUSED);
    close(fd); /* refused before custody moved */
    close(ring_fd);
    sophia_ss_image_close(&t.img);
    image_close(&t);
}

/* Native adoption of a DMA offer (launcher, DMA mode): blocked before
 * SourceReleased, refused after a Retained release, and consumed only
 * after Accepted plus SourceReleased(Captured). */
static void dma_native_adoption(void)
{
    struct image_rig t;
    struct sophia_ns n;
    struct sophia_ns_config ns_config = {0};
    struct sophia_ic_metadata m;
    struct sophia_ic_plane plane = {0, 40, 0};
    struct sophia_ic_record r;
    uint64_t offer, ticket;
    uint8_t packet[256];
    unsigned i;
    int source_fd, fd = -1;
    ns_config.first_transaction = 1000;
    /* Launcher variant of dma_start: exact native caps with DMA. */
    {
        struct sophia_ss_image_config c;
        struct sophia_ss_config config = image_config;
        struct sophia_ic_header h = {SOPHIA_IC_IMAGE_FORMATS, PEER_EPOCH, 0, 0};
        struct sophia_ic_formats f;
        static const struct sophia_ic_format_row row = {7, 226, 0, 0x34325258u,
                                                        SOPHIA_IC_FORMAT_OPAQUE, 0, 512, 512};
        config.profile = SOPHIA_SF_LAUNCHER;
        config.offer.maximum_revision = 10;
        config.offer.required_capabilities = 0x9a0 | RING_BIT_CAP | DMA_BIT_CAP;
        memset(&t, 0, sizeof(t));
        listen_dir(&t);
        make_set(&t.set, SOPHIA_IC_KIND_BIT_OWNED_RING | SOPHIA_IC_KIND_BIT_DMABUF);
        rig_start(&t.r, &config);
        t.r.p.api_override =
            "sophia-shell-files version=1 role=launcher epoch=17 fd_transfer=none\n";
        t.welcome.selected_revision = 10;
        t.welcome.connection_epoch = PEER_EPOCH;
        t.welcome.capabilities = config.offer.required_capabilities;
        t.welcome.max_descriptors = 16;
        t.welcome.max_label_bytes = 128;
        t.welcome.max_pending_activations = 16;
        t.welcome.limits_published = 1;
        t.r.p.welcome_override = &t.welcome;
        t.r.p.revision = 10;
        t.r.p.image_data[0] = (const uint8_t *)t.set.text;
        t.r.p.image_bytes[0] = t.set.text_bytes;
        t.r.p.image_data[1] = t.set.limits;
        t.r.p.image_bytes[1] = t.set.limits_bytes;
        memset(&f, 0, sizeof(f));
        f.channel_generation = GEN;
        f.formats_generation = 5;
        f.limits_generation = 6;
        f.row_count = 1;
        assert(sophia_ic_formats_encode(dma_formats_obj, sizeof(dma_formats_obj), &h, &f,
                                        &row, &dma_formats_obj_bytes) == SOPHIA_IC_OK);
        t.r.p.image_data[2] = dma_formats_obj;
        t.r.p.image_bytes[2] = dma_formats_obj_bytes;
        {
            struct sophia_sf_record record;
            size_t bytes;
            assert(!sophia_sf_decode(vector_Limits, sizeof(vector_Limits), &record));
            record.header.epoch = PEER_EPOCH;
            record.value.limits.grant_connection_epoch = PEER_EPOCH;
            record.value.limits.grant_content_epoch = 3;
            record.value.limits.max_resource_bytes = 1u << 20;
            record.value.limits.max_staging_bytes = 1u << 20;
            record.value.limits.max_resident_bytes = 1u << 20;
            record.value.limits.max_retiring_bytes = 1u << 20;
            record.value.limits.max_session_retiring_bytes = 3u << 20;
            record.value.limits.max_width_px = record.value.limits.max_height_px = 512;
            record.value.limits.max_outputs = 2;
            record.value.limits.max_open_transfers = 4;
            record.value.limits.max_chunk_bytes = 2048;
            record.value.limits.max_frame_payload = 4096;
            record.value.limits.max_input_queue_bytes = 8192;
            record.value.limits.transfer_timeout_ms = 2000;
            assert(!sophia_sf_encode(t.content_limits, sizeof(t.content_limits), &record,
                                     &bytes));
            t.r.p.object_data[SOPHIA_SF_LIMITS] = t.content_limits;
            t.r.p.object_bytes[SOPHIA_SF_LIMITS] = bytes;
        }
        c = adapter_config(&t);
        c.dma_buf = 1;
        assert(sophia_ss_image_init(&t.img, &c) == SOPHIA_IPR_OK);
        assert(sophia_ss_image_attach(&t.r.s, &t.img) == SOPHIA_IPR_OK);
    }
    dma_ready(&t);
    assert(!sophia_ns_init(&n, &t.r.s, &ns_config));
    ack_all(&t);

    for (i = 0; i < 2; i++) {
        uint64_t resource = 71 + i;
        uint32_t disposition = i ? SOPHIA_IC_CAPTURED : SOPHIA_IC_RETAINED;
        m = dma_image(resource);
        source_fd = dma_memfd(200);
        assert(sophia_ss_image_begin_dma_buf(&t.img, &m, &plane, source_fd, 0, t.r.now,
                                             &offer, &ticket) == SOPHIA_IPR_OK);
        close(source_fd);
        {
            unsigned k;
            for (k = 0; k < 16 && rig_outcome(&t.r, ticket) != SOPHIA_SS_SUBMITTED; k++)
                image_step(&t);
        }
        assert(rig_outcome(&t.r, ticket) == SOPHIA_SS_SUBMITTED);
        /* Blocked before any release, whatever the status. */
        assert(sophia_ns_adopt_image(&n, &t.img, offer) == SOPHIA_9P_INVALID);
        memset(&r, 0, sizeof(r));
        r.header.kind = SOPHIA_IC_IMAGE_ADMITTED;
        r.value.image = m;
        r.value.image.offer = offer;
        peer_image(&t.r.p, &r);
        fd = -1;
        {
            unsigned k;
            for (k = 0; k < 16 && owner_receive(&t, packet, sizeof(packet), &fd) < 0; k++)
                image_step(&t);
        }
        assert(fd >= 0);
        close(fd);
        peer_status(&t.r.p, offer, resource, 2, 0);
        {
            unsigned k;
            for (k = 0; k < 8; k++)
                image_step(&t);
        }
        /* Accepted alone still does not adopt. */
        assert(sophia_ns_adopt_image(&n, &t.img, offer) == SOPHIA_9P_INVALID);
        {
            uint16_t kinds[4];
            assert(app_events(&t, kinds, 4) == 1 && kinds[0] == SOPHIA_SF_RESOURCE_STATUS);
        }
        ack_all(&t);
        memset(&r, 0, sizeof(r));
        r.header.kind = SOPHIA_IC_SOURCE_RELEASED;
        r.value.released.offer = offer;
        r.value.released.resource_id = resource;
        r.value.released.resource_generation = 1;
        r.value.released.disposition = disposition;
        peer_image(&t.r.p, &r);
        {
            unsigned k;
            for (k = 0; k < 8; k++)
                image_step(&t);
        }
        if (!i) {
            /* Retained is the sealed rule: never adopts a DMA source. */
            assert(sophia_ns_adopt_image(&n, &t.img, offer) == SOPHIA_9P_INVALID);
            assert(sophia_ip_forget(&t.img.producer, offer) == SOPHIA_IPR_OK);
        }
    }
    /* The Captured offer adopts; the slot is consumed exactly once. */
    assert(!sophia_ns_adopt_image(&n, &t.img, offer));
    assert(sophia_ns_adopt_image(&n, &t.img, offer) == SOPHIA_9P_INVALID);
    sophia_ss_image_close(&t.img);
    image_close(&t);
}

static void dma_buf_session(void)
{
    dma_buf_end_to_end();
    dma_discovery6_granted4_ring_only();
    dma_native_adoption();
}

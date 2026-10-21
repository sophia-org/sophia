/* Shell r10 owned ring through the image adapter, over the scripted 9P peer
 * and a real SEQPACKET endpoint: eager registration, D4F1 source custody,
 * slot offers, store status matched by grant and resource, the exact
 * SlotReleased barrier, refusal and loss. Included by shell_image_test.c. */
#define RING (UINT64_C(1) << 15)
#define RING_CAPACITY (1u << 20)

static void ring_start_role(struct image_rig *t, enum sophia_sf_profile profile, uint32_t slots,
                            uint64_t capacity)
{
    struct sophia_ss_image_config c;
    struct sophia_ss_config config = image_config;
    config.profile = profile;
    config.offer.maximum_revision = 10;
    config.offer.required_capabilities =
        RING | (profile == SOPHIA_SF_LAUNCHER ? UINT64_C(0x9a0) : CONTENT);
    memset(t, 0, sizeof(*t));
    listen_dir(t);
    make_set(&t->set, SOPHIA_IC_KIND_BIT_OWNED_RING);
    rig_start(&t->r, &config);
    if (profile == SOPHIA_SF_LAUNCHER)
        t->r.p.api_override = "sophia-shell-files version=1 role=launcher epoch=17 fd_transfer=none\n";
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
    t->r.p.image_data[2] = t->set.formats;
    t->r.p.image_bytes[2] = t->set.formats_bytes;
    {
        /* Content grant epoch 3, as the scripted statuses and events carry. */
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
    c.ring_slots = slots;
    c.ring_capacity = capacity;
    assert(sophia_ss_image_init(&t->img, &c) == SOPHIA_IPR_OK);
    assert(sophia_ss_image_attach(&t->r.s, &t->img) == SOPHIA_IPR_OK);
}

static void ring_start(struct image_rig *t, uint32_t slots, uint64_t capacity)
{
    ring_start_role(t, SOPHIA_SF_BAR, slots, capacity);
}

static void ring_event(struct peer *p, enum sophia_or_event_kind kind, uint64_t offer,
                       uint32_t slot, uint64_t generation, uint64_t resource, uint16_t reason,
                       uint8_t changed)
{
    struct sophia_or_event e;
    uint8_t b[SOPHIA_OR_EVENT_BYTES];
    memset(&e, 0, sizeof(e));
    e.kind = kind;
    e.identity.connection_epoch = PEER_EPOCH;
    e.identity.content_epoch = 3;
    e.identity.channel_generation = GEN;
    e.identity.ring = e.identity.ring_generation = 1;
    e.sequence = ++p->sequence;
    e.offer = offer;
    e.slot = slot;
    e.slot_generation = generation;
    e.resource_id = resource;
    e.resource_generation = resource ? 1 : 0;
    e.reason = reason;
    e.changed_generation = changed;
    assert(sophia_or_event_encode(b, &e) == SOPHIA_IC_OK);
    peer_raw(p, b, sizeof(b));
}

static void ring_steps(struct image_rig *t, unsigned n)
{
    unsigned i;
    for (i = 0; i < n; i++) {
        t->r.now += 8;
        image_step(t);
    }
}

/* Hello, ChannelReady 4 and the Register write; returns the D4F1 fd after
 * RingAdmitted, with the ring READY. */
static int ring_ready(struct image_rig *t)
{
    struct sophia_or_registration reg;
    struct sophia_or_offer unused;
    struct sophia_ip *p;
    uint8_t b[256];
    unsigned i;
    int fd, kind = 0;
    rig_settle(&t->r);
    assert(sophia_ss_state(&t->r.s) == SOPHIA_SS_READY);
    for (i = 0; i < 32 && sophia_ss_image_poll_fd(&t->img) < 0; i++)
        image_step(t);
    p = sophia_ss_image_producer(&t->img);
    for (i = 0; i < 4 && sophia_ip_channel(p) != SOPHIA_IPC_HELLO_SENT; i++)
        image_step(t);
    assert(owner_receive(t, b, sizeof(b), &fd) == SOPHIA_IC_HELLO_BYTES && fd < 0);
    assert(sophia_ss_image_ring_state(&t->img, NULL) == SOPHIA_SS_RING_IDLE);
    peer_channel(&t->r.p, SOPHIA_IC_CHANNEL_READY, SOPHIA_IC_KIND_BIT_OWNED_RING);
    ring_steps(t, 6);
    /* Eager: Register precedes any image. */
    assert(sophia_ss_image_ring_state(&t->img, NULL) == SOPHIA_SS_RING_REGISTERING);
    assert(t->r.p.ring_writes >= 1 && t->r.p.ring_used[0] == SOPHIA_OR_REGISTER_BYTES);
    assert(sophia_or_control_decode(t->r.p.ring_bytes[0], SOPHIA_OR_REGISTER_BYTES, &kind, &reg,
                                    &unused) == SOPHIA_IC_OK && kind == 1);
    assert(reg.identity.connection_epoch == PEER_EPOCH && reg.identity.content_epoch == 3 &&
           reg.identity.channel_generation == GEN);
    if (t->r.p.ring_error[0])
        return -1;
    ring_event(&t->r.p, SOPHIA_OR_RING_ADMITTED, 0, 0, 0, 0, 0, 0);
    ring_steps(t, 4);
    assert(sophia_ss_image_ring_state(&t->img, NULL) == SOPHIA_SS_RING_ADMITTED);
    assert(owner_receive(t, b, sizeof(b), &fd) == SOPHIA_OR_SOURCE_BYTES && fd >= 0);
    {
        struct sophia_or_registration sent;
        assert(sophia_or_source_decode(b, SOPHIA_OR_SOURCE_BYTES, &sent) == SOPHIA_IC_OK &&
               sophia_or_identity_equal(&sent.identity, &reg.identity) &&
               sent.slots == reg.slots && sent.capacity == reg.capacity);
    }
    ring_event(&t->r.p, SOPHIA_OR_RING_READY, 0, 0, 0, 0, 0, 0);
    ring_steps(t, 4);
    assert(sophia_ss_image_ring_state(&t->img, NULL) == SOPHIA_SS_RING_READY);
    return fd;
}

static struct sophia_ss_ring_image ring_image(uint64_t resource, uint32_t w, uint32_t h)
{
    struct sophia_ss_ring_image m;
    memset(&m, 0, sizeof(m));
    m.resource_id = resource;
    m.resource_generation = 1;
    m.allocation_id = m.allocation_generation = 8;
    m.output_id = m.output_generation = 7;
    m.topology_generation = 9;
    m.width = w;
    m.height = h;
    return m;
}

static struct sophia_ss_ring_offer_info ring_info(struct image_rig *t, uint64_t offer)
{
    struct sophia_ss_ring_offer_info i;
    assert(sophia_ss_image_ring_info(&t->img, offer, &i) == SOPHIA_IPR_OK);
    return i;
}

static void ring_close(struct image_rig *t, int fd)
{
    if (fd >= 0)
        close(fd);
    sophia_ss_image_close(&t->img);
    image_close(t);
}

static void ring_end_to_end(void)
{
    struct image_rig t;
    struct sophia_ss_ring_image m = ring_image(41, 16, 16);
    struct sophia_or_registration reg;
    struct sophia_or_offer o;
    struct stat st;
    uint16_t kinds[4];
    uint64_t offer, word = 0;
    uint8_t *pixels;
    int fd, kind = 0, seals;
    ring_start(&t, 0, 0);
    fd = ring_ready(&t);
    /* Default geometry: one slot at the granted max_resource_bytes. */
    assert(sophia_or_control_decode(t.r.p.ring_bytes[0], SOPHIA_OR_REGISTER_BYTES, &kind, &reg,
                                    &o) == SOPHIA_IC_OK);
    assert(reg.slots == 1 && reg.capacity == RING_CAPACITY);
    seals = fcntl(fd, F_GET_SEALS);
    assert(seals == (F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_FUTURE_WRITE | F_SEAL_SEAL));
    assert(!fstat(fd, &st) && (size_t)st.st_size == 4096u + RING_CAPACITY);
    assert(pwrite(fd, "x", 1, 0) < 0 && errno == EPERM);
    {
        /* r10 has no sealed path; the descriptor stays with the caller. */
        struct sophia_ic_metadata sealed;
        uint64_t sealed_offer = 0, ticket = 0;
        memset(&sealed, 0, sizeof sealed);
        assert(sophia_ss_image_begin(&t.img, &sealed, fd, 2, &sealed_offer, &ticket) ==
               SOPHIA_IPR_REFUSED);
        assert(fcntl(fd, F_GETFD) >= 0 && !sealed_offer && !ticket);
    }
    assert(!sophia_ss_image_ring_pixels(&t.img, 1));
    pixels = sophia_ss_image_ring_pixels(&t.img, 0);
    assert(pixels);
    memset(pixels, 0x40, 16 * 16 * 4);
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_OK);
    assert(offer == 1 && ring_info(&t, offer).state != SOPHIA_SS_SLOT_FREE);
    assert(!sophia_ss_image_ring_pixels(&t.img, 0));
    ring_steps(&t, 4);
    assert(t.r.p.ring_writes == 2 && t.r.p.ring_used[1] == SOPHIA_OR_OFFER_BYTES);
    assert(sophia_or_control_decode(t.r.p.ring_bytes[1], SOPHIA_OR_OFFER_BYTES, &kind, &reg,
                                    &o) == SOPHIA_IC_OK && kind == 2);
    assert(o.offer == 1 && o.slot == 0 && o.slot_generation == 2 && o.resource_id == 41 &&
           o.width == 16 && o.height == 16);
    /* The generation word preceded the Offer, through Sophia's own fd. */
    assert(pread(fd, &word, sizeof(word), 0) == sizeof(word) && word == 2);
    assert(ring_info(&t, offer).state == SOPHIA_SS_SLOT_OFFERED);
    /* The transfer deadline is reported but releases nothing. */
    assert(!ring_info(&t, offer).deadline_passed);
    {
        int64_t due = sophia_ss_image_timeout(&t.img, t.r.now);
        assert(due > 0 && due <= 2000);
        t.r.now += (uint64_t)due;
    }
    ring_steps(&t, 1);
    assert(ring_info(&t, offer).deadline_passed &&
           ring_info(&t, offer).state == SOPHIA_SS_SLOT_OFFERED &&
           !ring_info(&t, offer).adoptable && !sophia_ss_image_ring_pixels(&t.img, 0) &&
           sophia_ss_image_ring_forget(&t.img, offer) != SOPHIA_IPR_OK);
    /* Status first (the application still sees it), then the exact release. */
    peer_status(&t.r.p, offer, 41, 2, 0);
    ring_steps(&t, 2);
    assert(app_events(&t, kinds, 4) == 1 && kinds[0] == SOPHIA_SF_RESOURCE_STATUS);
    assert(ring_info(&t, offer).status == SOPHIA_IPS_ACCEPTED);
    assert(!ring_info(&t, offer).adoptable);
    ring_event(&t.r.p, SOPHIA_OR_SLOT_RELEASED, offer, 0, 2, 41, 0, 0);
    ring_steps(&t, 4);
    assert(app_events(&t, kinds, 4) == 0);
    assert(ring_info(&t, offer).state == SOPHIA_SS_SLOT_RELEASED && ring_info(&t, offer).adoptable);
    assert(!sophia_ss_image_ring_pixels(&t.img, 0));
    assert(sophia_ss_image_ring_forget(&t.img, offer) == SOPHIA_IPR_OK);
    assert(sophia_ss_image_ring_pixels(&t.img, 0));
    /* Next offer: fresh id and an advanced generation. */
    m = ring_image(42, 8, 8);
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_OK);
    ring_steps(&t, 4);
    assert(sophia_or_control_decode(t.r.p.ring_bytes[2], SOPHIA_OR_OFFER_BYTES, &kind, &reg,
                                    &o) == SOPHIA_IC_OK);
    assert(offer == 2 && o.offer == 2 && o.slot_generation == 4);
    ack_all(&t);
    ring_close(&t, fd);
}

static void ring_local_and_control_refusals(void)
{
    struct image_rig t;
    struct sophia_ss_ring_image m = ring_image(41, 16, 16);
    struct sophia_or_registration reg;
    struct sophia_or_offer o;
    uint64_t offer, first;
    int fd, kind = 0;
    /* Register EAGAIN is retried with identical bytes; Offer EINVAL is final. */
    ring_start(&t, 2, 1024);
    t.r.p.ring_error[0] = 11;
    t.r.p.ring_error[3] = 22;
    rig_settle(&t.r);
    {
        struct sophia_ip *p;
        uint8_t b[256];
        unsigned i;
        for (i = 0; i < 32 && sophia_ss_image_poll_fd(&t.img) < 0; i++)
            image_step(&t);
        p = sophia_ss_image_producer(&t.img);
        for (i = 0; i < 4 && sophia_ip_channel(p) != SOPHIA_IPC_HELLO_SENT; i++)
            image_step(&t);
        assert(owner_receive(&t, b, sizeof(b), &fd) == SOPHIA_IC_HELLO_BYTES);
        peer_channel(&t.r.p, SOPHIA_IC_CHANNEL_READY, SOPHIA_IC_KIND_BIT_OWNED_RING);
        ring_steps(&t, 20);
        assert(t.r.p.ring_writes == 2 && t.r.p.ring_used[1] == SOPHIA_OR_REGISTER_BYTES &&
               !memcmp(t.r.p.ring_bytes[0], t.r.p.ring_bytes[1], SOPHIA_OR_REGISTER_BYTES));
        ring_event(&t.r.p, SOPHIA_OR_RING_ADMITTED, 0, 0, 0, 0, 0, 0);
        ring_steps(&t, 4);
        assert(owner_receive(&t, b, sizeof(b), &fd) == SOPHIA_OR_SOURCE_BYTES && fd >= 0);
        ring_event(&t.r.p, SOPHIA_OR_RING_READY, 0, 0, 0, 0, 0, 0);
        ring_steps(&t, 4);
        assert(sophia_ss_image_ring_state(&t.img, NULL) == SOPHIA_SS_RING_READY);
    }
    /* Local checks spend nothing: capacity, dimensions, slot range. */
    m.width = 17;
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_REFUSED);
    m.width = 16;
    assert(sophia_ss_image_ring_offer(&t.img, 2, &m, t.r.now, &offer) == SOPHIA_IPR_REFUSED);
    m.height = 0;
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_REFUSED);
    m.height = 16;
    assert(t.r.p.ring_writes == 2);
    /* Slot 0 is valid and under-capacity offers are allowed. */
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &first) == SOPHIA_IPR_OK);
    /* One control write at a time. */
    m = ring_image(42, 4, 4);
    assert(sophia_ss_image_ring_offer(&t.img, 1, &m, t.r.now, &offer) == SOPHIA_IPR_AGAIN);
    ring_steps(&t, 4);
    assert(ring_info(&t, first).state == SOPHIA_SS_SLOT_OFFERED);
    /* A resource live in another slot is refused locally. */
    m = ring_image(41, 4, 4);
    assert(sophia_ss_image_ring_offer(&t.img, 1, &m, t.r.now, &offer) == SOPHIA_IPR_REFUSED);
    m = ring_image(42, 4, 4);
    assert(sophia_ss_image_ring_offer(&t.img, 1, &m, t.r.now, &offer) == SOPHIA_IPR_OK);
    ring_steps(&t, 4);
    assert(ring_info(&t, offer).state == SOPHIA_SS_SLOT_REFUSED &&
           ring_info(&t, offer).control_error == 22);
    assert(sophia_ss_image_ring_forget(&t.img, offer) == SOPHIA_IPR_OK);
    assert(sophia_ss_image_ring_forget(&t.img, first) == SOPHIA_IPR_REFUSED);
    /* The refused offer's id and slot generation stay spent. */
    assert(sophia_ss_image_ring_offer(&t.img, 1, &m, t.r.now, &offer) == SOPHIA_IPR_OK);
    ring_steps(&t, 4);
    assert(sophia_or_control_decode(t.r.p.ring_bytes[4], SOPHIA_OR_OFFER_BYTES, &kind, &reg,
                                    &o) == SOPHIA_IC_OK);
    assert(offer == 3 && o.slot == 1 && o.slot_generation == 4);
    ack_all(&t);
    ring_close(&t, fd);
}

/* Cancel status carrying its own transaction still matches by grant and
 * resource; a release with a reason is settled but not adoptable. A
 * release before its status fails the ring and keeps custody unknown. */
static void ring_settlement_barrier(void)
{
    struct image_rig t;
    struct sophia_ss_ring_image m = ring_image(41, 16, 16);
    uint64_t offer, ticket, other;
    uint16_t kinds[4];
    int fd;
    ring_start(&t, 2, 0);
    fd = ring_ready(&t);
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_OK);
    ring_steps(&t, 4);
    assert(sophia_ss_image_ring_cancel(&t.img, offer, t.r.now, &ticket) == SOPHIA_IPR_OK);
    assert(sophia_ss_image_ring_cancel(&t.img, offer, t.r.now, &ticket) == SOPHIA_IPR_REFUSED);
    ring_steps(&t, 4);
    /* A wrong-grant status is ignored; the cancel transaction is not the offer. */
    {
        struct sophia_sf_record r;
        memset(&r, 0, sizeof(r));
        r.header.kind = SOPHIA_SF_RESOURCE_STATUS;
        r.value.resource_status.transaction = 999;
        r.value.resource_status.grant_connection_epoch = PEER_EPOCH;
        r.value.resource_status.grant_content_epoch = 4;
        r.value.resource_status.resource_id = 41;
        r.value.resource_status.resource_generation = 1;
        r.value.resource_status.status = 4;
        r.value.resource_status.reason = 11;
        peer_record(&t.r.p, &r);
        ring_steps(&t, 4);
        assert(app_events(&t, kinds, 4) >= 1 && kinds[0] == SOPHIA_SF_RESOURCE_STATUS);
        assert(!ring_info(&t, offer).status);
        r.value.resource_status.grant_content_epoch = 3;
        peer_record(&t.r.p, &r);
        ring_steps(&t, 4);
    }
    assert(app_events(&t, kinds, 4) >= 1);
    assert(ring_info(&t, offer).status == SOPHIA_IPS_CANCELLED);
    ring_event(&t.r.p, SOPHIA_OR_SLOT_RELEASED, offer, 0, 2, 41, 11, 0);
    ring_steps(&t, 4);
    assert(ring_info(&t, offer).state == SOPHIA_SS_SLOT_RELEASED && !ring_info(&t, offer).adoptable);
    /* SlotReleased before its status is a contradiction. */
    m = ring_image(43, 4, 4);
    assert(sophia_ss_image_ring_offer(&t.img, 1, &m, t.r.now, &other) == SOPHIA_IPR_OK);
    ring_steps(&t, 4);
    ring_event(&t.r.p, SOPHIA_OR_SLOT_RELEASED, other, 1, 2, 43, 0, 0);
    ring_steps(&t, 4);
    assert(sophia_ss_image_ring_state(&t.img, NULL) == SOPHIA_SS_RING_FAILED);
    assert(ring_info(&t, other).state == SOPHIA_SS_SLOT_UNKNOWN);
    assert(ring_info(&t, offer).state == SOPHIA_SS_SLOT_RELEASED);
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &other) == SOPHIA_IPR_CLOSED);
    ack_all(&t);
    ring_close(&t, fd);
}

static void ring_refused_and_lost(void)
{
    struct image_rig t;
    struct sophia_ss_ring_image m = ring_image(41, 16, 16);
    uint64_t offer;
    uint16_t reason = 0;
    int fd;
    uint8_t b[256];
    unsigned i;
    /* Budget refusal is final for the connection; nothing falls back. */
    ring_start(&t, 4, 4u << 20);
    rig_settle(&t.r);
    for (i = 0; i < 32 && sophia_ss_image_poll_fd(&t.img) < 0; i++)
        image_step(&t);
    for (i = 0; i < 4; i++)
        image_step(&t);
    assert(owner_receive(&t, b, sizeof(b), &fd) == SOPHIA_IC_HELLO_BYTES);
    peer_channel(&t.r.p, SOPHIA_IC_CHANNEL_READY, SOPHIA_IC_KIND_BIT_OWNED_RING);
    ring_steps(&t, 6);
    /* 4 MiB exceeds the 1 MiB grant: refused locally before any write. */
    assert(sophia_ss_image_ring_state(&t.img, NULL) == SOPHIA_SS_RING_FAILED);
    assert(t.r.p.ring_writes == 0);
    ring_close(&t, -1);

    ring_start(&t, 4, 0);
    rig_settle(&t.r);
    for (i = 0; i < 32 && sophia_ss_image_poll_fd(&t.img) < 0; i++)
        image_step(&t);
    for (i = 0; i < 4; i++)
        image_step(&t);
    assert(owner_receive(&t, b, sizeof(b), &fd) == SOPHIA_IC_HELLO_BYTES);
    peer_channel(&t.r.p, SOPHIA_IC_CHANNEL_READY, SOPHIA_IC_KIND_BIT_OWNED_RING);
    ring_steps(&t, 6);
    ring_event(&t.r.p, SOPHIA_OR_RING_REFUSED, 0, 0, 0, 0, 7, 0);
    ring_steps(&t, 4);
    assert(sophia_ss_image_ring_state(&t.img, &reason) == SOPHIA_SS_RING_REFUSED && reason == 7);
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_CLOSED);
    assert(t.r.p.ring_writes == 1);
    ack_all(&t);
    ring_close(&t, -1);

    /* Journal loss: a live slot becomes UNKNOWN and is never reused. */
    ring_start(&t, 1, 0);
    fd = ring_ready(&t);
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_OK);
    ring_steps(&t, 4);
    sophia_ss_close(&t.r.s);
    ring_steps(&t, 1);
    assert(ring_info(&t, offer).state == SOPHIA_SS_SLOT_UNKNOWN);
    assert(sophia_ss_image_ring_state(&t.img, NULL) == SOPHIA_SS_RING_FAILED);
    assert(!sophia_ss_image_ring_pixels(&t.img, 0));
    ring_close(&t, fd);
}

/* Sealed and owned-ring bits never mix; r10 offers need content and bit 15. */
static void ring_open_refused(void)
{
    struct image_rig t;
    uint8_t b[256];
    unsigned i;
    int fd;
    /* A refused Tlopen clunks its walked fid before the refusal surfaces. */
    ring_start(&t, 0, 0);
    t.r.p.ring_open_error = 13;
    rig_settle(&t.r);
    for (i = 0; i < 32 && sophia_ss_image_poll_fd(&t.img) < 0; i++)
        image_step(&t);
    for (i = 0; i < 4; i++)
        image_step(&t);
    assert(owner_receive(&t, b, sizeof(b), &fd) == SOPHIA_IC_HELLO_BYTES);
    peer_channel(&t.r.p, SOPHIA_IC_CHANNEL_READY, SOPHIA_IC_KIND_BIT_OWNED_RING);
    ring_steps(&t, 8);
    assert(t.r.p.ring_walks == 1 && t.r.p.ring_clunks == 1 && t.r.p.ring_writes == 0);
    for (i = 0; i < t.r.p.fid_count; i++)
        assert(t.r.p.files[i] != F_OWNED_RING);
    assert(sophia_ss_image_ring_state(&t.img, NULL) == SOPHIA_SS_RING_REFUSED);
    ring_close(&t, -1);
}

static void ring_retired_sealed_events(void)
{
    static const uint16_t retired[] = {SOPHIA_IC_IMAGE_ADMITTED, SOPHIA_IC_SOURCE_RELEASED};
    unsigned k, i;
    /* r10 keeps ChannelReady/ChannelClosed; sealed image events fail it. */
    for (k = 0; k < 2; k++) {
        struct image_rig t;
        struct sophia_ic_record r;
        int fd;
        ring_start(&t, 0, 0);
        fd = ring_ready(&t);
        memset(&r, 0, sizeof(r));
        r.header.kind = retired[k];
        if (k == 0) {
            r.value.image = image(41);
            r.value.image.offer = 1;
        } else {
            r.value.released.offer = 1;
            r.value.released.resource_id = 41;
            r.value.released.resource_generation = 1;
            r.value.released.disposition = SOPHIA_IC_RETAINED;
        }
        peer_image(&t.r.p, &r);
        for (i = 0; i < 8 && sophia_ss_state(&t.r.s) == SOPHIA_SS_READY; i++)
            ring_steps(&t, 1);
        assert(sophia_ss_state(&t.r.s) == SOPHIA_SS_FAILED);
        ring_close(&t, fd);
    }
}

/* One ResourceStatus with its own transaction; the application sees each. */
static void ring_status(struct image_rig *t, uint64_t tx, uint64_t resource, uint16_t status,
                        uint16_t reason)
{
    uint16_t kinds[4];
    peer_status(&t->r.p, tx, resource, status, reason);
    ring_steps(t, 4);
    assert(app_events(t, kinds, 4) == 1 && kinds[0] == SOPHIA_SF_RESOURCE_STATUS);
}

static void ring_status_evidence(void)
{
    struct sophia_ss_ring_image m;
    struct image_rig t;
    uint64_t offer;
    unsigned k;
    int fd;
    /* Failed ResourceRetire/Cancel replies are Rejected ResourceStatus
     * records for the same grant and resource. Neither first nor last
     * decides: the exact SlotReleased must match evidence already seen. */
    for (k = 0; k < 2; k++) {
        ring_start(&t, 1, 0);
        fd = ring_ready(&t);
        m = ring_image(41, 4, 4);
        assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_OK);
        ring_steps(&t, 4);
        if (k == 0) {
            /* Failed retire (tx == offer) and failed cancel, then the final. */
            ring_status(&t, offer, 41, 3, 1);
            ring_status(&t, 900, 41, 3, 1);
            assert(ring_info(&t, offer).status == SOPHIA_IPS_REJECTED);
            ring_status(&t, offer, 41, 2, 0);
        } else {
            /* The final first, then a failed cancel's Stale reply. */
            ring_status(&t, offer, 41, 2, 0);
            ring_status(&t, 901, 41, 3, 1);
        }
        assert(ring_info(&t, offer).status == SOPHIA_IPS_ACCEPTED);
        /* Statuses never free the slot. */
        assert(ring_info(&t, offer).state == SOPHIA_SS_SLOT_OFFERED &&
               !ring_info(&t, offer).adoptable && !sophia_ss_image_ring_pixels(&t.img, 0) &&
               sophia_ss_image_ring_forget(&t.img, offer) != SOPHIA_IPR_OK);
        ring_event(&t.r.p, SOPHIA_OR_SLOT_RELEASED, offer, 0, 2, 41, 0, 0);
        ring_steps(&t, 4);
        assert(sophia_ss_image_ring_state(&t.img, NULL) == SOPHIA_SS_RING_READY);
        assert(ring_info(&t, offer).state == SOPHIA_SS_SLOT_RELEASED &&
               ring_info(&t, offer).status == SOPHIA_IPS_ACCEPTED && ring_info(&t, offer).adoptable);
        ack_all(&t);
        ring_close(&t, fd);
    }
    /* A genuine refusal: Rejected Malformed (3) then release reason 3. */
    ring_start(&t, 1, 0);
    fd = ring_ready(&t);
    m = ring_image(41, 4, 4);
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_OK);
    ring_steps(&t, 4);
    ring_status(&t, 902, 41, 3, 1);
    ring_status(&t, offer, 41, 3, 3);
    ring_event(&t.r.p, SOPHIA_OR_SLOT_RELEASED, offer, 0, 2, 41, 3, 0);
    ring_steps(&t, 4);
    assert(ring_info(&t, offer).state == SOPHIA_SS_SLOT_RELEASED &&
           ring_info(&t, offer).status == SOPHIA_IPS_REJECTED && !ring_info(&t, offer).adoptable);
    ack_all(&t);
    ring_close(&t, fd);
    /* Accepted, then cancellation with a changed generation (as owned_release
     * reports it): the release selects Cancelled, never adoptable. */
    ring_start(&t, 1, 0);
    fd = ring_ready(&t);
    m = ring_image(41, 4, 4);
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_OK);
    ring_steps(&t, 4);
    ring_status(&t, offer, 41, 2, 0);
    ring_status(&t, 903, 41, 4, 11);
    ring_event(&t.r.p, SOPHIA_OR_SLOT_RELEASED, offer, 0, 2, 41, 11, 1);
    ring_steps(&t, 4);
    assert(sophia_ss_image_ring_state(&t.img, NULL) == SOPHIA_SS_RING_READY);
    assert(ring_info(&t, offer).state == SOPHIA_SS_SLOT_RELEASED &&
           ring_info(&t, offer).status == SOPHIA_IPS_CANCELLED &&
           ring_info(&t, offer).changed_generation && !ring_info(&t, offer).adoptable);
    ack_all(&t);
    ring_close(&t, fd);
    /* No matching kind fails the ring: success after only a failed control
     * reply, cancellation after only Rejected, refusal after only Accepted. */
    for (k = 0; k < 3; k++) {
        ring_start(&t, 1, 0);
        fd = ring_ready(&t);
        m = ring_image(41, 4, 4);
        assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_OK);
        ring_steps(&t, 4);
        ring_status(&t, offer, 41, k == 2 ? 2 : 3, k == 2 ? 0 : 1);
        ring_event(&t.r.p, SOPHIA_OR_SLOT_RELEASED, offer, 0, 2, 41,
                   k == 0 ? 0 : k == 1 ? 11 : 3, 0);
        ring_steps(&t, 4);
        assert(sophia_ss_image_ring_state(&t.img, NULL) == SOPHIA_SS_RING_FAILED &&
               ring_info(&t, offer).state == SOPHIA_SS_SLOT_UNKNOWN &&
               !ring_info(&t, offer).adoptable);
        ack_all(&t);
        ring_close(&t, fd);
    }
}

static void ring_attach_arguments(void)
{
    struct image_rig t;
    struct sophia_ss_image_config c;
    struct sophia_ss_config config = image_config;
    memset(&t, 0, sizeof(t));
    listen_dir(&t);
    c = adapter_config(&t);
    config.offer.maximum_revision = 10;
    config.offer.required_capabilities = RING | SEALED | CONTENT;
    rig_start(&t.r, &config);
    assert(sophia_ss_image_init(&t.img, &c) == SOPHIA_IPR_OK);
    assert(sophia_ss_image_attach(&t.r.s, &t.img) == SOPHIA_IPR_ARGUMENT);
    rig_close(&t.r);
    config.offer.required_capabilities = RING | CONTENT;
    config.offer.maximum_revision = 9;
    rig_start(&t.r, &config);
    assert(sophia_ss_image_init(&t.img, &c) == SOPHIA_IPR_OK);
    assert(sophia_ss_image_attach(&t.r.s, &t.img) == SOPHIA_IPR_ARGUMENT);
    assert(sophia_ss_image_ring_offer(&t.img, 0, NULL, 0, NULL) == SOPHIA_IPR_ARGUMENT);
    rig_close(&t.r);
    unlisten(&t);
}

/* Native adoption consumes only Accepted plus the exact successful release. */
static void ring_native_adoption(void)
{
    struct image_rig t;
    struct sophia_ns n;
    struct sophia_ns_config config = {0};
    struct sophia_ss_ring_image m = ring_image(41, 16, 16);
    struct sophia_ss_ring_offer_info info;
    uint64_t offer, rejected;
    uint16_t kinds[4];
    int fd;
    config.first_transaction = 1000;
    ring_start_role(&t, SOPHIA_SF_LAUNCHER, 2, 0);
    fd = ring_ready(&t);
    assert(!sophia_ns_init(&n, &t.r.s, &config));
    ack_all(&t);
    assert(sophia_ss_image_ring_offer(&t.img, 0, &m, t.r.now, &offer) == SOPHIA_IPR_OK);
    ring_steps(&t, 4);
    peer_status(&t.r.p, offer, 41, 2, 0);
    ring_steps(&t, 4);
    (void)app_events(&t, kinds, 4);
    /* Accepted alone is not settlement. */
    assert(sophia_ns_adopt_image(&n, &t.img, offer) == SOPHIA_9P_INVALID);
    ring_event(&t.r.p, SOPHIA_OR_SLOT_RELEASED, offer, 0, 2, 41, 0, 0);
    ring_steps(&t, 4);
    assert(!sophia_ns_adopt_image(&n, &t.img, offer));
    assert(sophia_ss_image_ring_info(&t.img, offer, &info) == SOPHIA_IPR_ARGUMENT);
    assert(sophia_ss_image_ring_pixels(&t.img, 0));
    assert(sophia_ns_adopt_image(&n, &t.img, offer) == SOPHIA_9P_INVALID);
    /* A rejected, released offer is settled but never adoptable. */
    m = ring_image(42, 8, 8);
    assert(sophia_ss_image_ring_offer(&t.img, 1, &m, t.r.now, &rejected) == SOPHIA_IPR_OK);
    ring_steps(&t, 4);
    peer_status(&t.r.p, rejected, 42, 3, 2);
    ring_steps(&t, 4);
    (void)app_events(&t, kinds, 4);
    ring_event(&t.r.p, SOPHIA_OR_SLOT_RELEASED, rejected, 1, 2, 42, 2, 0);
    ring_steps(&t, 4);
    assert(ring_info(&t, rejected).state == SOPHIA_SS_SLOT_RELEASED);
    assert(sophia_ns_adopt_image(&n, &t.img, rejected) == SOPHIA_9P_INVALID);
    assert(ring_info(&t, rejected).state == SOPHIA_SS_SLOT_RELEASED);
    ack_all(&t);
    ring_close(&t, fd);
}

static void owned_ring_session(void)
{
    ring_open_refused();
    ring_retired_sealed_events();
    ring_status_evidence();
    ring_attach_arguments();
    ring_end_to_end();
    ring_local_and_control_refusals();
    ring_settlement_barrier();
    ring_refused_and_lost();
    ring_native_adoption();
}

static void frame_image_close(struct image_rig *t, struct sophia_ss_frame_stream *f)
{
    struct sophia_sf_record closed = {0};
    uint64_t ticket;
    assert(!sophia_ss_frame_close(&t->r.s, f, 30, &ticket));
    assert(settle_ticket(t, ticket) == SOPHIA_SS_SUBMITTED);
    closed.header.kind = SOPHIA_SFT_CLOSED;
    closed.value.frame.stream.transaction = 30;
    closed.value.frame.stream.key = f->key;
    closed.value.frame.stream.reason = 11;
    assert(!frame_image_deliver(t, f, closed));
}

static void frame_initial_images(void)
{
    struct image_rig t;
    struct sophia_ss_frame_stream f = {0}, unknown = {0};
    struct sophia_sf_record r = image_frame_record();
    struct sophia_ss_ring_image m = frame_image(&r);
    struct sophia_sft_key key = {PEER_EPOCH, 3, m.output_id, m.output_generation,
        m.allocation_id, m.allocation_generation, m.topology_generation, r.value.frame.commit.candidate.pacing_permit};
    uint64_t offer = 0;
    unsigned i;
    int fd;
    ring_start_revision(&t, SOPHIA_SF_BAR, 2, 0, 13);
    fd = ring_ready(&t); ack_all(&t);
    for (i = 0; i < 3; ++i) {
        struct sophia_sft_key wrong = key;
        struct sophia_ss_ring_offer_info info;
        void *pixels = sophia_ss_image_ring_pixels(&t.img, 0);
        assert(pixels); memset(pixels, 41, 8);
        m.resource_id = 40 + i;
        wrong.allocation_generation++;
        assert(sophia_ss_frame_initial_ring_offer(&t.img, &f, &wrong, 0, &m, t.r.now, &offer) == SOPHIA_IPR_ARGUMENT);
        assert(*(unsigned char *)pixels == 41);
        assert(!sophia_ss_frame_initial_ring_offer(&t.img, &f, &key, 0, &m, t.r.now, &offer));
        assert(!sophia_ss_frame_has_credit(&f) && !f.key.connection_epoch && sophia_ss_frame_drained(&f));
        peer_status(&t.r.p, offer, m.resource_id, 2, 0); ring_steps(&t, 8);
        assert(app_events(&t, NULL, 0) == 1); ack_all(&t);
        info = ring_info(&t, offer);
        ring_event(&t.r.p, SOPHIA_OR_SLOT_RELEASED, offer, 0, info.offer.slot_generation, m.resource_id, 0, 0);
        ring_steps(&t, 8);
        assert(ring_info(&t, offer).adoptable);
        assert(!sophia_ss_image_ring_forget(&t.img, offer));
    }
    assert(t.r.p.ring_writes == 4);
    frame_image_open(&t, &f, &r);
    assert(sophia_ss_frame_has_credit(&f));
    assert(sophia_ss_frame_initial_ring_offer(&t.img, &f, &key, 0, &m, t.r.now, &offer) == SOPHIA_IPR_ARGUMENT);
    frame_image_close(&t, &f);
    assert(sophia_ss_frame_initial_ring_offer(&t.img, &f, &key, 0, &m, t.r.now, &offer) == SOPHIA_IPR_ARGUMENT);
    sophia_ss_frame_disconnect(&unknown);
    assert(sophia_ss_frame_initial_ring_offer(&t.img, &unknown, &key, 0, &m, t.r.now, &offer) == SOPHIA_IPR_ARGUMENT);
    assert(sophia_ss_frame_initial_ring_offer(&t.img, NULL, &key, 0, &m, t.r.now, &offer) == SOPHIA_IPR_ARGUMENT);
    ring_close(&t, fd);
}

static void frame_image_fallback(void)
{
    struct image_rig t;
    struct sophia_ss_frame_stream f = {0};
    struct sophia_sf_record r = image_frame_record(), terminal;
    struct sophia_ss_ring_image m = frame_image(&r), next;
    struct sophia_or_offer sent;
    struct sophia_or_registration registration;
    uint64_t ticket, first, offer = 0;
    unsigned i;
    int kind;
    void *pixels;
    int fd = frame_image_ready(&t, &f, &r);
    m.width = 2; m.height = 1;
    memset(sophia_ss_image_ring_pixels(&t.img, 0), 7, 8);
    assert(!sophia_ss_frame_submit_ring(&t.img, &f, 0, &m, &r, t.r.now, &ticket, &first));
    assert(settle_ticket(&t, ticket) == SOPHIA_SS_SUBMITTED);
    assert(!frame_image_deliver(&t, &f, frame_image_outcome(&f, 1)));
    next = m; next.resource_id++;
    pixels = sophia_ss_image_ring_pixels(&t.img, 1); assert(pixels);
    memset(pixels, 41, 8);
    assert(sophia_ss_frame_legacy_ring_offer(&t.img, &f, 1, &next, t.r.now, &offer) == SOPHIA_IPR_ARGUMENT);
    frame_image_close(&t, &f);
    assert(!sophia_ss_frame_drained(&f));
    assert(sophia_ss_frame_legacy_ring_offer(&t.img, &f, 1, &next, t.r.now, &offer) == SOPHIA_IPR_ARGUMENT);
    terminal = frame_image_outcome(&f, 2);
    terminal.value.frame.outcome.credit_expires_msec = 0;
    assert(!frame_image_deliver(&t, &f, terminal));
    assert(sophia_ss_frame_drained(&f));
    assert(sophia_ss_frame_legacy_ring_offer(&t.img, &f, 1, &next, t.r.now, &offer) == SOPHIA_IPR_BUSY);
    peer_status(&t.r.p, first, m.resource_id, 2, 0); ring_steps(&t, 8);
    assert(app_events(&t, NULL, 0) == 1); ack_all(&t);
    assert(sophia_ss_frame_legacy_ring_offer(&t.img, &f, 1, &next, t.r.now, &offer) == SOPHIA_IPR_BUSY);
    assert(!offer);
    for (i = 0; i < 8; ++i) assert(((unsigned char *)pixels)[i] == 41);
    ring_event(&t.r.p, SOPHIA_OR_SLOT_RELEASED, first, 0, 2, m.resource_id, 0, 0);
    ring_steps(&t, 8);
    assert(!sophia_ss_frame_legacy_ring_offer(&t.img, &f, 1, &next, t.r.now, &offer));
    assert(offer == first + 1 && !sophia_ss_frame_has_credit(&f));
    ring_steps(&t, 8);
    assert(t.r.p.ring_writes == 2);
    assert(sophia_or_control_decode(t.r.p.ring_bytes[1], t.r.p.ring_used[1], &kind,
                                    &registration, &sent) == SOPHIA_IC_OK && kind == 2);
    assert(sent.offer == offer && sent.slot == 1 && sent.slot_generation == 2 && sent.resource_id == next.resource_id);
    peer_status(&t.r.p, offer, next.resource_id, 2, 0); ring_steps(&t, 8);
    assert(app_events(&t, NULL, 0) == 1); ack_all(&t);
    ring_event(&t.r.p, SOPHIA_OR_SLOT_RELEASED, offer, 1, 2, next.resource_id, 0, 0);
    ring_steps(&t, 8);
    assert(ring_info(&t, first).adoptable && ring_info(&t, offer).adoptable);
    ring_close(&t, fd);
}

static void frame_image_fallback_source_first(void)
{
    struct image_rig t;
    struct sophia_ss_frame_stream f = {0};
    struct sophia_sf_record r = image_frame_record(), terminal;
    struct sophia_ss_ring_image m = frame_image(&r), next;
    uint64_t ticket, first, offer = 0;
    void *pixels;
    int fd = frame_image_ready(&t, &f, &r);
    m.width = 2; m.height = 1;
    memset(sophia_ss_image_ring_pixels(&t.img, 0), 7, 8);
    assert(!sophia_ss_frame_submit_ring(&t.img, &f, 0, &m, &r, t.r.now, &ticket, &first));
    assert(settle_ticket(&t, ticket) == SOPHIA_SS_SUBMITTED);
    assert(!frame_image_deliver(&t, &f, frame_image_outcome(&f, 1)));
    peer_status(&t.r.p, first, m.resource_id, 2, 0); ring_steps(&t, 8);
    assert(app_events(&t, NULL, 0) == 1); ack_all(&t);
    ring_event(&t.r.p, SOPHIA_OR_SLOT_RELEASED, first, 0, 2, m.resource_id, 0, 0);
    ring_steps(&t, 8);
    next = m; next.resource_id++;
    pixels = sophia_ss_image_ring_pixels(&t.img, 1); assert(pixels);
    memset(pixels, 41, 8);
    assert(sophia_ss_frame_legacy_ring_offer(&t.img, &f, 1, &next, t.r.now, &offer) == SOPHIA_IPR_ARGUMENT);
    frame_image_close(&t, &f);
    assert(sophia_ss_frame_legacy_ring_offer(&t.img, &f, 1, &next, t.r.now, &offer) == SOPHIA_IPR_ARGUMENT);
    assert(!offer && *(unsigned char *)pixels == 41);
    terminal = frame_image_outcome(&f, 2);
    terminal.value.frame.outcome.credit_expires_msec = 0;
    assert(!frame_image_deliver(&t, &f, terminal));
    assert(!sophia_ss_frame_legacy_ring_offer(&t.img, &f, 1, &next, t.r.now, &offer));
    ring_close(&t, fd);
}

static void frame_image_fallback_refusals(void)
{
    struct image_rig t;
    struct sophia_ss_frame_stream f = {0}, empty = {0};
    struct sophia_sf_record r = image_frame_record();
    struct sophia_ss_ring_image m = frame_image(&r), bad;
    uint64_t offer = 0;
    unsigned i;
    void *pixels;
    int fd = frame_image_ready(&t, &f, &r);
    m.width = 2; m.height = 1;
    frame_image_close(&t, &f);
    pixels = sophia_ss_image_ring_pixels(&t.img, 0); assert(pixels);
    memset(pixels, 29, 8);
    for (i = 0; i < 5; ++i) {
        bad = m;
        switch (i) {
        case 0: bad.output_id++; break;
        case 1: bad.output_generation++; break;
        case 2: bad.allocation_id++; break;
        case 3: bad.allocation_generation++; break;
        case 4: bad.topology_generation++; break;
        }
        assert(sophia_ss_frame_legacy_ring_offer(&t.img, &f, 0, &bad, t.r.now, &offer) == SOPHIA_IPR_ARGUMENT);
    }
    assert(sophia_ss_frame_legacy_ring_offer(&t.img, &empty, 0, &m, t.r.now, &offer) == SOPHIA_IPR_ARGUMENT);
    sophia_ss_frame_disconnect(&empty);
    assert(sophia_ss_frame_legacy_ring_offer(&t.img, &empty, 0, &m, t.r.now, &offer) == SOPHIA_IPR_ARGUMENT);
    assert(!offer && t.r.p.ring_writes == 1 && *(unsigned char *)pixels == 29);
    peer_close(&t.r.p); ring_steps(&t, 8);
    assert(sophia_ss_frame_legacy_ring_offer(&t.img, &f, 0, &m, t.r.now, &offer) == SOPHIA_IPR_CLOSED);
    assert(!offer && *(unsigned char *)pixels == 29);
    ring_close(&t, fd);
}

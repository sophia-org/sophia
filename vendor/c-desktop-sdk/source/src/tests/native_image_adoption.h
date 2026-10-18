/* Included by shell_image_test.c: supplied launcher authority, real 9P and
 * descriptor transport. Producer custody is driven only through public APIs;
 * invalid-context controls supply altered passive lifecycle limit snapshots. */
#include "../sophia_shell_native_session.h"

static void adopt_start(struct image_rig *t, struct sophia_ns *n)
{
    struct sophia_ns_config config = {0};
    config.first_transaction = 1000;
    image_start_role(t, SOPHIA_IC_KIND_BIT_SEALED, SOPHIA_SF_LAUNCHER, 1);
    image_ready(t);
    assert(!sophia_ns_init(n, &t->r.s, &config));
    ack_all(t);
}

static struct sophia_ic_metadata adopt_begin(struct image_rig *t, uint64_t resource)
{
    struct sophia_ic_metadata m = image(resource);
    uint64_t offer, ticket;
    unsigned i;
    assert(sophia_ss_image_begin(&t->img, &m, sealed(16, 16), t->r.now, &offer, &ticket) == SOPHIA_IPR_OK);
    for (i = 0; i < 16 && rig_outcome(&t->r, ticket) != SOPHIA_SS_SUBMITTED; i++)
        image_step(t);
    assert(rig_outcome(&t->r, ticket) == SOPHIA_SS_SUBMITTED);
    return m;
}

static void adopt_send(struct image_rig *t, struct sophia_ic_metadata m)
{
    struct sophia_ic_record r = {0};
    struct sophia_ip_offer_info info;
    uint8_t packet[256];
    unsigned i;
    int fd = -1;
    r.header.kind = SOPHIA_IC_IMAGE_ADMITTED;
    r.value.image = m;
    peer_image(&t->r.p, &r);
    for (i = 0; i < 16 && owner_receive(t, packet, sizeof(packet), &fd) < 0; i++)
        image_step(t);
    assert(fd >= 0);
    close(fd);
    assert(!sophia_ip_offer(&t->img.producer, m.offer, &info));
    assert(info.state == SOPHIA_IPO_SENT);
}

static void adopt_release(struct image_rig *t, struct sophia_ic_metadata m, uint32_t disposition)
{
    struct sophia_ic_record r = {0};
    struct sophia_ip_offer_info info;
    unsigned i;
    r.header.kind = SOPHIA_IC_SOURCE_RELEASED;
    r.value.released.offer = m.offer;
    r.value.released.resource_id = m.resource_id;
    r.value.released.resource_generation = m.resource_generation;
    r.value.released.disposition = disposition;
    peer_image(&t->r.p, &r);
    for (i = 0; i < 8; i++)
        image_step(t);
    assert(!sophia_ip_offer(&t->img.producer, m.offer, &info));
    assert(info.state == SOPHIA_IPO_SETTLED);
}

static struct sophia_ic_metadata adopt_complete(struct image_rig *t, uint64_t id)
{
    struct sophia_ic_metadata m = adopt_begin(t, id);
    uint16_t kinds[4];
    unsigned i;
    adopt_send(t, m);
    peer_status(&t->r.p, m.offer, id, 2, 0);
    for (i = 0; i < 8; i++)
        image_step(t);
    /* Consume the application head before awaiting a later journal read.
     * The producer has already checked this status and retains its result. */
    assert(app_events(t, kinds, 4) == 1 && kinds[0] == SOPHIA_SF_RESOURCE_STATUS);
    ack_all(t);
    adopt_release(t, m, SOPHIA_IC_RETAINED);
    return m;
}

static void adopt_observe(struct image_rig *t, struct sophia_ns *n)
{
    struct sophia_ns_event e;
    int status;
    while (!(status = sophia_ns_next(n, t->r.now, &e)))
        assert(!sophia_ns_done(n));
    assert(status == SOPHIA_9P_AGAIN);
    ack_all(t);
}

static unsigned adopted_state(const struct sophia_ns *n, uint64_t id)
{
    unsigned i;
    for (i = 0; i < SOPHIA_NS_RESOURCES; i++)
        if (n->resources[i].state && n->resources[i].id == id)
            return n->resources[i].state;
    return SOPHIA_NS_RESOURCE_FREE;
}

static void adopt_settlement_and_retirement(void)
{
    struct image_rig t;
    struct sophia_ns n;
    struct sophia_ic_metadata m;
    struct sophia_ip_offer_info info;
    struct sophia_sf_record released = {0};
    uint64_t ticket;
    unsigned i;
    adopt_start(&t, &n);
    m = adopt_begin(&t, 39);
    assert(sophia_ns_adopt_image(&n, &t.img, m.offer) == SOPHIA_9P_INVALID);
    adopt_send(&t, m);
    assert(sophia_ns_adopt_image(&n, &t.img, m.offer) == SOPHIA_9P_INVALID);
    peer_status(&t.r.p, m.offer, 39, 2, 0);
    for (i = 0; i < 6; i++) image_step(&t);
    assert(!sophia_ip_offer(&t.img.producer, m.offer, &info) && info.status == SOPHIA_IPS_ACCEPTED);
    assert(sophia_ns_adopt_image(&n, &t.img, m.offer) == SOPHIA_9P_INVALID);
    assert(adopted_state(&n, 39) == SOPHIA_NS_RESOURCE_FREE);
    /* The C journal retains its head; consuming this status allows a later
     * read to deliver SourceReleased. Adoption still cannot precede it. */
    adopt_observe(&t, &n);
    adopt_release(&t, m, SOPHIA_IC_RETAINED);
    assert(!sophia_ns_adopt_image(&n, &t.img, m.offer));
    assert(sophia_ip_offer(&t.img.producer, m.offer, &info) == SOPHIA_IPR_REFUSED);
    assert(sophia_ns_adopt_image(&n, &t.img, m.offer) == SOPHIA_9P_INVALID);
    /* A repeated matching Accepted after retirement must not resurrect it. */
    m = adopt_complete(&t, 40);
    assert(!sophia_ns_adopt_image(&n, &t.img, m.offer));
    assert(!sophia_ns_retire(&n, 40, 1, &ticket));
    peer_status(&t.r.p, m.offer, 40, 2, 0);
    for (i = 0; i < 8; i++)
        image_step(&t);
    adopt_observe(&t, &n);
    assert(adopted_state(&n, 40) == SOPHIA_NS_RESOURCE_RETIRING);
    released.header.kind = SOPHIA_SF_RESOURCE_RELEASED;
    released.value.resource_released.transaction = 1000;
    released.value.resource_released.grant_connection_epoch = PEER_EPOCH;
    released.value.resource_released.grant_content_epoch = 3;
    released.value.resource_released.resource_id = 40;
    released.value.resource_released.resource_generation = 1;
    released.value.resource_released.reason = 1;
    peer_record(&t.r.p, &released);
    for (i = 0; i < 6; i++) image_step(&t);
    adopt_observe(&t, &n);
    assert(adopted_state(&n, 40) == SOPHIA_NS_RESOURCE_FREE);
    assert(sophia_ns_adopt_image(&n, &t.img, m.offer) == SOPHIA_9P_INVALID);
    image_close(&t);
}

static void adopt_closure(void)
{
    unsigned close_role, i;
    for (close_role = 0; close_role < 2; close_role++) {
        struct image_rig t;
        struct sophia_ns n;
        struct sophia_ic_metadata m;
        adopt_start(&t, &n);
        m = adopt_complete(&t, 40);
        peer_channel(&t.r.p, SOPHIA_IC_CHANNEL_CLOSED, SOPHIA_IC_CLOSED_PEER);
        for (i = 0; i < 6; i++) image_step(&t);
        assert(sophia_ip_channel(&t.img.producer) == SOPHIA_IPC_CLOSED);
        if (close_role) {
            sophia_ss_close(&t.r.s);
            assert(sophia_ns_adopt_image(&n, &t.img, m.offer) != 0);
        } else
            assert(!sophia_ns_adopt_image(&n, &t.img, m.offer));
        image_close(&t);
    }
}

static void adopt_capacity_and_identity(void)
{
    struct image_rig t, other;
    struct sophia_ns n, other_n;
    struct sophia_ic_metadata m;
    struct sophia_ip_offer_info info;
    unsigned id;
    adopt_start(&t, &n);
    adopt_start(&other, &other_n);
    m = adopt_complete(&t, 40);
    assert(sophia_ns_adopt_image(&other_n, &t.img, m.offer) == SOPHIA_9P_ARGUMENT);
    assert(!sophia_ns_adopt_image(&n, &t.img, m.offer));
    adopt_observe(&t, &n);
    m = adopt_complete(&t, 40);
    assert(sophia_ns_adopt_image(&n, &t.img, m.offer) == SOPHIA_9P_INVALID);
    assert(!sophia_ip_offer(&t.img.producer, m.offer, &info));
    assert(!sophia_ip_forget(&t.img.producer, m.offer));
    for (id = 1; id < SOPHIA_NS_RESOURCES; id++) {
        m = adopt_complete(&t, 40 + id);
        assert(!sophia_ns_adopt_image(&n, &t.img, m.offer));
        adopt_observe(&t, &n);
    }
    m = adopt_complete(&t, 99);
    assert(sophia_ns_adopt_image(&n, &t.img, m.offer) == SOPHIA_9P_BUSY);
    assert(!sophia_ip_offer(&t.img.producer, m.offer, &info) && info.state == SOPHIA_IPO_SETTLED);
    image_close(&other);
    image_close(&t);
}

static void adopt_refused(void)
{
    unsigned status, i;
    for (status = 3; status <= 4; status++) {
        struct image_rig t;
        struct sophia_ns n;
        struct sophia_ic_metadata m;
        adopt_start(&t, &n);
        m = adopt_begin(&t, 40);
        adopt_send(&t, m);
        peer_status(&t.r.p, m.offer, 40, (uint16_t)status, 0);
        for (i = 0; i < 8; i++)
            image_step(&t);
        adopt_observe(&t, &n);
        adopt_release(&t, m, status == 3 ? SOPHIA_IC_BEFORE_USE : SOPHIA_IC_CANCELLED);
        assert(sophia_ns_adopt_image(&n, &t.img, m.offer) == SOPHIA_9P_INVALID);
        assert(adopted_state(&n, 40) == SOPHIA_NS_RESOURCE_FREE);
        image_close(&t);
    }
}

static void adopt_invalid_context(void)
{
    struct image_rig t;
    struct sophia_ns n, invalid;
    struct sophia_ic_metadata m;
    struct sophia_ip_offer_info info;
    unsigned mutation;
    adopt_start(&t, &n);
    m = adopt_complete(&t, 40);
    for (mutation = 0; mutation < 5; mutation++) {
        invalid = n;
        switch (mutation) {
        case 0: invalid.limits.grant_connection_epoch++; break;
        case 1: invalid.limits.grant_content_epoch++; break;
        case 2: invalid.limits.max_width_px = 1; break;
        case 3: invalid.limits.max_height_px = 1; break;
        default: invalid.limits.max_resource_bytes = 4; break;
        }
        assert(sophia_ns_adopt_image(&invalid, &t.img, m.offer) == SOPHIA_9P_INVALID);
        assert(adopted_state(&invalid, 40) == SOPHIA_NS_RESOURCE_FREE);
        assert(!sophia_ip_offer(&t.img.producer, m.offer, &info));
        assert(info.state == SOPHIA_IPO_SETTLED);
    }
    assert(!sophia_ns_adopt_image(&n, &t.img, m.offer));
    image_close(&t);
}

static void adopt_contradictory_status(void)
{
    unsigned retiring, fault, i;
    for (retiring = 0; retiring < 2; retiring++) {
        for (fault = 0; fault < 3; fault++) {
            struct image_rig t;
            struct sophia_ns n;
            struct sophia_ns_event event;
            struct sophia_ic_metadata m;
            uint64_t ticket;
            adopt_start(&t, &n);
            m = adopt_complete(&t, 40);
            assert(!sophia_ns_adopt_image(&n, &t.img, m.offer));
            if (retiring)
                assert(!sophia_ns_retire(&n, 40, 1, &ticket));
            peer_status(&t.r.p, m.offer + (fault == 0), 40,
                        fault == 1 ? 3 : fault == 2 ? 1 : 2, 0);
            for (i = 0; i < 8; i++)
                image_step(&t);
            assert(sophia_ns_next(&n, t.r.now, &event) == SOPHIA_9P_INVALID);
            assert(sophia_ns_state(&n) == SOPHIA_NS_FAILED);
            image_close(&t);
        }
    }
}

static void native_image_adoption(void)
{
    adopt_settlement_and_retirement();
    adopt_closure();
    adopt_capacity_and_identity();
    adopt_refused();
    adopt_invalid_context();
    adopt_contradictory_status();
}

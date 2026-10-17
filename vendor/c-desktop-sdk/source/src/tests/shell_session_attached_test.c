/* Admission is supplied here; production Factotum exchange is a separate join. */
#include "shell_session_peer.h"

static void handoff(enum sophia_sf_profile profile, int staging)
{
    struct rig r;
    struct sophia_9p_client wire, original;
    struct sophia_ss_config c = rig_config;
    struct sophia_sf_negotiated welcome = {0};
    size_t bytes = sophia_ss_storage_bytes(c.msize, c.queue_bytes);
    size_t wb = sophia_9p_storage_bytes(c.msize, 8);
    void *ws = malloc(wb);
    uint8_t transaction[8192];
    uint64_t ticket;
    struct sophia_sf_record request;
    memset(&r, 0, sizeof(r));
    assert(ws && !socketpair(AF_UNIX, SOCK_STREAM, 0, r.fd));
    r.storage = malloc(bytes);
    assert(r.storage);
    r.p.fd = r.fd[1];
    r.p.attached_only = r.p.require_custody_ack = 1;
    r.now = 1000;
    assert(!sophia_9p_init(&wire, r.fd[0], c.msize, 8, 32, ws, wb));
    wire.phase = 2;
    wire.fid_count = 1;
    wire.fids[0] = 41;
    peer_bind(&r.p, 41, F_ROOT);
    c.profile = profile;
    if (profile == SOPHIA_SF_DESCRIPTOR) {
        c.offer = (struct sophia_sf_negotiate){1, 1, 1};
        r.p.descriptor = 1;
    } else if (profile == SOPHIA_SF_LAUNCHER || profile == SOPHIA_SF_DOCK) {
        unsigned revision = profile == SOPHIA_SF_LAUNCHER ? 7 : 8;
        c.offer = (struct sophia_sf_negotiate){revision, revision,
                    profile == SOPHIA_SF_LAUNCHER ? 0x9a0 : 0x11a2};
        welcome.selected_revision = revision;
        welcome.connection_epoch = PEER_EPOCH;
        welcome.capabilities = c.offer.required_capabilities;
        welcome.max_descriptors = 16;
        welcome.max_label_bytes = 128;
        welcome.max_pending_activations = 16;
        welcome.limits_published = 1;
        r.p.welcome_override = &welcome;
        r.p.api_override = profile == SOPHIA_SF_LAUNCHER ?
            "sophia-shell-files version=1 role=launcher epoch=17 fd_transfer=none\n" :
            "sophia-shell-files version=1 role=dock epoch=17 fd_transfer=none\n";
    }
    original = wire;
    assert(sophia_ss_open_attached(&r.s, &wire, 42, &c, r.storage, bytes) == SOPHIA_9P_ARGUMENT);
    assert(!memcmp(&original, &wire, sizeof(wire)));
    wire.rx_used = 1;
    assert(sophia_ss_open_attached(&r.s, &wire, 41, &c, r.storage, bytes) == SOPHIA_9P_ARGUMENT);
    wire = original;
    assert(sophia_ss_open_attached(&r.s, &wire, 41, &c, ws, bytes) == SOPHIA_9P_ARGUMENT);
    assert(!memcmp(&original, &wire, sizeof(wire)));
    if (staging)
        assert(!sophia_ss_open_attached_staging(&r.s, &wire, 41, &c, r.storage, bytes,
                                               transaction, sizeof(transaction)));
    else {
        int status = sophia_ss_open_attached(&r.s, &wire, 41, &c, r.storage, bytes);
        if (status) fprintf(stderr, "profile=%d status=%d\n", profile, status);
        assert(!status);
    }
    assert(sophia_ss_poll_fd(&r.s) == r.fd[0]);
    assert((sophia_ss_poll_events(&r.s) & POLLOUT) &&
           "shell bootstrap must schedule its first request without peer input");
    rig_settle(&r);
    assert(r.s.files.wire == &wire && r.s.files.root == 41);
    assert(sophia_ss_state(&r.s) == SOPHIA_SS_READY && sophia_ss_epoch(&r.s) == PEER_EPOCH);
    rig_ack(&r);
    if (profile == SOPHIA_SF_BAR) {
        request = rig_request(91);
        assert(!sophia_ss_submit(&r.s, &request, 1, &ticket));
        assert(rig_until(&r, ticket, 64) == SOPHIA_SS_SUBMITTED);
        {
            struct sophia_sf_resource_begin b = {0};
            const struct sophia_sf_record *event;
            uint8_t pixels[16] = {0};
            unsigned j;
            rig_ack(&r);
            r.s.files.limits.max_height_px = 4;
            r.s.files.limits.max_resource_bytes = 16;
            b.transaction = 92;
            b.grant_connection_epoch = PEER_EPOCH;
            b.grant_content_epoch = r.s.files.limits.grant_content_epoch;
            b.resource_id = b.resource_generation = 1;
            b.width_px = 1;
            b.height_px = b.chunk_count = 4;
            b.total_bytes = 16;
            b.rendered_scale_numerator = b.rendered_scale_denominator = b.pixel_format = 1;
            assert(!sophia_ss_upload_begin(&r.s, b, &ticket));
            assert(rig_until(&r, ticket, 64) == SOPHIA_SS_SUBMITTED);
            peer_resource_status(&r.p, &b, 1);
            rig_run(&r, 8);
            assert(!sophia_ss_event(&r.s, &event));
            assert(event->header.kind == SOPHIA_SF_RESOURCE_STATUS);
            assert(!sophia_ss_consume(&r.s));
            rig_ack(&r);
            rig_run(&r, 8);
            r.p.hold_upload = 1;
            assert(!sophia_ss_upload_chunk(&r.s, pixels, sizeof(pixels)));
            rig_run(&r, 8);
            assert(r.p.upload_held == 3); /* Eight slots leave five controls. */
            peer_count(&r.p, r.p.upload_tags[0], 4);
            rig_run(&r, 4);
            assert(r.p.upload_held == 4);
            for (j = 1; j < 4; j++) peer_count(&r.p, r.p.upload_tags[j], 4);
            rig_run(&r, 4);
            assert(sophia_ss_upload_ready(&r.s));
        }
    }
    rig_close(&r);
    free(ws);
}
int main(void)
{
    unsigned i;
    for (i = 0; i < 2; ++i) {
        handoff(SOPHIA_SF_BAR, i);
        handoff(SOPHIA_SF_LAUNCHER, i);
        handoff(SOPHIA_SF_DOCK, i);
        handoff(SOPHIA_SF_DESCRIPTOR, i);
    }
    puts("shell admitted-root handoff: ok");
    return 0;
}

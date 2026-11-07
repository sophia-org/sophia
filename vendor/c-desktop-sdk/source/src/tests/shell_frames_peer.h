#include "shell_session_peer.h"
#include "../sophia_shell_frames.h"

#define FRAME_CAPS (SOPHIA_SFT_CAPABILITY | SOPHIA_SF_CAP_OWNED_RING | UINT64_C(0x81))
static const struct sophia_sf_negotiated welcome = {13, PEER_EPOCH, FRAME_CAPS, 16, 128, 16, 1};
static const struct sophia_ss_config config = {{13, 13, FRAME_CAPS}, SOPHIA_SF_BAR, 4096, 4, 8192, NULL, 0};

static inline void start_frame_options(struct rig *r, const struct sophia_sf_negotiated *v, const struct sophia_ss_config *cfg, unsigned direct, uint32_t lookup_error, uint32_t open_error, uint32_t iounit)
{
    static uint8_t limits[512];
    struct sophia_sf_record value;
    size_t bytes;
    rig_start(r, cfg);
    assert(!sophia_sf_decode(vector_Limits, sizeof(vector_Limits), &value));
    value.header.epoch = value.value.limits.grant_connection_epoch = PEER_EPOCH;
    value.value.limits.max_candidate_bytes = 8192;
    assert(!sophia_sf_encode(limits, sizeof(limits), &value, &bytes));
    r->p.object_data[SOPHIA_SF_LIMITS] = limits;
    r->p.object_bytes[SOPHIA_SF_LIMITS] = bytes;
    r->p.descriptor = cfg->profile == SOPHIA_SF_DESCRIPTOR;
    r->p.api_override = cfg->profile == SOPHIA_SF_LAUNCHER ? "sophia-shell-files version=1 role=launcher epoch=17 fd_transfer=none\n"
                        : cfg->profile == SOPHIA_SF_DOCK ? "sophia-shell-files version=1 role=dock epoch=17 fd_transfer=none\n" : NULL;
    r->p.revision = 13;
    r->p.welcome_override = v;
    r->p.reusable = direct != 0;
    r->p.frame_submit = direct == 1;
    r->p.frame_lookup_error = lookup_error;
    r->p.frame_open_error = open_error;
    r->p.frame_iounit = iounit;
    rig_settle(r);
}

static inline void start_as(struct rig *r, const struct sophia_sf_negotiated *v, const struct sophia_ss_config *cfg)
{
    start_frame_options(r, v, cfg, 0, 0, 0, 0);
}

static inline void ready(struct rig *r)
{
    start_as(r, &welcome, &config);
    assert(sophia_ss_state(&r->s) == SOPHIA_SS_READY);
    rig_ack(r);
}

static inline struct sophia_sf_record open_record(void)
{
    struct sophia_sf_record r = {0};
    r.header.kind = SOPHIA_SFT_OPEN;
    r.value.frame.stream.transaction = 10;
    r.value.frame.stream.key = (struct sophia_sft_key){PEER_EPOCH, 1, 1, 1, 1, 1, 1, 1};
    return r;
}

/* SFT uses the existing transaction lane and typed journal, never a second wire. */
#include "session_internal.h"

int sf_session_frame(const struct sophia_sf_client *c)
{
    return c->negotiated && c->welcome.selected_revision == SOPHIA_SFT_REVISION &&
           (c->welcome.capabilities & SOPHIA_SFT_CAPABILITY);
}

int sf_session_lean(const struct sophia_sf_client *c)
{
    return sf_session_frame(c) && (c->welcome.capabilities & SOPHIA_SFT_LEAN_CAPABILITY);
}

int sf_frame_welcome(const struct sophia_sf_negotiate *offer, const struct sophia_sf_negotiated *v)
{
    const uint64_t required = SOPHIA_SFT_CAPABILITY | SOPHIA_SF_CAP_OWNED_RING | (UINT64_C(1) << 7);
    const uint64_t options = SOPHIA_SFT_TERMINAL_ONLY_CAPABILITY | SOPHIA_SFT_LEAN_CAPABILITY;
    const uint64_t unsupported = SOPHIA_SF_CAP_SEALED_IMAGE | SOPHIA_SF_CAP_DMABUF_IMAGE |
                                 (UINT64_C(1) << 16) | (UINT64_C(1) << 17);
    if (!((offer->required_capabilities | v->capabilities) & (SOPHIA_SFT_CAPABILITY | options)) &&
        v->selected_revision != SOPHIA_SFT_REVISION) return 1;
    return v->selected_revision == SOPHIA_SFT_REVISION &&
           (offer->required_capabilities & required) == required &&
           (v->capabilities & required) == required && !(v->capabilities & unsupported) &&
        !((offer->required_capabilities ^ v->capabilities) & options) &&
        /* Lean needs terminal-only. */
        (!(v->capabilities & SOPHIA_SFT_LEAN_CAPABILITY) ||
         (v->capabilities & SOPHIA_SFT_TERMINAL_ONLY_CAPABILITY)) &&
        v->limits_published;
}

/* Retired byte records never acquire queue or submission custody. */
#include "shell_session_peer.h"

int main(void)
{
    struct rig r;
    struct sophia_sf_record records[2], batch[2], control = rig_request(80);
    struct sophia_ss_reservation reservation;
    uint8_t encoded[256];
    uint64_t ticket = 1234, before;
    size_t length;
    unsigned i;
    rig_ready(&r, &rig_config);
    before = r.s.next_ticket;
    memset(records, 0, sizeof(records));
    records[0].header.kind = SOPHIA_SF_RESOURCE_BEGIN;
    records[0].header.epoch = PEER_EPOCH;
    records[0].value.resource_begin.transaction = 1;
    records[0].value.resource_begin.grant_connection_epoch = PEER_EPOCH;
    records[0].value.resource_begin.grant_content_epoch = r.s.files.limits.grant_content_epoch;
    records[0].value.resource_begin.resource_id = 1;
    records[0].value.resource_begin.resource_generation = 1;
    records[0].value.resource_begin.width_px = 1;
    records[0].value.resource_begin.height_px = 1;
    records[0].value.resource_begin.rendered_scale_numerator = 1;
    records[0].value.resource_begin.rendered_scale_denominator = 1;
    records[0].value.resource_begin.pixel_format = 1;
    records[0].value.resource_begin.chunk_count = 1;
    records[0].value.resource_begin.total_bytes = 4;
    records[1].header.kind = SOPHIA_SF_RESOURCE_END;
    records[1].header.epoch = PEER_EPOCH;
    records[1].value.resource_end.transaction = 2;
    records[1].value.resource_end.grant_connection_epoch = PEER_EPOCH;
    records[1].value.resource_end.grant_content_epoch = r.s.files.limits.grant_content_epoch;
    records[1].value.resource_end.resource_id = 1;
    records[1].value.resource_end.resource_generation = 1;
    records[1].value.resource_end.total_bytes = 4;
    records[1].value.resource_end.chunk_count = 1;
    for (i = 0; i < 2; i++) {
        assert(sophia_sf_client_submit(&r.s.files, &records[i]) == SOPHIA_9P_ARGUMENT);
        records[i].header.submission = 1;
        assert(!sophia_sf_encode(encoded, sizeof(encoded), &records[i], &length));
        records[i].header.submission = 0;
        memset(encoded + 16, 0, 8);
        assert(sophia_sf_client_submit_bytes(&r.s.files, encoded, length) == SOPHIA_9P_ARGUMENT);
        assert(sophia_ss_submit(&r.s, &records[i], 1, &ticket) == SOPHIA_9P_ARGUMENT);
        batch[i] = records[i];
        batch[1 - i] = control;
        assert(sophia_ss_submit(&r.s, batch, 2, &ticket) == SOPHIA_9P_ARGUMENT);
        assert(!sophia_ss_reserve(&r.s, 2, sizeof(encoded), &reservation));
        assert(sophia_ss_commit(&r.s, &reservation, batch, 2, &ticket) == SOPHIA_9P_ARGUMENT);
        assert(!sophia_ss_cancel(&r.s, &reservation));
        assert(ticket == 1234 && r.s.next_ticket == before && !r.s.queued);
    }
    assert(!sophia_ss_submit(&r.s, &control, 1, &ticket));
    assert(ticket == before && rig_until(&r, ticket, 64) == SOPHIA_SS_SUBMITTED);
    assert(!r.p.upload_bytes);
    rig_close(&r);
    puts("shell byte refusal: typed, encoded and reserved batches preserve custody");
    return 0;
}

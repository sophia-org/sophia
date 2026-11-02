#include "sophia_shell_retire.h"
#undef NDEBUG
#include <assert.h>
#include <string.h>

static struct sophia_sf_record outcome(uint16_t kind)
{
    struct sophia_sf_record r;
    memset(&r, 0, sizeof r);
    r.header.kind = SOPHIA_SF_CANDIDATE_OUTCOME;
    r.value.candidate_outcome = (struct sophia_sf_candidate_outcome){7, 1, 2, 3, 4, 5, kind, 0, 0, 0, 0};
    return r;
}

static struct sophia_sf_record released(void)
{
    struct sophia_sf_record r;
    memset(&r, 0, sizeof r);
    r.header.kind = SOPHIA_SF_RESOURCE_RELEASED;
    r.value.resource_released = (struct sophia_sf_resource_released){91, 1, 2, 92, 93, 0};
    return r;
}

int main(void)
{
    struct sophia_sr tracker;
    struct sophia_sr_entry entries[1];
    struct sophia_sf_candidate_retire intent = {91, 92, 93}, other = {101, 102, 103};
    struct sophia_sr_progress progress;
    struct sophia_sf_record event;
    assert(!sophia_sr_init(&tracker, 1, 2, entries, 1));
    assert(!sophia_sr_name(&tracker, 4, 5, 3, &intent));
    assert(sophia_sr_holds(&tracker, 92, 93));
    assert(sophia_sr_name(&tracker, 6, 5, 3, &other) == SOPHIA_SR_DUPLICATE);
    assert(sophia_sr_name(&tracker, 4, 5, 4, &intent) == SOPHIA_SR_DUPLICATE);
    assert(sophia_sr_name(&tracker, 4, 5, 4, &other) == SOPHIA_SR_CAPACITY);
    event = released();
    assert(sophia_sr_observe(&tracker, &event, &progress) == SOPHIA_SR_MISATTRIBUTED);
    event = outcome(1);
    assert(!sophia_sr_observe(&tracker, &event, &progress) && progress.kind == SOPHIA_SR_UNRELATED);
    assert(sophia_sr_holds(&tracker, 92, 93));
    event = outcome(2);
    ++event.value.candidate_outcome.output_generation;
    assert(sophia_sr_observe(&tracker, &event, &progress) == SOPHIA_SR_MISATTRIBUTED);
    event = outcome(2);
    ++event.value.candidate_outcome.grant_content_epoch;
    assert(sophia_sr_observe(&tracker, &event, &progress) == SOPHIA_SR_WRONG_GRANT);
    event = outcome(2);
    assert(!sophia_sr_observe(&tracker, &event, &progress) && progress.kind == SOPHIA_SR_RETIRING);
    assert(progress.resource_id == 92 && progress.resource_generation == 93);
    assert(!sophia_sr_observe(&tracker, &event, &progress) && progress.kind == SOPHIA_SR_UNRELATED);
    assert(sophia_sr_holds(&tracker, 92, 93));
    event = released();
    ++event.value.resource_released.transaction;
    assert(sophia_sr_observe(&tracker, &event, &progress) == SOPHIA_SR_MISATTRIBUTED);
    event = released();
    ++event.value.resource_released.resource_generation;
    assert(sophia_sr_observe(&tracker, &event, &progress) == SOPHIA_SR_MISATTRIBUTED);
    event = released();
    ++event.value.resource_released.grant_connection_epoch;
    assert(sophia_sr_observe(&tracker, &event, &progress) == SOPHIA_SR_WRONG_GRANT);
    event = released();
    assert(!sophia_sr_observe(&tracker, &event, &progress) && progress.kind == SOPHIA_SR_RELEASED);
    assert(!sophia_sr_holds(&tracker, 92, 93));
    assert(!sophia_sr_observe(&tracker, &event, &progress) && progress.kind == SOPHIA_SR_UNRELATED);
    assert(!sophia_sr_name(&tracker, 4, 5, 3, &intent));
    event = outcome(3);
    assert(!sophia_sr_observe(&tracker, &event, &progress) && progress.kind == SOPHIA_SR_KEPT);
    assert(!sophia_sr_holds(&tracker, 92, 93));
    assert(!sophia_sr_name(&tracker, 4, 5, 3, &intent));
    sophia_sr_revoke(&tracker);
    assert(!sophia_sr_holds(&tracker, 92, 93));
    assert(!sophia_sr_name(&tracker, 4, 5, 3, &intent));
    event = outcome(2);
    assert(!sophia_sr_observe(&tracker, &event, &progress));
    sophia_sr_revoke(&tracker);
    assert(!sophia_sr_holds(&tracker, 92, 93));
    return 0;
}

#ifndef SOPHIA_SHELL_PRIVATE_NATIVE_LIFECYCLE_H
#define SOPHIA_SHELL_PRIVATE_NATIVE_LIFECYCLE_H
#include "../sophia_shell_native_lifecycle.h"
#include "../sophia_shell_content_limits.h"
#include "../sophia_shell_content_control.h"
struct native_scene {
    struct sophia_shell_native_candidate begin;
    struct sophia_shell_native_chunk chunk;
    uint64_t begin_tx, epoch;
    int valid, end_queued, prepared;
};
struct native_response {
    int active;
    uint16_t source_kind;
    uint64_t source_tx;
    size_t source_bytes;
    uint8_t source[512];
    struct sophia_shell_outbox_reservation ticket;
    unsigned count;
    uint8_t frames[2][256];
    size_t lengths[2];
    struct sophia_shell_native_activation activation;
    uint64_t activation_tx;
};
struct sophia_shell_native_lifecycle {
    struct sophia_shell_content_limits limits;
    struct sophia_shell_outbox *outbox;
    uint64_t catalog, last_opening, generation, revision;
    uint64_t last_event, last_action, last_issued, last_lease;
    struct sophia_shell_native_opening opening;
    struct sophia_shell_native_binding focus;
    int open, focus_valid, dismissed, in_callback;
    struct native_scene pending, shown;
    struct native_response response;
    struct sophia_shell_native_activation activation;
    uint64_t activation_tx;
    int activation_pending;
};
static inline int native_id_equal(struct sophia_shell_native_id a, struct sophia_shell_native_id b)
{ return a.id==b.id && a.generation==b.generation; }
static inline int native_grant_equal(struct sophia_shell_native_grant a, struct sophia_shell_native_grant b)
{ return a.connection_epoch==b.connection_epoch && a.content_grant_epoch==b.content_grant_epoch; }
static inline struct sophia_shell_native_grant native_grant(const struct sophia_shell_native_lifecycle *o)
{ return (struct sophia_shell_native_grant){o->limits.grant.connection_epoch,o->limits.grant.content_grant_epoch}; }
static inline int native_binding_equal(const struct sophia_shell_native_binding *a, const struct sophia_shell_native_binding *b)
{
    return native_grant_equal(a->grant,b->grant) && a->opening==b->opening &&
        native_id_equal(a->output,b->output) && native_id_equal(a->allocation,b->allocation) &&
        a->catalog_generation==b->catalog_generation && a->candidate_generation==b->candidate_generation &&
        a->presentation_epoch==b->presentation_epoch && a->interaction_generation==b->interaction_generation &&
        a->state_revision==b->state_revision && a->focus_lease==b->focus_lease;
}
static inline int native_event_equal(const struct sophia_shell_native_event *a, const struct sophia_shell_native_event *b)
{ return native_binding_equal(&a->binding,&b->binding) && a->event_id==b->event_id && a->state_revision==b->state_revision; }
static inline int native_current(const struct sophia_shell_native_lifecycle *o)
{ return o->open && !o->dismissed && o->opening.catalog_generation==o->catalog; }
int native_receive_input(struct sophia_shell_native_lifecycle *,const struct sophia_shell_frame *,
                         uint64_t *,sophia_shell_native_edit,void *);
int native_receive_action(struct sophia_shell_native_lifecycle *,const struct sophia_shell_frame *,uint64_t *);
int native_retry_response(struct sophia_shell_native_lifecycle *,const struct sophia_shell_frame *);
#endif

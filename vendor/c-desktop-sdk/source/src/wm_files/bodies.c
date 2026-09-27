#include "internal.h"

static struct sophia_wf_surface surface(const uint8_t *p) {
  struct sophia_wf_surface v;
  v.index = (uint32_t)wf_get(p, 4);
  v.generation = (uint32_t)wf_get(p + 4, 4);
  return v;
}
static void put_surface(uint8_t *p, struct sophia_wf_surface v) {
  wf_put(p, v.index, 4);
  wf_put(p + 4, v.generation, 4);
}
static int outputs(const uint8_t *p, uint16_t count, uint64_t *out) {
  size_t i, j;
  if (!count || count > SOPHIA_WF_MAX_OUTPUTS)
    return -1;
  for (i = 0; i < count; ++i) {
    out[i] = wf_get(p + i * 8, 8);
    if (!out[i])
      return -1;
    for (j = 0; j < i; ++j)
      if (out[j] == out[i])
        return -1;
  }
  return 0;
}
static int contains(const struct sophia_wf_cycle *v, uint64_t output) {
  size_t i;
  for (i = 0; i < v->output_count; ++i)
    if (v->outputs[i] == output)
      return 1;
  return 0;
}
static int cause_size(uint16_t kind) {
  static const int sizes[] = {0, 16, 8, 16, 32, 32, 64};
  return kind < sizeof(sizes) / sizeof(sizes[0]) ? sizes[kind] : -1;
}
static uint64_t cause_cap(uint16_t kind) {
  switch (kind) {
  case SOPHIA_WF_ACTION:
    return SOPHIA_WF_CAP_ACTIONS;
  case SOPHIA_WF_POINTER_FOCUS:
    return SOPHIA_WF_CAP_POINTER_FOCUS;
  case SOPHIA_WF_INTERACTION:
    return SOPHIA_WF_CAP_POINTER_INTERACTIONS;
  case SOPHIA_WF_OUTPUT_ACTION:
    return SOPHIA_WF_CAP_ACTIONS | SOPHIA_WF_CAP_OUTPUT_ACTIONS;
  case SOPHIA_WF_PRESENTATION_ACTION:
    return SOPHIA_WF_CAP_ACTIONS | SOPHIA_WF_CAP_SURFACE_INSTANCES |
           SOPHIA_WF_CAP_PRESENTATION_ACTIONS;
  default:
    return 0;
  }
}
static int cause_read(const uint8_t *p, uint64_t caps,
                      struct sophia_wf_cycle *v) {
  uint64_t required = cause_cap(v->cause);
  if ((caps & required) != required)
    return -1;
  switch (v->cause) {
  case SOPHIA_WF_SCENE_CHANGED:
    return 0;
  case SOPHIA_WF_ACTION:
    v->value.action.serial = wf_get(p, 8);
    v->value.action.action = wf_get(p + 8, 8);
    return v->value.action.serial && v->value.action.action ? 0 : -1;
  case SOPHIA_WF_FOCUS:
    v->value.focus = surface(p);
    return wf_surface(v->value.focus, 0) ? 0 : -1;
  case SOPHIA_WF_POINTER_FOCUS:
    v->value.pointer_focus.output = wf_get(p, 8);
    v->value.pointer_focus.target = surface(p + 8);
    return contains(v, v->value.pointer_focus.output) &&
                   wf_surface(v->value.pointer_focus.target, 1)
               ? 0
               : -1;
  case SOPHIA_WF_INTERACTION: {
    struct sophia_wf_interaction *i = &v->value.interaction;
    i->phase = (uint16_t)wf_get(p, 2);
    i->kind = (uint16_t)wf_get(p + 2, 2);
    i->axis = (uint16_t)wf_get(p + 4, 2);
    i->target = surface(p + 8);
    i->x = wf_signed(p + 16, 4);
    i->y = wf_signed(p + 20, 4);
    i->width = wf_signed(p + 24, 4);
    i->height = wf_signed(p + 28, 4);
    if (!wf_zero(p + 6, 2) || !wf_surface(i->target, 0) || !i->phase ||
        i->phase > 4 || !i->kind || i->kind > 4)
      return -1;
    if (i->kind == 4)
      return i->axis >= 1 && i->axis <= 2 && !i->width && !i->height &&
                     (i->phase == 4 || i->x || i->y)
                 ? 0
                 : -1;
    return !i->axis && i->width > 0 && i->height > 0 ? 0 : -1;
  }
  case SOPHIA_WF_OUTPUT_ACTION: {
    struct sophia_wf_output_action *a = &v->value.output_action;
    a->serial = wf_get(p, 8);
    a->action = wf_get(p + 8, 8);
    a->output = wf_get(p + 16, 8);
    a->output_generation = wf_get(p + 24, 8);
    return a->serial && a->action && a->output_generation &&
                   contains(v, a->output)
               ? 0
               : -1;
  }
  case SOPHIA_WF_PRESENTATION_ACTION: {
    struct sophia_wf_presentation_action *a = &v->value.presentation_action;
    a->serial = wf_get(p, 8);
    a->action = wf_get(p + 8, 8);
    a->publication_generation = wf_get(p + 16, 8);
    a->output = wf_get(p + 24, 8);
    a->output_generation = wf_get(p + 32, 8);
    a->presentation_epoch = wf_get(p + 40, 8);
    a->target_id = wf_get(p + 48, 8);
    a->target_generation = wf_get(p + 56, 8);
    return a->serial && a->action && a->publication_generation &&
                   a->output_generation && a->presentation_epoch &&
                   contains(v, a->output) &&
                   ((!a->target_id) == (!a->target_generation))
               ? 0
               : -1;
  }
  default:
    return -1;
  }
}

static size_t minimum(uint16_t kind) {
  switch (kind) {
  case SOPHIA_WF_NEGOTIATED:
    return 8;
  case SOPHIA_WF_NEGOTIATE:
  case SOPHIA_WF_SUBMITTED:
  case SOPHIA_WF_DIRTY:
    return 16;
  case SOPHIA_WF_CONFIGURATION_OUTCOME:
  case SOPHIA_WF_SESSION_OPERATION_OUTCOME:
    return 24;
  case SOPHIA_WF_LIMITS:
  case SOPHIA_WF_SNAPSHOT:
  case SOPHIA_WF_SESSION_OPERATION:
  case SOPHIA_WF_PROJECTION_OUTCOME:
    return 32;
  case SOPHIA_WF_PROJECTION:
    return 40;
  case SOPHIA_WF_PROFILE_PREPARE:
  case SOPHIA_WF_PROFILE_ACTIVATE:
  case SOPHIA_WF_PROFILE_ROLLBACK:
  case SOPHIA_WF_CONFIGURATION:
  case SOPHIA_WF_CYCLE:
  case SOPHIA_WF_PRESENTATION_RECEIPT:
    return 48;
  case SOPHIA_WF_PROFILE_PREPARED:
  case SOPHIA_WF_PROFILE_ACTIVE:
  case SOPHIA_WF_PROFILE_ROLLED_BACK:
    return 56;
  default:
    return 0;
  }
}
static uint64_t family_cap(uint16_t kind) {
  switch (kind) {
  case SOPHIA_WF_PROFILE_PREPARE:
  case SOPHIA_WF_PROFILE_ACTIVATE:
  case SOPHIA_WF_PROFILE_ROLLBACK:
  case SOPHIA_WF_PROFILE_PREPARED:
  case SOPHIA_WF_PROFILE_ACTIVE:
  case SOPHIA_WF_PROFILE_ROLLED_BACK:
    return SOPHIA_WF_CAP_PROFILE_ACTIVATION;
  case SOPHIA_WF_CONFIGURATION:
    return SOPHIA_WF_CAP_CONFIGURATION;
  case SOPHIA_WF_DIRTY:
    return SOPHIA_WF_CAP_POLICY_DIRTY;
  case SOPHIA_WF_SESSION_OPERATION:
    return SOPHIA_WF_CAP_SESSION_OPERATIONS;
  default:
    return 0;
  }
}
int wf_prefix_read(const uint8_t *p, size_t size, uint64_t caps,
                   struct sophia_wf_record *r, size_t *prefix) {
  uint16_t k = r->header.kind;
  size_t n = minimum(k);
  uint64_t required = family_cap(k);
  if (!n || size < n || (caps & required) != required)
    return -1;
  *prefix = n;
  switch (k) {
  case SOPHIA_WF_LIMITS: {
    struct sophia_wf_limits *v = &r->value.limits;
    v->capability_ceiling = wf_get(p, 8);
    v->max_object_bytes = (uint32_t)wf_get(p + 8, 4);
    v->max_journal_bytes = (uint32_t)wf_get(p + 12, 4);
    v->max_journal_records = (uint16_t)wf_get(p + 16, 2);
    v->max_sections = (uint16_t)wf_get(p + 18, 2);
    v->assembly_timeout_ms = (uint32_t)wf_get(p + 20, 4);
    v->send_timeout_ms = (uint32_t)wf_get(p + 24, 4);
    v->profile_required = (uint16_t)wf_get(p + 28, 2);
    return v->max_object_bytes == SOPHIA_WF_MAX_RECORD &&
                   v->max_journal_bytes == SOPHIA_WF_MAX_RECORD &&
                   v->max_journal_records == 64 && v->max_sections == 32 &&
                   v->assembly_timeout_ms == 12000 &&
                   v->send_timeout_ms == 4000 && v->profile_required <= 1 &&
                   wf_zero(p + 30, 2)
               ? 0
               : -1;
  }
  case SOPHIA_WF_NEGOTIATE:
    r->value.negotiate.required = wf_get(p, 8);
    r->value.negotiate.optional = wf_get(p + 8, 8);
    return r->value.negotiate.required & r->value.negotiate.optional ? -1 : 0;
  case SOPHIA_WF_NEGOTIATED:
    r->value.selected_capabilities = wf_get(p, 8);
    return 0;
  case SOPHIA_WF_SUBMITTED:
    r->value.submitted.submission = wf_get(p, 8);
    r->value.submitted.kind = (uint16_t)wf_get(p + 8, 2);
    return r->value.submitted.submission && r->value.submitted.kind >= 256 &&
                   r->value.submitted.kind <= 263 && wf_zero(p + 10, 6)
               ? 0
               : -1;
  case SOPHIA_WF_PROFILE_PREPARE:
  case SOPHIA_WF_PROFILE_ACTIVATE:
  case SOPHIA_WF_PROFILE_ROLLBACK:
  case SOPHIA_WF_PROFILE_PREPARED:
  case SOPHIA_WF_PROFILE_ACTIVE:
  case SOPHIA_WF_PROFILE_ROLLED_BACK: {
    struct sophia_wf_profile *v = &r->value.profile;
    v->transaction = wf_get(p, 8);
    v->generation = wf_get(p + 8, 8);
    memcpy(v->digest, p + 16, 32);
    if (!v->transaction || !v->generation || wf_zero(v->digest, 32))
      return -1;
    if (k < 256)
      return 0;
    v->outcome = (uint16_t)wf_get(p + 48, 2);
    return v->outcome >= 1 && v->outcome <= 3 && wf_zero(p + 50, 6) ? 0 : -1;
  }
  case SOPHIA_WF_SNAPSHOT: {
    struct sophia_wf_snapshot *v = &r->value.snapshot;
    v->transaction = wf_get(p, 8);
    v->scene_generation = wf_get(p + 8, 8);
    v->active_output = wf_get(p + 16, 8);
    r->section_count = (uint16_t)wf_get(p + 24, 2);
    return v->transaction && v->scene_generation && v->active_output &&
                   r->section_count <= 32 && wf_zero(p + 26, 6)
               ? 0
               : -1;
  }
  case SOPHIA_WF_PROJECTION: {
    struct sophia_wf_projection *v = &r->value.projection;
    v->transaction = wf_get(p, 8);
    v->request_id = wf_get(p + 8, 8);
    v->base_generation = wf_get(p + 16, 8);
    v->active_output = wf_get(p + 24, 8);
    r->section_count = (uint16_t)wf_get(p + 32, 2);
    return v->transaction && v->request_id && v->base_generation &&
                   v->active_output && r->section_count <= 32 &&
                   wf_zero(p + 34, 6)
               ? 0
               : -1;
  }
  case SOPHIA_WF_CONFIGURATION: {
    struct sophia_wf_configuration *v = &r->value.configuration;
    v->transaction = wf_get(p, 8);
    v->generation = wf_get(p + 8, 8);
    v->style_bits = (uint16_t)wf_get(p + 16, 2);
    r->section_count = (uint16_t)wf_get(p + 18, 2);
    v->focus_width = (uint32_t)wf_get(p + 20, 4);
    v->focus_rgb = (uint32_t)wf_get(p + 24, 4);
    v->frame_width = (uint32_t)wf_get(p + 28, 4);
    v->frame_focused_rgb = (uint32_t)wf_get(p + 32, 4);
    v->frame_unfocused_rgb = (uint32_t)wf_get(p + 36, 4);
    return v->transaction && v->generation && v->style_bits <= 3 &&
                   r->section_count <= 32 && v->focus_rgb <= 0xffffff &&
                   v->frame_focused_rgb <= 0xffffff &&
                   v->frame_unfocused_rgb <= 0xffffff && wf_zero(p + 40, 8)
               ? 0
               : -1;
  }
  case SOPHIA_WF_CYCLE: {
    struct sophia_wf_cycle *v = &r->value.cycle;
    int cause;
    v->snapshot_transaction = wf_get(p, 8);
    v->request_transaction = wf_get(p + 8, 8);
    v->request_id = wf_get(p + 16, 8);
    v->scene_generation = wf_get(p + 24, 8);
    v->policy_generation = wf_get(p + 32, 8);
    v->cause = (uint16_t)wf_get(p + 40, 2);
    v->output_count = (uint16_t)wf_get(p + 42, 2);
    cause = cause_size(v->cause);
    if (!v->snapshot_transaction || !v->request_transaction || !v->request_id ||
        !v->scene_generation || !v->policy_generation || !wf_zero(p + 44, 4) ||
        cause < 0 || v->output_count > 16)
      return -1;
    n += (size_t)v->output_count * 8;
    *prefix = n + (size_t)cause;
    if (size < *prefix || outputs(p + 48, v->output_count, v->outputs))
      return -1;
    return cause_read(p + n, caps, v);
  }
  case SOPHIA_WF_DIRTY: {
    struct sophia_wf_dirty *v = &r->value.dirty;
    v->generation = wf_get(p, 8);
    v->output_count = (uint16_t)wf_get(p + 8, 2);
    if (!v->generation || v->output_count > 16 || !wf_zero(p + 10, 6))
      return -1;
    *prefix = 16 + (size_t)v->output_count * 8;
    return size >= *prefix ? outputs(p + 16, v->output_count, v->outputs) : -1;
  }
  case SOPHIA_WF_SESSION_OPERATION: {
    struct sophia_wf_session_operation *v = &r->value.session_operation;
    v->transaction = wf_get(p, 8);
    v->request_id = wf_get(p + 8, 8);
    v->operation = wf_get(p + 16, 8);
    v->target = surface(p + 24);
    return v->transaction && v->request_id && v->operation &&
                   wf_surface(v->target, 1)
               ? 0
               : -1;
  }
  case SOPHIA_WF_CONFIGURATION_OUTCOME: {
    struct sophia_wf_configuration_outcome *v = &r->value.configuration_outcome;
    v->transaction = wf_get(p, 8);
    v->generation = wf_get(p + 8, 8);
    v->outcome = (uint16_t)wf_get(p + 16, 2);
    return v->transaction && v->generation && v->outcome >= 1 &&
                   v->outcome <= 5 && wf_zero(p + 18, 6)
               ? 0
               : -1;
  }
  case SOPHIA_WF_SESSION_OPERATION_OUTCOME: {
    struct sophia_wf_session_operation_outcome *v =
        &r->value.session_operation_outcome;
    v->transaction = wf_get(p, 8);
    v->request_id = wf_get(p + 8, 8);
    v->outcome = (uint16_t)wf_get(p + 16, 2);
    return v->transaction && v->request_id && v->outcome >= 1 &&
                   v->outcome <= 5 && wf_zero(p + 18, 6)
               ? 0
               : -1;
  }
  case SOPHIA_WF_PROJECTION_OUTCOME: {
    struct sophia_wf_projection_outcome *v = &r->value.projection_outcome;
    v->transaction = wf_get(p, 8);
    v->request_id = wf_get(p + 8, 8);
    v->scene_generation = wf_get(p + 16, 8);
    v->outcome = (uint16_t)wf_get(p + 24, 2);
    v->expect_session_operation = (uint16_t)wf_get(p + 26, 2);
    return v->transaction && v->request_id && v->scene_generation &&
                   v->outcome >= 1 && v->outcome <= 5 &&
                   v->expect_session_operation <= 1 &&
                   (!v->expect_session_operation || v->outcome == 1) &&
                   wf_zero(p + 28, 4)
               ? 0
               : -1;
  }
  case SOPHIA_WF_PRESENTATION_RECEIPT: {
    struct sophia_wf_presentation_receipt *v = &r->value.presentation_receipt;
    v->transaction = wf_get(p, 8);
    v->publication_generation = wf_get(p + 8, 8);
    v->output = wf_get(p + 16, 8);
    v->output_generation = wf_get(p + 24, 8);
    v->presentation_epoch = wf_get(p + 32, 8);
    v->outcome = (uint16_t)wf_get(p + 40, 2);
    return v->transaction && v->publication_generation && v->output &&
                   v->output_generation && v->presentation_epoch &&
                   v->outcome >= 1 && v->outcome <= 3 && wf_zero(p + 42, 6)
               ? 0
               : -1;
  }
  default:
    return -1;
  }
}

static void cause_write(uint8_t *p, const struct sophia_wf_cycle *v) {
  switch (v->cause) {
  case SOPHIA_WF_ACTION:
    wf_put(p, v->value.action.serial, 8);
    wf_put(p + 8, v->value.action.action, 8);
    break;
  case SOPHIA_WF_FOCUS:
    put_surface(p, v->value.focus);
    break;
  case SOPHIA_WF_POINTER_FOCUS:
    wf_put(p, v->value.pointer_focus.output, 8);
    put_surface(p + 8, v->value.pointer_focus.target);
    break;
  case SOPHIA_WF_INTERACTION: {
    const struct sophia_wf_interaction *i = &v->value.interaction;
    wf_put(p, i->phase, 2);
    wf_put(p + 2, i->kind, 2);
    wf_put(p + 4, i->axis, 2);
    put_surface(p + 8, i->target);
    wf_put(p + 16, (uint32_t)i->x, 4);
    wf_put(p + 20, (uint32_t)i->y, 4);
    wf_put(p + 24, (uint32_t)i->width, 4);
    wf_put(p + 28, (uint32_t)i->height, 4);
    break;
  }
  case SOPHIA_WF_OUTPUT_ACTION: {
    const struct sophia_wf_output_action *a = &v->value.output_action;
    wf_put(p, a->serial, 8);
    wf_put(p + 8, a->action, 8);
    wf_put(p + 16, a->output, 8);
    wf_put(p + 24, a->output_generation, 8);
    break;
  }
  case SOPHIA_WF_PRESENTATION_ACTION: {
    const struct sophia_wf_presentation_action *a =
        &v->value.presentation_action;
    wf_put(p, a->serial, 8);
    wf_put(p + 8, a->action, 8);
    wf_put(p + 16, a->publication_generation, 8);
    wf_put(p + 24, a->output, 8);
    wf_put(p + 32, a->output_generation, 8);
    wf_put(p + 40, a->presentation_epoch, 8);
    wf_put(p + 48, a->target_id, 8);
    wf_put(p + 56, a->target_generation, 8);
    break;
  }
  default:
    break;
  }
}
int wf_prefix_write(uint8_t *p, const struct sophia_wf_record *r,
                    size_t *prefix) {
  uint16_t k = r->header.kind;
  size_t n = minimum(k), i;
  if (!n)
    return -1;
  memset(p, 0, 240); /* Maximum Cycle: 48 + 16*8 + 64. */
  switch (k) {
  case SOPHIA_WF_LIMITS: {
    const struct sophia_wf_limits *v = &r->value.limits;
    wf_put(p, v->capability_ceiling, 8);
    wf_put(p + 8, v->max_object_bytes, 4);
    wf_put(p + 12, v->max_journal_bytes, 4);
    wf_put(p + 16, v->max_journal_records, 2);
    wf_put(p + 18, v->max_sections, 2);
    wf_put(p + 20, v->assembly_timeout_ms, 4);
    wf_put(p + 24, v->send_timeout_ms, 4);
    wf_put(p + 28, v->profile_required, 2);
    break;
  }
  case SOPHIA_WF_NEGOTIATE:
    wf_put(p, r->value.negotiate.required, 8);
    wf_put(p + 8, r->value.negotiate.optional, 8);
    break;
  case SOPHIA_WF_NEGOTIATED:
    wf_put(p, r->value.selected_capabilities, 8);
    break;
  case SOPHIA_WF_SUBMITTED:
    wf_put(p, r->value.submitted.submission, 8);
    wf_put(p + 8, r->value.submitted.kind, 2);
    break;
  case SOPHIA_WF_PROFILE_PREPARE:
  case SOPHIA_WF_PROFILE_ACTIVATE:
  case SOPHIA_WF_PROFILE_ROLLBACK:
  case SOPHIA_WF_PROFILE_PREPARED:
  case SOPHIA_WF_PROFILE_ACTIVE:
  case SOPHIA_WF_PROFILE_ROLLED_BACK:
    wf_put(p, r->value.profile.transaction, 8);
    wf_put(p + 8, r->value.profile.generation, 8);
    memcpy(p + 16, r->value.profile.digest, 32);
    if (k >= 256)
      wf_put(p + 48, r->value.profile.outcome, 2);
    break;
  case SOPHIA_WF_SNAPSHOT:
    wf_put(p, r->value.snapshot.transaction, 8);
    wf_put(p + 8, r->value.snapshot.scene_generation, 8);
    wf_put(p + 16, r->value.snapshot.active_output, 8);
    wf_put(p + 24, r->section_count, 2);
    break;
  case SOPHIA_WF_PROJECTION:
    wf_put(p, r->value.projection.transaction, 8);
    wf_put(p + 8, r->value.projection.request_id, 8);
    wf_put(p + 16, r->value.projection.base_generation, 8);
    wf_put(p + 24, r->value.projection.active_output, 8);
    wf_put(p + 32, r->section_count, 2);
    break;
  case SOPHIA_WF_CONFIGURATION: {
    const struct sophia_wf_configuration *v = &r->value.configuration;
    wf_put(p, v->transaction, 8);
    wf_put(p + 8, v->generation, 8);
    wf_put(p + 16, v->style_bits, 2);
    wf_put(p + 18, r->section_count, 2);
    wf_put(p + 20, v->focus_width, 4);
    wf_put(p + 24, v->focus_rgb, 4);
    wf_put(p + 28, v->frame_width, 4);
    wf_put(p + 32, v->frame_focused_rgb, 4);
    wf_put(p + 36, v->frame_unfocused_rgb, 4);
    break;
  }
  case SOPHIA_WF_CYCLE: {
    const struct sophia_wf_cycle *v = &r->value.cycle;
    int cause = cause_size(v->cause);
    if (cause < 0 || v->output_count > 16)
      return -1;
    wf_put(p, v->snapshot_transaction, 8);
    wf_put(p + 8, v->request_transaction, 8);
    wf_put(p + 16, v->request_id, 8);
    wf_put(p + 24, v->scene_generation, 8);
    wf_put(p + 32, v->policy_generation, 8);
    wf_put(p + 40, v->cause, 2);
    wf_put(p + 42, v->output_count, 2);
    for (i = 0; i < v->output_count; ++i)
      wf_put(p + 48 + i * 8, v->outputs[i], 8);
    n += (size_t)v->output_count * 8;
    cause_write(p + n, v);
    n += (size_t)cause;
    break;
  }
  case SOPHIA_WF_DIRTY:
    if (r->value.dirty.output_count > 16)
      return -1;
    wf_put(p, r->value.dirty.generation, 8);
    wf_put(p + 8, r->value.dirty.output_count, 2);
    for (i = 0; i < r->value.dirty.output_count; ++i)
      wf_put(p + 16 + i * 8, r->value.dirty.outputs[i], 8);
    n += (size_t)r->value.dirty.output_count * 8;
    break;
  case SOPHIA_WF_SESSION_OPERATION:
    wf_put(p, r->value.session_operation.transaction, 8);
    wf_put(p + 8, r->value.session_operation.request_id, 8);
    wf_put(p + 16, r->value.session_operation.operation, 8);
    put_surface(p + 24, r->value.session_operation.target);
    break;
  case SOPHIA_WF_CONFIGURATION_OUTCOME:
    wf_put(p, r->value.configuration_outcome.transaction, 8);
    wf_put(p + 8, r->value.configuration_outcome.generation, 8);
    wf_put(p + 16, r->value.configuration_outcome.outcome, 2);
    break;
  case SOPHIA_WF_SESSION_OPERATION_OUTCOME:
    wf_put(p, r->value.session_operation_outcome.transaction, 8);
    wf_put(p + 8, r->value.session_operation_outcome.request_id, 8);
    wf_put(p + 16, r->value.session_operation_outcome.outcome, 2);
    break;
  case SOPHIA_WF_PROJECTION_OUTCOME:
    wf_put(p, r->value.projection_outcome.transaction, 8);
    wf_put(p + 8, r->value.projection_outcome.request_id, 8);
    wf_put(p + 16, r->value.projection_outcome.scene_generation, 8);
    wf_put(p + 24, r->value.projection_outcome.outcome, 2);
    wf_put(p + 26, r->value.projection_outcome.expect_session_operation, 2);
    break;
  case SOPHIA_WF_PRESENTATION_RECEIPT:
    wf_put(p, r->value.presentation_receipt.transaction, 8);
    wf_put(p + 8, r->value.presentation_receipt.publication_generation, 8);
    wf_put(p + 16, r->value.presentation_receipt.output, 8);
    wf_put(p + 24, r->value.presentation_receipt.output_generation, 8);
    wf_put(p + 32, r->value.presentation_receipt.presentation_epoch, 8);
    wf_put(p + 40, r->value.presentation_receipt.outcome, 2);
    break;
  default:
    return -1;
  }
  *prefix = n;
  return 0;
}

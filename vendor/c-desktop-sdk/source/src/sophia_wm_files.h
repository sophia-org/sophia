#ifndef SOPHIA_WM_FILES_H
#define SOPHIA_WM_FILES_H
#include "sophia_wm_records.h"
#define SOPHIA_WF_HEADER_BYTES 32u
#define SOPHIA_WF_MAX_RECORD 1048576u
#define SOPHIA_WF_MAX_SECTIONS 32u
#define SOPHIA_WF_MAX_OUTPUTS 16u
#define SOPHIA_WF_SUBMIT_BYTES 24u
#define SOPHIA_WF_ACK_BYTES 16u

enum sophia_wf_kind {
  SOPHIA_WF_LIMITS = 1,
  SOPHIA_WF_SNAPSHOT = 2,
  SOPHIA_WF_NEGOTIATED = 16,
  SOPHIA_WF_SUBMITTED = 17,
  SOPHIA_WF_PROFILE_PREPARE = 18,
  SOPHIA_WF_PROFILE_ACTIVATE = 19,
  SOPHIA_WF_PROFILE_ROLLBACK = 20,
  SOPHIA_WF_CONFIGURATION_OUTCOME = 21,
  SOPHIA_WF_CYCLE = 22,
  SOPHIA_WF_PROJECTION_OUTCOME = 23,
  SOPHIA_WF_SESSION_OPERATION_OUTCOME = 24,
  SOPHIA_WF_PRESENTATION_RECEIPT = 25,
  SOPHIA_WF_NEGOTIATE = 256,
  SOPHIA_WF_PROFILE_PREPARED = 257,
  SOPHIA_WF_PROFILE_ACTIVE = 258,
  SOPHIA_WF_PROFILE_ROLLED_BACK = 259,
  SOPHIA_WF_CONFIGURATION = 260,
  SOPHIA_WF_DIRTY = 261,
  SOPHIA_WF_PROJECTION = 262,
  SOPHIA_WF_SESSION_OPERATION = 263
};
enum sophia_wf_cause {
  SOPHIA_WF_SCENE_CHANGED,
  SOPHIA_WF_ACTION,
  SOPHIA_WF_FOCUS,
  SOPHIA_WF_POINTER_FOCUS,
  SOPHIA_WF_INTERACTION,
  SOPHIA_WF_OUTPUT_ACTION,
  SOPHIA_WF_PRESENTATION_ACTION,
  SOPHIA_WF_ACTION_LIFECYCLE
};
/* ActionLifecycle phases and reasons. Held carries reason 0 only; Ended one
 * of the reasons below, and completed, aborted and timed out end only
 * sequence leaders. */
enum sophia_wf_lifecycle_phase {
  SOPHIA_WF_LIFECYCLE_HELD = 1,
  SOPHIA_WF_LIFECYCLE_ENDED = 2
};
enum sophia_wf_lifecycle_reason {
  SOPHIA_WF_LIFECYCLE_RELEASED = 1,
  SOPHIA_WF_LIFECYCLE_CANCELLED = 2,
  SOPHIA_WF_LIFECYCLE_COMPLETED = 3,
  SOPHIA_WF_LIFECYCLE_ABORTED = 4,
  SOPHIA_WF_LIFECYCLE_TIMED_OUT = 5
};
struct sophia_wf_header {
  uint16_t kind;
  uint64_t epoch, submission, sequence;
};
struct sophia_wf_surface {
  uint32_t index, generation;
};
struct sophia_wf_limits {
  uint64_t capability_ceiling;
  uint32_t max_object_bytes, max_journal_bytes;
  uint16_t max_journal_records, max_sections;
  uint32_t assembly_timeout_ms, send_timeout_ms;
  uint16_t profile_required;
};
struct sophia_wf_negotiate {
  uint64_t required, optional;
};
struct sophia_wf_profile {
  uint64_t transaction, generation;
  uint8_t digest[32];
  uint16_t outcome; /* Used only by completion candidates. */
};
struct sophia_wf_snapshot {
  uint64_t transaction, scene_generation, active_output;
};
struct sophia_wf_projection {
  uint64_t transaction, request_id, base_generation, active_output;
};
struct sophia_wf_configuration {
  uint64_t transaction, generation;
  uint16_t style_bits;
  uint32_t focus_width, focus_rgb, frame_width, frame_focused_rgb,
      frame_unfocused_rgb;
};
struct sophia_wf_action {
  uint64_t serial, action;
};
struct sophia_wf_pointer_focus {
  uint64_t output;
  struct sophia_wf_surface target;
};
struct sophia_wf_interaction {
  uint16_t phase, kind, axis;
  struct sophia_wf_surface target;
  int32_t x, y, width, height;
};
struct sophia_wf_output_action {
  uint64_t serial, action, output, output_generation;
};
struct sophia_wf_presentation_action {
  uint64_t serial, action, publication_generation, output, output_generation;
  uint64_t presentation_epoch, target_id, target_generation;
};
/* serial names the chord's first admitted Action; count its admitted Actions. */
struct sophia_wf_action_lifecycle {
  uint64_t serial, action;
  uint16_t phase, reason;
  uint32_t count;
};
struct sophia_wf_cycle {
  uint64_t snapshot_transaction, request_transaction, request_id;
  uint64_t scene_generation, policy_generation;
  uint16_t cause, output_count;
  uint64_t outputs[SOPHIA_WF_MAX_OUTPUTS];
  union {
    struct sophia_wf_action action;
    struct sophia_wf_surface focus;
    struct sophia_wf_pointer_focus pointer_focus;
    struct sophia_wf_interaction interaction;
    struct sophia_wf_output_action output_action;
    struct sophia_wf_presentation_action presentation_action;
    struct sophia_wf_action_lifecycle action_lifecycle;
  } value;
};
struct sophia_wf_dirty {
  uint64_t generation;
  uint16_t output_count;
  uint64_t outputs[16];
};
struct sophia_wf_session_operation {
  uint64_t transaction, request_id, operation;
  struct sophia_wf_surface target;
};
struct sophia_wf_configuration_outcome {
  uint64_t transaction, generation;
  uint16_t outcome;
};
struct sophia_wf_projection_outcome {
  uint64_t transaction, request_id, scene_generation;
  uint16_t outcome, expect_session_operation;
};
struct sophia_wf_session_operation_outcome {
  uint64_t transaction, request_id;
  uint16_t outcome;
};
struct sophia_wf_presentation_receipt {
  uint64_t transaction, publication_generation, output, output_generation,
      presentation_epoch;
  uint16_t outcome;
};
struct sophia_wf_submitted {
  uint64_t submission;
  uint16_t kind;
};
/* Sections borrow immutable, complete, fixed-row bytes. Use sophia_wf_* row
 * codecs to construct/read them. No socket framing is accepted. */
struct sophia_wf_section {
  uint16_t kind;
  uint32_t count;
  const uint8_t *rows;
  size_t bytes;
};
struct sophia_wf_record {
  struct sophia_wf_header header;
  uint16_t section_count;
  struct sophia_wf_section sections[SOPHIA_WF_MAX_SECTIONS];
  union {
    struct sophia_wf_limits limits;
    struct sophia_wf_negotiate negotiate;
    uint64_t selected_capabilities;
    struct sophia_wf_profile profile;
    struct sophia_wf_snapshot snapshot;
    struct sophia_wf_projection projection;
    struct sophia_wf_configuration configuration;
    struct sophia_wf_cycle cycle;
    struct sophia_wf_dirty dirty;
    struct sophia_wf_session_operation session_operation;
    struct sophia_wf_configuration_outcome configuration_outcome;
    struct sophia_wf_projection_outcome projection_outcome;
    struct sophia_wf_session_operation_outcome session_operation_outcome;
    struct sophia_wf_presentation_receipt presentation_receipt;
    struct sophia_wf_submitted submitted;
  } value;
};
/* Return 0 on success, -1 invalid record, -4 invalid arguments/capacity.
 * Outputs (including destination bytes) remain unchanged on error. Sections
 * borrow src until the caller releases that record. Encoding dst must not
 * overlap the value or its borrowed rows. No allocation or partial objects.
 * selected is the negotiated capability set, not the offered set. The codec
 * checks wire and selected-family rules; role phase, scene truth, application
 * admission and presentation are decisions of the server's owners. */
int sophia_wf_decode(const void *src, size_t bytes, uint64_t selected,
                     struct sophia_wf_record *out);
int sophia_wf_encode(void *dst, size_t capacity, uint64_t selected,
                     const struct sophia_wf_record *record, size_t *bytes);
int sophia_wf_submit_encode(uint8_t dst[24], uint64_t epoch, uint64_t id,
                            uint32_t bytes);
int sophia_wf_ack_encode(uint8_t dst[16], uint64_t epoch, uint64_t sequence);
#endif

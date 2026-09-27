#ifndef SOPHIA_SHELL_FILES_ROLES_H
#define SOPHIA_SHELL_FILES_ROLES_H
#include "sophia_shell_files_content.h"
#include <stddef.h>
#include <stdint.h>

/* Text and row views borrow the caller's immutable storage. Decoded views
 * remain valid only while the complete source record remains alive. */
struct sophia_sf_text {
    const uint8_t *data;
    uint16_t size;
};
struct sophia_sf_catalog_entry {
    uint16_t slot, available;
    struct sophia_sf_text label, keywords, identity;
};
struct sophia_sf_catalog {
    uint64_t transaction, connection_epoch, generation;
    uint16_t entry_count, identities_present;
    const uint8_t *rows;
    size_t rows_bytes;
};
struct sophia_sf_indicator_output_status {
    uint64_t output_id;
    uint16_t focus_bits;
    struct sophia_sf_text layout;
};
struct sophia_sf_indicator_entry {
    uint64_t output_id, indicator, action;
    uint32_t slot;
    uint16_t state_bits;
    struct sophia_sf_text label;
};
struct sophia_sf_indicators {
    uint64_t transaction, connection_epoch, generation, active_output_id;
    uint16_t active_output_present, status_count, indicator_count;
    const uint8_t *rows;
    size_t rows_bytes;
};
struct sophia_sf_native_opening {
    uint64_t transaction, grant_connection_epoch, grant_content_epoch, opening;
    uint64_t output_id, output_generation, catalog_generation, state_revision;
};
struct sophia_sf_native_binding {
    uint64_t transaction, grant_connection_epoch, grant_content_epoch, opening;
    uint64_t output_id, output_generation, allocation_id, allocation_generation;
    uint64_t catalog_generation, candidate_generation, presentation_epoch;
    /* This is binding_state_revision in records that also carry a newer
     * input state_revision. */
    uint64_t interaction_generation, state_revision, focus_lease;
};
struct sophia_sf_native_focus_revoked {
    struct sophia_sf_native_binding binding;
    uint16_t reason;
};
struct sophia_sf_native_input {
    struct sophia_sf_native_binding binding;
    uint64_t event_id, state_revision, issued_mono_usec;
    uint16_t kind;
    struct sophia_sf_text text;
};
struct sophia_sf_native_input_ack {
    struct sophia_sf_native_binding binding;
    uint64_t event_id, state_revision;
    uint16_t disposition;
};
struct sophia_sf_native_activate {
    struct sophia_sf_native_binding binding;
    uint64_t event_id, state_revision;
    uint16_t cause, slot;
};
struct sophia_sf_native_activation_outcome {
    struct sophia_sf_native_activate activation;
    uint16_t status, reason;
};
struct sophia_sf_native_closed {
    uint64_t transaction, grant_connection_epoch, grant_content_epoch, opening;
    uint16_t reason;
};
struct sophia_sf_native_allocation_request {
    uint64_t transaction, grant_connection_epoch, grant_content_epoch, opening;
    uint64_t output_id, output_generation, request_id, prior_id, prior_generation;
    uint16_t operation, edge;
    uint32_t desired_width, desired_height;
    int16_t margin_top, margin_right, margin_bottom, margin_left;
};
/* Native uses one role=3 surface and its displayed slots. Catalog uses
 * role=1 surfaces. The content arrays have their original fixed bounds. */
struct sophia_sf_role_candidate {
    struct sophia_sf_candidate content;
    uint64_t opening, catalog_generation, state_revision;
    uint16_t selected, row_count, rows[32];
};
struct sophia_sf_catalog_activate {
    struct sophia_sf_action action;
    uint64_t catalog_generation;
};
struct sophia_sf_catalog_activation_outcome {
    struct sophia_sf_catalog_activate activation;
    uint16_t status, reason;
};
struct sophia_sf_indicator_activate {
    uint64_t transaction, connection_epoch, snapshot_generation, output_id;
    uint64_t indicator, action, event_id;
};
struct sophia_sf_indicator_activation_outcome {
    uint64_t transaction, connection_epoch, snapshot_generation, event_id;
    uint16_t status, reason;
};

/* Build or inspect rows without allocating a full catalog array. Encode
 * destinations and decode outputs are unchanged on error. */
int sophia_sf_catalog_entry_encode(uint8_t dst[656], const struct sophia_sf_catalog_entry *);
int sophia_sf_catalog_entry_decode(const uint8_t src[656], struct sophia_sf_catalog_entry *);
int sophia_sf_catalog_entry_at(const struct sophia_sf_catalog *, size_t,
                               struct sophia_sf_catalog_entry *);
int sophia_sf_indicator_status_encode(uint8_t dst[46],
                                      const struct sophia_sf_indicator_output_status *);
int sophia_sf_indicator_status_decode(const uint8_t src[46],
                                      struct sophia_sf_indicator_output_status *);
int sophia_sf_indicator_entry_encode(uint8_t dst[66], const struct sophia_sf_indicator_entry *);
int sophia_sf_indicator_entry_decode(const uint8_t src[66], struct sophia_sf_indicator_entry *);
/* Candidate BODY codecs check field/row bounds without the family value
 * rules. This allows malformed-value test submissions. The normal record
 * encode/decode APIs also run validate_value, as production submit does.
 * Current catalog availability, focus and owner-only target/count rules
 * remain runtime decisions. Output values/destinations stay unchanged on error. */
int sophia_sf_role_candidate_encode_bytes(void *, size_t, uint16_t kind,
                                          const struct sophia_sf_role_candidate *, size_t *);
int sophia_sf_role_candidate_decode_bytes(uint16_t kind, const void *, size_t,
                                          struct sophia_sf_role_candidate *);
int sophia_sf_role_candidate_validate_value(uint16_t kind, const struct sophia_sf_role_candidate *);
#endif

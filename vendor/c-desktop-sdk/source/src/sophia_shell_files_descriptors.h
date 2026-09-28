#ifndef SOPHIA_SHELL_FILES_DESCRIPTORS_H
#define SOPHIA_SHELL_FILES_DESCRIPTORS_H
#include "sophia_shell_files_roles.h"

/* Descriptor values, pinned to spec/.
 * These passive records do not grant the descriptor role or implement a
 * session. Transactions are body identities, distinct from submission IDs.
 * Decoded text and row views borrow the immutable source record. */
struct sophia_sf_descriptor_outcome {
    uint64_t transaction, connection_epoch, candidate_generation, presentation_epoch;
    uint16_t kind;
};
struct sophia_sf_descriptor_activation {
    uint64_t transaction, connection_epoch, candidate_generation, presentation_epoch;
    uint64_t activation, action_token, action_issuer_epoch, action_issuer_revocation_epoch;
    uint64_t action_recipient_epoch;
    uint16_t action_target_slot;
    uint64_t action_target_generation;
};
struct sophia_sf_reference_request {
    uint64_t transaction, connection_epoch, catalog_generation, request_generation;
    uint64_t output_id, output_generation, presentation_epoch;
    uint16_t operation;
};
struct sophia_sf_reference_outcome {
    uint64_t transaction, connection_epoch, catalog_generation, request_generation;
    uint64_t candidate_generation, presentation_epoch;
    uint16_t page, pages, kind;
};
struct sophia_sf_descriptor_launcher_request {
    struct sophia_sf_reference_request request;
    struct sophia_sf_text query;
};
struct sophia_sf_descriptor_launcher_outcome {
    uint64_t transaction, connection_epoch, request_generation, candidate_generation;
    uint64_t presentation_epoch;
    uint16_t kind;
};
struct sophia_sf_descriptor_launcher_activation {
    uint64_t transaction, connection_epoch, catalog_generation, request_generation;
    uint64_t candidate_generation, presentation_epoch, activation;
    uint16_t slot;
};
struct sophia_sf_descriptor_launch_outcome {
    struct sophia_sf_descriptor_launcher_activation grant;
    uint16_t status;
};
struct sophia_sf_descriptor_activation_ack {
    uint64_t transaction, connection_epoch, activation;
    uint16_t disposition;
};
struct sophia_sf_descriptor_launcher_activation_ack {
    struct sophia_sf_descriptor_launcher_activation grant;
    uint16_t consumed;
};
struct sophia_sf_descriptor_candidate_entry {
    uint16_t slot;
    uint64_t generation;
};
struct sophia_sf_descriptor_candidate {
    uint64_t transaction, connection_epoch, snapshot_generation, candidate_generation, output_id;
    uint16_t visible, reservation_edge, reservation_thickness, selected_slot, entry_count;
    struct sophia_sf_descriptor_candidate_entry entries[16];
};
struct sophia_sf_tabs_candidate {
    uint64_t transaction, connection_epoch, snapshot_generation, candidate_generation;
    uint16_t group_count;
    /* group_count consecutive native u64 rows, encoded with tab_order_encode. */
    const uint8_t *rows;
    size_t rows_bytes;
};
struct sophia_sf_reference_style {
    uint16_t body_size, title_size, padding, row_gap, key_gap, column_gap, border, margin, columns;
    /* Background, border, key text, label text, key background, title text. */
    uint32_t colors[6];
    struct sophia_sf_text title;
};
struct sophia_sf_reference_entry {
    uint16_t slot;
    struct sophia_sf_text key, label;
};
struct sophia_sf_reference_candidate {
    uint64_t transaction, connection_epoch, catalog_generation, request_generation;
    uint64_t candidate_generation, output_id;
    uint16_t visible, page, entry_count;
    struct sophia_sf_reference_style style;
    const uint8_t *rows;
    size_t rows_bytes;
};
struct sophia_sf_descriptor_launcher_candidate {
    uint64_t transaction, connection_epoch, catalog_generation, request_generation;
    uint64_t candidate_generation, output_id;
    uint16_t visible, selected, entry_count, font_size;
    /* Background, foreground, selection background, selection foreground. */
    uint32_t colors[4];
    uint16_t entries[32];
};
/* Row helpers use the same unchanged-output-on-error convention as the
 * complete record codec. A row must have its full declared byte capacity.
 * _at checks the view's bounds and selected row, not whole-view uniqueness. */
int sophia_sf_tab_order_encode(uint8_t dst[8], uint64_t group_slot);
int sophia_sf_tab_order_at(const struct sophia_sf_tabs_candidate *, size_t, uint64_t *);
int sophia_sf_reference_entry_encode(uint8_t dst[204], const struct sophia_sf_reference_entry *);
int sophia_sf_reference_entry_decode(const uint8_t src[204], struct sophia_sf_reference_entry *);
int sophia_sf_reference_entry_at(const struct sophia_sf_reference_candidate *, size_t,
                                 struct sophia_sf_reference_entry *);

struct sophia_sf_descriptor_entry {
    uint16_t slot, trust_level, attention, label_present, label_redacted;
    uint64_t generation, action_token, action_issuer_epoch, action_issuer_revocation_epoch;
    uint64_t action_recipient_epoch;
    uint16_t action_target_slot;
    uint64_t action_target_generation;
    struct sophia_sf_text label;
};
struct sophia_sf_descriptors {
    uint64_t transaction, connection_epoch, snapshot_generation, output_id, output_generation;
    uint64_t broker_epoch, broker_revocation_epoch;
    uint16_t descriptor_count;
    const uint8_t *rows;
    size_t rows_bytes;
};
struct sophia_sf_tab_group {
    uint64_t group_slot, output_id;
    uint16_t selected_slot, focused, entry_count;
};
struct sophia_sf_tabs {
    uint64_t transaction, connection_epoch, generation;
    uint16_t group_count, entry_count;
    /* All 24-byte group rows, followed by all 196-byte descriptor rows.
     * Each group's entry_count partitions that one descriptor array in order. */
    const uint8_t *rows;
    size_t rows_bytes;
};
struct sophia_sf_shortcut_entry {
    uint16_t slot, label_present, group_present;
    struct sophia_sf_text chord, action, label, group;
};
struct sophia_sf_shortcuts {
    uint64_t transaction, connection_epoch, generation;
    uint16_t entry_count;
    const uint8_t *rows;
    size_t rows_bytes;
};
int sophia_sf_descriptor_entry_encode(uint8_t dst[196], const struct sophia_sf_descriptor_entry *);
int sophia_sf_descriptor_entry_decode(const uint8_t src[196], struct sophia_sf_descriptor_entry *);
int sophia_sf_descriptor_entry_at(const struct sophia_sf_descriptors *, size_t,
                                  struct sophia_sf_descriptor_entry *);
int sophia_sf_tab_group_encode(uint8_t dst[24], const struct sophia_sf_tab_group *);
int sophia_sf_tab_group_decode(const uint8_t src[24], struct sophia_sf_tab_group *);
int sophia_sf_tab_group_at(const struct sophia_sf_tabs *, size_t, struct sophia_sf_tab_group *);
/* Descriptor index is global across all groups. */
int sophia_sf_tab_entry_at(const struct sophia_sf_tabs *, size_t,
                           struct sophia_sf_descriptor_entry *);
int sophia_sf_shortcut_entry_encode(uint8_t dst[408], const struct sophia_sf_shortcut_entry *);
int sophia_sf_shortcut_entry_decode(const uint8_t src[408], struct sophia_sf_shortcut_entry *);
int sophia_sf_shortcut_entry_at(const struct sophia_sf_shortcuts *, size_t,
                                struct sophia_sf_shortcut_entry *);
#endif

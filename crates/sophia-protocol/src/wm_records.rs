//! Complete WM record sections and the semantic codecs for their rows.
//!
//! Every WM transport carries the same passive rows: scene snapshots,
//! projection proposals, configuration and their capability-gated extensions.
//! This owner encodes, bounds and validates those rows as complete sections.
//! It has no socket frames, chunk ordinals, Begin/End assembly, file
//! envelopes, queues or publication state; the legacy socket adapters under
//! `crate::ipc` and the file codec in `crate::wm_files` both call it.
//!
//! Raw generated rows and constants are still reached through their
//! root-exported names until `crate::wm_rows` owns them.
mod configuration;
mod launch_origins;
mod output_launch_contexts;
mod output_policy_keys;
mod presentation;
mod projection;
mod sections;
mod snapshot;
mod tab_groups;
mod translation;
mod values;

pub use configuration::{decode_policy_configuration_records, encode_policy_configuration_records};
pub use launch_origins::{
    LAUNCH_CONTEXT_RECORD_LEN, PROJECTION_LAUNCH_CONTEXT_RECORD_KIND,
    SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND, decode_policy_launch_contexts_records,
    decode_wm_launch_context_records, encode_policy_launch_contexts_records,
    encode_wm_launch_context_records,
};
pub use output_launch_contexts::{
    OUTPUT_LAUNCH_CONTEXT_RECORD_LEN, PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND,
    decode_policy_output_launch_contexts_records, encode_policy_output_launch_contexts_records,
};
pub use output_policy_keys::{
    SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND, apply_policy_output_key_records,
    encode_policy_output_key_records,
};
pub use presentation::{
    PROJECTION_PRESENTATION_BINDING_RECORD_KIND, PROJECTION_PRESENTATION_OUTPUT_RECORD_KIND,
    PROJECTION_PRESENTATION_RECORD_KIND, PROJECTION_PRESENTATION_REGION_RECORD_KIND,
    PROJECTION_SURFACE_INSTANCE_RECORD_KIND, decode_policy_presentation_records,
    encode_policy_presentation_records, wm_presentation_record_layout,
};
pub use projection::{decode_policy_projection_records, encode_policy_projection_records};
pub use sections::{
    PolicyConfigurationMetadata, PolicyDecodedSnapshot, PolicyProjectionMetadata,
    PolicyRecordContext, PolicyRecordSection, PolicyRecordSectionRef, PolicySnapshotMetadata,
    coalesce_policy_record_sections, policy_record_layout, validate_policy_record_sections,
};
pub use snapshot::{
    POLICY_SURFACE_CAPABILITY_CLOSABLE, POLICY_SURFACE_CAPABILITY_FOCUSABLE,
    POLICY_SURFACE_CAPABILITY_FULLSCREENABLE, POLICY_SURFACE_CAPABILITY_MOVABLE,
    POLICY_SURFACE_CAPABILITY_RESIZABLE, SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND,
    WmV1SnapshotSurfaceClassificationRecord, decode_policy_snapshot_records,
    decode_wm_v1_snapshot_surface_classification_records, encode_policy_snapshot_records,
    encode_wm_v1_snapshot_surface_classification_records,
};
pub use tab_groups::{
    PROJECTION_TAB_GROUP_RECORD_KIND, PROJECTION_TAB_GROUP_RECORD_LEN,
    PROJECTION_TAB_MEMBER_RECORD_KIND, PROJECTION_TAB_MEMBER_RECORD_LEN,
    decode_policy_tab_groups_records, encode_policy_tab_groups_records,
};
pub use translation::{
    PROJECTION_TRANSLATION_GROUP_RECORD_KIND, PROJECTION_TRANSLATION_GROUP_RECORD_LEN,
    PROJECTION_TRANSLATION_MEMBER_RECORD_KIND, PROJECTION_TRANSLATION_MEMBER_RECORD_LEN,
    decode_policy_translation_groups_records, encode_policy_translation_groups_records,
};

// The legacy socket adapters decode the same rows but add their envelope's
// declared counts and scalar messages. They reach these pieces, not copies.
pub(crate) use configuration::{decode_policy_action_rows, validate_policy_configuration};
pub(crate) use projection::decode_projection_sections;
pub(crate) use snapshot::decode_snapshot_sections;
pub(crate) use values::{decode_optional_surface, require_count};

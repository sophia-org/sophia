mod broker;
mod broker_v1;
mod control_v1;
mod cursor;
mod frame;
mod output_v1;
mod portal;
mod primitives;
mod shell_tabs;
mod shell_v1;
mod types;
mod wm_record_sections;
mod wm_v1;
mod wm_v1_profile;
mod wm_v1_records;

pub use broker::{decode_broker_health_frame, encode_broker_health_frame};
pub use broker_v1::*;
pub use control_v1::*;
pub use frame::{decode_frame, encode_frame};
pub use output_v1::*;
// Socket adapters use the same strict semantics as file codecs. Their
// historical exceptions stay in the adapter, never in these validators.
use crate::policy_scalars::*;
use crate::wm_rows::*;
pub use portal::{
    decode_portal_broker_request_frame, decode_portal_broker_response_frame,
    decode_portal_clipboard_payload_frame, encode_portal_broker_request_frame,
    encode_portal_broker_response_frame, encode_portal_clipboard_payload_frame,
};
pub use shell_tabs::*;
pub use shell_v1::*;
pub use types::*;
pub use wm_v1::*;
pub use wm_v1_profile::*;
pub use wm_v1_records::*;
// Legacy chunk adapters around the extension rows in `crate::wm_records`.
pub use wm_record_sections::{
    append_wm_launch_origins, decode_wm_launch_contexts, decode_wm_output_launch_contexts,
    decode_wm_presentation, decode_wm_tab_groups, decode_wm_translation_groups,
    encode_wm_launch_contexts, encode_wm_output_launch_contexts, encode_wm_presentation,
    encode_wm_tab_groups, encode_wm_translation_groups,
};

mod shell_indicators;
pub use shell_indicators::*;

mod shell_content;
pub use shell_content::*;

mod shell_reference;
pub use shell_reference::*;

mod shell_launcher;
pub use shell_launcher::*;

mod wm_output_actions;
pub use wm_output_actions::*;

mod wm_presentation_actions;
pub use wm_presentation_actions::*;

mod shell_native_launcher;
pub use shell_native_launcher::*;

mod shell_catalog_actions;
pub use shell_catalog_actions::*;

mod shell_catalog_transaction;
pub use shell_catalog_transaction::*;

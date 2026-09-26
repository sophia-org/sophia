use super::*;
use std::fmt::Write;

/// Stable JSON text, without Debug, locale, raw WM fields or terminal escapes.
/// The exact allowlisted names and enum spellings are shared with the server.
pub fn format_inspection_snapshot(
    value: &InspectionSnapshotRecord,
) -> Result<String, InspectionRecordError> {
    String::from_utf8(encode_inspection_snapshot(value)?).map_err(|_| InspectionRecordError::Json)
}
pub fn format_inspection_status(value: &InspectionStatus) -> Result<String, InspectionRecordError> {
    String::from_utf8(encode_inspection_status(value)?).map_err(|_| InspectionRecordError::Json)
}
pub fn format_inspection_event(
    value: &InspectionEventRecord,
) -> Result<String, InspectionRecordError> {
    String::from_utf8(encode_inspection_event(value)?).map_err(|_| InspectionRecordError::Json)
}

pub fn inspection_api_text() -> String {
    let mut text = String::from(
        "sophia_wm_inspection_v1\nschema=1\nauthority=host_read_only\nu64=decimal_json_string\n",
    );
    let _ = writeln!(text, "snapshot_bytes={INSPECTION_MAX_SNAPSHOT_BYTES}");
    let _ = writeln!(text, "event_records={INSPECTION_MAX_EVENTS}");
    let _ = writeln!(text, "event_bytes={INSPECTION_MAX_RING_BYTES}");
    text.push_str("events=owner_reports_not_physical_completion\ngap=ESTALE_resnapshot\n");
    text
}

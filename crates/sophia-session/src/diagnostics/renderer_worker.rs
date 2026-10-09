//! Rare renderer-worker and present-deferral evidence, reduced before any other rule.
//!
//! These records are judged whole rather than field by field: which fields a
//! record may carry depends on its status, so a field is admitted only after
//! the status it belongs to is known. Every admitted value is a bounded
//! integer, a boolean, a fixed status word or a compiler-owned detail name.
//! A record whose status is missing or not one of its producer's rare
//! statuses is dropped whole, so request chatter cannot ride this route. In an
//! admitted record anything else -- a repeated key, a private field, a Debug
//! rendering that is not a variant name -- is dropped, never copied.
use sophia_renderer_live::LiveRendererScanoutBufferExportDetail as Detail;

const WORKER: &str = "sophia_renderer_worker";
const PRESENT_DEFER: &str = "sophia_live_present_defer";
/// More fields than any producer prints; the rest are not read.
const FIELD_LIMIT: usize = 16;

pub(super) fn record(name: &str) -> bool {
    matches!(name, WORKER | PRESENT_DEFER)
}

pub(super) fn reduce<'a>(name: &str, fields: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut pairs: Vec<(&str, &str, &str)> = Vec::with_capacity(FIELD_LIMIT);
    for field in fields.take(FIELD_LIMIT) {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        if pairs.iter().all(|(seen, _, _)| *seen != key) {
            pairs.push((key, value, field));
        }
    }
    let status = pairs
        .iter()
        .find(|(key, _, _)| *key == "status")
        .map(|(_, value, _)| *value)
        .filter(|value| status_allowed(name, value))?;
    let mut result = name.to_owned();
    for (key, value, field) in pairs {
        if field_allowed(name, status, key, value) {
            result.push(' ');
            result.push_str(field);
        }
    }
    Some(result)
}

fn status_allowed(name: &str, value: &str) -> bool {
    match name {
        WORKER => matches!(
            value,
            "soft_stall" | "hard_stall" | "stall_recovered" | "failed" | "result_misrouted"
        ),
        PRESENT_DEFER => value == "output_busy",
        _ => false,
    }
}

fn field_allowed(name: &str, status: &str, key: &str, value: &str) -> bool {
    match (name, key) {
        (WORKER, "schema") => value == "3",
        (PRESENT_DEFER, "schema") => value == "1",
        (_, "status") => value == status,
        (WORKER, _) => worker_field(status, key, value),
        (PRESENT_DEFER, _) => present_defer_field(key, value),
        _ => false,
    }
}

fn worker_field(status: &str, key: &str, value: &str) -> bool {
    match key {
        "output" | "request" => integer(value),
        "age_ms" => {
            matches!(status, "soft_stall" | "hard_stall" | "stall_recovered") && integer(value)
        }
        "abandon_after_ms" => status == "hard_stall" && integer(value),
        "expected" | "observed" => status == "result_misrouted" && integer(value),
        "detail" => status == "failed" && detail_name(value),
        _ => false,
    }
}

fn present_defer_field(key: &str, value: &str) -> bool {
    match key {
        "defers" | "transaction" | "output" => integer(value),
        "in_flight" | "cleanup_pending" | "pending_frame" => matches!(value, "true" | "false"),
        _ => false,
    }
}

fn integer(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

/// Lists every detail variant exactly once. The exhaustive match fails to
/// compile when the renderer adds a variant this list does not name, so the
/// admitted vocabulary cannot drift from the enum the producer formats. The
/// enum's derived Debug prints a unit variant's name, which `stringify!` gives.
macro_rules! details {
    ($($variant:ident),* $(,)?) => {
        const DETAIL_NAMES: &[&str] = &[$(stringify!($variant)),*];

        #[allow(dead_code)]
        fn listed(detail: &Detail) {
            match detail {
                $(Detail::$variant)|* => {}
            }
        }
    };
}

details!(
    Exported,
    WorkerPending,
    WorkerQueueFull,
    WorkerDisconnected,
    WorkerStalled,
    InvalidTarget,
    ComposeRefused,
    BackendDeviceUnavailable,
    GbmDeviceUnavailable,
    EglUnavailable,
    EglDisplayUnavailable,
    EglInitializeFailed,
    EglBindApiFailed,
    EglConfigUnavailable,
    GbmSurfaceUnavailable,
    EglSurfaceUnavailable,
    EglContextUnavailable,
    EglMakeCurrentFailed,
    GlSmokeFailed,
    CpuLayerUploadFailed,
    DmaBufImageCreateFailed,
    DmaBufImageBindFailed,
    CompositionDrawFailed,
    CompositionFinishFailed,
    EglImageDestroyFailed,
    DmaBufImportFailed,
    EglSwapBuffersFailed,
    FrontBufferLockFailed,
    InvalidBufferDescriptor,
    InvalidRendererImageId,
    DmaBufDescriptorMismatch,
    DmaBufImportCacheFull,
    RendererImageStoreFull,
    PendingFrameMissing,
    ExportedDescriptorMissing,
    ExportedOwnerMissing,
    WorkerLeaseIdExhausted,
    FrameSlotIncarnationExhausted,
    RetainedBufferMissing,
    RendererImageTransferBusy,
);

/// Whether `value` is exactly the Debug name of a detail variant.
fn detail_name(value: &str) -> bool {
    DETAIL_NAMES.contains(&value)
}

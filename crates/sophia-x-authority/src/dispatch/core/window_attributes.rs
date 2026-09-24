// The window attributes X refuses (t216), judged before any is applied.
// Included by dispatch.rs beside the other core families.

/// The attributes a CreateWindow or ChangeWindowAttributes names that can
/// be refused.
#[derive(Clone, Copy, Default)]
struct XRefusableAttributes {
    background_pixmap: Option<crate::XWindowBackground>,
    background_pixel: bool,
    border_pixmap: Option<u32>,
    border_pixel: bool,
}

/// The error X answers for these attributes on a window of `depth` under
/// `parent`, if any: dix's order, the InputOnly refusal first, then each
/// pixmap by bit.
fn refused_window_attributes(
    runtime: &XAuthorityRuntime,
    namespace: NamespaceId,
    input_only: bool,
    depth: u8,
    parent: XResourceId,
    attributes: XRefusableAttributes,
) -> Option<(XErrorCode, u32)> {
    if input_only
        && (attributes.background_pixmap.is_some()
            || attributes.background_pixel
            || attributes.border_pixmap.is_some()
            || attributes.border_pixel)
    {
        return Some((XErrorCode::BadMatch, 0));
    }
    let parent_depth = runtime.window_visual(parent).0;
    let pixmap = |raw: u32| {
        let pixmap = XResourceId::new(u64::from(raw), 1);
        match runtime.pixmap_depth(namespace, pixmap) {
            Err(_) => Some((XErrorCode::BadPixmap, raw)),
            Ok(pixmap_depth) if pixmap_depth != depth => Some((XErrorCode::BadMatch, raw)),
            Ok(_) => None,
        }
    };
    match attributes.background_pixmap {
        Some(crate::XWindowBackground::ParentRelative) if parent_depth != depth => {
            return Some((XErrorCode::BadMatch, 1));
        }
        Some(crate::XWindowBackground::Pixmap(id)) => {
            if let Some(error) = pixmap(u32::try_from(id.local.raw()).unwrap_or(0)) {
                return Some(error);
            }
        }
        _ => {}
    }
    match attributes.border_pixmap {
        Some(0) if parent_depth != depth => Some((XErrorCode::BadMatch, 0)),
        Some(0) | None => None,
        Some(raw) => pixmap(raw),
    }
}

/// The refusal as the request's error record.
fn window_attribute_refusal(context: XDispatchContext, (code, resource_id): (XErrorCode, u32)) -> XDispatchResult {
    XDispatchResult {
        response: None,
        outputs: vec![XClientOutput::Error(crate::XClientError {
            code,
            sequence: context.sequence,
            resource_id,
            minor_code: 0,
            major_code: context.major_opcode,
        })],
        metadata_candidates: Vec::new(),
    }
}

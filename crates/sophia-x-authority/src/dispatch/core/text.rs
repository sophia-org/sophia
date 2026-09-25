// PolyText8/16 and ImageText8/16: the text items a request carries, drawn
// through the runtime one item at a time.
// Included by dispatch.rs; one module with it.

fn dispatch_text_draw(
    context: XDispatchContext,
    runtime: &mut XAuthorityRuntime,
    drawable: XResourceId,
    gc: XResourceId,
    mut draw: XTextDraw<'_>,
) -> XDispatchResult {
    let transaction = context.transaction;
    if let Err(error) = runtime.validate_drawable_access(context.namespace, drawable) {
        return core_draw_validation_error(
            context,
            transaction,
            error,
            XErrorCode::BadDrawable,
            drawable,
        );
    }
    let (gc_depth, gc_values, font) =
        match runtime.graphics_context_depth_values_and_font(context.namespace, gc) {
            Ok(record) => record,
            Err(error) => {
                return core_draw_validation_error(
                    context,
                    transaction,
                    error,
                    XErrorCode::BadGraphicsContext,
                    gc,
                );
            }
        };
    if runtime.drawable_depth(context.namespace, drawable) != Ok(gc_depth) {
        return core_draw_validation_error(
            context,
            transaction,
            XAuthorityRuntimeError::InvalidSurface,
            XErrorCode::BadMatch,
            drawable,
        );
    }
    draw.font = font;
    let response = runtime.draw_through_inferiors(
        transaction,
        context.namespace,
        drawable,
        gc_values.subwindow_mode,
        |runtime| {
            runtime.apply_text_draw(
                transaction,
                context.namespace,
                drawable,
                &[draw],
                &gc_values,
            )
        },
    );
    let outputs = if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
        vec![XClientOutput::Error(x_error_from_runtime(
            error,
            context.sequence,
            context.major_opcode,
            0,
            u32::try_from(drawable.local.raw()).unwrap_or(0),
        ))]
    } else {
        Vec::new()
    };
    XDispatchResult {
        response: Some(response),
        outputs,
        metadata_candidates: Vec::new(),
    }
}

fn dispatch_poly_text(
    context: XDispatchContext,
    runtime: &mut XAuthorityRuntime,
    drawable: XResourceId,
    gc: XResourceId,
    x: i16,
    baseline: i16,
    items: &[XPolyTextItem],
) -> XDispatchResult {
    let transaction = context.transaction;
    if let Err(error) = runtime.validate_drawable_access(context.namespace, drawable) {
        return core_draw_validation_error(
            context,
            transaction,
            error,
            XErrorCode::BadDrawable,
            drawable,
        );
    }
    let (gc_depth, gc_values, mut font) =
        match runtime.graphics_context_depth_values_and_font(context.namespace, gc) {
            Ok(record) => record,
            Err(error) => {
                return core_draw_validation_error(
                    context,
                    transaction,
                    error,
                    XErrorCode::BadGraphicsContext,
                    gc,
                );
            }
        };
    if runtime.drawable_depth(context.namespace, drawable) != Ok(gc_depth) {
        return core_draw_validation_error(
            context,
            transaction,
            XAuthorityRuntimeError::InvalidSurface,
            XErrorCode::BadMatch,
            drawable,
        );
    }

    let mut current_x = i32::from(x);
    let mut draws = Vec::new();
    let mut font_error = None;
    for item in items {
        match item {
            XPolyTextItem::Text { delta, chars } => {
                current_x = current_x.saturating_add(i32::from(*delta));
                let advance = font.metrics.text_extents(chars).overall_width;
                draws.push(XTextDraw {
                    x: current_x,
                    baseline: i32::from(baseline),
                    text: chars,
                    image: false,
                    font: font.clone(),
                });
                current_x = current_x.saturating_add(advance);
            }
            XPolyTextItem::Font { font: requested } => {
                match runtime.font_face(context.namespace, *requested) {
                    Ok(resolved) => font = resolved,
                    Err(error) => {
                        let code = if matches!(
                            error,
                            XAuthorityRuntimeError::InvalidNamespace
                                | XAuthorityRuntimeError::CrossNamespaceDenied
                        ) {
                            XErrorCode::BadAccess
                        } else {
                            XErrorCode::BadFont
                        };
                        font_error = Some(XClientOutput::Error(crate::XClientError {
                            code,
                            sequence: context.sequence,
                            resource_id: u32::try_from(requested.local.raw()).unwrap_or(0),
                            minor_code: 0,
                            major_code: context.major_opcode,
                        }));
                        break;
                    }
                }
            }
        }
    }

    let response = runtime.draw_through_inferiors(
        transaction,
        context.namespace,
        drawable,
        gc_values.subwindow_mode,
        |runtime| {
            runtime.apply_text_draw(transaction, context.namespace, drawable, &draws, &gc_values)
        },
    );
    let outputs = if let Some(error) = font_error {
        vec![error]
    } else if let XAuthorityResponseOutcome::Rejected(error) = response.outcome {
        vec![XClientOutput::Error(x_error_from_runtime(
            error,
            context.sequence,
            context.major_opcode,
            0,
            u32::try_from(drawable.local.raw()).unwrap_or(0),
        ))]
    } else {
        Vec::new()
    };
    XDispatchResult {
        response: Some(response),
        outputs,
        metadata_candidates: Vec::new(),
    }
}

fn projected_density_millis(transform: HeadLogicalTransform) -> u32 {
    let x = i64::from(transform.projected_scene.width.max(1)) * 1_000
        / i64::from(transform.source.width.max(1));
    let y = i64::from(transform.projected_scene.height.max(1)) * 1_000
        / i64::from(transform.source.height.max(1));
    u32::try_from(x.min(y).max(1)).unwrap_or(u32::MAX)
}

fn project_scene(source: Size, destination: Size, mapping: OutputHeadMapping) -> Rect {
    if source.width <= 0 || source.height <= 0 || destination.width <= 0 || destination.height <= 0
    {
        return Rect::default();
    }
    let (width, height) = match mapping {
        OutputHeadMapping::Exact => (source.width, source.height),
        OutputHeadMapping::Fit | OutputHeadMapping::Cover => {
            let by_width = i64::from(destination.width) * i64::from(source.height);
            let by_height = i64::from(destination.height) * i64::from(source.width);
            let use_width = if mapping == OutputHeadMapping::Fit {
                by_width <= by_height
            } else {
                by_width >= by_height
            };
            if use_width {
                (
                    destination.width,
                    i32::try_from(
                        i64::from(destination.width) * i64::from(source.height)
                            / i64::from(source.width),
                    )
                    .unwrap_or(i32::MAX),
                )
            } else {
                (
                    i32::try_from(
                        i64::from(destination.height) * i64::from(source.width)
                            / i64::from(source.height),
                    )
                    .unwrap_or(i32::MAX),
                    destination.height,
                )
            }
        }
    };
    Rect {
        x: (destination.width - width) / 2,
        y: (destination.height - height) / 2,
        width,
        height,
    }
}

/// [`project_child_rect`] rounding outward: the left and top edges floor,
/// the right and bottom edges ceil.
fn project_child_rect_outward(child: Rect, source: Size, projected: Rect) -> Rect {
    project_child_rect_rounded(child, source, projected, true)
}

/// Rounding inward, the opposite: what a policy border's inner edge uses, so
/// its ring keeps at least one native pixel wherever it had a logical one.
fn project_child_rect_inward(child: Rect, source: Size, projected: Rect) -> Rect {
    project_child_rect_rounded(child, source, projected, false)
}

fn project_child_rect_rounded(child: Rect, source: Size, projected: Rect, outward: bool) -> Rect {
    if source.width <= 0 || source.height <= 0 || projected.is_empty() || child.is_empty() {
        return Rect::default();
    }
    let edge = |value: i32, source_extent: i32, origin: i32, extent: i32, up: bool| {
        let scaled = i64::from(value) * i64::from(extent);
        let source_extent = i64::from(source_extent);
        let offset = if up {
            -(-scaled).div_euclid(source_extent)
        } else {
            scaled.div_euclid(source_extent)
        };
        (i64::from(origin) + offset).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    };
    let left = edge(child.x, source.width, projected.x, projected.width, !outward);
    let right = edge(
        child.x.saturating_add(child.width),
        source.width,
        projected.x,
        projected.width,
        outward,
    );
    let top = edge(child.y, source.height, projected.y, projected.height, !outward);
    let bottom = edge(
        child.y.saturating_add(child.height),
        source.height,
        projected.y,
        projected.height,
        outward,
    );
    Rect {
        x: left,
        y: top,
        width: right.saturating_sub(left).max(0),
        height: bottom.saturating_sub(top).max(0),
    }
}

fn project_child_rect(child: Rect, source: Size, projected: Rect) -> Rect {
    if source.width <= 0 || source.height <= 0 || projected.is_empty() {
        return Rect::default();
    }
    let edge = |value: i32, source_extent: i32, origin: i32, extent: i32| {
        let projected =
            i64::from(origin) + i64::from(value) * i64::from(extent) / i64::from(source_extent);
        projected.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    };
    let left = edge(child.x, source.width, projected.x, projected.width);
    let right = edge(
        child.x.saturating_add(child.width),
        source.width,
        projected.x,
        projected.width,
    );
    let top = edge(child.y, source.height, projected.y, projected.height);
    let bottom = edge(
        child.y.saturating_add(child.height),
        source.height,
        projected.y,
        projected.height,
    );
    Rect {
        x: left.min(right),
        y: top.min(bottom),
        width: right.saturating_sub(left).abs(),
        height: bottom.saturating_sub(top).abs(),
    }
}

fn clip_to_target(rect: Rect, target: Size) -> Rect {
    let left = rect.x.max(0).min(target.width);
    let top = rect.y.max(0).min(target.height);
    let right = rect.x.saturating_add(rect.width).max(0).min(target.width);
    let bottom = rect.y.saturating_add(rect.height).max(0).min(target.height);
    Rect {
        x: left,
        y: top,
        width: right.saturating_sub(left),
        height: bottom.saturating_sub(top),
    }
}

fn background_commands(scene: Rect, target: Size) -> Vec<HeadCompositorCommand> {
    let clipped = clip_to_target(scene, target);
    let black = CompositorRgb8 {
        red: 0,
        green: 0,
        blue: 0,
    };
    let mut commands = Vec::with_capacity(4);
    for geometry in [
        Rect {
            x: 0,
            y: 0,
            width: target.width,
            height: clipped.y,
        },
        Rect {
            x: 0,
            y: clipped.y.saturating_add(clipped.height),
            width: target.width,
            height: target
                .height
                .saturating_sub(clipped.y.saturating_add(clipped.height)),
        },
        Rect {
            x: 0,
            y: clipped.y,
            width: clipped.x,
            height: clipped.height,
        },
        Rect {
            x: clipped.x.saturating_add(clipped.width),
            y: clipped.y,
            width: target
                .width
                .saturating_sub(clipped.x.saturating_add(clipped.width)),
            height: clipped.height,
        },
    ] {
        if !geometry.is_empty() {
            commands.push(HeadCompositorCommand::Background(CompositorSolidRect {
                geometry,
                color: black,
            }));
        }
    }
    commands
}

/// Whether a projected border paints any pixel inside its clip.
fn policy_border_draws(border: &HeadCompositorBorder) -> bool {
    crate::compositor_border_bands(CompositorBorder {
        node: border.node,
        generation: border.generation,
        outer: border.outer,
        inner: border.inner,
        color: border.color,
    })
    .iter()
    .any(|band| !intersect_rect(band.geometry, border.clip).is_empty())
}

fn project_border(
    border: CompositorBorder,
    viewport: Rect,
    transform: HeadLogicalTransform,
    clip: Rect,
) -> HeadCompositorBorder {
    // A WM region's stroke is a policy target: its outer edge rounds outward
    // and its inner edge inward, so a ring with a logical pixel keeps a
    // native one and the region is never listed without a drawn pixel.
    let policy = matches!(border.node, CompositorNodeId::PolicyRegion { .. });
    let (outer, inner) = if policy {
        transform.project_local_policy_border(localize(viewport, border.outer), localize(viewport, border.inner))
    } else {
        (
            transform.project_root_rect(viewport, border.outer),
            transform.project_root_rect(viewport, border.inner),
        )
    };
    HeadCompositorBorder {
        node: border.node,
        generation: border.generation,
        outer,
        inner,
        color: border.color,
        clip,
    }
}

fn project_rect(
    rect: CompositorRect,
    viewport: Rect,
    transform: HeadLogicalTransform,
    clip: Rect,
) -> HeadCompositorRect {
    HeadCompositorRect {
        opacity: rect.opacity,
        node: rect.node,
        generation: rect.generation,
        // A WM region is a policy target: it rounds outward like an instance,
        // so it is never rounded out of the frame that retires. Other
        // compositor rects keep their established projection.
        geometry: intersect_rect(
            if matches!(rect.node, CompositorNodeId::PolicyRegion { .. }) {
                transform.project_root_rect_outward(viewport, rect.geometry)
            } else {
                transform.project_root_rect(viewport, rect.geometry)
            },
            clip,
        ),
        color: rect.color,
    }
}

fn project_text(
    text: &CompositorText,
    viewport: Rect,
    transform: HeadLogicalTransform,
    clip: Rect,
) -> HeadCompositorText {
    let density = projected_density_millis(transform);
    HeadCompositorText {
        node: text.node,
        generation: text.generation,
        geometry: intersect_rect(transform.project_root_rect(viewport, text.geometry), clip),
        text: text.text.clone(),
        font_size_millis: u32::try_from(
            u64::from(text.font_size_millis)
                .saturating_mul(u64::from(density))
                .saturating_div(1_000),
        )
        .unwrap_or(u32::MAX)
        .max(1),
        color: text.color,
    }
}

fn project_indicator_strip(
    strip: &CompositorIndicatorStrip,
    viewport: Rect,
    transform: HeadLogicalTransform,
    clip: Rect,
) -> HeadCompositorIndicatorStrip {
    let project = |geometry| intersect_rect(transform.project_root_rect(viewport, geometry), clip);
    HeadCompositorIndicatorStrip {
        node: strip.node,
        generation: strip.generation,
        strip: IndicatorChromeStrip {
            output: strip.strip.output,
            geometry: project(strip.strip.geometry),
            labels: strip
                .strip
                .labels
                .iter()
                .map(|(geometry, label, state)| (project(*geometry), label.clone(), *state))
                .collect(),
            status: strip
                .strip
                .status
                .as_ref()
                .map(|(geometry, label, state)| (project(*geometry), label.clone(), *state)),
            hit_targets: strip
                .strip
                .hit_targets
                .iter()
                .cloned()
                .map(|mut target| {
                    target.geometry = project(target.geometry);
                    target
                })
                .collect(),
        },
    }
}

/// The head-native region the scene is allowed to paint into.
///
/// Not the framebuffer. Every policy until centre-unscaled projected the scene
/// across the whole head, so these were one rect and the distinction cost
/// nothing; a scene placed inside a border separates them, and content clipped
/// to the framebuffer then paints into the margin that is supposed to hold
/// background alone. Borders showed it first because they are bright lines, but
/// surfaces and the cursor were bounded by the same wrong rect.
fn scene_clip(projected_scene: Rect, native: Size) -> Rect {
    clip_to_target(projected_scene, native)
}

fn ceil_div_positive(value: i32, divisor: i32) -> Option<i32> {
    (value > 0 && divisor > 0).then(|| value.checked_add(divisor - 1)?.checked_div(divisor))?
}

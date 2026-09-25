/// Why a frame cannot be scanned out directly, or that it can.
///
/// A verdict rather than a boolean, because the reason is what the evidence
/// records and what an operator reads when a frame that looked eligible was
/// composed instead.
///
/// The derived default is `CompositionRequired`: a value that arrives without
/// a plan behind it has proven nothing, and the absence of a proof must read
/// as "compose", never as "eligible". This is what makes it safe to carry the
/// verdict on a lowered frame whose other construction sites -- tests,
/// fixtures -- say nothing about it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectScanoutVerdict {
    /// One opaque client layer fills the head and nothing else draws.
    Eligible,
    /// Not exactly one layer: none to scan out, or several to combine.
    LayerCount(usize),
    /// The layer fell back to another variant, so its pixels are not the
    /// client's committed content.
    LayerNotActive,
    /// The client's buffer would have to be scaled to fill the head.
    LayerResampled,
    /// The layer is the head's size but not at its origin.
    LayerOffset,
    /// The layer sits at the origin but is not the head's size.
    LayerNotHeadSized,
    /// The layer is clipped to less than itself, so part of the head shows
    /// something else.
    LayerClipped,
    /// The layer is not a client DMA-BUF; a CPU buffer has no framebuffer to
    /// hand the plane.
    LayerNotDmaBuf,
    /// The layer is translucent, so what is behind it is part of the image.
    LayerTranslucent,
    /// Chrome, an overlay, or a resolved effect changes the composed image.
    /// Carries the command that disqualified the frame, because "something
    /// painted" is not an answer: a letterbox that should have been empty and
    /// an indicator strip drawn on purpose need different work.
    ///
    /// Also the default, so an unproven frame composes.
    CompositionRequired(&'static str),
    /// A composed cursor is part of the image. The hardware cursor rides its
    /// own plane and does not appear here.
    ComposedCursor,
}

impl Default for DirectScanoutVerdict {
    /// A verdict that arrived without a plan behind it has proven nothing, and
    /// the absence of a proof must read as "compose". Named `unproven` rather
    /// than after a command, because no command disqualified it -- nothing
    /// examined it at all.
    fn default() -> Self {
        Self::CompositionRequired("unproven")
    }
}

impl DirectScanoutVerdict {
    pub const fn is_eligible(self) -> bool {
        matches!(self, Self::Eligible)
    }

    /// Every verdict, in the order `reduced_index` numbers them.
    ///
    /// Exists so evidence can report a histogram without a reader having to
    /// know the enum, and so adding a verdict without extending the histogram
    /// is a compile error rather than a silently missing column.
    /// How many verdicts there are, so a histogram over them cannot be
    /// declared at the wrong width. Sized from `VERDICTS` rather than written
    /// out: adding a verdict without widening every array is a compile error,
    /// where a literal would have been a panic at the index instead.
    pub const COUNT: usize = Self::VERDICTS.len();

    pub const VERDICTS: [Self; 11] = [
        Self::Eligible,
        Self::LayerCount(0),
        Self::LayerNotActive,
        Self::LayerResampled,
        Self::LayerOffset,
        Self::LayerNotHeadSized,
        Self::LayerClipped,
        Self::LayerNotDmaBuf,
        Self::LayerTranslucent,
        Self::CompositionRequired(""),
        Self::ComposedCursor,
    ];

    /// This verdict's slot in a histogram over `VERDICTS`.
    pub const fn reduced_index(self) -> usize {
        match self {
            Self::Eligible => 0,
            Self::LayerCount(_) => 1,
            Self::LayerNotActive => 2,
            Self::LayerResampled => 3,
            Self::LayerOffset => 4,
            Self::LayerNotHeadSized => 5,
            Self::LayerClipped => 6,
            Self::LayerNotDmaBuf => 7,
            Self::LayerTranslucent => 8,
            Self::CompositionRequired(_) => 9,
            Self::ComposedCursor => 10,
        }
    }

    /// A stable name for evidence records.
    pub const fn reduced_name(self) -> &'static str {
        match self {
            Self::Eligible => "eligible",
            Self::LayerCount(_) => "layer_count",
            Self::LayerNotActive => "layer_not_active",
            Self::LayerResampled => "layer_resampled",
            Self::LayerOffset => "layer_offset",
            Self::LayerNotHeadSized => "layer_not_head_sized",
            Self::LayerClipped => "layer_clipped",
            Self::LayerNotDmaBuf => "layer_not_dma_buf",
            Self::LayerTranslucent => "layer_translucent",
            Self::CompositionRequired(_) => "composition_required",
            Self::ComposedCursor => "composed_cursor",
        }
    }
}

/// Whether one compositor command changes the image a direct scanout would
/// show.
///
/// Matched exhaustively and without a wildcard: a command variant added later
/// must be classified deliberately, and until it is, the compiler stops the
/// change rather than letting an unconsidered primitive ride along invisibly
/// on someone's screen. This follows the rule scanout cloning already states
/// for plan fields -- unconsidered state disables the optimization rather
/// than wrongly preserving it.
/// The command's name when it disqualifies the frame, or `None` when it is
/// neutral. A name rather than a boolean because "composition required" covers
/// every painting primitive, and which one it was is the difference between a
/// letterbox that should not be there and chrome that should.
fn command_requires_composition(command: &HeadCompositorCommand) -> Option<&'static str> {
    match command {
        // The letterbox fill, emitted unconditionally and empty exactly when
        // the projected scene already covers the framebuffer.
        //
        // Unreachable as a verdict under the ordering below: letterboxing
        // means the scene is smaller than the head, so the layer inside it
        // cannot cover the head either, and the geometry check answers first
        // with something more precise. Stated anyway, because a rect that
        // paints is composition whatever else is true of the frame, and a
        // later reordering must not make an empty-looking plan eligible.
        HeadCompositorCommand::Background(rect) => {
            (rect.geometry.width != 0 && rect.geometry.height != 0).then_some("background")
        }
        // The client's own content, which the plane will scan out directly.
        HeadCompositorCommand::Surface { .. } => None,
        HeadCompositorCommand::SurfacePreview(_) => Some("surface_preview"),
        HeadCompositorCommand::Border(_) => Some("border"),
        HeadCompositorCommand::Rect(_) => Some("rect"),
        HeadCompositorCommand::Text(_) => Some("text"),
        HeadCompositorCommand::IndicatorStrip(_) => Some("indicator_strip"),
        HeadCompositorCommand::ContentImage(_) => Some("shell_content"),
    }
}

/// Whether this exact frame can go to the plane without being composed.
///
/// Engine proves structure only. Whether the buffer's format and modifier can
/// actually be scanned out is the backend's atomic test to answer, and no
/// verdict here promises a flip will be accepted.
///
/// Every check is stated against the finished plan rather than the scene it
/// came from, because the plan is what reaches the screen: a frame is
/// eligible or not, never a surface or a session.
pub fn direct_scanout_verdict(plan: &HeadCompositionPlan) -> DirectScanoutVerdict {
    if plan.layers.len() != 1 {
        return DirectScanoutVerdict::LayerCount(plan.layers.len());
    }
    let layer = &plan.layers[0];
    if layer.outcome != HeadBindingOutcome::Active {
        return DirectScanoutVerdict::LayerNotActive;
    }
    if layer.requested_sampling != HeadSamplingClass::Exact {
        return DirectScanoutVerdict::LayerResampled;
    }
    if !matches!(layer.source, BufferSource::DmaBuf { .. }) {
        return DirectScanoutVerdict::LayerNotDmaBuf;
    }
    if layer.opacity_millis != 1_000 {
        return DirectScanoutVerdict::LayerTranslucent;
    }
    // Three separate reasons a layer might not be the head, reported apart.
    // Conflating them cost a physical run: "does not cover the head" is true
    // of a window at the wrong place, a window of the wrong size, and a window
    // clipped smaller than itself, and knowing which is the difference between
    // fixing it and guessing again.
    if layer.native_geometry.x != 0 || layer.native_geometry.y != 0 {
        return DirectScanoutVerdict::LayerOffset;
    }
    if layer.native_geometry.width != plan.native_size.width
        || layer.native_geometry.height != plan.native_size.height
    {
        return DirectScanoutVerdict::LayerNotHeadSized;
    }
    if layer.native_clip != layer.native_geometry {
        return DirectScanoutVerdict::LayerClipped;
    }
    if plan.cursor.is_some() {
        return DirectScanoutVerdict::ComposedCursor;
    }
    if let Some(command) = plan
        .compositor
        .iter()
        .find_map(command_requires_composition)
    {
        return DirectScanoutVerdict::CompositionRequired(command);
    }
    DirectScanoutVerdict::Eligible
}

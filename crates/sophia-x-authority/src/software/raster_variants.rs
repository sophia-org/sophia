use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use sophia_protocol::{
    MAX_SURFACE_CONTENT_VARIANTS, Rect, Region, SURFACE_CONTENT_DENSITY_1X_MILLIS, Size,
    SurfaceContentFidelity, SurfaceContentSet, SurfaceContentVariant, SurfaceRasterClass,
    SurfaceRasterRequirements, SurfaceRasterTransform,
};

use crate::image::X_IMAGE_FORMAT_Z_PIXMAP;
use crate::{X_GX_COPY, XByteOrder, XFontHandle, XGraphicsContextValues, XResourceId};

use super::raster_replay::apply_command;
use super::update::packed_patch_region;
use super::{
    X_AUTHORITY_CPU_BUFFER_FORMAT_XRGB8888, X_AUTHORITY_SOFTWARE_BUFFER_MAX_BYTES,
    XAuthorityCpuBufferPatchBatch, XAuthorityCpuBufferSnapshot, XAuthorityCpuBufferUpdate,
};

pub(crate) const X_AUTHORITY_RASTER_JOURNAL_MAX_COMMANDS: usize = 4_096;
pub(crate) const X_AUTHORITY_RASTER_JOURNAL_MAX_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;

/// Why a surface published sampled compatibility content instead of an
/// authority-owned native-density variant.
///
/// The cause stays authority-private. Engine continues to observe only
/// `SurfaceContentFidelity`, so classification can name X11 operations without
/// leaking protocol semantics across the boundary.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum XRasterFallbackCause {
    /// An accepted `PutImage` fell outside the replayable format subset.
    UnsupportedPutImage,
    /// A `CopyArea` named a source drawable other than its destination.
    UnsupportedCrossDrawableCopy,
    /// A RENDER operation wrote the drawable; compositing results have no
    /// journal representation yet, so density variants scale the 1x raster.
    UnsupportedRenderOperation,
    /// Some other drawing operation has no journal representation.
    UnsupportedCommand,
    /// The requirement named a content generation the authority has already
    /// advanced past. Engine builds requirements from its committed scene, so
    /// this fires whenever the client drew again before the requirement
    /// arrived.
    StaleContentGeneration,
    /// The requirement's logical extent disagrees with the canonical drawable.
    LogicalExtentMismatch,
    /// The surface has no canonical CPU drawable to replay from, because its
    /// content arrived through a renderer or pixmap presentation path.
    NoCanonicalRaster,
    /// The semantic journal exceeded its command or payload bound.
    JournalCapacity,
    /// The derived stores would exceed the variant or backing-byte bound.
    BackingCapacity,
    /// The requirement named a transform the derived stores do not render.
    TransformMismatch,
}

impl XRasterFallbackCause {
    /// Stable snake_case token for structured logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedPutImage => "unsupported_put_image",
            Self::UnsupportedCrossDrawableCopy => "unsupported_cross_drawable_copy",
            Self::UnsupportedRenderOperation => "unsupported_render_operation",
            Self::UnsupportedCommand => "unsupported_command",
            Self::StaleContentGeneration => "stale_content_generation",
            Self::LogicalExtentMismatch => "logical_extent_mismatch",
            Self::NoCanonicalRaster => "no_canonical_raster",
            Self::JournalCapacity => "journal_capacity",
            Self::BackingCapacity => "backing_capacity",
            Self::TransformMismatch => "transform_mismatch",
        }
    }
}

/// Which operation poisoned a surface's journal. Recorded at the drawing site,
/// where the distinction is still known, so late demand can report a cause
/// rather than a bare fallback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum XRasterUnsupportedKind {
    PutImage,
    CrossDrawableCopy,
    RenderOperation,
}

impl XRasterUnsupportedKind {
    fn cause(self) -> XRasterFallbackCause {
        match self {
            Self::PutImage => XRasterFallbackCause::UnsupportedPutImage,
            Self::CrossDrawableCopy => XRasterFallbackCause::UnsupportedCrossDrawableCopy,
            Self::RenderOperation => XRasterFallbackCause::UnsupportedRenderOperation,
        }
    }
}

/// Result of answering one Engine raster requirement from the journal.
#[derive(Clone, Debug)]
pub(crate) enum XRasterSatisfyOutcome {
    Satisfied(Vec<XAuthorityCpuBufferUpdate>),
    Fallback(XRasterFallbackCause),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct XRasterPoint {
    pub x: i32,
    pub y: i32,
}

/// Every visible plane of a depth-24 drawable. A `PutImage` whose plane mask
/// omits any of these does not write the canonical drawable unconditionally,
/// so it cannot serve as replayable content.
const X_VISIBLE_PLANE_MASK: u32 = 0x00ff_ffff;

/// Client raster bytes retained for replay, in the exact layout the canonical
/// drawable consumed: tight `width * 4` rows of little-endian XRGB8888.
///
/// Bytes are retained verbatim rather than transformed, so replaying at 1x
/// reproduces the canonical drawable bit for bit. `byte_order` records the
/// connection's declared order for evidence; it does not reorder pixels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct XOwnedImagePixels {
    pub rect: Rect,
    pub pixels: Vec<u8>,
    pub depth: u8,
    pub byte_order: XByteOrder,
}

/// Wire facts an accepted `PutImage` needs before the journal can decide
/// whether it is replayable. Built at the dispatch site, where the format,
/// padding, and byte order are still in scope.
#[derive(Clone, Debug)]
pub struct XPutImageSemantics {
    pub format: u8,
    pub depth: u8,
    pub left_pad: u8,
    pub byte_order: XByteOrder,
    pub gc: XGraphicsContextValues,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct XOwnedTextDraw {
    pub x: i32,
    pub baseline: i32,
    pub text: Vec<u16>,
    pub image: bool,
    pub font: XFontHandle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum XAuthorityRasterCommand {
    Paint {
        rects: Vec<Rect>,
        gc: XGraphicsContextValues,
    },
    Clear {
        rect: Rect,
        pixel: u32,
    },
    Lines {
        points: Vec<XRasterPoint>,
        gc: XGraphicsContextValues,
    },
    Rectangles {
        rectangles: Vec<Rect>,
        gc: XGraphicsContextValues,
    },
    Text {
        draws: Vec<XOwnedTextDraw>,
        gc: XGraphicsContextValues,
    },
    CopyArea {
        source: Rect,
        destination_x: i32,
        destination_y: i32,
        gc: XGraphicsContextValues,
    },
    PutImage {
        image: XOwnedImagePixels,
        gc: XGraphicsContextValues,
    },
    Unsupported(XRasterUnsupportedKind),
}

impl XAuthorityRasterCommand {
    /// Classifies one accepted `PutImage` against the replayable subset.
    ///
    /// Retention is fail-closed: anything outside tight ZPixmap depth-24/32
    /// bytes written unconditionally through the graphics context poisons the
    /// journal with a named cause rather than retaining pixels whose replay
    /// would not reproduce the canonical drawable.
    pub(crate) fn from_put_image(semantics: &XPutImageSemantics, rect: Rect, data: &[u8]) -> Self {
        let width = usize::try_from(rect.width.max(0)).unwrap_or(0);
        let height = usize::try_from(rect.height.max(0)).unwrap_or(0);
        let required = width.saturating_mul(4).saturating_mul(height);
        let replayable = semantics.format == X_IMAGE_FORMAT_Z_PIXMAP
            && matches!(semantics.depth, 24 | 32)
            && semantics.left_pad == 0
            && width > 0
            && height > 0
            && required > 0
            && data.len() >= required
            && semantics.gc.function == X_GX_COPY
            && semantics.gc.plane_mask & X_VISIBLE_PLANE_MASK == X_VISIBLE_PLANE_MASK
            && semantics.gc.clip_rectangles.is_none();
        if !replayable {
            return Self::Unsupported(XRasterUnsupportedKind::PutImage);
        }
        Self::PutImage {
            image: XOwnedImagePixels {
                rect,
                pixels: data[..required].to_vec(),
                depth: semantics.depth,
                byte_order: semantics.byte_order,
            },
            gc: semantics.gc.clone(),
        }
    }
    pub(crate) fn translated(mut self, x: i32, y: i32) -> Self {
        let translate_rect = |rect: &mut Rect| {
            rect.x = rect.x.saturating_add(x);
            rect.y = rect.y.saturating_add(y);
        };
        match &mut self {
            Self::Paint { rects, gc } => {
                rects.iter_mut().for_each(translate_rect);
                translate_gc_clip(gc, x, y);
            }
            Self::Clear { rect, .. } => translate_rect(rect),
            Self::Lines { points, gc } => {
                for point in points {
                    point.x = point.x.saturating_add(x);
                    point.y = point.y.saturating_add(y);
                }
                translate_gc_clip(gc, x, y);
            }
            Self::Rectangles { rectangles, gc } => {
                rectangles.iter_mut().for_each(translate_rect);
                translate_gc_clip(gc, x, y);
            }
            Self::Text { draws, gc } => {
                for draw in draws {
                    draw.x = draw.x.saturating_add(x);
                    draw.baseline = draw.baseline.saturating_add(y);
                }
                translate_gc_clip(gc, x, y);
            }
            Self::CopyArea {
                source,
                destination_x,
                destination_y,
                gc,
            } => {
                translate_rect(source);
                *destination_x = destination_x.saturating_add(x);
                *destination_y = destination_y.saturating_add(y);
                translate_gc_clip(gc, x, y);
            }
            Self::PutImage { image, gc } => {
                translate_rect(&mut image.rect);
                translate_gc_clip(gc, x, y);
            }
            Self::Unsupported(_) => {}
        }
        self
    }

    fn payload_bytes(&self) -> usize {
        match self {
            Self::Paint { rects, gc } => {
                rects.len().saturating_mul(size_of::<Rect>()) + gc_bytes(gc)
            }
            Self::Clear { .. } => size_of::<Rect>() + size_of::<u32>(),
            Self::Lines { points, gc } => {
                points.len().saturating_mul(size_of::<XRasterPoint>()) + gc_bytes(gc)
            }
            Self::Rectangles { rectangles, gc } => {
                rectangles.len().saturating_mul(size_of::<Rect>()) + gc_bytes(gc)
            }
            Self::Text { draws, gc } => {
                draws.iter().map(|draw| draw.text.len()).sum::<usize>() + gc_bytes(gc)
            }
            Self::CopyArea { gc, .. } => size_of::<Rect>() + gc_bytes(gc),
            Self::PutImage { image, gc } => image.pixels.len() + gc_bytes(gc),
            Self::Unsupported(_) => 0,
        }
    }

    /// The kind that poisons the journal, if this command cannot be replayed.
    fn unsupported_kind(&self) -> Option<XRasterUnsupportedKind> {
        match self {
            Self::Unsupported(kind) => Some(*kind),
            _ => None,
        }
    }

    /// Whether this command replaces every visible pixel of the drawable on
    /// its own, so replaying it alone still reproduces the canonical
    /// protocol-visible content.
    ///
    /// A baseline may discard older journal commands. `PutImage` qualifies
    /// only because retention already required an unconditional GXcopy with no
    /// clip rectangles, which is exactly what the canonical writer performed.
    fn is_full_opaque_baseline(&self, logical_size: Size) -> bool {
        let covers = |rect: &Rect| {
            rect.x <= 0
                && rect.y <= 0
                && rect.x.saturating_add(rect.width) >= logical_size.width
                && rect.y.saturating_add(rect.height) >= logical_size.height
        };
        match self {
            Self::Clear { rect, .. } => covers(rect),
            Self::PutImage { image, .. } => covers(&image.rect),
            _ => false,
        }
    }
}

fn gc_bytes(gc: &XGraphicsContextValues) -> usize {
    size_of::<XGraphicsContextValues>().saturating_add(
        gc.clip_rectangles
            .as_ref()
            .map_or(0, Vec::len)
            .saturating_mul(size_of::<Rect>()),
    )
}

fn translate_gc_clip(gc: &mut XGraphicsContextValues, x: i32, y: i32) {
    gc.clip_x_origin = i16::try_from(i32::from(gc.clip_x_origin).saturating_add(x))
        .unwrap_or(if x < 0 { i16::MIN } else { i16::MAX });
    gc.clip_y_origin = i16::try_from(i32::from(gc.clip_y_origin).saturating_add(y))
        .unwrap_or(if y < 0 { i16::MIN } else { i16::MAX });
}

#[derive(Clone, Debug)]
struct VariantBacking {
    variant: u32,
    snapshot: XAuthorityCpuBufferSnapshot,
}

#[derive(Clone, Debug)]
struct SurfaceRasterState {
    logical_size: Size,
    journal: Vec<XAuthorityRasterCommand>,
    journal_payload_bytes: usize,
    replayable: bool,
    /// Why replay was abandoned. Retained so late density demand reports the
    /// original operation rather than a bare fallback.
    poison: Option<XRasterFallbackCause>,
    next_variant: u32,
    required: BTreeSet<SurfaceRasterClass>,
    variants: BTreeMap<SurfaceRasterClass, VariantBacking>,
}

impl SurfaceRasterState {
    fn new(logical_size: Size) -> Self {
        Self {
            logical_size,
            journal: Vec::new(),
            journal_payload_bytes: 0,
            replayable: true,
            poison: None,
            next_variant: 2,
            required: BTreeSet::new(),
            variants: BTreeMap::new(),
        }
    }
}

/// Authority-private semantic journal and native-density presentation stores.
/// Canonical X11 drawable pixels remain in `XSoftwareBufferStore`; this owner
/// never exposes a connector, CRTC, or client XID to Engine.
#[derive(Debug)]
pub(crate) struct XAuthorityRasterStore {
    next_handle: u64,
    surfaces: BTreeMap<XResourceId, SurfaceRasterState>,
}

impl Default for XAuthorityRasterStore {
    fn default() -> Self {
        Self {
            // Derived handles occupy a disjoint authority-owned range.
            next_handle: 1_u64 << 63,
            surfaces: BTreeMap::new(),
        }
    }
}

impl XAuthorityRasterStore {
    pub(crate) fn remove(&mut self, presentation: XResourceId) {
        self.surfaces.remove(&presentation);
    }

    /// Invalidates semantic replay after pixels arrive through a presentation
    /// path that has no journal representation.
    ///
    /// Keeping the old journal would let a later density request label stale
    /// pixels as an authority raster. Reinitializing it as replayable would be
    /// equally wrong because replaying an empty journal produces a blank
    /// derived buffer. A later full opaque baseline may recover the store.
    pub(crate) fn invalidate_unjournaled_presentation(
        &mut self,
        presentation: XResourceId,
        logical_size: Size,
    ) {
        let mut state = SurfaceRasterState::new(logical_size);
        state.replayable = false;
        state.poison = Some(XRasterFallbackCause::UnsupportedCommand);
        self.surfaces.insert(presentation, state);
    }

    pub(crate) fn record(
        &mut self,
        presentation: XResourceId,
        logical_size: Size,
        command: XAuthorityRasterCommand,
    ) -> Vec<XAuthorityCpuBufferUpdate> {
        let state = self
            .surfaces
            .entry(presentation)
            .or_insert_with(|| SurfaceRasterState::new(logical_size));
        if state.logical_size != logical_size {
            *state = SurfaceRasterState::new(logical_size);
            state.replayable = false;
            state.poison = Some(XRasterFallbackCause::LogicalExtentMismatch);
        }
        if command.is_full_opaque_baseline(logical_size) {
            state.journal.clear();
            state.journal_payload_bytes = 0;
            state.replayable = true;
            state.poison = None;
        }
        let payload = command.payload_bytes();
        let poison = command
            .unsupported_kind()
            .map(XRasterUnsupportedKind::cause);
        let over_budget = state.journal.len() >= X_AUTHORITY_RASTER_JOURNAL_MAX_COMMANDS
            || state.journal_payload_bytes.saturating_add(payload)
                > X_AUTHORITY_RASTER_JOURNAL_MAX_PAYLOAD_BYTES;
        if let Some(cause) = poison.or(over_budget.then_some(XRasterFallbackCause::JournalCapacity))
        {
            state.replayable = false;
            state.poison = Some(cause);
            state.journal.clear();
            state.journal_payload_bytes = 0;
            state.variants.clear();
            return Vec::new();
        }
        state.journal_payload_bytes = state.journal_payload_bytes.saturating_add(payload);
        state.journal.push(command.clone());
        if !state.replayable {
            return Vec::new();
        }
        let mut updates = Vec::new();
        for (class, backing) in &mut state.variants {
            // Where the replay painted, in this variant's own density space.
            // `apply_command` reports it beside each branch's projection, so
            // the extent cannot disagree with the paint.
            let painted = apply_command(&mut backing.snapshot, *class, &command);
            backing.snapshot.generation = backing.snapshot.generation.saturating_add(1);
            // A derived variant used to publish its whole buffer for every
            // drawing command, once per retained density. A one-glyph edit on a
            // window with two variants copied two whole buffers to carry a few
            // hundred bytes of change.
            //
            // Fail closed on anything the replay could not place: an unknown
            // extent, or one this buffer cannot represent as a patch, owes a
            // full replacement rather than a patch that might miss a region.
            let update = match painted {
                Some(extent) if extent.width <= 0 || extent.height <= 0 => continue,
                Some(extent) => packed_patch_region(&backing.snapshot, extent).map_or_else(
                    || XAuthorityCpuBufferUpdate::Replace(backing.snapshot.clone()),
                    |region| {
                        XAuthorityCpuBufferUpdate::PatchBatch(XAuthorityCpuBufferPatchBatch {
                            handle: backing.snapshot.handle,
                            drawable: backing.snapshot.drawable,
                            size: backing.snapshot.size,
                            stride: backing.snapshot.stride,
                            format: backing.snapshot.format,
                            generation: backing.snapshot.generation,
                            patches: vec![region],
                        })
                    },
                ),
                None => XAuthorityCpuBufferUpdate::Replace(backing.snapshot.clone()),
            };
            updates.push(update);
        }
        updates
    }

    pub(crate) fn satisfy(
        &mut self,
        presentation: XResourceId,
        requirements: &SurfaceRasterRequirements,
        canonical_bytes: usize,
    ) -> Result<XRasterSatisfyOutcome, &'static str> {
        requirements
            .validate()
            .map_err(|_| "invalid surface raster requirements")?;
        let state = self
            .surfaces
            .entry(presentation)
            .or_insert_with(|| SurfaceRasterState::new(requirements.logical_extent));
        if state.logical_size != requirements.logical_extent {
            return Ok(XRasterSatisfyOutcome::Fallback(
                XRasterFallbackCause::LogicalExtentMismatch,
            ));
        }
        if !state.replayable {
            return Ok(XRasterSatisfyOutcome::Fallback(
                state
                    .poison
                    .unwrap_or(XRasterFallbackCause::UnsupportedCommand),
            ));
        }

        // Requirement satisfaction is atomic. In particular, a four-class
        // non-1x request cannot partially replace retained variants and leak
        // unpublished backing handles before the caller selects fallback.
        let mut candidate = state.clone();
        let mut next_handle = self.next_handle;
        candidate.required = requirements.classes.iter().copied().collect();
        candidate
            .variants
            .retain(|class, _| candidate.required.contains(class));
        let mut projected_bytes = canonical_bytes.saturating_add(
            candidate
                .variants
                .values()
                .map(|variant| variant.snapshot.bytes.len())
                .sum::<usize>(),
        );
        let mut updates = Vec::new();
        // Classes iterate in `BTreeSet` order, so a multi-class requirement
        // reports the same cause on every run.
        let mut fallback_cause: Option<XRasterFallbackCause> = None;
        for class in &candidate.required {
            if class.transform != SurfaceRasterTransform::Normal {
                // Derived stores render the normal transform only.
                fallback_cause.get_or_insert(XRasterFallbackCause::TransformMismatch);
                continue;
            }
            if class.density_millis == SURFACE_CONTENT_DENSITY_1X_MILLIS
                || candidate.variants.contains_key(class)
            {
                continue;
            }
            // The canonical 1x X11 backing always occupies one protocol
            // variant. A requirement may legally name the protocol-wide
            // maximum without including 1x, so fail it as sampled fallback
            // instead of constructing an over-capacity content set.
            if candidate.variants.len() >= MAX_SURFACE_CONTENT_VARIANTS.saturating_sub(1) {
                fallback_cause.get_or_insert(XRasterFallbackCause::BackingCapacity);
                continue;
            }
            let size = projected_size(candidate.logical_size, class.density_millis)
                .ok_or("surface raster requirement size overflow")?;
            let bytes =
                buffer_bytes(size).ok_or("surface raster requirement exceeds backing bound")?;
            if projected_bytes.saturating_add(bytes) > X_AUTHORITY_SOFTWARE_BUFFER_MAX_BYTES {
                fallback_cause.get_or_insert(XRasterFallbackCause::BackingCapacity);
                continue;
            }
            projected_bytes = projected_bytes.saturating_add(bytes);
            let handle = next_handle.max(1_u64 << 63);
            next_handle = handle.saturating_add(1).max(1_u64 << 63);
            let mut snapshot = XAuthorityCpuBufferSnapshot {
                handle,
                drawable: presentation,
                size,
                stride: u32::try_from(usize::try_from(size.width).unwrap_or(0).saturating_mul(4))
                    .map_err(|_| "surface raster stride overflow")?,
                format: X_AUTHORITY_CPU_BUFFER_FORMAT_XRGB8888,
                generation: 1,
                bytes: Arc::new(vec![0; bytes]),
            };
            for command in &candidate.journal {
                apply_command(&mut snapshot, *class, command);
            }
            let variant = candidate.next_variant.max(2);
            candidate.next_variant = variant.saturating_add(1).max(2);
            updates.push(XAuthorityCpuBufferUpdate::Replace(snapshot.clone()));
            candidate
                .variants
                .insert(*class, VariantBacking { variant, snapshot });
        }
        let all_satisfied = candidate.required.iter().all(|class| {
            (class.transform == SurfaceRasterTransform::Normal
                && class.density_millis == SURFACE_CONTENT_DENSITY_1X_MILLIS)
                || candidate.variants.contains_key(class)
        });
        if !all_satisfied {
            return Ok(XRasterSatisfyOutcome::Fallback(
                fallback_cause.unwrap_or(XRasterFallbackCause::BackingCapacity),
            ));
        }
        *state = candidate;
        self.next_handle = next_handle;
        Ok(XRasterSatisfyOutcome::Satisfied(updates))
    }

    pub(crate) fn content_set(
        &self,
        presentation: XResourceId,
        canonical: &XAuthorityCpuBufferSnapshot,
    ) -> SurfaceContentSet {
        let mut variants = vec![SurfaceContentVariant {
            variant: 1,
            source: sophia_protocol::BufferSource::CpuBuffer {
                handle: canonical.handle,
            },
            pixel_size: canonical.size,
            density_millis: SURFACE_CONTENT_DENSITY_1X_MILLIS,
            transform: SurfaceRasterTransform::Normal,
            fidelity: SurfaceContentFidelity::AuthorityRaster,
            damage: full_damage(canonical.size),
        }];
        if let Some(state) = self.surfaces.get(&presentation) {
            variants.extend(
                state
                    .variants
                    .iter()
                    .take(MAX_SURFACE_CONTENT_VARIANTS.saturating_sub(1))
                    .map(|(class, backing)| SurfaceContentVariant {
                        variant: backing.variant,
                        source: sophia_protocol::BufferSource::CpuBuffer {
                            handle: backing.snapshot.handle,
                        },
                        pixel_size: backing.snapshot.size,
                        density_millis: class.density_millis,
                        transform: class.transform,
                        fidelity: SurfaceContentFidelity::AuthorityRaster,
                        damage: full_damage(backing.snapshot.size),
                    }),
            );
        }
        SurfaceContentSet::new(canonical.size, variants)
            .expect("authority raster store preserves content-set invariants")
    }
}

fn full_damage(size: Size) -> Region {
    Region::single(Rect {
        x: 0,
        y: 0,
        width: size.width,
        height: size.height,
    })
}

fn projected_size(logical: Size, density: u32) -> Option<Size> {
    let edge = |value: i32| {
        i32::try_from(
            u64::try_from(value)
                .ok()?
                .checked_mul(u64::from(density))?
                .checked_add(999)?
                / 1_000,
        )
        .ok()
    };
    Some(Size {
        width: edge(logical.width)?,
        height: edge(logical.height)?,
    })
}

fn buffer_bytes(size: Size) -> Option<usize> {
    usize::try_from(size.width)
        .ok()?
        .checked_mul(usize::try_from(size.height).ok()?)?
        .checked_mul(4)
        .filter(|bytes| *bytes <= X_AUTHORITY_SOFTWARE_BUFFER_MAX_BYTES)
}

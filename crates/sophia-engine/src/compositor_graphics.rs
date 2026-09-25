use crate::prelude::*;
use crate::{HeadlessOutput, IndicatorChromeStrip, OutputFrameDamageSnapshot, output_frame_damage};

#[path = "compositor_graphics/chrome_layout.rs"]
mod chrome_layout;
pub use chrome_layout::*;
#[path = "compositor_graphics/chrome_summary.rs"]
mod chrome_summary;
pub use chrome_summary::*;

// Includes surface chrome, 2,048 member labels, 1,024 empty-cell labels,
// indicators and the descriptor switcher, all within one explicit bound.
pub const MAX_COMPOSITOR_DISPLAY_COMMANDS: usize = 10_240;
pub const MAX_OUTPUT_DAMAGE_RECTS: usize = MAX_COMPOSITOR_DISPLAY_COMMANDS * 2;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SurfaceChromeRole {
    Frame,
    FocusRing,
    FloatingOutline,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DescriptorOverlayNodeRole {
    Panel,
    Row,
    Selection,
    Trust,
    Attention,
    Label,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CompositorNodeId {
    SurfaceChrome {
        surface: SurfaceId,
        role: SurfaceChromeRole,
    },
    IndicatorStrip {
        output: OutputId,
    },
    TabBar {
        output: OutputId,
        group: u64,
        slot: u16,
        label: bool,
    },
    DescriptorOverlay {
        projection: u64,
        slot: u16,
        role: DescriptorOverlayNodeRole,
    },
    ShellContent {
        grant: sophia_protocol::ContentGrant,
        output: OutputId,
        candidate: u64,
        surface: u16,
        placement: u16,
    },
    /// A WM surface instance: the admitted WM connection epoch qualifies the
    /// WM's opaque id, so an id of an old connection cannot alias a new one.
    /// Distinct from the source surface, which keeps its own identity.
    PolicyInstance {
        owner_epoch: u64,
        id: u64,
    },
    /// A WM presentation region, drawn by Engine in its chrome palette.
    /// Ids share the publication's namespace with instances; the variant
    /// keeps a region distinct from an instance in every table.
    PolicyRegion {
        owner_epoch: u64,
        id: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompositorRgb8 {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompositorSolidRect {
    pub geometry: Rect,
    pub color: CompositorRgb8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompositorRect {
    pub opacity: u8,
    pub node: CompositorNodeId,
    pub generation: u64,
    pub geometry: Rect,
    pub color: CompositorRgb8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositorText {
    pub node: CompositorNodeId,
    pub generation: u64,
    pub geometry: Rect,
    pub text: String,
    pub font_size_millis: u32,
    pub color: CompositorRgb8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompositorBorder {
    pub node: CompositorNodeId,
    pub generation: u64,
    pub outer: Rect,
    pub inner: Rect,
    pub color: CompositorRgb8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositorIndicatorStrip {
    pub node: CompositorNodeId,
    pub generation: u64,
    pub strip: IndicatorChromeStrip,
}

#[path = "compositor_graphics/content_identity.rs"]
mod content_identity;
pub use content_identity::*;

/// A second presentation of a committed source surface, proposed by the WM
/// and drawn by Engine: the source's committed content scaled into
/// `destination` and clipped to `clip`. It changes neither the source's
/// allocation nor its input geometry, and it is never an application input
/// target. `generation` is the WM's interaction generation; the source's
/// committed generation is resolved by Engine at frame capture
/// ([`resolve_surface_instance_sources`]) and drives repaint without
/// changing the interaction generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompositorSurfaceInstance {
    pub owner_epoch: u64,
    pub id: u64,
    pub generation: u64,
    pub source: SurfaceId,
    pub source_generation: u64,
    pub destination: Rect,
    pub clip: Rect,
    pub opacity_millis: u16,
}

impl CompositorSurfaceInstance {
    pub const fn node(&self) -> CompositorNodeId {
        CompositorNodeId::PolicyInstance {
            owner_epoch: self.owner_epoch,
            id: self.id,
        }
    }

    /// The pixels this instance can change: its destination within its clip.
    pub fn visible(&self) -> Rect {
        rect_intersection(self.destination, self.clip)
    }

    fn is_valid(&self) -> bool {
        self.owner_epoch != 0
            && self.id != 0
            && self.generation != 0
            && self.source.is_valid()
            && self.source_generation != 0
            && !self.destination.is_empty()
            && !self.clip.is_empty()
            && !self.visible().is_empty()
            && (1..=1_000).contains(&self.opacity_millis)
    }
}

fn rect_intersection(first: Rect, second: Rect) -> Rect {
    let left = first.x.max(second.x);
    let top = first.y.max(second.y);
    let right = first
        .x
        .saturating_add(first.width)
        .min(second.x.saturating_add(second.width));
    let bottom = first
        .y
        .saturating_add(first.height)
        .min(second.y.saturating_add(second.height));
    Rect {
        x: left,
        y: top,
        width: right.saturating_sub(left).max(0),
        height: bottom.saturating_sub(top).max(0),
    }
}

/// An instance names a source with no committed content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompositorMissingInstanceSource {
    pub source: SurfaceId,
}

impl fmt::Display for CompositorMissingInstanceSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "surface instance source {:?} has no committed content",
            self.source
        )
    }
}

impl std::error::Error for CompositorMissingInstanceSource {}

/// Resolves every instance's source generation from the committed table at
/// frame capture. The WM never supplies it. A source with no committed
/// content refuses the whole list, unchanged: a presentation is drawn
/// complete or not at all, and one instance is never silently dropped.
pub fn resolve_surface_instance_sources<C>(
    display_list: &mut CompositorDisplayList<C>,
    committed: &[CommittedSurfaceState],
) -> Result<(), CompositorMissingInstanceSource> {
    let mut resolved = Vec::new();
    for (index, command) in display_list.commands.iter().enumerate() {
        let CompositorDisplayCommand::SurfaceInstance(instance) = command else {
            continue;
        };
        let state = committed
            .iter()
            .find(|state| state.surface == instance.source)
            .ok_or(CompositorMissingInstanceSource {
                source: instance.source,
            })?;
        resolved.push((index, state.committed_generation));
    }
    for (index, generation) in resolved {
        if let CompositorDisplayCommand::SurfaceInstance(instance) =
            &mut display_list.commands[index]
        {
            instance.source_generation = generation;
        }
    }
    Ok(())
}

/// Which admitted WM publication an output frame presents. It draws nothing;
/// it travels with the frame so the frame that retires names exactly the
/// publication its pixels show, distinct from whatever publication is
/// requested by then. A change of stamp, including a binding-only
/// publication with identical pixels and a withdrawal, damages the old and
/// new coverage, so a frame carrying the change is presented and retires.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompositorPresentationStamp {
    pub owner_epoch: u64,
    pub publication_generation: u64,
    pub output: OutputId,
    pub output_generation: u64,
    /// The output's presentation coverage, in this list's coordinates.
    pub coverage: Rect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompositorDisplayCommand<C = CompositorContentImage> {
    Surface { surface: SurfaceId },
    SurfaceInstance(CompositorSurfaceInstance),
    PresentationStamp(CompositorPresentationStamp),
    Border(CompositorBorder),
    Rect(CompositorRect),
    Text(CompositorText),
    IndicatorStrip(CompositorIndicatorStrip),
    ContentImage(C),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositorDisplayList<C = CompositorContentImage> {
    pub output: OutputId,
    pub commands: Vec<CompositorDisplayCommand<C>>,
}

impl<C> CompositorDisplayList<C> {
    pub fn presentation_stamp(&self) -> Option<CompositorPresentationStamp> {
        self.commands.iter().find_map(|command| match command {
            CompositorDisplayCommand::PresentationStamp(stamp) => Some(*stamp),
            _ => None,
        })
    }

    pub fn surface_instances(&self) -> impl Iterator<Item = CompositorSurfaceInstance> + '_ {
        self.commands.iter().filter_map(|command| match command {
            CompositorDisplayCommand::SurfaceInstance(instance) => Some(*instance),
            _ => None,
        })
    }

    pub fn empty(output: OutputId) -> Self {
        Self {
            output,
            commands: Vec::new(),
        }
    }

    pub fn borders(&self) -> impl Iterator<Item = CompositorBorder> + '_ {
        self.commands.iter().filter_map(|command| match command {
            CompositorDisplayCommand::Border(border) => Some(*border),
            CompositorDisplayCommand::Surface { .. }
            | CompositorDisplayCommand::SurfaceInstance(_)
            | CompositorDisplayCommand::PresentationStamp(_)
            | CompositorDisplayCommand::Rect(_)
            | CompositorDisplayCommand::Text(_)
            | CompositorDisplayCommand::IndicatorStrip(_)
            | CompositorDisplayCommand::ContentImage(_) => None,
        })
    }

    pub fn rects(&self) -> impl Iterator<Item = CompositorRect> + '_ {
        self.commands.iter().filter_map(|command| match command {
            CompositorDisplayCommand::Rect(rect) => Some(*rect),
            CompositorDisplayCommand::Surface { .. }
            | CompositorDisplayCommand::SurfaceInstance(_)
            | CompositorDisplayCommand::PresentationStamp(_)
            | CompositorDisplayCommand::Border(_)
            | CompositorDisplayCommand::Text(_)
            | CompositorDisplayCommand::IndicatorStrip(_)
            | CompositorDisplayCommand::ContentImage(_) => None,
        })
    }

    pub fn texts(&self) -> impl Iterator<Item = &CompositorText> + '_ {
        self.commands.iter().filter_map(|command| match command {
            CompositorDisplayCommand::Text(text) => Some(text),
            CompositorDisplayCommand::Surface { .. }
            | CompositorDisplayCommand::SurfaceInstance(_)
            | CompositorDisplayCommand::PresentationStamp(_)
            | CompositorDisplayCommand::Border(_)
            | CompositorDisplayCommand::Rect(_)
            | CompositorDisplayCommand::IndicatorStrip(_)
            | CompositorDisplayCommand::ContentImage(_) => None,
        })
    }

    pub fn indicator_strips(&self) -> impl Iterator<Item = &CompositorIndicatorStrip> + '_ {
        self.commands.iter().filter_map(|command| match command {
            CompositorDisplayCommand::IndicatorStrip(strip) => Some(strip),
            CompositorDisplayCommand::Surface { .. }
            | CompositorDisplayCommand::SurfaceInstance(_)
            | CompositorDisplayCommand::PresentationStamp(_)
            | CompositorDisplayCommand::Border(_)
            | CompositorDisplayCommand::Rect(_)
            | CompositorDisplayCommand::Text(_)
            | CompositorDisplayCommand::ContentImage(_) => None,
        })
    }

    pub fn content_images(&self) -> impl Iterator<Item = &C> + '_ {
        self.commands.iter().filter_map(|command| match command {
            CompositorDisplayCommand::ContentImage(image) => Some(image),
            _ => None,
        })
    }
}

pub(crate) fn compositor_display_list_structure_is_valid<C: CompositorContentMetadata>(
    display_list: &CompositorDisplayList<C>,
) -> bool {
    if display_list.commands.len() > MAX_COMPOSITOR_DISPLAY_COMMANDS {
        return false;
    }
    let mut nodes = BTreeSet::new();
    let mut stamped = false;
    display_list.commands.iter().all(|command| match command {
        CompositorDisplayCommand::Surface { .. } => true,
        // At most one, for this list's output.
        CompositorDisplayCommand::PresentationStamp(stamp) => {
            !std::mem::replace(&mut stamped, true)
                && stamp.owner_epoch != 0
                && stamp.publication_generation != 0
                && stamp.output_generation != 0
                && stamp.output == display_list.output
        }
        CompositorDisplayCommand::SurfaceInstance(instance) => {
            instance.is_valid() && nodes.insert(instance.node())
        }
        CompositorDisplayCommand::Border(border) => nodes.insert(border.node),
        CompositorDisplayCommand::Rect(rect) => {
            rect.generation != 0 && !rect.geometry.is_empty() && nodes.insert(rect.node)
        }
        CompositorDisplayCommand::Text(text) => {
            text.generation != 0
                && !text.geometry.is_empty()
                && !text.text.is_empty()
                && text.text.len() <= sophia_protocol::MAX_CHROME_LABEL_LEN
                && !text.text.chars().any(char::is_control)
                && text.font_size_millis != 0
                && nodes.insert(text.node)
        }
        CompositorDisplayCommand::IndicatorStrip(strip) => nodes.insert(strip.node),
        CompositorDisplayCommand::ContentImage(image) => {
            let image = image.content_identity();
            image.generation != 0
                && image.output_size_px.width > 0
                && image.output_size_px.height > 0
                && !image.geometry_px.is_empty()
                && image.size_px.width > 0
                && image.size_px.height > 0
                && image.stride
                    == u32::try_from(image.size_px.width)
                        .ok()
                        .and_then(|width| width.checked_mul(4))
                        .unwrap_or(0)
                && image.format == sophia_renderer_live_format_argb8888()
                && image.source_bytes
                    == usize::try_from(image.stride)
                        .ok()
                        .and_then(|stride| {
                            usize::try_from(image.size_px.height)
                                .ok()
                                .and_then(|height| stride.checked_mul(height))
                        })
                        .unwrap_or(usize::MAX)
                && nodes.insert(image.node)
        }
    })
}

const fn sophia_renderer_live_format_argb8888() -> u32 {
    // DRM_FORMAT_ARGB8888. Kept here to avoid making Engine depend on the
    // renderer crate merely for a wire-format constant.
    u32::from_le_bytes(*b"AR24")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusRingStyle {
    pub width: i32,
    pub color: CompositorRgb8,
}

impl Default for FocusRingStyle {
    fn default() -> Self {
        Self {
            width: 2,
            color: CompositorRgb8 {
                red: 0x70,
                green: 0xb7,
                blue: 0xff,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceFrameStyle {
    pub width: i32,
    pub focused_color: CompositorRgb8,
    pub unfocused_color: CompositorRgb8,
}

impl Default for SurfaceFrameStyle {
    fn default() -> Self {
        Self {
            width: 0,
            focused_color: FocusRingStyle::default().color,
            unfocused_color: CompositorRgb8 {
                red: 0x30,
                green: 0x30,
                blue: 0x30,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SurfaceChromeStyle {
    pub focus_ring: FocusRingStyle,
    pub frame: SurfaceFrameStyle,
}

impl SurfaceChromeStyle {
    pub const fn clearance(self) -> i32 {
        let ring = if self.focus_ring.width > 0 {
            self.focus_ring.width
        } else {
            0
        };
        let frame = if self.frame.width > 0 {
            self.frame.width
        } else {
            0
        };
        if ring > frame { ring } else { frame }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompositorDisplayListError {
    InvalidOutput,
    InvalidSurface,
    DuplicateSurface,
    CapacityExceeded,
}

include!("compositor_graphics/frame_presentation.rs");

impl fmt::Display for CompositorDisplayListError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for CompositorDisplayListError {}

/// Builds one immutable compositor display list from committed visual state.
///
/// Surface commands preserve Engine presentation order. Engine-owned frame and
/// focus-ring nodes are inserted before their content surface.
pub fn surface_chrome_display_list(
    output: OutputId,
    presentation_order: &[SurfaceId],
    committed_surfaces: &[CommittedSurfaceState],
    focused_surface: Option<SurfaceId>,
    style: SurfaceChromeStyle,
) -> Result<CompositorDisplayList, CompositorDisplayListError> {
    surface_chrome_display_list_for_surfaces(
        output,
        presentation_order,
        presentation_order,
        committed_surfaces,
        focused_surface,
        style,
    )
}

pub fn surface_chrome_display_list_for_surfaces(
    output: OutputId,
    presentation_order: &[SurfaceId],
    chrome_surfaces: &[SurfaceId],
    committed_surfaces: &[CommittedSurfaceState],
    focused_surface: Option<SurfaceId>,
    style: SurfaceChromeStyle,
) -> Result<CompositorDisplayList, CompositorDisplayListError> {
    if !output.is_valid() {
        return Err(CompositorDisplayListError::InvalidOutput);
    }
    let mut commands = Vec::with_capacity(
        presentation_order
            .len()
            .saturating_mul(2)
            .saturating_add(1)
            .min(MAX_COMPOSITOR_DISPLAY_COMMANDS),
    );
    let mut seen = BTreeSet::new();
    for surface in presentation_order.iter().copied() {
        if !surface.is_valid() {
            return Err(CompositorDisplayListError::InvalidSurface);
        }
        if !seen.insert(surface) {
            return Err(CompositorDisplayListError::DuplicateSurface);
        }
        let Some(committed) = committed_surfaces
            .iter()
            .find(|committed| committed.surface == surface)
        else {
            push_display_command(&mut commands, CompositorDisplayCommand::Surface { surface })?;
            continue;
        };
        let focused = focused_surface == Some(surface);
        if !chrome_surfaces.contains(&surface) {
            push_display_command(&mut commands, CompositorDisplayCommand::Surface { surface })?;
            continue;
        }
        if let Some(frame) = surface_chrome_border(
            committed,
            SurfaceChromeRole::Frame,
            style.frame.width,
            if focused {
                style.frame.focused_color
            } else {
                style.frame.unfocused_color
            },
        ) {
            push_display_command(&mut commands, CompositorDisplayCommand::Border(frame))?;
        }
        if focused
            && let Some(ring) = surface_chrome_border(
                committed,
                SurfaceChromeRole::FocusRing,
                style.focus_ring.width,
                style.focus_ring.color,
            )
        {
            push_display_command(&mut commands, CompositorDisplayCommand::Border(ring))?;
        }
        push_display_command(&mut commands, CompositorDisplayCommand::Surface { surface })?;
    }
    Ok(CompositorDisplayList { output, commands })
}

/// Instance damage is keyed by instance, never by source: a changed
/// placement, opacity, interaction generation or resolved source generation
/// damages the visible rectangle it had and has, and a changed stacking
/// order among instances damages every instance in both lists. Repeated
/// sources therefore keep distinct damage.
fn push_surface_instance_damage<A, B>(
    damage: &mut Region,
    previous: &CompositorDisplayList<A>,
    current: &CompositorDisplayList<B>,
) {
    let before = previous
        .surface_instances()
        .map(|instance| (instance.node(), instance))
        .collect::<BTreeMap<_, _>>();
    let after = current
        .surface_instances()
        .map(|instance| (instance.node(), instance))
        .collect::<BTreeMap<_, _>>();
    let order_changed = previous
        .surface_instances()
        .map(|instance| instance.node())
        .filter(|node| after.contains_key(node))
        .ne(current
            .surface_instances()
            .map(|instance| instance.node())
            .filter(|node| before.contains_key(node)));
    for node in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        let (old, new) = (before.get(node), after.get(node));
        if old == new && !order_changed {
            continue;
        }
        let old = old.map(CompositorSurfaceInstance::visible);
        let new = new.map(CompositorSurfaceInstance::visible);
        if let Some(old) = old {
            damage.push(old);
        }
        if let Some(new) = new.filter(|new| Some(*new) != old) {
            damage.push(new);
        }
    }
}

/// Computes compositor-owned damage between two immutable display lists.
///
/// Stable nodes with an unchanged generation, geometry, and color contribute
/// no damage. Changed and removed nodes damage their old extents; changed and
/// created nodes damage their new extents.
pub fn compositor_display_list_damage<
    A: CompositorContentMetadata,
    B: CompositorContentMetadata,
>(
    previous: &CompositorDisplayList<A>,
    current: &CompositorDisplayList<B>,
) -> Region {
    let previous_borders = previous
        .borders()
        .map(|border| (border.node, border))
        .collect::<BTreeMap<_, _>>();
    let current_borders = current
        .borders()
        .map(|border| (border.node, border))
        .collect::<BTreeMap<_, _>>();
    let mut damage = Region::empty();
    push_surface_instance_damage(&mut damage, previous, current);
    let (before, after) = (previous.presentation_stamp(), current.presentation_stamp());
    if before != after {
        let old = before
            .map(|stamp| stamp.coverage)
            .filter(|coverage| !coverage.is_empty());
        let new = after
            .map(|stamp| stamp.coverage)
            .filter(|coverage| !coverage.is_empty());
        if let Some(old) = old {
            damage.push(old);
        }
        if let Some(new) = new.filter(|new| Some(*new) != old) {
            damage.push(new);
        }
    }
    for node in previous_borders
        .keys()
        .chain(current_borders.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (previous_borders.get(&node), current_borders.get(&node)) {
            (Some(before), Some(after)) if before == after => {}
            (Some(before), Some(after)) => {
                push_border_damage(&mut damage, *before);
                push_border_damage(&mut damage, *after);
            }
            (Some(before), None) => push_border_damage(&mut damage, *before),
            (None, Some(after)) => push_border_damage(&mut damage, *after),
            (None, None) => unreachable!("node came from one display list"),
        }
    }
    let previous_strips = previous
        .indicator_strips()
        .map(|strip| (strip.node, strip))
        .collect::<BTreeMap<_, _>>();
    let current_strips = current
        .indicator_strips()
        .map(|strip| (strip.node, strip))
        .collect::<BTreeMap<_, _>>();
    for node in previous_strips
        .keys()
        .chain(current_strips.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (previous_strips.get(&node), current_strips.get(&node)) {
            (Some(before), Some(after)) if before == after => {}
            (Some(before), Some(after)) => {
                damage.push(before.strip.geometry);
                damage.push(after.strip.geometry);
            }
            (Some(before), None) => damage.push(before.strip.geometry),
            (None, Some(after)) => damage.push(after.strip.geometry),
            (None, None) => unreachable!("node came from one display list"),
        }
    }
    let previous_rects = previous
        .rects()
        .map(|rect| (rect.node, rect))
        .collect::<BTreeMap<_, _>>();
    let current_rects = current
        .rects()
        .map(|rect| (rect.node, rect))
        .collect::<BTreeMap<_, _>>();
    for node in previous_rects
        .keys()
        .chain(current_rects.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (previous_rects.get(&node), current_rects.get(&node)) {
            (Some(before), Some(after)) if before == after => {}
            (Some(before), Some(after)) => {
                damage.push(before.geometry);
                damage.push(after.geometry);
            }
            (Some(before), None) => damage.push(before.geometry),
            (None, Some(after)) => damage.push(after.geometry),
            (None, None) => unreachable!("node came from one display list"),
        }
    }
    let previous_texts = previous
        .texts()
        .map(|text| (text.node, text))
        .collect::<BTreeMap<_, _>>();
    let current_texts = current
        .texts()
        .map(|text| (text.node, text))
        .collect::<BTreeMap<_, _>>();
    for node in previous_texts
        .keys()
        .chain(current_texts.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (previous_texts.get(&node), current_texts.get(&node)) {
            (Some(before), Some(after)) if before == after => {}
            (Some(before), Some(after)) => {
                damage.push(before.geometry);
                damage.push(after.geometry);
            }
            (Some(before), None) => damage.push(before.geometry),
            (None, Some(after)) => damage.push(after.geometry),
            (None, None) => unreachable!("node came from one display list"),
        }
    }
    let previous_images = previous
        .content_images()
        .map(|image| {
            let image = image.content_identity();
            (image.node, image)
        })
        .collect::<BTreeMap<_, _>>();
    let current_images = current
        .content_images()
        .map(|image| {
            let image = image.content_identity();
            (image.node, image)
        })
        .collect::<BTreeMap<_, _>>();
    for node in previous_images
        .keys()
        .chain(current_images.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (previous_images.get(&node), current_images.get(&node)) {
            (Some(before), Some(after)) if before == after => {}
            (Some(before), Some(after)) => {
                damage.push(before.geometry_px);
                damage.push(after.geometry_px);
            }
            (Some(before), None) => damage.push(before.geometry_px),
            (None, Some(after)) => damage.push(after.geometry_px),
            (None, None) => unreachable!("node came from one display list"),
        }
    }
    damage
}

fn push_display_command(
    commands: &mut Vec<CompositorDisplayCommand>,
    command: CompositorDisplayCommand,
) -> Result<(), CompositorDisplayListError> {
    if commands.len() >= MAX_COMPOSITOR_DISPLAY_COMMANDS {
        return Err(CompositorDisplayListError::CapacityExceeded);
    }
    commands.push(command);
    Ok(())
}

fn surface_chrome_border(
    committed: &CommittedSurfaceState,
    role: SurfaceChromeRole,
    width: i32,
    color: CompositorRgb8,
) -> Option<CompositorBorder> {
    let inner = committed.geometry;
    if inner.is_empty() || width <= 0 {
        return None;
    }
    let doubled = width.checked_mul(2)?;
    let outer = Rect {
        x: inner.x.checked_sub(width)?,
        y: inner.y.checked_sub(width)?,
        width: inner.width.checked_add(doubled)?,
        height: inner.height.checked_add(doubled)?,
    };
    Some(CompositorBorder {
        node: CompositorNodeId::SurfaceChrome {
            surface: committed.surface,
            role,
        },
        generation: surface_chrome_generation(inner, width, color, role),
        outer,
        inner,
        color,
    })
}

pub fn compositor_floating_outline(
    surface: SurfaceId,
    geometry: Rect,
    width: i32,
    color: CompositorRgb8,
) -> Option<CompositorBorder> {
    if !surface.is_valid() || geometry.is_empty() || width <= 0 {
        return None;
    }
    let committed = CommittedSurfaceState {
        surface,
        committed_generation: 0,
        geometry,
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::None,
            sophia_protocol::Size {
                width: geometry.width,
                height: geometry.height,
            },
        ),
        damage: Region::empty(),
    };
    surface_chrome_border(&committed, SurfaceChromeRole::FloatingOutline, width, color)
}

pub fn compositor_border_bands(border: CompositorBorder) -> [CompositorSolidRect; 4] {
    let outer = border.outer;
    let inner = border.inner;
    [
        CompositorSolidRect {
            geometry: Rect {
                height: inner.y.saturating_sub(outer.y),
                ..outer
            },
            color: border.color,
        },
        CompositorSolidRect {
            geometry: Rect {
                y: inner.y.saturating_add(inner.height),
                height: outer
                    .y
                    .saturating_add(outer.height)
                    .saturating_sub(inner.y.saturating_add(inner.height)),
                ..outer
            },
            color: border.color,
        },
        CompositorSolidRect {
            geometry: Rect {
                y: inner.y,
                width: inner.x.saturating_sub(outer.x),
                height: inner.height,
                ..outer
            },
            color: border.color,
        },
        CompositorSolidRect {
            geometry: Rect {
                x: inner.x.saturating_add(inner.width),
                y: inner.y,
                width: outer
                    .x
                    .saturating_add(outer.width)
                    .saturating_sub(inner.x.saturating_add(inner.width)),
                height: inner.height,
            },
            color: border.color,
        },
    ]
}

fn push_border_damage(damage: &mut Region, border: CompositorBorder) {
    for band in compositor_border_bands(border) {
        if !band.geometry.is_empty() {
            damage.push(band.geometry);
        }
    }
}

fn surface_chrome_generation(
    geometry: Rect,
    width: i32,
    color: CompositorRgb8,
    role: SurfaceChromeRole,
) -> u64 {
    let mut generation = 0xcbf2_9ce4_8422_2325u64;
    for byte in geometry
        .x
        .to_le_bytes()
        .into_iter()
        .chain(geometry.y.to_le_bytes())
        .chain(geometry.width.to_le_bytes())
        .chain(geometry.height.to_le_bytes())
        .chain(width.to_le_bytes())
        .chain([color.red, color.green, color.blue, role as u8])
    {
        generation = (generation ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
    }
    generation.max(1)
}

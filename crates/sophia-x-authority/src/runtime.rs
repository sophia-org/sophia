use std::collections::{BTreeMap, BTreeSet};
use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use sophia_portal::ClipboardPortal;
use sophia_protocol::{
    AuthoritySurface, NamespaceId, OutputTopologyError, OutputTopologySnapshot, Rect, Region, Size,
    TransactionId,
};

use crate::{
    ClipboardSelectionDispatch, ClipboardSelectionExecutionError,
    ClipboardSelectionExecutionOutcome, ClipboardSelectionFailureRequest,
    ClipboardSelectionHandoff, ClipboardSelectionNotify, ClipboardSelectionProxy,
    ClipboardSourcePayload, ClipboardTextProperty, PendingClipboardSelection, X_ATOM_ATOM,
    X_ATOM_NONE, XAtomTable, XAuthorityCpuBufferUpdate, XAuthorityPortalCommand,
    XAuthorityRasterCommand, XAuthorityRasterStore, XAuthorityRequestKind, XAuthorityRequestPacket,
    XAuthorityResponsePacket, XAuthorityRuntimeError, XAuthoritySelectionArtifact, XByteOrder,
    XDrawingUpdate, XFontHandle, XGraphicsContextTable, XGraphicsContextValues, XOwnedTextDraw,
    XPoint, XPropertyChange, XPropertyMode, XPropertyTable, XPutImageSemantics, XRasterPoint,
    XRasterUnsupportedKind, XResourceKind, XResourceTable, XSelectionEvent, XSelectionMonitor,
    XShmSegmentTable, XSoftwareBufferStore, XTextDraw, XWindowLifecycleEvent, XWindowTable,
    clipboard_selection_failure_notify, dispatch_clipboard_selection_request,
    surface_transaction_from_drawing_update,
};

include!("runtime/clipboard.rs");
include!("runtime/color.rs");
include!("runtime/drawing.rs");
include!("runtime/graphics_contexts.rs");
include!("runtime/drawing/copy_plane.rs");
include!("runtime/drawing/image_ops.rs");
include!("runtime/render_resources.rs");
include!("runtime/dmabuf_capabilities.rs");
include!("runtime/device_connections.rs");
include!("runtime/render_pictures.rs");
include!("runtime/render_picture_lifetime.rs");
include!("runtime/pixmap_publication.rs");
include!("runtime/render_glyphs.rs");
include!("runtime/render_traps.rs");
include!("runtime/xfixes_regions.rs");
include!("runtime/shape.rs");
include!("runtime/sync.rs");
include!("runtime/windows.rs");
include!("runtime/window_allocation.rs");
include!("runtime/window_cursors.rs");
include!("runtime/glx_resources.rs");
include!("runtime/pointer_query.rs");

/// Effects of releasing every currently supported resource allocated from one
/// X11 client connection's setup range.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XAuthorityClientResourceRelease {
    /// X11 windows whose properties must be removed from the frontend table.
    pub destroyed_windows: Vec<crate::XResourceId>,
    /// Selection ownerships this client's departure ended.
    ///
    /// Carried out with the release rather than left on the runtime's shared
    /// queue: that queue is drained by whichever request next reaches it, so
    /// another connection could take these, reorder them against this
    /// teardown, or route them before this release has finished.
    pub retired_selection_ownerships: Vec<crate::XSelectionOwnerUpdate>,
    /// Sophia surfaces that must be removed from Engine's committed snapshot.
    pub removed_surfaces: Vec<sophia_protocol::SurfaceId>,
    pub released_pixmaps: usize,
    pub released_fonts: usize,
    pub released_cursors: usize,
    pub released_colormaps: usize,
    pub released_graphics_contexts: usize,
    pub released_shm_segments: usize,
    pub released_glx_contexts: usize,
    pub released_glx_windows: usize,
    /// Renderer-visible DRI3 sources released by disconnect cleanup.
    pub released_dma_bufs: Vec<sophia_protocol::BufferHandle>,
    /// Renderer-visible xshmfences released by disconnect cleanup.
    pub released_fences: Vec<sophia_protocol::FenceHandle>,
}

#[derive(Clone, Debug)]
struct XShmPixmapBinding {
    offset: u32,
    size: Size,
    mapping: Arc<sophia_sysv_shm::ClientMapping>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct XPixmapRecord {
    size: Size,
    depth: u8,
}

/// One DRI3-imported pixmap: the facts it was imported with, and the plane
/// descriptors it was imported from.
///
/// The descriptors are kept because DRI3 asks for them back. A client that
/// imported a pixmap may call `BuffersFromPixmap` to recover the same buffer,
/// and the authority cannot borrow the renderer's copy to answer: the renderer
/// import boundary owns keeping its handles out of protocol authorities. So the
/// authority keeps its own, for exactly as long as the pixmap lives.
#[derive(Clone, Debug)]
struct XDri3PixmapRecord {
    descriptor: sophia_protocol::DmaBufDescriptor,
    plane_fds: Vec<Arc<OwnedFd>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct XFontRecord {
    face: XFontHandle,
}

/// What kind of thing a drawable id names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XDrawableKind {
    Root,
    Window,
    Pixmap,
    /// An offscreen GLX surface. It answers geometry, and nothing draws into it.
    GlxPbuffer,
}

/// The facts every drawable can answer, whatever kind it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XDrawableFacts {
    pub kind: XDrawableKind,
    pub geometry: Rect,
    pub depth: u8,
}

/// One GLX drawable's bookkeeping.
///
/// GLX owns no pixels here. A window alias borrows its geometry from the X window
/// it names; anything else has to carry its own, because nothing else does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct XGlxDrawableRecord {
    owner: NamespaceId,
    fbconfig: u32,
    backing: XGlxDrawableBacking,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum XGlxDrawableBacking {
    Window(crate::XResourceId),
    /// An offscreen surface. Sophia stores no pixels for it, so it carries the
    /// extent it was created with; nothing else knows one.
    Pbuffer(Size),
    /// A GLX drawable over an X pixmap, and the texture target it binds to.
    ///
    /// It names the backing rather than copying its extent, so a pixmap that
    /// outlives its XID keeps answering through the one record that retained
    /// it, and a GLX pixmap cannot drift from the pixels it wraps. The target
    /// is resolved once here, because the extent that admits it is the extent
    /// at creation.
    Pixmap {
        pixmap: crate::XResourceId,
        texture: crate::XGlxPixmapTexture,
    },
}

#[derive(Debug)]
pub struct XAuthorityRuntime {
    resources: XResourceTable,
    windows: XWindowTable,
    shm_segments: XShmSegmentTable,
    selections: XSelectionMonitor,
    clipboard: ClipboardPortal,
    pending_clipboard: BTreeMap<sophia_protocol::PortalTransferId, PendingClipboardSelection>,
    clipboard_proxies: BTreeMap<crate::XResourceId, ClipboardSelectionProxy>,
    next_clipboard_proxy: u32,
    software_buffers: XSoftwareBufferStore,
    raster_store: XAuthorityRasterStore,
    pending_raster_command: Option<XAuthorityRasterCommand>,
    pixmaps: BTreeMap<crate::XResourceId, XPixmapRecord>,
    fonts: BTreeMap<crate::XResourceId, XFontRecord>,
    /// The font path and the faces loaded from it. Indexed once at startup
    /// and never changed by a client request.
    font_catalog: crate::XFontCatalog,
    shm_pixmaps: BTreeMap<crate::XResourceId, XShmPixmapBinding>,
    shm_mappings: BTreeMap<u32, Weak<sophia_sysv_shm::ClientMapping>>,
    /// The live mapping for each descriptor-backed segment.
    ///
    /// Held here rather than on the segment record because a record is cloned
    /// and compared, and a mapping is neither. Dropped when the segment is
    /// detached or its client goes away, which is what unmaps it.
    shm_descriptor_mappings: BTreeMap<crate::XResourceId, Arc<sophia_sysv_shm::ClientMapping>>,
    /// Descriptors a `CreateSegment` reply still owes its client, held only
    /// until the socket layer puts them on the wire.
    shm_reply_descriptors: BTreeMap<crate::XResourceId, std::os::fd::OwnedFd>,
    dri3_pixmaps: BTreeMap<crate::XResourceId, XDri3PixmapRecord>,
    next_dma_buf_handle: u64,
    dri3_fences: BTreeMap<crate::XResourceId, sophia_protocol::FenceHandle>,
    sync_counters: BTreeMap<crate::XResourceId, i64>,
    xfixes_regions: BTreeMap<crate::XResourceId, Region>,
    render_pictures: BTreeMap<crate::XResourceId, XRenderPictureRecord>,
    retained_pixmap_backings: BTreeMap<crate::XResourceId, XRetainedPixmapBacking>,
    next_render_backing: u64,
    /// Renderer registrations owed a release once their backing was dropped.
    ///
    /// Queued rather than released here: the provider call must leave the
    /// runtime lock, and an owed release is retried rather than discarded.
    pending_backing_releases: std::collections::VecDeque<sophia_protocol::BufferHandle>,
    provider_pixmap_backings: std::collections::BTreeSet<sophia_protocol::BufferHandle>,
    /// Per-backing publication state: what it owes, and the one update in the
    /// air for it.
    pixmap_publications: BTreeMap<sophia_protocol::BufferHandle, XPixmapPublication>,
    pixmap_export_handles: BTreeMap<crate::XResourceId, sophia_protocol::BufferHandle>,
    pixmap_publication_targets: BTreeMap<u64, XPixmapPublicationTarget>,
    next_pixmap_publication_target: u64,
    retired_pixmap_registrations: BTreeMap<NamespaceId, Vec<sophia_protocol::BufferHandle>>,
    /// Selection ownerships ended by a window or client going away, waiting to
    /// be told to the watchers that asked about them.
    ///
    /// These transitions happen inside the runtime rather than through a
    /// request of their own, so they are collected here and drained by the
    /// layer that knows who subscribed.
    retired_selection_ownerships: Vec<crate::XSelectionOwnerUpdate>,
    /// Glyph-set resource ids, each naming a shared store. Two ids name one
    /// store after `ReferenceGlyphSet`.
    render_glyphsets: BTreeMap<crate::XResourceId, u64>,
    render_glyph_stores: BTreeMap<u64, XRenderGlyphStore>,
    /// Cursor images a client supplied through RENDER. Stored so the resource
    /// is real and FreeCursor means something; display stays config-driven.
    render_cursor_images: BTreeMap<crate::XResourceId, XRenderCursorImage>,
    window_shapes: BTreeMap<crate::XResourceId, XWindowShapeState>,
    /// Which client is watching which window's shape, mirrored here so the
    /// `InputSelected` reply can be answered from dispatch.
    shape_selections: BTreeSet<(u64, crate::XResourceId)>,
    next_glyph_store: u64,
    next_fence_handle: u64,
    graphics_contexts: XGraphicsContextTable,
    window_background_pixels: BTreeMap<crate::XResourceId, u32>,
    window_visuals: BTreeMap<crate::XResourceId, (u8, u32, crate::XResourceId)>,
    /// The cursor each window asks for, absent when it shows its parent's.
    window_cursors: BTreeMap<crate::XResourceId, crate::XResourceId>,
    /// Windows created InputOnly. They take input and geometry requests but
    /// have no pixels, so the drawing family refuses them.
    input_only_windows: BTreeSet<crate::XResourceId>,
    window_allocation: XWindowAllocationState,
    colormaps: BTreeMap<crate::XResourceId, u32>,
    glx_contexts: BTreeMap<crate::XResourceId, (NamespaceId, u32, bool)>,
    glx_drawables: BTreeMap<crate::XResourceId, XGlxDrawableRecord>,
    last_cpu_buffer_updates: Vec<XAuthorityCpuBufferUpdate>,
    output_topology: OutputTopologySnapshot,
    input_focus: BTreeMap<NamespaceId, (crate::XResourceId, u8)>,
    /// The last-focus-change time, kept per namespace beside the focus it
    /// orders rather than once for the whole server as the protocol
    /// describes it. The protocol has one focus so it has one time; we have
    /// one focus per namespace, and a single shared time would let a change
    /// in one namespace make an honest request in another look stale.
    last_focus_change: BTreeMap<NamespaceId, crate::XTimestamp>,
    #[cfg(unix)]
    private_focus_source: Option<crate::x11_socket::XPrivateFocusRuntimeSource>,
    defer_policy_maps: bool,
    /// Whether the provider keeps pixmap backings a GL client can sample.
    ///
    /// Set once when the frontend is built and never again: `GetFBConfigs` and
    /// `QueryExtensionsString` are answered once per client, so a value that
    /// moved would leave clients holding configurations no longer honoured.
    pixmap_textures_supported: bool,
    dma_buf_import_formats: Option<BTreeMap<u32, Vec<u64>>>,
    device_connections: BTreeMap<u64, Option<std::sync::Arc<crate::XServerFrontendDeviceBundle>>>,
    xkb_keymap: crate::XkbKeymapSnapshot,
    input_authority: Arc<Mutex<crate::XInputAuthorityState>>,
}

impl Default for XAuthorityRuntime {
    fn default() -> Self {
        Self {
            resources: Default::default(),
            windows: Default::default(),
            shm_segments: Default::default(),
            selections: Default::default(),
            clipboard: Default::default(),
            pending_clipboard: Default::default(),
            clipboard_proxies: Default::default(),
            next_clipboard_proxy: 0,
            software_buffers: Default::default(),
            raster_store: Default::default(),
            pending_raster_command: None,
            pixmaps: Default::default(),
            fonts: Default::default(),
            font_catalog: crate::XFontCatalog::builtin_only(),
            shm_pixmaps: Default::default(),
            shm_mappings: Default::default(),
            shm_descriptor_mappings: Default::default(),
            shm_reply_descriptors: Default::default(),
            dri3_pixmaps: Default::default(),
            next_dma_buf_handle: 1,
            dri3_fences: Default::default(),
            sync_counters: Default::default(),
            xfixes_regions: Default::default(),
            render_pictures: Default::default(),
            retained_pixmap_backings: Default::default(),
            pending_backing_releases: Default::default(),
            provider_pixmap_backings: Default::default(),
            pixmap_publications: Default::default(),
            pixmap_export_handles: Default::default(),
            pixmap_publication_targets: Default::default(),
            next_pixmap_publication_target: 1,
            retired_pixmap_registrations: Default::default(),
            retired_selection_ownerships: Default::default(),
            next_render_backing: u64::from(u32::MAX) + 1,
            render_glyphsets: Default::default(),
            render_glyph_stores: Default::default(),
            render_cursor_images: Default::default(),
            window_shapes: Default::default(),
            shape_selections: Default::default(),
            next_glyph_store: 1,
            next_fence_handle: 1,
            graphics_contexts: Default::default(),
            window_background_pixels: Default::default(),
            window_visuals: Default::default(),
            window_cursors: Default::default(),
            input_only_windows: Default::default(),
            window_allocation: Default::default(),
            colormaps: Default::default(),
            glx_contexts: Default::default(),
            glx_drawables: Default::default(),
            last_cpu_buffer_updates: Vec::new(),
            output_topology: OutputTopologySnapshot::deterministic(),
            input_focus: Default::default(),
            last_focus_change: Default::default(),
            #[cfg(unix)]
            private_focus_source: None,
            defer_policy_maps: false,
            pixmap_textures_supported: false,
            dma_buf_import_formats: None,
            device_connections: BTreeMap::new(),
            xkb_keymap: crate::XkbKeymapSnapshot::new(&crate::XkbRmlvoConfig::default())
                .expect("the deterministic default XKB keymap must compile"),
            input_authority: Arc::new(Mutex::new(crate::XInputAuthorityState::default())),
        }
    }
}

impl XAuthorityRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_xkb_config(
        config: &crate::XkbRmlvoConfig,
    ) -> Result<Self, crate::XkbKeyboardError> {
        Ok(Self {
            xkb_keymap: crate::XkbKeymapSnapshot::new(config)?,
            ..Self::default()
        })
    }

    pub const fn xkb_keymap(&self) -> &crate::XkbKeymapSnapshot {
        &self.xkb_keymap
    }

    pub fn set_policy_map_deferred(&mut self, deferred: bool) {
        self.defer_policy_maps = deferred;
    }

    pub fn set_pixmap_textures_supported(&mut self, supported: bool) {
        self.pixmap_textures_supported = supported;
    }

    pub const fn pixmap_textures_supported(&self) -> bool {
        self.pixmap_textures_supported
    }

    pub fn input_authority_mut(&self) -> MutexGuard<'_, crate::XInputAuthorityState> {
        self.input_authority
            .lock()
            .expect("X11 input authority lock poisoned")
    }

    pub fn set_input_authority(
        &mut self,
        input_authority: Arc<Mutex<crate::XInputAuthorityState>>,
    ) {
        self.input_authority = input_authority;
    }

    pub(crate) fn shared_input_authority(&self) -> Arc<Mutex<crate::XInputAuthorityState>> {
        self.input_authority.clone()
    }

    pub fn with_output_topology(
        output_topology: OutputTopologySnapshot,
    ) -> Result<Self, OutputTopologyError> {
        output_topology.validate()?;
        Ok(Self {
            output_topology,
            ..Self::default()
        })
    }

    /// Index a font path into this runtime's catalog.
    ///
    /// Done once, at construction, from session configuration. The catalog is
    /// never reindexed afterwards, so a directory changing underneath a
    /// running session cannot change what a client resolves.
    pub fn index_font_path(&mut self, font_path: &[std::path::PathBuf]) {
        self.font_catalog = crate::XFontCatalog::index(font_path);
    }

    pub fn with_output_topology_and_xkb_config(
        output_topology: OutputTopologySnapshot,
        xkb_config: &crate::XkbRmlvoConfig,
    ) -> Result<Self, String> {
        output_topology
            .validate()
            .map_err(|error| format!("invalid Engine output topology: {error:?}"))?;
        Ok(Self {
            output_topology,
            xkb_keymap: crate::XkbKeymapSnapshot::new(xkb_config)
                .map_err(|error| format!("invalid XKB configuration: {error}"))?,
            ..Self::default()
        })
    }

    pub fn output_topology(&self) -> &OutputTopologySnapshot {
        &self.output_topology
    }

    pub fn update_output_topology(
        &mut self,
        output_topology: OutputTopologySnapshot,
    ) -> Result<bool, OutputTopologyError> {
        output_topology.validate()?;
        if output_topology.generation <= self.output_topology.generation {
            return Ok(false);
        }
        self.output_topology = output_topology;
        Ok(true)
    }

    /// Reserves the namespace's default focus storage during connection setup,
    /// before any worker or common-held private focus producer is exposed.
    /// Existing focus survives another connection in the same namespace. This
    /// prepares storage only; it publishes no applied-focus authority.
    pub(crate) fn prepare_input_focus_namespace(&mut self, namespace: NamespaceId) {
        self.input_focus.entry(namespace).or_insert((
            crate::XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1),
            1,
        ));
    }

    #[cfg(unix)]
    pub(crate) fn bind_private_focus_source(
        &mut self,
        source: crate::x11_socket::XPrivateFocusRuntimeSource,
    ) -> Result<(), XAuthorityRuntimeError> {
        if let Some(bound) = self.private_focus_source.as_ref() {
            if !bound.same_origin(&source) {
                return Err(XAuthorityRuntimeError::FocusAuthorityUnavailable);
            }
        } else {
            self.private_focus_source = Some(source);
        }
        Ok(())
    }

    pub fn input_focus(&self, namespace: NamespaceId) -> (crate::XResourceId, u8) {
        self.input_focus.get(&namespace).copied().unwrap_or((
            crate::XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1),
            1,
        ))
    }

    /// Private effect entry: preparation is an admission prerequisite, never
    /// permission to allocate missing namespace state while common is held.
    pub(crate) fn set_prepared_input_focus(
        &mut self,
        namespace: NamespaceId,
        focus: crate::XResourceId,
        revert_to: u8,
    ) -> Result<(), XAuthorityRuntimeError> {
        if revert_to > 2 {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        if focus.local.raw() != 0 && focus.local.raw() != u64::from(crate::X_SETUP_DEFAULT_ROOT) {
            self.validate_window_access(namespace, focus)?;
        }
        let prepared = self
            .input_focus
            .get_mut(&namespace)
            .ok_or(XAuthorityRuntimeError::FocusAuthorityUnavailable)?;
        *prepared = (focus, revert_to);
        Ok(())
    }

    /// Everything SetInputFocus must refuse, with no state changed either way.
    ///
    /// Split out from the effect because X11 orders the two: the errors are
    /// reported whatever the request's timestamp says, and only a request
    /// that would otherwise succeed is then measured against the clock.
    pub fn validate_input_focus(
        &self,
        namespace: NamespaceId,
        focus: crate::XResourceId,
        revert_to: u8,
    ) -> Result<(), XAuthorityRuntimeError> {
        if revert_to > 2 {
            return Err(XAuthorityRuntimeError::InvalidResource);
        }
        if focus.local.raw() != 0 && focus.local.raw() != u64::from(crate::X_SETUP_DEFAULT_ROOT) {
            self.validate_window_access(namespace, focus)?;
        }
        Ok(())
    }

    /// The namespace's last-focus-change time, zero before its first change.
    #[must_use]
    pub fn last_focus_change(&self, namespace: NamespaceId) -> crate::XTimestamp {
        self.last_focus_change
            .get(&namespace)
            .copied()
            .unwrap_or(crate::X_CURRENT_TIME)
    }

    /// Whether a focus request bearing `time` may take effect, and at what
    /// instant it would be recorded.
    ///
    /// X11 orders focus changes by the client's stated time rather than by
    /// arrival, so a request is discarded outright when it names a moment
    /// before the last change this namespace already made, or one the server
    /// has not reached yet. Discarded is not an error: the protocol says such
    /// a request has no effect, so the client is owed no reply, no error and
    /// no events. `CurrentTime` is the client declining to name a moment and
    /// passes both bounds, taking the server's own clock as its instant.
    #[must_use]
    pub fn focus_time_admits(
        &self,
        namespace: NamespaceId,
        time: crate::XTimestamp,
        server_time: crate::XTimestamp,
    ) -> Option<crate::XTimestamp> {
        if time == crate::X_CURRENT_TIME {
            return Some(server_time);
        }
        if crate::x_time_is_after(self.last_focus_change(namespace), time)
            || crate::x_time_is_after(time, server_time)
        {
            return None;
        }
        Some(time)
    }

    /// Records the instant a focus change took effect at. Only a request that
    /// names a time does this: reversion moves the focus without moving the
    /// clock, so a later request that was honest when it was sent still lands.
    pub fn note_focus_change(&mut self, namespace: NamespaceId, time: crate::XTimestamp) {
        self.last_focus_change.insert(namespace, time);
    }

    pub fn set_input_focus(
        &mut self,
        namespace: NamespaceId,
        focus: crate::XResourceId,
        revert_to: u8,
    ) -> Result<(), XAuthorityRuntimeError> {
        self.validate_input_focus(namespace, focus, revert_to)?;
        self.input_focus.insert(namespace, (focus, revert_to));
        Ok(())
    }

    pub fn begin_dispatch(&mut self) {
        self.last_cpu_buffer_updates.clear();
        self.pending_raster_command = None;
    }

    /// Takes every immutable CPU-buffer mutation produced by one authority
    /// dispatch. A surface transaction may publish multiple density variants,
    /// so dispatch ownership is a bounded ordered collection rather than a
    /// singleton side channel.
    pub fn take_cpu_buffer_updates(&mut self) -> Vec<XAuthorityCpuBufferUpdate> {
        core::mem::take(&mut self.last_cpu_buffer_updates)
    }

    /// Compatibility accessor for direct runtime tests and single-buffer
    /// callers. Production dispatch uses [`Self::take_cpu_buffer_updates`].
    pub fn take_cpu_buffer_update(&mut self) -> Option<XAuthorityCpuBufferUpdate> {
        if self.last_cpu_buffer_updates.is_empty() {
            None
        } else {
            Some(self.last_cpu_buffer_updates.remove(0))
        }
    }

    pub fn apply(&mut self, request: XAuthorityRequestPacket) -> XAuthorityResponsePacket {
        match self.apply_checked(&request) {
            Ok(response) => response,
            Err(error) => {
                let mut response = XAuthorityResponsePacket::rejected(request.transaction, error);
                if let XAuthorityRequestKind::RequestSelection {
                    requestor,
                    selection,
                    target,
                    time,
                    transfer,
                    ..
                } = request.kind
                {
                    response
                        .selection_artifacts
                        .push(XAuthoritySelectionArtifact::Failure(
                            clipboard_selection_failure_notify(ClipboardSelectionFailureRequest {
                                transfer,
                                requestor,
                                selection,
                                target,
                                time,
                            }),
                        ));
                }
                response
            }
        }
    }

    fn apply_checked(
        &mut self,
        request: &XAuthorityRequestPacket,
    ) -> Result<XAuthorityResponsePacket, XAuthorityRuntimeError> {
        let mut response = XAuthorityResponsePacket::accepted(request.transaction);

        match &request.kind {
            XAuthorityRequestKind::CreateWindow {
                window,
                surface,
                geometry,
                constraints,
                generation,
            } => {
                self.resources.insert(
                    *window,
                    XResourceKind::Window,
                    request.namespace,
                    *generation,
                )?;
                if let Some(surface) = self.windows.apply(XWindowLifecycleEvent::Created {
                    id: *window,
                    surface: *surface,
                    namespace: request.namespace,
                    geometry: *geometry,
                    constraints: *constraints,
                    generation: *generation,
                })? {
                    response.surfaces.push(surface);
                }
            }
            XAuthorityRequestKind::MapWindow { window, generation } => {
                self.resources
                    .lookup(request.namespace, *window, XResourceKind::Window)?;
                let role = self
                    .windows
                    .get(*window)
                    .ok_or(XAuthorityRuntimeError::UnknownResource)?
                    .presentation_role();
                let event = if role == sophia_protocol::SurfacePresentationRole::ClientPositioned
                    || !self.defer_policy_maps
                {
                    XWindowLifecycleEvent::Mapped {
                        id: *window,
                        generation: *generation,
                    }
                } else {
                    XWindowLifecycleEvent::PolicyPending {
                        id: *window,
                        generation: *generation,
                    }
                };
                if let Some(surface) = self.windows.apply(event)? {
                    response.surfaces.push(surface);
                }
            }
            XAuthorityRequestKind::PresentPixmap {
                window,
                pixmap,
                damage,
                previous_committed_generation,
                timeout_msec,
            } => {
                let mut transaction = surface_transaction_from_drawing_update(
                    &self.windows,
                    XDrawingUpdate::present_pixmap(
                        request.transaction,
                        request.namespace,
                        *window,
                        *pixmap,
                        damage.clone(),
                        *previous_committed_generation,
                        *timeout_msec,
                    ),
                )?;
                transaction.input_region =
                    match self.effective_shape(*window, crate::X_SHAPE_KIND_INPUT) {
                        (true, rects) => Some(Region { rects }),
                        (false, _) => None,
                    };
                self.windows
                    .advance_generation(*window, *previous_committed_generation)?;
                response.transactions.push(transaction);
            }
            XAuthorityRequestKind::SetSelectionOwner {
                selection,
                owner,
                timestamp,
                selection_timestamp,
                kind,
            } => {
                if let Some(owner) = owner {
                    self.resources
                        .lookup(request.namespace, *owner, XResourceKind::Window)?;
                }
                let update = self.selections.apply_event_in_namespace(
                    XSelectionEvent {
                        selection: *selection,
                        owner: *owner,
                        timestamp: *timestamp,
                        selection_timestamp: *selection_timestamp,
                        kind: *kind,
                    },
                    &self.windows,
                    Some(request.namespace),
                );
                if let Some(previous_owner) = update.previous.and_then(|record| record.owner)
                    && Some(previous_owner) != *owner
                {
                    response
                        .selection_artifacts
                        .push(XAuthoritySelectionArtifact::Clear {
                            owner: previous_owner,
                            selection: *selection,
                            time: *timestamp,
                        });
                }
            }
            XAuthorityRequestKind::RequestSelection {
                requestor,
                selection,
                target,
                target_name,
                property,
                time,
                transfer,
            } => {
                self.resources
                    .lookup(request.namespace, *requestor, XResourceKind::Window)?;
                let dispatch = dispatch_clipboard_selection_request(
                    crate::XSelectionRequest {
                        requestor: *requestor,
                        selection: *selection,
                        target: *target,
                        target_name: target_name.clone(),
                        property: *property,
                        time: *time,
                    },
                    &self.selections,
                    &self.windows,
                    *transfer,
                    &mut self.clipboard,
                )?;
                match dispatch {
                    ClipboardSelectionDispatch::SameNamespace(request) => response
                        .selection_artifacts
                        .push(XAuthoritySelectionArtifact::Request(request)),
                    ClipboardSelectionDispatch::CrossNamespace {
                        portal_request,
                        command,
                    } => {
                        self.pending_clipboard.insert(
                            *transfer,
                            PendingClipboardSelection {
                                namespace: request.namespace,
                                portal_request,
                                byte_order: XByteOrder::LittleEndian,
                            },
                        );
                        if let Some(command) = XAuthorityPortalCommand::from_portal_command(command)
                        {
                            response.portal_commands.push(command);
                        }
                    }
                }
            }
        }

        Ok(response)
    }
}

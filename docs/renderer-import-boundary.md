# Renderer Import Boundary

The renderer import boundary is separate from backend discovery. Output
discovery answers "what displays exist?" Input discovery answers "what devices
exist?" Renderer import answers "can this already-validated surface buffer become
a renderer-private texture for this frame?"

Sophia keeps those questions apart. A backend may discover `/dev/dri` and input
devices without owning GBM, EGL, DMA-BUF import, or MIT-SHM mapping. The renderer
boundary is admitted only after the engine has a ready `SurfaceTransaction` with
matching geometry and buffer identity.

[Multi-Monitor Per-Head Composition](multi-monitor-composition.md) extends this
rule to bounded `SurfaceContentSet` values. Each selected variant still enters
through this import boundary; per-head composition does not move raw handles or
import authority into Engine.

## Ownership

The engine owns:

- atomic validation of geometry and buffer readiness;
- committed visual state;
- frame planning and render reports;
- protocol-neutral `BufferSource` and `BufferImportPath` values.

The live renderer boundary owns:

- deciding whether a source can use a native import path;
- falling back or deferring when the path is not available;
- keeping renderer-private handles out of WM IPC, portals, and protocol
  authorities;
- reporting the reduced import outcome for tests and runtime observations.

The live renderer boundary does not own:

- output discovery;
- input polling;
- protocol authority parsing;
- client protocol or namespace policy;
- client metadata.

## Current Admission Rule

CPU-backed uploads are the only always-accepted path. `XPixmap` and `DmaBuf`
sources are reduced records at the Engine boundary; their raw handles never
leave the native renderer boundary. Live backend startup therefore defaults to
the CPU path. Native import-capable rendering must be selected through startup
configuration; it is not implied by discovering a DRM/KMS output.

The target multi-monitor adapter treats an existing `BufferSource` as a
singleton surface-content set. When an authority supplies several immutable
variants for one committed content generation, Engine selects a variant in the
head plan and this boundary imports only that selection. Import caches may
share an immutable source across heads on a compatible render device, while
native render targets and KMS leases remain per head. Import-path fallback and
sampling quality are orthogonal: CPU fallback for the same exact variant does
not make its density inexact, while selecting a different variant requires a
new or updated head plan and the matching sampling classification. An import
failure cannot silently bind another content generation.

An imported presentation does not imply that X Authority can semantically
rerasterize it. Standard pixmap and DRI3 Present therefore invalidate any older
authority replay journal and derived CPU variants for that surface. Density
demand falls back with a reduced cause until a new full semantic baseline is
recorded; stale commands are never replayed as exact content, and this private
limitation is not a session-fatal resource error. A client DMA-BUF rendered at
the selected head's exact extent remains exact independently of replay.

Persistent renderer-image IDs preserve semantic source identity but their
backing imports remain local to one physical head's renderer store. When a
topology assigns retained content to a head that has not rendered it, the
native owner must realize that identity in the target store from a
compositor-owned snapshot before submitting the head's worker request. It may
not borrow another head's cache entry or scanout-buffer lease, and failure to
find or restore a donor rejects the topology before KMS mutation.

## Capture execution and immutable storage

Each renderer image still receives a fresh capture surface and BO. An exported
snapshot can outlive the client's pixmap, the local image-store entry and the
context that captured it. Eviction is therefore not permission to reuse its
storage.

The EGL owner retains at most two compatible capture contexts and pipelines,
one for XR24 and one for AR24. A new size updates the drawing extent; a changed
EGL configuration replaces the cached context. Each capture imports the actual
client descriptors into a fresh, one-entry cache, copies the full source, flushes
the copy, clears that import and unbinds the context. Cross-device transfer
keeps its explicit completion fence. Cleanup or rendering failure invalidates
execution reuse; a surface-allocation failure before binding can try the next
modifier with the same context. CPU upload textures are allocated only when a
draw needs them.

This reuses execution resources without pooling capture storage or retaining
mutable client imports. Per-head composition targets, image custody, KMS
retirement and transfer completion retain their existing owners.

## Damage provenance

Engine retains up to 16 transitions per surface, 32 rectangles per transition
and 8 MiB of damage metadata per journal or frame view. A view refuses more
than 1024 source surfaces. Candidate endpoints take priority over older history
when a view reaches its byte budget.

A prepared commit has an opaque identity which survives its successful commit.
Public generations and buffer handles alone cannot identify pixels: a rejected
candidate and a later commit can reuse both. A frame snapshot retains the
identities of its sources, including preview-only sources. A precise chain
must connect the retained slot's identity through every intervening commit.
The final edge must also reach the target preparation's identity, including
when an unassociated view reuses an older generation and buffer.
Missing, evicted or unproved history falls back to full surface damage.
Synchronization and recovery replacements clear the journal; rebased client
transactions carry full raster damage.

The first precise path is canonical, unscaled, unclipped, Normal-transform
sampling. X Authority supplies source-pixel rectangles only for an unoffset
top-level DMA-BUF Present without a valid-region override. Other mappings keep
full damage. A changed source also damages its preview instances.
Density-selected or scaled alternate variants therefore retain correctness but
do not receive this partial-damage optimization.

Both native head-composition paths and CPU repaint carry the exact in-flight
preparation into the frame snapshot. A view without an associated preparation
gets a fresh local identity which cannot masquerade as a later commit. The CPU
raster has a separate key: it follows CPU registry uploads and compositor
content, excluding GPU-only generations. Equal-generation uploads and buffer
eviction invalidate its retained baselines. Busy CPU frame bytes remain
immutable.

## Measuring render work

Session emits cumulative `sophia_live_render_work schema=1` counters on its
existing resource-sampling cadence. They distinguish CPU raster reuse, capture
context reuse, fresh capture surfaces, transfers and full/partial composition.
Repaint pixels describe the actual repaint after buffer-age selection. Shared
worker-context counters are counted once using the newest observation; output
slot counters remain per output.
Replacing a context can lower its cumulative counters. Window-delta analysis
must treat a decreasing counter as a reset rather than negative work.

`SOPHIA_RENDER_TIMING=1` additionally records elapsed and calling-thread CPU
time for CPU composition and native setup/copy/cleanup/composition. Normal
rendering does not take these extra per-frame clocks. Thread time excludes
driver helper threads, and the root context's stage timings exclude work in a
separate transfer context. Transfer counters identify that limitation. These
measurements contain no client text, handles, paths or pixel data.

The render-node tests compare partial and full pixels through buffer age and
rejected preparations, and check exported snapshots after local eviction and
context destruction. The fixed-cadence capture and CPU-scene benchmarks measure
individual paths; they do not establish a live desktop's total CPU use, KMS
behaviour or input latency.

Startup reports expose only reduced renderer import health: CPU fallback, native
import capable, or degraded. Per-path status is reduced to disabled, enabled, or
degraded for XPixmap and DMA-BUF. No renderer-private handle, file descriptor,
device path, or client buffer identity belongs in that health report.

`sophia-backend-live` consumes that startup health when it builds its live
runtime assembly wrapper. Each tick report can carry the same reduced renderer
observation beside the engine's protocol-neutral tick report. The engine remains
free of renderer-live dependencies, and runtime consumers still learn whether
the session is using CPU fallback or a native import-capable renderer.

Degraded renderer health has two sources. Startup capability probes can mark a
path degraded before the session starts, and per-frame import failures can
degrade runtime observation after startup. Both are modeled through deterministic
fake paths in `sophia-renderer-live`, while the native scanout adapter owns the
experimental GBM/EGL DMA-BUF implementation and its renderer-private resource
caches.

MIT-SHM is now a bounded authority-owned upload path rather than a renderer
import. The X frontend validates namespace and segment bounds, attaches the
client SysV segment read-only, copies the admitted range into an immutable CPU
buffer generation, and detaches immediately. Neither Engine nor the renderer retains the client mapping. Accepted immutable
replacements and packed damage patches enter a renderer-owned CPU buffer
registry. The registry rejects malformed size/stride/byte bounds, missing patch
bases, metadata changes, and stale generations before composition; it retains
only handles referenced by the transitional scene projection.

Standard X11 DRI3 1.2 and Present are the admitted GPU handoff. Renderer-private
registries retain imported multi-plane DMA-BUF sources and fences; mixed CPU and
GPU composition, controlled acquire delay and rejection recovery, KMS page-flip
retirement, Copy Idle-before-Complete feedback, idle-fence triggering, and exact
cleanup pass the retained Milestone 4 X13 gate. The retired Wayland prototype's
controlled linear DMA-BUF proofs are historical evidence under
`research/wayland`.

The shared lifecycle is explicit:

`admitted descriptor → renderer-private import/submission → observed page flip
→ protocol completion/release → renderer resource retirement`.

`sophia-backend-live` remains the owner of renderer/KMS adapters and native
resources. The production session coordinator orders those adapters; it does not
move their handles into Engine or runtime state.

## Native Dependency Admission

The live runtime wrapper stays outside `sophia-engine`. That is a deliberate
boundary: the engine owns protocol-neutral state and deterministic frame
validation, while backend-live and renderer-live own live startup health and
renderer capability facts. Move renderer policy into the engine only if atomic
visual correctness requires it.

The first real native renderer dependency candidate is a GBM capability probe,
not full EGL rendering and not DMA-BUF import. GBM is the smallest useful probe
because it can establish whether the live renderer can speak to a DRM render
device and create renderer-private allocation context. That probe must be gated
behind an optional crate feature and must not be required for the default
workspace test suite.

The initial `gbm-probe` feature admits only the safe `gbm` crate and keeps it
behind an optional feature. It exposes fake probes for deterministic default
coverage and native reduced probes for feature-enabled validation.

The GBM probe API uses a backend-provided reduced render-device token. It does
not accept a public device path, and it does not expose borrowed file descriptors
through Sophia's stable data boundary. Backend-live may own the real device
opening later; renderer-live receives only the reduced token needed to report
capability health. This keeps raw kernel authority out of engine state, WM IPC,
portals, and protocol authorities.

The feature-enabled local validation command is documented in
`docs/validation.md`. The GBM crate is admitted only through that path. Real
device probing is available only through backend-owned fd-like authority. It
still collapses native failures into reduced degraded health and does not expose
fds, paths, GBM handles, or native errors through Sophia's public data model.
Backend-live reports render-device discovery separately as reduced path-free
state so startup diagnostics can distinguish "not requested" from "unavailable"
without leaking device identity.

Backend-live also reports a reduced GPU startup status. This status is more
specific than renderer health, but still contains no path, fd, handle, or driver
message. It can distinguish:

- GPU startup was not requested;
- the backend could not open a render device;
- GBM rejected the opened device;
- the opened GBM device could not allocate the first private renderer buffer;
- native GPU startup is capable.

Renderer startup policy is explicit:

- `GpuPreferred` attempts GBM and selects CPU fallback when the probe degrades;
- `CpuOnly` never opens a render device;
- `GpuRequired` fails closed when GBM does not prove native capability.

Degraded GBM does not produce a partial import-capable renderer. This keeps the
atomic visual path honest: native import is either capable, or the session runs
through the fallback renderer with reduced degraded startup health.

Native GBM capability requires more than opening a GBM device or checking a
format flag. The probe must also allocate and immediately drop a tiny
renderer-private buffer. The allocation result is reduced to startup health; the
buffer object, handle, driver error, and fd never cross the adapter boundary.

Admission tests for the first real dependency must prove:

- the crate still builds and tests offline without the feature;
- fake degraded capability coverage remains the default test path;
- absence of GBM produces reduced degraded health, not a panic;
- failed private GBM allocation produces reduced degraded health;
- no raw file descriptor, device path, or renderer-private handle crosses into
  `sophia-engine`, WM IPC, portals, or protocol authorities.

Sophia should not expose a third-party GBM crate directly through its public
renderer-live API. The native binding belongs behind a tiny adapter module that
translates from reduced Sophia tokens into reduced capability health. This keeps
crate-specific handles, error types, lifetime rules, and unsafe requirements
contained inside renderer-live.

The adapter module owns:

- native GBM crate imports behind the `gbm-probe` feature;
- fake GBM capability probes for deterministic default-style coverage;
- reduced native GBM capability probes for feature-enabled validation.
- tiny private allocation probes that are dropped before returning reduced
  health.

The adapter module may later own:

- conversion from backend-owned device authority into renderer-private probe
  context;
- reduced degraded-health mapping for missing devices, unsupported GBM
  operations, or native probe errors.

It must not export:

- raw GBM handles;
- raw file descriptors;
- device paths;
- native error payloads;
- renderer-private allocation objects.

## Candidate Dependency

The first admitted candidate is the safe `gbm` crate, currently documented as `gbm`
0.18.0 on docs.rs. It wraps `libgbm`, documents itself as safe GBM bindings, and
depends on `gbm-sys`. Its optional `drm-support` feature is useful later, but
the first Sophia probe should not require it. The initial probe only needs to
prove that renderer-live can compile the native GBM boundary and translate a
reduced render-device token or backend-owned device authority into reduced GBM
capability health. Capability includes a private allocation check, not just
device construction. Backend-live wires that authority through reduced startup
reports and renderer preference policy.

`gbm-sys` is not the first choice. It exposes raw FFI functions and low-level GBM
types directly. Keep it as a fallback only if the safe `gbm` crate cannot support
the narrow capability probe without pulling in broader rendering or DRM policy.

Admission notes for the admitted `gbm` dependency:

- keep it only as an optional dependency under the existing `gbm-probe` feature;
- keep `default = []`;
- keep all native GBM calls inside a private adapter module;
- map native errors to reduced degraded health;
- keep the fake probe tests as the default path;
- run `cargo test --workspace --offline` without feature flags;
- run `cargo test --offline -p sophia-renderer-live --features gbm-probe`
  separately for native probe changes;
- run `cargo test --offline -p sophia-backend-live --features gbm-probe`
  separately for backend-owned device discovery changes.

## EGL Context Boundary

EGL/OpenGL is Sophia's first compositor drawing API above the GBM platform
boundary. GBM remains responsible for render-device authority and allocation
capability; EGL is only the context/drawing layer that sits on top of a proven
platform.

The first `egl-probe` feature adds fake reduced records for platform readiness
and context readiness, then projects those records through backend-live as
reduced startup status. It also admits a native dynamic EGL probe behind an
internal adapter crate. The public renderer-live and backend-live APIs still
expose only reduced startup records.

The EGL probe boundary must not expose:

- EGL displays, contexts, configs, or surfaces;
- GBM devices, buffers, handles, paths, or file descriptors;
- native driver error text;
- renderer-private allocation objects.

The first native EGL draw smoke proves a private clear-color path. It creates a
private 1x1 pbuffer target, creates a context, makes that context current
against the pbuffer, loads GL procedures inside the adapter, clears the target,
and tears everything down inside the native adapter. It does not compile
shaders, export buffers, present frames, or hand native handles to
renderer-live.

When both `gbm-probe` and `egl-probe` are enabled, backend-live may project a
reduced GBM startup report into EGL platform status. Degraded GBM startup maps
to degraded EGL platform status; it must not become native drawing capability.
wgpu remains deferred until GBM/EGL startup, drawing, and presentation seams are
proven.

## GBM-Backed EGL Platform Candidate

The current native EGL clear-color smoke uses `DEFAULT_DISPLAY` only as a
temporary platform probe. The first production-shaped Linux path must move EGL
onto backend-owned GBM authority: backend-live discovers and opens the render
device, renderer-live proves GBM capability with a reduced token or owned fd,
and the native EGL adapter creates an EGL display from the private GBM device.

This is a platform handoff, not a public handle handoff. The Engine, WM,
protocol authorities, and safe renderer-live API must still see only reduced
status. They must not see the GBM device, EGL display, driver path, fd, surface,
native visual, or driver error text.

The admitted reduced platform shape is `LiveGbmBackedEglPlatformReport`. It
projects GBM startup into EGL platform readiness without exposing the render
device or EGL objects. The native platform smoke proves that backend-owned
device authority can become a private GBM device, that EGL can create and
initialize a GBM platform display from that private state, and that teardown can
complete without exposing the display.

The GBM-backed private target smoke now reuses that private platform to create a
1x1 GBM surface, create an EGL window surface from the private GBM surface, make
an OpenGL context current, clear the target, optionally cross an offscreen
presentation boundary with `eglSwapBuffers`, and tear everything down. It
returns either the existing reduced `EglDrawSmokeReport` shape or the reduced
`LiveRendererPresentationReport` shape. It does not lock a GBM front buffer,
export a DMA-BUF, create a KMS framebuffer, or present to scanout.
The path is:

1. Backend-live discovers a render node and holds the opened device authority.
2. Renderer-live proves GBM native capability with a private allocation smoke.
3. The native EGL adapter receives only backend-owned authority, creates a GBM
   platform display, initializes it, chooses a window-capable config, creates a
   private GBM/EGL target, clears it, optionally swaps it through the offscreen
   presentation smoke, and tears it down.
4. Buffer export, front-buffer locking, and scanout stay out of scope until the
   next boundary admits them.

Admission rules for the GBM-backed EGL platform:

- keep it behind the combined `gbm-probe,egl-probe` feature path;
- require GBM native capability before attempting GBM-backed EGL;
- return reduced platform status only: ready, unavailable, degraded, or context
  unavailable;
- keep all fd ownership, GBM devices, EGL displays, configs, surfaces, native
  error text, and driver details inside live adapters;
- do not expose DMA-BUF, KMS framebuffer IDs, GBM buffer handles, EGLImages, or
  GL object names;
- keep `DEFAULT_DISPLAY` as a fallback host smoke until GBM-backed drawing is
  exercised against real render nodes in validation.

`DEFAULT_DISPLAY` retirement requires evidence, not intent. Sophia may remove
the fallback host smoke only after all of these are true:

- `SOPHIA_RUN_REAL_GBM_SMOKE=1` passes the GBM capability, GBM-backed EGL draw,
  and offscreen presentation smokes on a real render node;
- the same command remains green for repeated local runs after a clean build;
- native driver crashes stay isolated to the child smoke process and report as
  opt-in validation failures;
- public reports still expose only reduced GBM, EGL, presentation, and scanout
  readiness status;
- the reduced scanout-adjacent readiness report has a successor page-flip event
  shape that does not expose KMS object identity.

Until then, `DEFAULT_DISPLAY` remains a compatibility smoke. It is not the
production compositor platform, but it proves the dynamic EGL and GL loading
path on machines where backend-owned GBM/EGL cannot yet create a stable private
target.

Decision after the first repeated real GBM/EGL validation pass: keep
`DEFAULT_DISPLAY` as a compatibility smoke. The production-shaped renderer path
is GBM-backed EGL, but `DEFAULT_DISPLAY` still catches host EGL loader issues on
machines where backend-owned render-node validation is unavailable or not yet
trusted across a broader device matrix.

Retirement requires at least Intel, AMD, degraded-driver, and restricted/headless
host evidence. Each result must use the reduced real-GBM evidence shape; device
paths, handles, driver errors, pixels, and KMS identity stay out of project
records.

Rejected shortcuts:

- using `DEFAULT_DISPLAY` as the production compositor platform: it does not
  prove Sophia controls the backend-owned render device;
- exporting a GBM handle to renderer-live so safe code can assemble EGL state:
  that would leak renderer-private authority across the boundary;
- moving directly to DMA-BUF or scanout before the reduced presentation status
  exists.

## Presentation Smoke Boundary

Presentation is the next boundary after private drawing. A presentation smoke
must prove that renderer-live can advance a frame to a reduced presentation
status without exposing the GPU objects that made it happen.

The first native presentation smoke is not real KMS scanout. It is a
renderer-owned, offscreen GBM/EGL boundary that models the final
PageFlipCommitGate shape: a frame is staged, `eglSwapBuffers` reaches the
presentation boundary, and the public result says only whether presentation was
ready, unavailable, or degraded.

The presentation smoke must not expose:

- GBM buffer handles or buffer object pointers;
- DRM framebuffer IDs, connector IDs, CRTC IDs, plane IDs, or device paths;
- DMA-BUF fds, modifiers, fences, or EGLImages;
- native driver errors or GL object names;
- frame pixels or readback data.

The admitted public shape is `LiveRendererPresentationReport` with only three
statuses: ready, unavailable, or degraded. The fake presentation smoke exercises
those statuses without native dependencies. The native offscreen smoke preserves
the same shape while keeping GBM surfaces, EGL surfaces, driver errors, pixels,
and GL object names private. Only after this reduced shape holds under real
render-node validation should Sophia admit scanout-facing code. This keeps the
macOS-style invariant intact: the Engine can learn that a presentation boundary
happened, but it cannot accidentally present partially assembled native state
through a leaky renderer API.

The backend may project presentation into a scanout-adjacent readiness report,
but that report stays outside renderer-private state. `LiveScanoutReadinessReport`
is reduced to ready, output unavailable, presentation unavailable, or degraded.
It is not a KMS page-flip result and must not contain connector IDs, CRTC IDs,
plane IDs, framebuffer IDs, fds, paths, driver errors, or framebuffer handles.
`LiveKmsScanoutTargetReport` is the first reduced KMS target report. It combines
output availability, reduced frame-target status, and presentation status into
ready, output unavailable, frame-target unavailable, invalid frame target,
presentation unavailable, or degraded. It may carry the intended target size,
but no output ID, connector ID, CRTC ID, plane ID, framebuffer ID, fd, path,
driver error, or KMS handle. Page-flip readiness is derived from this target
report so invalid or missing frame targets cannot be treated as ready.

Real GBM-backed validation evidence is reduced through
`LiveRealGbmSmokeEvidence`. It records only whether the opt-in smoke passed, the
reduced EGL draw-smoke status, the reduced presentation status, and the reduced
frame-target allocation status. It does not store the render-node path, fd,
GBM/EGL objects, driver errors, pixels, renderer-private frame-target handles,
or KMS identity.

The next production-shaped step after offscreen presentation evidence is a
reduced GBM/EGL frame-target record. `LiveGbmEglFrameTargetRecord` records only
the intended target size and whether that size is valid. It is deliberately not
a GBM surface, EGL surface, framebuffer, DMA-BUF, or scanout object. Future
renderer integration can use it to agree on target dimensions before native
allocation code exists.
Backend-live may project the selected startup output into this record because
the projection contains no renderer-private authority. That projection is a
startup readiness hint, not a native allocation request.
Backend-live runtime may also update the record from an output-size change. The
update accepts only `Size`, derives the reduced status again, and does not own
or borrow native renderer resources.

The first allocation seam is `LiveGbmEglFrameTargetAllocator`. It accepts a
`LiveGbmEglFrameTargetAllocationRequest` and returns only
`LiveGbmEglFrameTargetAllocationReport`: reduced allocation status plus the
reduced target record. Implementations may allocate native GBM/EGL resources
privately, but those handles must not appear in renderer-live, backend-live, or
runtime observations.
Backend-live observes this seam by invoking the allocator with its current
reduced target and storing only the returned reduced report. A target-size
change invalidates the retained report.
Frame-target lifecycle is also reduced before it leaves renderer/backend code.
`LiveGbmEglFrameTargetLifecycleReport` records only the reduced target and one
of five states: created, retained, resized, invalidated, or retired. A retained
target may keep the previous reduced allocation report; resize, invalidation,
and retirement clear it. Runtime ticks may report the current reduced lifecycle
state, but they must not allocate native frame targets implicitly.

Production native composition treats a complete render target as the private
lease unit. The target owns its EGL context, selected configuration, GL
pipeline, GBM surface, and matching EGL window surface. Its exported buffer
retains the scanout surface until page-flip retirement; the one-shot context
and pipeline are destroyed rather than rebound to a different surface. Physical
AMDGPU evidence rejected both same-surface reuse and retained-context use
across fresh surfaces. Reduced evidence reports target and surface creation
counts separately without exposing native objects.

The first native skeleton is `NativeGbmBackedEglFrameTargetAllocator`, gated
behind the existing GBM/EGL probe features. It accepts a backend-owned render
device result and a reduced allocation request, performs any native work inside
the renderer adapter, and returns only the reduced allocation report.
Backend-live exposes this through feature-gated helpers so startup and runtime
code can test the native seam without importing renderer-private handles.
`LiveBackendRuntimeAssembly` may invoke the native seam explicitly and retain
only the reduced allocation report. A runtime tick must not allocate native
frame targets implicitly.

The next reduced boundary is `LivePageFlipEvent`. It can be derived from
scanout readiness or from the engine's `PageFlipCommitOutcome`, but it drops the
output ID, transaction ID, surface IDs, commit payload, and all native KMS
identity. Presented and rejected events retain only the frame serial needed to
relate the event to the compositor timeline.

## EGL Candidate Dependency

The admitted native EGL dependency is `khronos-egl` 6.0.0. It binds the Khronos
EGL 1.5 API, exposes an explicit `Instance` API, and supports dynamic loading
through its `dynamic` feature. Sophia admits it only inside
`sophia-renderer-native-egl`, a tiny internal adapter crate that owns the
unavoidable unsafe FFI calls. `sophia-renderer-live` depends on that adapter
only when `egl-probe` is enabled.

Admission rules for `khronos-egl`:

- keep it optional under the existing `egl-probe` feature path;
- use dynamic loading so missing `libEGL.so.1` becomes reduced startup
  failure instead of a hard link/load failure;
- keep all EGL displays, contexts, configs, surfaces, errors, and loaded
  library handles private to the native adapter;
- project native failures only to reduced EGL startup status;
- keep fake EGL tests as the default feature-enabled coverage path;
- run `cargo test --workspace --offline` without feature flags;
- run `cargo test --offline -p sophia-renderer-live --features egl-probe`;
- run `cargo test --offline -p sophia-backend-live --features egl-probe`;
- run `cargo test --offline -p sophia-backend-live --features gbm-probe,egl-probe`.

Rejected candidates:

- `egl` 0.2.7: older low-level binding with 0% docs.rs documentation, and the
  `khronos-egl` docs describe it as left unmaintained;
- `glutin` 0.32.3: documented and useful for applications, but it is a broad
  cross-platform OpenGL context abstraction and pulls in policy beyond the
  first narrow EGL probe;
- Smithay EGL helpers: useful reference material for compositor architecture,
  but adopting Smithay would pull in a large compositor framework surface before
  Sophia has proven its own reduced GBM/EGL boundary.

## GL Function Loading Dependency

The admitted GL function loading dependency is `glow` 0.17.0. Sophia admits it
only inside `sophia-renderer-native-egl` for the first clear-color smoke: load
GL entry points from the current EGL context, clear a private 1x1 pbuffer, and
return only reduced smoke status to renderer-live.

Admission rules for `glow`:

- keep GL loading optional under the existing `egl-probe` feature path;
- keep it inside `sophia-renderer-native-egl`, after the EGL context is current;
- load procedures only through the adapter's EGL procedure lookup path;
- do not expose `glow::Context`, GL procedure pointers, GL object names,
  shaders, textures, framebuffers, programs, native error strings, or pbuffer
  details outside the adapter;
- limit the first smoke to setting a clear color, clearing the current private
  target, and flushing or finishing as required for a deterministic reduced
  result;
- map every native GL failure to reduced `GlUnavailable` draw-smoke status;
- keep wgpu deferred until GBM/EGL startup, drawing, and presentation seams are
  proven.

Rejected GL-loading candidates for the first smoke:

- `gl` 0.14.0: its global `load_with` model and generated unsafe function
  surface are broader than the current adapter needs;
- `gl_generator` 0.14.0: generated bindings may be useful later, but they add a
  build-time binding surface before Sophia needs it;
- raw manual `eglGetProcAddress` calls: smallest in dependency count, but they
  would hand-roll procedure pointer safety and duplicate a tested loader
  boundary.

## Failure Shape

Unsupported import paths fail closed as reduced decisions. They do not panic, do
not partially start the compositor, and do not cause protocol authorities or the
window manager to see renderer-private state. The session may still render via a
safe fallback when one exists.


## Committed Snapshot Composition

CPU pixel bytes live in `LiveCpuBufferRegistry`, but visual identity does not.
The production path must pass the immutable Engine `CommittedSurfaceState` slice
to composition after authority commit. Geometry, buffer selection, generation,
and stacking are resolved from that slice; the registry supplies bytes only for
its referenced CPU handles and retires everything else. A CLI or renderer-local
`SurfaceId` projection is not an admissible cache because it can diverge from a
rejected or removed Engine transaction. Native KMS initialization waits for this
first committed-state frame rather than requiring a speculative or blank visual
bootstrap.

## Generational Renderer-Image Residency

Mixed composition assigns every admitted client buffer an opaque
`LiveRendererImageId`. The client DMA-BUF is a capture source only. Before the
output frame is assembled, the native renderer copies it into a same-format
GBM image owned by the renderer context; the temporary client EGLImage and GL
texture are then destroyed. Retained scene records carry only the image ID,
size, format, and placement. They never retain client file descriptors.

Captured images enter a staged table bounded to 256 entries and 512 MiB. The
exact mixed-frame page flip promotes the staged image, retires the client
source, and emits X Present Copy with Idle before Complete. Export or submit
failure removes a staged image. Replacing or removing a displayed surface
first evicts every output import of its renderer image and then drops the
compositor-owned GBM backing store. Capacity exhaustion fails closed; the
renderer never evicts a live image to make room.

The renderer-image table belongs to the native context rather than its output
composition target, so target recreation may clear and rebuild EGL imports
without losing displayed content. Import counts and cache validation remain
separate from snapshot captures, promotions, rollbacks, evictions, live
entries, and live bytes. These metrics make a future persistent capture-target
optimization measurable without weakening the ownership boundary.

A full native-owner replacement is a renderer-generation transition, not an
output-target recreation. Before a planned seat release, the backend drains
native work and asks the old renderer to duplicate DMA-BUF leases for the exact
set of promoted images retained by runtime state. The leases contain no client
source, EGL object, GBM context, KMS device, or protocol metadata. The
replacement renderer copies those snapshots into its bounded table under the
same opaque IDs and promotes them before native resume may queue the first
retained frame.

Resume admission requires exact, unique image-ID coverage. A missing handoff,
an extra or duplicate ID, or a failed restore terminates the transition rather
than allowing an `InvalidTarget` frame to poison future composition. An
unplanned device revoke, where safe export is no longer guaranteed, clears the
stale displayed-image records and waits for client repaint. This is a degraded
content path, but it preserves renderer and input liveness without pretending
that the destroyed generation remains usable.

The handoff is intentionally compatible with a future persistent render-node
owner. Once rendering is independent of seat-scoped primary/KMS authority, the
same admission contract can transfer an existing renderer generation without
copying pixels. That optimization must also export scanout plane FDs for KMS
import; renderer-private GEM handles are not valid in a different DRM file.

# Compositor Graphics

**Role:** normative compositor-owned graphics architecture.

This document defines how Sophia describes and renders compositor-owned visual
content. [Architecture](architecture.md) defines visual authority and process
ownership. [Engine Architecture](engine-architecture.md) places this graphics
pipeline inside the complete Engine domain flow. [Data-Oriented Design](dod.md)
defines the records that cross those boundaries. [Renderer Import
Boundary](renderer-import-boundary.md) defines how client buffers enter the
renderer. [Multi-Monitor Per-Head
Composition](multi-monitor-composition.md) defines how the display list and
client layers are lowered separately for each physical head. Compositor
graphics share the final composition and presentation path with those buffers,
but they are not client surfaces and do not weaken the authority boundaries
around them.

## Design Direction

Shell rendering and compositor rendering have separate owners. A GPU-capable
shell may rasterize its own interface under an explicit launch permission and
submit renderer-neutral images through the content protocol. That does not
expose the compositor scene or make Vello/wgpu a Sophia dependency. The
[execution decision](notes/decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md)
keeps GPU readback plus immutable CPU-byte transfer as the first Lom path.
Engine-owned upload/backing charges remain bounded through retirement, while
direct client GPU access carries no hard aggregate VRAM guarantee. A future
GPU job broker is separate from the trusted compositor-effect provider below;
neither is a new public shader-upload interface by implication.

Sophia uses a small, renderer-neutral display list for compositor-owned
content. The native renderer lowers that list into specialized EGL/OpenGL
primitives and cached textures. It does not expose graphics-API objects to
Engine, the WM, metadata brokers, portals, or protocol authorities.

The intended path is:

```text
 sanitized metadata     Engine/session state       portal decisions
         │                       │                         │
         └───────────────────────┼─────────────────────────┘
                                 ▼
                  compositor-owned semantic nodes
                                 │
                 bounded immutable display list
                                 │
                ┌────────────────┴────────────────┐
                ▼                                 ▼
      native EGL/OpenGL lowering          CPU/reference lowering
      shaders + cached textures           deterministic validation
                │                                 │
                └────────────────┬────────────────┘
                                 ▼
               ordinary Sophia frame composition
                                 │
                         GBM buffer → DRM/KMS
```

The display list describes visual intent rather than drawing commands for one
graphics API. Its initial vocabulary should remain deliberately small:

- solid rectangles;
- rounded rectangles;
- rounded or gradient borders;
- analytic rounded-rectangle shadows;
- image and cached text textures;
- clips, opacity, placement, and stacking;
- optional offscreen groups when an effect or grouped animation requires them.

This vocabulary covers focus rings, title bars, tab strips, trust badges,
notifications, portal prompts, workspace indicators, selection overlays, and
other shell-owned UI without turning Engine into a general-purpose UI toolkit.
New primitives require demonstrated compositor use. One-off visual novelty is
not sufficient reason to expand the stable Engine boundary.

New additions to this display list are governed by two principles:

1. **The Compositing Operator Rule:** The Engine only admits a
   primitive if it represents a mathematical 2D compositing operation that the
   client physically cannot execute itself due to security (pixel blindness). For
   example, a `BackdropBlurNode` must be an Engine primitive because the shell is
   forbidden from reading the underlying window pixels to perform the blur. Conversely,
   custom visual novelties (like audio visualizers or widgets) must be rasterized
   entirely by the client.
2. **Bandwidth Preservation and Resolution Independence:** Primitives
   such as analytic rounded rectangles, borders, and shadows are admitted because computing
   them in a fragment shader saves massive memory and bus bandwidth compared to uploading
   large, flat client-rasterized textures on every frame.

### Degradation Contract

Only a visual effect declared optional may be skipped or degraded, and its
committed fallback must remain legible. Mandatory content—including text,
controls, and trust indications—cannot disappear silently. For a per-head
composition cohort, mandatory content must be produced on every required head;
failure on one head rejects the complete candidate before submission rather
than presenting unequal semantics. Performance targets must name a workload,
hardware class, and measurement method; this architecture does not promise one
refresh rate across all hardware.

“Shell-owned” is an ownership rule, not a product description. It covers a
small status bar and a full set of panels and decorations alike. The display
list describes the resulting visual intent without learning which kind of
session asked for it.

## Ownership

Sophia Engine owns:

- the semantic compositor nodes included in a frame;
- their stable opaque identities, generations, ordering, geometry, and damage;
- validation of chrome actions against the matching committed surface;
- frame scheduling and the atomic relationship between client content, chrome,
  and the frame submitted for presentation.

The metadata broker and shell own:

- the disclosure rule that decides how much of a label an authority may emit;
- icon tokens, trust state, and attention state, which are cross-authority facts;
- proposing bounded compositor content from those sanitized facts;
- shell interaction policy that does not belong to the external WM.

They do not own the reduction itself. An authority applies the published rule to
text it already holds, so raw identity never crosses to the broker.

The native renderer owns:

- GL programs, uniform layouts, textures, atlases, and offscreen targets;
- lowering semantic nodes to solid draws, shader elements, or texture draws;
- text/image raster caches and their renderer-private lifetime;
- composition with imported CPU and DMA-BUF client layers;
- reduced failure and capability reports.

The backend owns:

- render-device and output authority;
- GBM allocation and scanout ownership;
- DRM/KMS submission, page-flip observation, and final resource retirement.

The external WM does not receive compositor display lists, sanitized labels,
icons, text, pixels, renderer handles, or shader parameters. It continues to
propose layout using opaque surface and workspace facts. A compositor close
button remains an Engine/session action routed to the owning protocol authority,
not a WM command.

## Visual Policy And Effect Extensibility

Except for the existing `WmChromePolicy` slice, this section describes target
architecture. Effect intents, the provider registry, and visual providers are
not implemented.

The compositor role remains inside Engine, but the desktop's visual identity
does not. A WM or shell family may define its own styling and effect policy
without making that policy part of Sophia's universal Engine. The boundary has
three forms:

| Visual form | Policy owner | Execution path |
| --- | --- | --- |
| Artwork or a novel widget that does not sample the scene | shell | rasterize once where possible, transfer as bounded content-addressed content, and reuse as an Engine-composited texture |
| A recurring mathematical compositing operation | WM or shell through a role-appropriate semantic intent | Engine validates and lowers a stable compositor operator |
| A specialized operation that needs compositor pixels or custom renderer code | WM or shell selects an admitted semantic capability | a trusted, installed visual provider lowers it behind Engine's renderer boundary |

The existing `WmChromePolicy` is the first narrow metadata-blind styling
surface: a WM may choose frame and focus-ring policy for allocations it already
owns. Revision 3 of `sophia_wm_v1` remains frozen. Richer WM-authored styling
requires a separately authorized companion shell or a later outbound-gated WM
extension; it does not justify exposing display lists, shader parameters, or
metadata through the frozen interface. A shell may propose richer display-list
content and transitions from the sanitized facts its role already holds.

Public policy protocols carry semantic visual intent, never renderer programs.
An effect intent must name a negotiated capability, bounded parameters, a
generation, and a committed legible fallback. It cannot carry shader source,
SPIR-V, native handles, procedure pointers, arbitrary renderer state, or client
pixels. Engine resolves the intent against the provider set admitted for that
session, validates its bounds and freshness, and includes the resolved effect
in the same immutable frame candidate as client content and ordinary chrome.

The first provider boundary is deliberately source-level rather than a stable
runtime ABI. A visual provider is a separately maintained Rust module linked
into the trusted renderer build behind a private, version-coupled provider
trait. The packaged desktop profile selects and hashes the provider set as a
release input before the session starts. A shell, WM, application, or mutable
personal configuration cannot upload or load provider code at runtime. Dynamic
libraries and a sandboxed effect host remain possible later designs, but neither
is promised until multiple provider implementations demonstrate the required
ABI, isolation, synchronization, and failure behavior.

That private implementation seam does not weaken the language neutrality of
the [Sophia Native Protocol Family](sophia-policy-ipc.md). WM and shell
authors select semantic capabilities through their role IPC and never link the
provider trait. Ordinary visual identity—
layout, color, artwork, cached content, and admitted transitions—must remain
expressible without provider code. A family that needs a genuinely new
scene-sampling or renderer-specialized operation may maintain a separately
packaged provider, but that is renderer integration rather than a requirement
for implementing the WM or shell protocol.

A provider is implementation, not authority. It receives only the bounded
renderer-private input needed to lower an already validated effect. It does not
receive protocol objects, raw metadata, physical input, policy state, DRM/KMS
ownership, or permission to schedule or present a frame. Engine owns effect
lifetime, offscreen allocation, damage, capability degradation, and the
animation clock. WMs and shells declare transition targets and policy; they do
not submit frame-by-frame animation timing.

Effects are optional embellishment. Mandatory text, controls, trust state, and
security surfaces remain independently renderable. An unknown capability,
malformed parameter set, stale generation, or unavailable provider rejects the
affected candidate or renders its committed fallback. A provider lowering
failure emits only a reduced report and uses that fallback when it can still
produce a complete frame; otherwise Engine preserves the prior coherent scene.

An active overlay or effect that samples existing scene pixels, uses an
offscreen group, or otherwise changes the final composed image makes that exact
frame ineligible for direct scanout. Engine records composition as required,
and the backend remains responsible for the final format/modifier atomic test.
Removing the effect may restore eligibility on a later independently validated
frame. Providers cannot bypass either decision.

## Native Rendering Strategy

The native implementation should extend Sophia's existing EGL/OpenGL
composition path. Recurring geometry is rendered directly with small,
purpose-built shader programs:

- solid color needs no intermediate texture;
- rounded fills and borders use analytic distance calculations;
- shadows use an analytic rounded-rectangle shadow where that visual is
  sufficient;
- gradients use explicit, bounded shader parameters;
- clips use geometry or scissor state appropriate to the primitive;
- opacity uses Sophia's premultiplied-alpha composition convention.

On a multi-head desktop, these primitives remain semantic until the logical
scene has fanned out into head plans. Every head lowers them at its native
target size and density. Repositioning or filtering primitives from a flattened
primary-output image is not per-head composition. Imported client content may
still require a selected raster variant or an explicit sampling fallback, as
defined by [Multi-Monitor Per-Head
Composition](multi-monitor-composition.md).

Text-heavy or layout-heavy panels should be rasterized outside the frame hot
path and uploaded as cached premultiplied-alpha textures. Text shaping and
rasterization are separate from GPU composition. Cache keys must include every
fact that changes pixels, including content generation, scale, font/style
selection, color, wrapping constraints, and relevant locale or direction.
Stable content should reuse its texture until one of those facts changes.

Offscreen rendering is an explicit tool rather than the default. It is
appropriate when a group must fade as one unit, when an effect consumes already
composed pixels, or when reuse avoids repeated work. Each offscreen allocation
must have bounded dimensions and renderer-owned lifetime.

The renderer may choose different implementations for the same semantic node
when capabilities differ. Degradation must remain deterministic and reduced:
for example, a shadow may be omitted or simplified according to explicit
policy, but native failure must not leak GL errors or renderer objects across
the boundary.

## Damage And Atomic Presentation

Compositor-owned nodes participate in the same frame and damage model as client
layers. Creating, removing, moving, restyling, or changing the opacity of a node
damages both its previous and current extents. A stable node with an unchanged
generation must not force full-output damage.

Chrome does not get a presentation shortcut. A visual response to focus,
attention, trust, portal, or metadata state becomes visible only through an
Engine-planned frame and the ordinary rendered scanout lifecycle. When chrome
is attached to a client surface, its geometry must be derived from the same
committed surface state used for that frame. Pending client geometry must not
move committed chrome ahead of matching client pixels.

In the target multi-monitor path, each head composition carries the immutable
display list used to produce its pixels. Engine owns a bounded per-head
`pending → rendering → submitted → presented` ledger for those lists and a
logical-output cohort that joins required heads. New damage is computed against
that head's in-flight submitted list when one exists, otherwise against its
last presented list. A superseded or failed pending frame cannot advance the
baseline; an accepted KMS submit moves the list to submitted, and only its
page-flip callback makes it presented. A mirror output publishes logical
presentation only after every required head reaches that state. This keeps
compositor damage synchronized with the pixels and retirement event it
describes even when head sizes differ.

Every CPU or mixed frame also carries an immutable output-damage snapshot. It
combines ordered opaque surface IDs, committed client generations, geometry,
buffer identity, the compositor display list, and optional software-cursor
bounds. Stacking, creation, removal, generation, geometry, buffer, chrome, and
software-cursor changes therefore enter one retirement-safe region. Initial
state and output size or scale changes force full-output damage. A hardware
cursor remains on its independently retired plane and does not create false
primary-plane damage.

Before native consumption, combined damage is reduced to an output-local
repaint plan. Rectangles are clipped to the output and coalesced only when
their union remains an exact rectangle. Empty damage becomes `skip`; bounded
low-coverage damage becomes `partial`; excessive input count, fragmentation,
or coverage becomes a full-output fallback. The default policy admits at most
32 partial rectangles and switches to full output at 60 percent coverage.
These are generic Engine policy values, not WM-, application-, toolkit-, or
protocol-specific behavior. A failed or incomplete proof increases work
rather than risking stale pixels.

Animations are Engine-clocked state. The Engine or session reducer determines
the semantic state for each frame; the renderer only draws that immutable
state. The WM, metadata broker, and renderer do not drive independent animation
timelines. Primary-plane content is coalesced by a monotonic deadline derived
from the active output refresh. Authority commits may replace the pending
state, but unrelated input wakeups cannot create, postpone, or accelerate that
deadline. Pointer motion reaches the independently retired hardware cursor
path immediately; when a software cursor is required it enters the same
bounded primary repaint as other visual state.

Cursor styling follows the same ownership rule. Configuration selects a theme,
nominal size, and semantic shape; the trusted session resolves one bounded,
immutable asset and passes its pixels and hotspot to both render backends. A
renderer never looks up theme names, and a WM or shell never supplies cursor
pixels or KMS state. Xcursor animation currently resolves deterministically to
its first closest-size frame and reports the ignored-frame count; independent
animated cursor scheduling is not yet part of the public visual vocabulary.

## Text And Metadata Safety

Only sanitized, bounded text may reach compositor text layout. Protocol-local
titles, classes, paths, namespace identity, and arbitrary markup do not pass
directly into the renderer. Markup, if Sophia admits it for a specific shell
surface, must be generated from trusted compositor templates with untrusted
content escaped.

Text caches and diagnostics must not become metadata side channels. Cache
identity remains renderer-private. Reduced reports may expose counts, sizes,
generations, cache outcomes, or timings, but not rendered strings, glyph
content, client titles, paths, or texture bytes.

## Current And Target State

### Implemented

- Engine frame plans carry ordered client layers with targets, clips,
  transforms, opacity, and damage.
- Engine owns a bounded renderer-neutral display list with ordered surface,
  semantic rectangular-border, generic solid-rectangle, and sanitized-text
  commands, stable semantic node identities, generations, and old/new extent
  damage.
- Focus rings and frames use one stable Engine-owned clearance inside the WM's
  outer allocation. Client content is inset by that clearance, so chrome never
  covers client pixels or a neighboring allocation. Frame and ring commands
  precede their client surface and lower through one fixed four-band geometry
  system without exposing chrome policy to a protocol frontend.
- The CPU/reference renderer lowers ordered surface and solid commands into
  deterministic XRGB pixels. The native EGL renderer lowers the same solid
  command with a clipped opaque draw and no intermediate texture allocation.
- The native GBM/EGL path composes CPU and DMA-BUF textures using rectangular
  placement, scissor clipping, scaling, and premultiplied-alpha blending.
- The production path exports the rendered GBM front buffer and retains its
  resources through KMS page-flip retirement.
- CPU and mixed CPU/DMA-BUF frames retain their immutable display-list
  identity through composition and cloning. The per-output Engine ledger
  computes exact chrome damage and advances it through accepted submit and
  page-flip retirement; reduced native evidence reports the retired rectangle
  count.
- Engine combines client generation/geometry/stacking, compositor-node, and
  software-cursor changes in one bounded output snapshot attached to the
  corresponding pixels. The same accepted-submit/page-flip ledger advances
  that snapshot.
- Engine owns a refresh-relative primary-frame pacer. Busy content is
  latest-wins until the monotonic deadline, while physical-input turns remain
  outside that cadence; reduced session evidence reports deferred batches,
  deadline repaints, and the effective interval.
- The live cursor uses one validated immutable asset for CPU composition and
  hardware upload. The default is the canonical X11 core `left_ptr`; standard
  Xcursor themes and semantic shapes are configurable with a bounded static
  first-frame fallback.
- Engine reduces combined output damage into bounded `skip`, `partial`, or
  fail-safe `full` repaint plans after output clipping and exact rectangular
  coalescing. Native evidence separately reports compositor damage, combined
  output damage, and the selected repaint mode, rectangle count, and pixel
  count.
- Engine has generation-checked `ChromeDescriptor` and `ChromeActionRequest`
  records for sanitized compositor metadata and actions.
- A pure reference reducer projects at most sixteen exact-generation chrome
  descriptors into a centered title-only list with selected, trust, and
  attention markers. The shell supplies order and selection; Engine validates
  epochs and targets, computes damage, and lowers the same logical commands
  independently for unequal heads.
- The live renderer rasterizes sanitized compositor text with bundled JetBrains
  Mono NL Regular 2.304. Its renderer-private least-recently-used cache is
  bounded to 1,024 entries and 16 MiB, and shared raster bytes remain valid after
  eviction while a frame retains them.

### Target

- Consume the retirement-safe repaint plans in partial composition, output
  frame scheduling, and supported KMS damage clips so stable nodes avoid
  redundant output work. Until destination-buffer preservation or buffer-age
  reconstruction is proven, `skip` remains an observation rather than
  authority to suppress a complete frame.
- Extend native primitives only when demonstrated shell requirements need
  rounded borders, shadows, or images.
- Add deterministic capability degradation for each admitted primitive.
- Capability degradation and cache behavior are observable only through reduced
  reports.

Production rendering claims rectangular focus rings, frames, and Tier-0
indicators. The title-only descriptor overlay is an offline reference gate,
not a live shell or a general shell toolkit.

## Architectural Reference: Niri

Niri is a useful architectural reference for this component. Its compositor UI
is assembled from damage-aware render elements rather than routed through one
general drawing engine. It combines:

- client and offscreen textures;
- solid-color elements;
- specialized GLES shaders for rounded borders, gradients, clipping, shadows,
  blur, and animation effects;
- Pango/Cairo rasterization for text-heavy panels, followed by cached texture
  upload;
- a common ordered render-element stream for final composition.

Sophia adopts the separation demonstrated by that design: semantic compositor
UI, specialized native primitives, cached raster content, and one final
damage-aware composition path. Sophia does not adopt Niri's Smithay types,
Wayland authority model, renderer ownership, or source code. Niri is
GPL-3.0-or-later and tightly integrated with Smithay; it is a design reference,
not a dependency or implementation source.

Sophia's version must remain subordinate to its own boundaries:

- Engine remains the sole visual and input authority;
- compositor nodes use Sophia typed IDs and immutable frame values;
- the renderer remains private behind reduced reports;
- the WM remains metadata-blind;
- client-buffer readiness and compositor chrome share Sophia's atomic
  presentation lifecycle;
- EGL/OpenGL, GBM, DMA-BUF, and KMS objects remain in their existing native
  ownership domains.

The value of the reference is the shape of the solution: use the smallest
native primitive that expresses recurring compositor content, cache expensive
raster work, and compose everything through one frame model.

Client texture residency follows the same rule. A retained compositor repaint
references an opaque renderer-image generation and reuses the renderer-owned
texture; it does not clone protocol state or recreate an EGLImage. This cache
is generic scene infrastructure shared by client pixels and compositor chrome,
not GLX-, X11-, WM-, or application-specific policy.

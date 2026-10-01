# Multi-Monitor Per-Head Composition

**Role:** normative target architecture for composing logical visual state onto
physical display heads.

[Architecture](architecture.md) defines Sophia's authority boundaries.
[Engine Architecture](engine-architecture.md)'s Atomic Visual Path owns visual
ordering.
[Compositor Graphics](compositor-graphics.md) defines renderer-neutral
compositor content. [Data-Oriented Design](dod.md) defines the passive records
that cross these boundaries, and [Renderer Import
Boundary](renderer-import-boundary.md) defines native client-buffer admission.
This document specializes those contracts for extended desktops, mirror groups,
and other logical-output topologies with more than one physical display head.

The executable implementation sequence is maintained in the
[Multi-Monitor Per-Head Composition Critical
Path](notes/indexes/plans.md). This
document is normative; the roadmap is the active checklist. Any change to an
implemented capability, transitional limitation, or acceptance condition must
update both documents in the same change. Completed slices are archived in
[Milestone History](notes/indexes/milestones.md), while run diagnoses and retained
evidence belong in the [Development Notebook](notes/README.md).

Normative target behavior that is not implemented is labelled as such. The
current mirror path remains useful physical evidence, but composing one logical
frame and then scaling that flattened result is not the target architecture.

## Decision

Sophia has one authoritative logical scene and a distinct native composition
for every physical head.

A logical output is an Engine and policy fact. A physical head is a presentation
target below the policy boundary. One logical output normally has one head and
may have several when mirrored. Several logical outputs form an extended
desktop. The WM continues to see only logical outputs; it never receives
physical mode, scale, transform, connector, CRTC, card, mode-object,
framebuffer, render-target, or head-count identity.

Physical topology control is a separate privileged authority. An explicitly
configured protected output process may receive the exclusive output file role and
opaque head capability handles needed to choose each head's mode, scale,
transform, position, and mirror membership. Those handles and capabilities do
not enter the ordinary `sophia_wm_v1` scene/policy projection, and neither role
exposes connector, CRTC, card, mode-object, or framebuffer identity.

Mirroring is semantic rather than framebuffer identity:

- every successfully retired mirror cohort contains the same logical scene
  generation, surface set, stacking, geometry, focus, and compositor state on
  every required head; transiently, a faster head may still scan the prior
  cohort while a sibling finishes;
- each head receives its own native-size render target, composition plan,
  framebuffer, KMS ownership, and page-flip observation;
- Engine-owned graphics are rasterized for the head rather than scaled from a
  previously flattened output;
- client raster content uses the best ready authority-owned variant for the
  head, with explicit resampling when no matching variant exists; and
- the logical output retires only after every required head retires that scene
  generation.

This contract does not promise that arbitrary client pixels can be recreated at
another resolution. A raster buffer contains pixels, not the semantic operations
that produced them. Native-sharp client content therefore requires another
ready content variant from its owning authority. Sophia must report a fallback
as resampled rather than disguising it as per-head rasterization.

Two limits follow from that, and neither is a defect to be fixed:

- A mirror always resamples for at least one head. One client renders one
  raster at one size, so heads of differing density cannot all receive natively
  rendered content however faithful the authority is. Per-head composition is
  therefore proven visually on extended topologies, where a window lives on one
  head and is rendered at that head's density with nothing resampled. A mirror
  proves the plan, cohort, variant, and retirement contracts, not visual
  sharpness.
- Exact per-head rendering requires semantic content to replay. That holds for
  server-side core drawing — core-font text, lines, rectangles — and not for
  clients that rasterize their own text and upload finished pixels, which is
  what modern toolkits do. For those surfaces the authority holds a raster and
  can only resample it more faithfully, never re-render it.

Fidelity also depends on the ratio and the content. A fixed-cell bitmap font at
a fractional density has no crisp rendering at all: at 0.75 a six-pixel cell
becomes 4.5 pixels, alternate characters straddle pixel boundaries, and every
one-pixel stem covers three quarters of a pixel. Semantic replay and faithful
resampling converge there. They diverge on content whose form is
resolution-independent, where replay rasterizes a one-pixel line as a solid
one-pixel line at the target density and resampling cannot.

## Ownership

### Sophia Engine

Engine owns:

- the immutable logical scene generation;
- logical-output views, surface placement, stacking, focus, and hit-testing;
- derivation of one renderer-neutral composition plan per required head;
- deterministic selection of a surface content variant for each head plan;
- head-local damage planning and the atomic relationship between client
  content, compositor graphics, cursor state, and submission;
- the required-head set for each logical output generation; and
- promotion of visual state, input geometry, and protocol feedback after the
  applicable retirement set completes.

Engine does not own X11 drawing requests, client font semantics, renderer
textures, GBM objects, DRM objects, or connector naming.

### Protocol authorities

A protocol authority owns the client-visible resource and rendering semantics
that can produce surface pixels. It emits a bounded, protocol-neutral content
set for one surface generation. The active X Server Frontend may produce
multiple variants for server-rendered core content. It does not expose XIDs,
drawing requests, fonts, GCs, or pixmap object graphs to Engine.

X Authority retains one canonical protocol-visible drawable. Derived raster
variants are authority-private caches, not additional X resources: they cannot
change `GetImage`, event ordering, GC behavior, drawable lifetime, or any other
client-visible X11 result. If an accepted operation cannot be reproduced
faithfully for a derived variant, the authority invalidates that variant and
publishes the canonical raster alone.

An authority that has only one client buffer emits one variant. Absence of an
exact variant does not authorize Engine to infer or replay protocol operations.

### Native renderer

The renderer owns:

- per-head render-target pools and complete native target slots;
- texture import, immutable source sharing, atlases, and raster caches;
- lowering a head composition plan into its native target;
- head-specific sampling, color conversion, clipping, and compositor-primitive
  rasterization; and
- reduced import, sampling-quality, cache, and failure observations.

Renderer-private handles remain below the Engine boundary. Source textures and
caches may be shared when their ownership permits it; final head targets and
their scanout leases are permanently head-exclusive. This preserves the
complete-target lifetime rule in [Architecture](architecture.md) and avoids the
AMDGPU lifetime failures already observed when incomplete native owners were
shared or reused across presentation lanes.

### Live backend

The target backend owns physical connector/card identity, native mode capability,
render-device transport, framebuffer creation, KMS requests, callbacks, and
resource retirement. It projects those facts upward through an opaque head
identity and reduced target capabilities. It does not choose scene content,
variant policy, layout, or logical retirement requirements.

This boundary is implemented for head identity: the backend mints
`RenderHeadId` when it builds page-flip sessions, retains the
card/connector/CRTC/name mapping privately, and Engine records carry only
reduced `HeadRenderTarget` facts. No physical identity enters
`sophia-protocol` or the WM wire.

## Passive Target Records

The names below define the target data shape. They are passive records, not
objects with callbacks or hidden authority.

### `RenderHeadId`

A session-scoped opaque typed ID for one physical presentation target. The
backend retains the card, connector, CRTC, and plane mapping. `RenderHeadId`
may cross the Engine/backend boundary for planning and observations, but it
does not cross the WM, portal, metadata, shell-policy, or application-protocol
boundary.

### `HeadRenderTarget`

Describes one current native target:

- `head: RenderHeadId`;
- owning `OutputId`;
- target/topology generation;
- native pixel size and transform;
- refresh and reduced presentation capabilities; and
- renderer/KMS transport and format capability identities in reduced form.

A mode, transform, device, or capability change creates a new target
generation. Work prepared for an older target is stale and cannot be relabelled.

### `SurfaceContentSet`

Describes the bounded raster content admitted for one committed surface
generation:

- surface and committed content generation;
- logical content extent;
- one or more `SurfaceContentVariant` records; and
- authority-owned ready variant and damage identity.

The set is immutable after admission. Supplying a replacement or additional
variant creates a new authority transaction and generation; it cannot mutate a
submitted frame. The list is bounded and deduplicated by density/transform
class. Its implementation must introduce a dedicated named capacity alongside
the record; logical-output and connector-table capacities are not that bound.

### `SurfaceContentVariant`

Describes one authority-asserted realization of the content:

- stable variant identity within the content generation;
- protocol-neutral `BufferSource`;
- pixel size, logical extent, scale/density, and source transform;
- ready-only publication plus synchronization identity inherited from the
  committing authority transaction; and
- variant damage.

All variants in a set belong to the same surface, namespace, transaction, and
content generation. Engine validates those identities and bounds but cannot
prove that opaque pixels are semantically equal; that assertion remains the
protocol authority's responsibility.

Existing single `BufferSource` transactions normalize into a one-variant set.
This preserves current frontend compatibility while making degradation
explicit.

### `SurfaceRasterRequirements`

Engine may publish a bounded, protocol-neutral request describing useful raster
classes for one surface:

- surface and committed-content generation;
- requirement/topology generation;
- logical content extent; and
- ordered target density and transform classes.

The record contains no output, head, connector, mode-object, or framebuffer
identity. It is advisory: the owning authority may answer later with a new
`SurfaceContentSet`, while Engine continues using the best ready variant. A
response for an older content, layout, or requirement generation is rejected as
stale. The renderer cannot issue this request directly or block a frame while
waiting for it.

### `OutputSceneSnapshot`

Captures one immutable logical-output view of the authoritative scene:

- `OutputId` and scene generation;
- logical viewport and output transform policy;
- ordered surface generations, immutable content-set identities, geometry,
  clips, and opacity;
- Engine-owned compositor display-list nodes;
- software-cursor state; and
- logical damage.

It contains no physical target size, framebuffer, renderer handle, or selected
content variant. It is the fan-out point shared by all heads of the output.

### `HeadCompositionPlan`

Lowers one `OutputSceneSnapshot` for one `HeadRenderTarget`:

- scene, output, head, and target generations;
- the exact rational logical-to-head transform;
- ordered `HeadLayerBinding` values naming selected content variants and their
  sampling class;
- native placements, clips, compositor primitives, and cursor state; and
- head-local repaint work.

The plan is complete and immutable. A renderer must not consult current scene
state, a primary head, or a newer variant while executing it.

### `HeadFrameCandidate` and `OutputPresentationCohort`

`HeadFrameCandidate` is the reduced descriptor for one prepared native result.
The passive identity and outcome cross through `sophia-engine`; the native owner
behind them remains in `sophia-renderer-live` and the applicable native renderer.
The descriptor carries the exact head-plan identity, target descriptor, and
affine lease token it realizes. Renderer/backend-private state behind that token
owns the native target and lease until KMS submission transfers the applicable
ownership; native handles do not enter the passive Engine record.

`OutputPresentationCohort` is the logical join record:

- output and scene generation;
- fixed required-head set;
- prepared, submitted, flipped, lost, and cleanup state by head; and
- terminal outcome and logical timing evidence.

It does not own native buffers directly. Renderer candidates and backend
submissions own their private resources; the cohort decides when their common
logical generation may be reported as presented.

## Target Module Boundaries

The record ownership maps onto the existing crates rather than creating a
parallel multi-monitor subsystem:

- `sophia-protocol` carries `SurfaceContentSet`,
  `SurfaceContentVariant`, and `SurfaceRasterRequirements`, because those
  passive values cross the protocol-authority/Engine boundary.
- `sophia-engine` owns `RenderHeadId`, `HeadRenderTarget`,
  `OutputSceneSnapshot`, `HeadCompositionPlan`, pure variant/geometry/damage
  reducers, the passive `HeadFrameCandidate` identity/outcome, and
  `OutputPresentationCohort`.
- `sophia-renderer-live` and its native implementations own target pools,
  imports, caches, lowering, and the native owner behind a
  `HeadFrameCandidate`; they return only reduced identities and outcomes.
- `sophia-backend-live` owns the physical head table, renderer/export workers,
  KMS submissions, callback routing, cleanup, and translation between opaque
  head IDs and card-local native objects.

The production session coordinator orders these owners through the existing
runtime driver. It must not add a second scene, presentation scheduler, or
mirror-only composition authority.

## Ordered Data Flow

```text
protocol transactions          backend topology observations
        │                                  │
        ▼                                  ▼
SurfaceContentSet       logical output ↔ HeadRenderTarget set
        │                                  │
        └──────────────┬───────────────────┘
                       ▼
             immutable Engine visual state
                       ├──── SurfaceRasterRequirements ────► authority
                       │              (advisory)               │
                       ◄──── later SurfaceContentSet generation┘
                       ▼
              OutputSceneSnapshot
                       │ fan out before rasterization
          ┌────────────┼─────────────┐
          ▼            ▼             ▼
    head plan A   head plan B   head plan C
          │            │             │
          ▼            ▼             ▼
    native target A native target B native target C
          │            │             │
          ▼            ▼             ▼
       KMS A          KMS B         KMS C
          │            │             │
          └────────────┼─────────────┘
                       ▼
       output/head retirement reduction
                       │
                       ▼
 committed visual state, input state, feedback, release
```

The fan-out point is load-bearing. Flattening `OutputSceneSnapshot` into pixels
before the split destroys the information needed for native compositor
rasterization, variant selection, head-local damage, and target-specific color
or transform work.

## Per-Head Composition Rules

### Logical-to-head mapping

One pure rational transform maps the logical output view to each head. Fit,
cover, and exact policies select that transform; they do not select a prebuilt
source framebuffer. The same transform implementation projects layer edges,
clips, damage, software cursors, hardware-cursor coordinates, and reverse input
coordinates. Rectangle edges are projected before widths are derived so
adjacent logical geometry cannot gain inconsistent seams through repeated
rounding.

Configuration owns the operator's fit, cover, or exact choice. Engine validates
and normalizes that choice and derives the transform; the backend executes the
immutable head plan. The migration may generalize the existing edge-based
`project_mirror_child_rect` reducer, but must not reuse whole-frame extent
projection for individual layers.

Bars introduced by fit are explicit Engine background in the head plan. Cover
and exact may clip content only through their named policy. A target-size or
mapping change invalidates the prior damage baseline and forces a full repaint.

### Compositor-owned content

Renderer-neutral display-list nodes remain semantic until the head plan is
lowered. Borders, fills, shadows, cached compositor text, images, and software
cursors are rasterized at the head's native density. Cache identity includes
the target density, transform, color-relevant capability generation, and every
existing content/style key.

Mandatory compositor content cannot disappear on one head. A supported reduced
fallback is allowed only under the degradation contract in
[Compositor Graphics](compositor-graphics.md). Failure to produce mandatory
content for any required head fails the complete candidate before submission.

### Client content selection

Variant choice is a pure bounded reducer. It applies this total preference:

1. exact target density, extent, and compatible transform;
2. a compatible variant that avoids upscaling, with the least excess density;
3. the compatible remaining variant with the least scale error; then
4. stable variant identity as the tie-breaker.

The requested and effective binding classes are each `Exact`, `Downsampled`, or
`Upsampled`. Sampling is applied to that client layer while the rest of the scene
remains native; a whole output is never resampled to compensate for one missing
client variant. `Active`, `Fallback`, and `Unavailable` are outcomes, not fourth
sampling classes. Sampling filters and failure policy remain renderer
capabilities, but requested class, effective class, and outcome are reduced
Engine-visible evidence.

Server-rendered X11 core content may gain native variants inside X Authority by
applying the accepted protocol semantics to authority-owned variant storage in
response to current raster requirements. This is a target capability, not
permission for Engine to ingest or replay X11 drawing commands.
Client-uploaded bitmaps and DRI3 content remain single-raster unless their
owning path supplies a genuine additional variant.

### Mirror-group sizing policy

A mirror group has one logical size and places every member into it, so at most
one member is pixel-exact and the rest reach that size by resampling. Which
member is exact is a property of the group, not a mode change: every head keeps
its own mode either way. The choice is expressed in the group proposal as the
logical extent plus a per-member `Exact` or `Fit` mapping, so no compositor
change is needed to move it.

| Policy | Larger member | Smaller member | Precedent |
| --- | --- | --- | --- |
| Optimize for the primary (default) | native | resampled down | macOS "optimize for display" |
| Optimize for the smaller member | resampled up | native | macOS, other direction |
| Centre unscaled | native, smaller image inside a border | native | X/Windows 1:1 centred modes |

The first two are what macOS offers; letterboxing there answers aspect-ratio
mismatch, not resolution mismatch, and does nothing about resampling. Windows
Duplicate and X.Org take the other route entirely and refuse to scale: X clones
only outputs whose modes are equal (`xf86ModesEqual`, "different modes, can't
clone"), and Windows restricts the desktop to a mode every display supports.
Sophia can mirror unequal modes because it composes per head, and the price of
that ability is this choice.

Centre-unscaled is the only policy where nothing resamples. Padding cannot
rescue the other direction: showing a larger desktop unscaled on a smaller panel
would have to *crop* the picture, not pad it.

All three are implemented, as `MirrorSizingPolicy` in the reference policy
client. The compositor needed nothing for the third either: `OutputHeadMapping::
Exact` already takes the logical size verbatim and the projection already centres
it, so "this head owns the size" and "this head shows the image unscaled inside a
border" are the same placement, differing only in whether there is a remainder to
leave. What the policy chooses is the logical size; the mapping follows.

Two details the third policy forced into the open. Its size is the minimum on
each axis across the group, not whichever member is smaller: two heads need not
be ordered -- one can be wider while the other is taller -- and taking either
mode whole would run the other head's image past its edge, where clipping crops
it rather than bordering it. A policy promising that nothing resamples would
instead have silently lost pixels. And the applied-topology predicate now reads
the logical size rather than the member mappings alone, because two exact members
sized to the larger head are that cropping configuration and wear exactly the
mappings centre-unscaled produces.

### Sampling quality

The requested and effective classes stay `Exact`, `Downsampled`, or `Upsampled`
whatever filter is chosen. A better filter never reclassifies a resampled frame
as native, and evidence that reads the class must keep meaning what it says.

Filter choice is a renderer capability, and the two directions are not the same
problem:

| Direction | Where it occurs | Current filter | Better answers |
| --- | --- | --- | --- |
| Downscale | smaller member of a group optimized for the larger | Catmull-Rom bicubic, in linear light | a sharpening pass after the resample; a kernel that widens past 2x |
| Upscale | larger member of a group optimized for the smaller | Catmull-Rom bicubic, in linear light | Lanczos-2, or an edge-adaptive upscaler such as FSR 1 EASU or NIS |

Both directions run the same program. Catmull-Rom is an interpolating kernel --
it passes through its samples -- so it is the textbook bicubic upsample as well
as a reduction filter, and the upscale direction no longer falls through to a
hardware bilinear. One program means one place where light is decoded and
re-encoded rather than two that could drift apart, and it removed the need to
choose an upscale kernel before the colour space was fixed.

The sampler is set to `NEAREST` for every reconstructed draw, which is
load-bearing rather than incidental: the kernel gathers its own 4x4 footprint at
texel centres, and a hardware `LINEAR` filter would blend those texels in
gamma-encoded space before the shader ever ran. That failure is invisible in the
evidence -- the draw would still report `sharp_downscale status=active` while
producing a partly uncorrected frame -- so the filter and the program are chosen
by one function rather than at two sites that could disagree. `LINEAR` survives
only on the fallback, where no shader is running.

FSR 1 and NIS are spatial upscalers and apply only to the second row. Neither
improves a downscale, and both expect to know their input's colour space.

Colour space was the load-bearing part, and it was wrong in both directions:
every filter weight was applied to gamma-encoded bytes as though they were
light, which is the ordinary cause of muddy edges on resampled text and corrupts
a good kernel as thoroughly as a poor one. Averaging the encoded bytes 0 and 255
gives 127, about a fifth of the light of white rather than half of it, so every
resampled edge landed too dark.

The reconstruction shader now decodes each tap before weighting it and re-encodes
the sum once at the end. Gamma 2.0 -- `c*c` and `sqrt` -- rather than the sRGB
curve, for the reason `software/raster_replay.rs` gives where it made the same
choice for the CPU raster path: a squared approximation is cheap enough to apply
to all sixteen taps and keeps one transfer function in the tree instead of two
that could disagree. A residual error remains in the deep shadows, by choice.

Premultiplied sources are unpremultiplied across the decode, or the alpha would
be squared along with the colour. Under gamma 2.0 both directions collapse to
something cheap: `(v/a)^2 * a` is `v*v/a`, and `sqrt(L/a) * a` is `sqrt(L*a)`.
Alpha itself is never transformed, being coverage rather than light. The clamp
happens before the encode and not after it, because Catmull-Rom rings below zero
on a hard edge and the square root of a negative reaches the screen as a hole
rather than as a dark pixel.

Two things this does not reach. The fixed-function blend still mixes
`dst*(1-src.a)` on gamma-encoded destination bytes, and no shader can touch it --
it needs `GL_FRAMEBUFFER_SRGB` and an sRGB-capable EGL surface, and imported
client textures take their format from EGLImage rather than from us. The
shader's `rgb * opacity` and its `min(rgb, a)` ringing clamp stay in encoded
space alongside it deliberately: moving one without the other would leave them
inconsistent, so they travel together as one later change.

An earlier revision of this section claimed the correction was measurable from
the composition-region pixel populations the renderer already reported. It was
not. Those populations key on which channels are lit, never on how brightly —
`pixel_evidence.rs` says so in as many words, because a palette check wants to
survive an intensity conversion in order to expose a channel swap. The property
that makes them good at their job makes them blind to this one. Gamma moves
intensity and nothing else, so every one of them would have held still while
the pixels underneath changed; only `checksum` would have moved, and a hash
proves a difference without saying which direction it went.

`region_luminance_sum` and `region_luminance_histogram` were added for this.
Both are integer, with weights summing to 256 so the shift never rounds, which
keeps them as reproducible as the checksum beside them. Judge a filtering change
on the histogram — a mean can hold still while the population behind it splits —
and read the exact-sampled head as the control, because a head that resamples
nothing must not move at all.

Sharpening after an upscale is a taste with a cost: contrast-adaptive
sharpening rings on glyphs that were already crisp, which is why the desktop
case wants a sharpen-only mode rather than a game-tuned preset, if it wants one
at all.

### Damage

Damage has two stages:

1. logical scene damage names changed surface generations, geometry, stacking,
   compositor nodes, and cursor state; and
2. each head plan projects that damage, adds old and new variant extents, and
   expands it by the selected sampling filter's footprint.

Every head owns its own `pending → rendering → submitted → presented` damage
baseline. Native sizes, selected variants, transforms, and color paths may
differ, so one head's damage snapshot cannot be cloned into another's ledger.
Incomplete damage proof becomes a full head repaint, never stale pixels.

Logical content identity is the scene generation plus its ordered committed
surface/content generations. A diagnostic logical-content checksum is equal
across the cohort, and the join refuses heads that disagree on it: one scene
composed once cannot yield two logical checksums. Optional native head-pixel
checksums, damaged-pixel counts, and framebuffer bytes are expected to differ
and must not be used as the mirror join identity.

## Scheduling and Retirement

For one presentation cohort, the order is:

```text
scene accepted
  → all required head plans built
  → all required head candidates prepared
  → per-head KMS submissions
  → per-head callbacks and cleanup
  → logical output retirement
```

All required heads of one logical-output cohort must prepare successfully before
that cohort's first KMS submission. This is a per-output barrier: unrelated
logical outputs retain independent preparation, submission, and frame-clock
progress. Preparation includes rendering/export, framebuffer creation, lease
acquisition, and every validation possible without accepting the real page flip.
It does not pretend the later kernel commit cannot fail. Submission remains per
head, matching the per-output KMS/worker ownership established in
[Engine Architecture](engine-architecture.md). Once any
submission is accepted, a later failure poisons the cohort: already accepted
work drains, no successor can be relabelled into the failed generation, and the
logical candidate does not commit.

Initial modesetting follows the same rule. Before the first modeset of a mirror
cohort, every required head has a prepared `HeadFrameCandidate` containing the
same real committed scene generation at that head's native target. A valid
direct-CPU lowering may satisfy this requirement; a blank, speculative, or
primary-scaled bootstrap cannot. The cohort records that its first submissions
are initial modesets rather than creating a separate startup lifecycle.

A mirror output owns a monotonically increasing newest complete generation.
Every head prepares that generation before any head may submit it, preserving
the atomic scene/content contract. After preparation, each idle head may submit
the newest generation without waiting for a sibling's older KMS callback. A
lagging head skips directly to the newest generation; it never relabels stale
renderer work and never submits two generations concurrently.

Head preparation may execute concurrently through a bounded worker set. The
bound is derived from admitted live heads and target slots; the scheduler does
not spawn unbounded work per layer, surface, or frame. Backpressure preserves
every already-submitted native owner and coalesces only unsubmitted work to the
complete latest successor.

The primary head's callback owns logical presentation within a mirror group.
Other heads remain physical owners only: their callbacks advance their own
displayed generation and release older generations, but do not delay frame
feedback to the primary's refresh. A transaction affecting several logical
outputs may derive their cohorts from one immutable candidate, while each
cohort keeps its own prepare-before-submit barrier and its KMS retirement
remains a separate physical instant. Engine publishes transaction feedback and
cross-output input state only after the applicable logical-output presentation
set completes. Sophia does not claim globally simultaneous multi-output
presentation.

Page-flip timing remains physical evidence. Logical completion uses the primary
head's callback UST and the scene/cohort generation, never a fabricated
combination of CRTC-local sequences. Per-head callback and release records prove
that independently paced secondary owners subsequently converge and drain.

## Extended Desktops and Spanning Surfaces

The same path serves an extended desktop. Each logical output derives its own
`OutputSceneSnapshot` from one authoritative Engine scene read, then lowers it
for every head backing that output. There is no rendering meaning attached to
"primary output"; primary selection may remain an operator/configuration
default but cannot decide scene size, buffer ownership, damage, or content for
another output.

A surface intersecting several logical output views is clipped independently
into each view and bound to the best content variant for each head. It retains
one committed surface generation. Geometry, focus, and feedback whose contract
spans those outputs advance only after their applicable retirements complete.
Unrelated outputs retain independent frame clocks and do not join merely because
they share a session or render device.

## Topology, Failure, and Resource Lifetime

The required-head set and every target generation are frozen when a cohort is
prepared. A mode change, transform change, render-device change, or head-set
change makes the cohort stale.

A lost mirror head never counts as a flip and never silently shrinks an active
cohort. It settles with a distinct `head_lost` outcome rather than the surface
removal outcome. The candidate fails closed, accepted head submissions drain, and a
surviving topology becomes a new Engine candidate. Losing an independent
output is isolated unless an in-flight transaction names that output in its
applicable retirement set.

Renderer target slots are keyed by head, target generation, size, format, and
normalized modifier/capability set. The complete target follows
`free → rendering → scanout leased → retiring → free`. Page-flip retirement or
an explicit controlled detach is the only authority that releases the scanout
lease.

Immutable source storage and imports may be shared across head plans without
copying CPU pixels per head. Those shared sources remain leased until the last
candidate that references them retires or reaches a controlled terminal
failure. Reference counts do not replace the explicit presentation lifecycle.

Cross-card composition obeys the same records. The renderer either imports a
complete source through the declared independent-file transport or uses the
admitted CPU fallback. It may not submit a renderer-local handle on another
card or degrade one mirror head silently.

## Observability and Acceptance

Reduced evidence must make the architecture distinguishable from output-level
scaling without exposing pixels or client metadata. A physical proof records,
for one common scene generation:

- one plan, render completion, KMS submission, callback, and retirement per
  required head;
- each head's native target size and target generation;
- requested and effective exact/downsampled/upsampled client-binding counts plus
  active/fallback/unavailable outcomes;
- independently rasterized compositor-primitive counts;
- head-local repaint mode and damage; and
- the final logical output join.

Every fail-closed rejection names the reduced output, head, surface, generation,
or target that failed; a subjectless renderer or lifecycle error is insufficient
production evidence. All `OutputSceneSnapshot` values for one transaction are
derived from one immutable Engine scene read rather than independently sampling
mutable state.

The evidence must reject a missing head, duplicate head, target-size mismatch,
source-sized damage on another head, whole-output resampling, partial logical
retirement, renderer fallback reported as exact, or resource ownership left
after teardown. Logs retain typed IDs and reduced counts only; client titles,
glyphs, pixels, paths, and renderer-native handles remain private.

Deterministic coverage must include:

- unequal-size mirror heads with one scene generation and distinct native
  targets;
- exact and missing client variants alongside native compositor text;
- CPU, DMA-BUF, renderer-image, solid, and cursor layers;
- a surface spanning differently scaled extended outputs;
- filter-footprint damage and per-head baseline divergence;
- callback reordering and one head flipping before its sibling;
- render failure before submission and KMS failure after a partial submit;
- head loss and topology-generation replacement; and
- proof that WM and protocol-facing records contain no physical head identity.

Acceptance also requires production reachability. Every new passive target
record lands with a named producer and consumer on the running session path, a
test that exercises that path, and a negative test that fails when either end is
unwired. A type or isolated reducer with no production caller is incomplete.

The bounded visual-retirement model in
[`validation/tla/VisualRetirement.tla`](../validation/tla/VisualRetirement.tla)
carries this contract: per-head preparation, output-scoped committed
generations, exclusive head ownership across generations, distinct head leases,
and last-head retirement. Supersession is output-scoped and happens before the
kernel, matching the latest-wins successor above -- a candidate loses an output
a newer generation already committed, and only an output with no submitted head
can be lost. Negative controls show that submitting before all required heads
prepare, retiring after one head, overlapping one head across generations or
splitting one output cohort across generations, submitting a generation the
output has already passed, relabelling a submitted cohort, or releasing one
head's target while it remains in flight each violates the model. Scheduling
changes extend that model before they reach production.

## Current-To-Target Terminology

The target records replace existing responsibilities; they do not create a
parallel multi-monitor subsystem.

| Architecture term | Current implementation seed | Target owner |
| --- | --- | --- |
| logical output | `OutputId`, policy-facing `SnapshotOutput` | Engine |
| physical render head | opaque `RenderHeadId` (implemented); backend head table maps native identity | opaque `RenderHeadId`; backend maps native identity |
| head target | native selection/exporter state | `HeadRenderTarget`; Engine sees reduced capability only |
| presentation cohort | `LiveProductionMirrorGroupLifecycle` plus `NativeMirrorGrouping` | `OutputPresentationCohort` in Engine |
| fit policy | `DesktopMirrorFit` mapped to `NativeMirrorFit` | configuration choice normalized by Engine |
| sampling | `NativeCompositionSampling` plus fallback counters | requested/effective class plus outcome evidence |
| logical checksum | source-frame checksum copied to every head | diagnostic `logical_content_checksum` |
| head pixel checksum | not currently produced | optional head-local evidence, never join identity |

## Current And Target State

### Implemented

- Mirroring is one logical output backed by multiple physical heads and remains
  invisible to WM policy.
- Each current mirror head owns a native-size buffer, KMS submission, callback,
  and resource lifetime; logical retirement joins the callbacks.
- CPU, mixed CPU/DMA-BUF, retained renderer images, compositor solids, cursor
  coordinates, and damage snapshots fan out to the heads.
- Retained renderer images are realized once per render device, in the store
  the device's worker owns, so outputs sharing a device import a client buffer
  once rather than once each. What stays private to a head is its target
  slots, its scanout leases, and its pixel proof: topology preparation derives
  each candidate and rollback head's requirements and restores
  compositor-owned snapshots into a newly assigned head before worker
  submission, and it never treats another head's scanout lease as locally
  usable.
- The current renderer can apply exact, downsampled, and upsampled sampling and
  reports requested/effective paths and fallback outcomes.
- Content crosses the authority boundary as a bounded `SurfaceContentSet` of
  density-deduplicated `SurfaceContentVariant` records with a dedicated named
  capacity; non-X producers currently normalize into a one-variant set while
  X Authority may publish its bounded density set, and committed state retains
  the whole set. Sets contain ready-only variants;
  per-variant damage, fidelity, and transform class are validated and consumed.
- Engine raises raster requirements only for surfaces whose canonical variant
  is a CPU buffer. Renderer and pixmap presentation content carries pixels and
  no semantic form, so no authority can re-render it at another density, and
  demanding one would spend a round trip to be told so. An authority that is
  asked anyway answers with a named fallback: one surface's unanswerable demand
  never fails the protocol runtime.
- Engine reduces all visible physical-head targets into one bounded,
  `SurfaceId`-keyed `SurfaceRasterRequirements` union. The edge-triggered
  tracker emits only missing or changed density classes, prioritizes an
  already-exact authority raster and classes serving the most heads, and
  rejects late responses by source-content and requirement generation before
  transaction admission. A requirement is advisory demand rather than a
  contract pinned to the generation it was built from: an authority answers
  from its current state, reports the generation those pixels were produced
  from, and anchors the reply there, so it commits when Engine's ordered
  transaction chain reaches it. Admission therefore refuses a reply describing
  content *older* than the demand, or one naming a different requirement edge,
  while accepting one that leads it. Exact-generation equality is not
  achievable in practice, because the authority advances a generation per draw
  while Engine commits at frame cadence, so a drawing client leaves every
  requirement naming a generation the authority has already passed. Connector,
  CRTC, output-protocol object, and XID identities do not enter this route.
- X Authority retains its canonical 1x drawable and owns stable derived
  presentation stores for requested density classes. A bounded semantic
  journal (4096 commands, 4 MiB owned payload, four total variants including
  the canonical store, and 64 MiB total canonical-plus-derived backing) replays
  clear/fill, line, rectangle,
  ImageText8/PolyText8, same-drawable CopyArea, and core `PutImage` operations
  when demand arrives after the original request. Full opaque clears reset the
  journal; unsupported or over-budget replay is reported as sampled fallback
  rather than impersonating a native authority raster.
- Accepted core and MIT-SHM `PutImage` retains its own bounded client pixels
  inside X Authority, together with the destination rectangle, depth,
  connection byte order, and graphics-context semantics. Retention is fail
  closed: only tight ZPixmap depth-24/32 rows with no left padding, written
  through an unconditional GXcopy with a full visible plane mask and no clip
  rectangles, are replayable, because only those reproduce the canonical
  drawable. Anything else poisons the journal with a named cause. A full-window
  upload meeting those conditions may replace the journal as a new baseline on
  the same terms as a full opaque clear. Replay projects the retained 1x pixels
  directly rather than resampling the canonical store, so ordering against
  later text and copy commands is preserved.
- Sampled fallback is cause-classified. X Authority reports unsupported
  `PutImage`, unsupported cross-drawable copy, unsupported command, stale
  content generation, logical extent mismatch, absent canonical raster,
  journal capacity, backing capacity, and transform mismatch. A stale generation carries the authority's
  own observed generation beside the requested one, so a run shows the size of
  the lag rather than only its existence. A
  bounded per-surface coalescer emits the first occurrence and each subsequent
  power of two with a cumulative count, so repeated warnings never hide their
  volume. The cause stays authority-private; Engine continues to observe only
  `SurfaceContentFidelity`.
- Fixed 6x13 text remains bit-exact at canonical density. Fractional derived
  stores use one integer rational-edge projector and deterministic 8-bit area
  coverage for GXcopy glyphs; non-copy raster operations use a binary coverage
  decision. Retained client rasters use the same rational-overlap rule as a
  per-channel area average, so a fully covered destination pixel keeps its
  source color exactly and only boundary pixels blend. Partial coverage is
  weighted as light rather than as gamma-encoded bytes: components are squared
  before mixing and square-rooted after, which keeps the arithmetic exact and
  reproducible while giving a half-covered pixel its intended luminance.
  Averaging encoded bytes directly under-weights every antialiased edge, which
  makes sub-pixel strokes read as out of focus. Full and zero coverage still
  map to the endpoints exactly, so canonical-density text stays bit-identical
  to the 1x drawable. One authority
  transaction carries every immutable CPU mutation and
  content variant, while production intake validates each update against
  exactly one member of the set. Authority request publication and late raster
  responses share a lossless ordered egress boundary, with bounded waits held
  outside runtime, atom, and property locks.
- Head identity is opaque end to end: the backend mints session-scoped
  `RenderHeadId`s, Engine's `EngineHeadRegistry` holds generation-stamped
  `HeadRenderTarget` records grouped by logical output, and mirror
  lifecycles, callback routing, and per-head evidence are head-keyed while
  connector/CRTC integers stay in the backend's private head table.
- `OutputSceneSnapshot`, `HeadCompositionPlan`, and the pure fit/cover/exact
  planner are implemented in Engine. The production CPU transaction builds
  every head plan from the exact committed slice, resolves the selected CPU
  variant, and lowers each plan into its own native-size mixed frame. The
  backend queues those frames by opaque head and rejects incomplete coverage,
  duplicate heads, checksum disagreement, or head-local damage with the wrong
  native extent.
- Tier-0 indicator chrome is a retained semantic display-list command rather
  than a flattened surface. Each logical output carries its own committed
  descriptor into every head plan, and the renderer rasterizes the strip at
  that head's native extent from one bundled font. Damage, raster caching, and
  last-presented hit targets remain output- and head-resolved. Signed Hagia
  archive `0005` physically proves the path on two outputs.
- `OutputPresentationCohort` and `OutputTopologyTransaction` are implemented as
  Engine reducers. They enforce prepare-before-submit, primary-owned logical
  presentation, last-head cleanup,
  fail-closed head loss, partial-apply rollback, and a first-presentation barrier
  before topology publication. The live multi-head scheduler now renders and
  creates each head's framebuffer/import/blob and atomic request as an affine
  prepared owner, records those candidates in `OutputPresentationCohort`, and
  performs no KMS submit until the complete required-head set is prepared.
  Preparation failure cancels every prepared owner before the generation is
  poisoned; shutdown cancels prepared owners before callback-only drain.
- `TransactionPresentationCohort` is the Engine-owned cross-output join. A pure
  reducer selects the union of output viewports intersecting a surface's old
  and new root-space geometry. DMA-BUF and software Presents may submit those
  logical outputs independently, but protocol feedback, source release,
  displayed-image promotion, and cross-output input publication wait for the
  last applicable output retirement. The latest output UST and transaction
  generation form the logical completion clock; CRTC-local sequences are not
  combined.
- The exclusive output file role and authenticated 9P transport exist
  with bounded capability snapshots and complete topology proposals. Backend
  projection binds capabilities to opaque heads and resolves independently
  selected modes into mixed mirror and extended groups. A session-side candidate
  owner now joins that projection to `OutputTopologyTransaction`: validation is
  nonmutating, fresh identities remain provisional through preparation and
  rollback, and the replacement snapshot stays private until every new logical
  output presents. The public C SDK supplies independent conformance evidence.
- The output transport also has an optional cancellable service loop and
  incrementally buffered proposal intake. No-client startup and shutdown are
  bounded, and a client may pause between frame header and payload without
  blocking the visual owner. A native live session now binds that service in
  explicit output process's private endpoint, authorizes its captured process
  identity and advertises it through `SOPHIA_OUTPUT_9P_SOCKET`. WM restarts
  leave that assignment unchanged; output peer departure fences its epoch. Complete
  proposals reach the session-side authority owner. Validate-only proposals
  settle normally. Apply proposals cross into the visual/session owner as
  immutable effect contracts; that owner binds every enabled and disabled head
  to its existing card/connector/CRTC/plane, resolves each requested mode
  against the live DRM master, and retains per-head target generations without
  mutating the published topology.
- Retained native composition intersects policy presentation order with the
  Engine-owned committed surface table before resolving sources. A policy-
  admitted surface with no committed pixels is temporarily absent, matching
  the canonical output-scene projection. Once a surface is committed, absence
  of an authority-owned CPU or renderer source remains fatal.
- Resolved output candidates retain root-space logical viewports as well as
  `HeadlessOutput` extents. The visual runtime can therefore capture one
  committed root scene and independently lower every provisional extended or
  mirrored head at native size. Candidate and rollback composition resolve that
  scene through the same authority-owned CPU buffers and retained renderer-image
  identities as ordinary presentation; a committed DMA-BUF handle alone is not
  a renderer source. Topology-specific renderer preparation creates framebuffer/
  import/mode-blob owners without committing; those owners can join one card-
  scoped atomic modeset containing both enabled and explicitly disabled heads,
  then become ordinary retirement owners only after kernel acceptance.
  The card executor submits that complete change as one blocking modeset, and a
  passive coordinator orders cards deterministically. If a later card refuses
  the candidate, the accepted prefix rolls back in reverse card order. The
  published snapshot is projected independently back into the live heads'
  current native sizes and generations, so rollback composition cannot inherit
  a provisional viewport or mode. A typed resource cohort requires one
  candidate owner (enabled framebuffer or explicit disabled-head properties)
  and one rollback owner (framebuffer or explicit disabled-head properties)
  for every affected head before apply can begin;
  rejected insertions return their affine owner. Physical head/output
  observation batches advance protocol authority transactionally, so a bad
  member cannot leave a valid prefix recorded.
- The native live owner drives candidate and rollback renderer workers to
  completion while ordinary presentation scheduling is quarantined. Before
  creating topology-specific resources, the owner enters a bounded quiescence
  phase: it stops ordinary policy and authority intake, retains those packets at
  their existing bounded queues, and continues native frame service until every
  ordinary renderer, exporter, scanout, and lifecycle owner retires. Only the
  session owner can observe that condition. The output client submits one
  proposal and awaits one terminal result; it neither polls renderer lifetime
  nor retries preparation failures. Cancellation wins over readiness, readiness
  wins at the deadline, and a timeout rejects without KMS mutation. It applies
  cards in deterministic order, installs accepted owners into rebuilt output
  runtimes without a second modeset, queues one native-size cohort for every
  replacement logical output, and keeps the published authority private until
  every output crosses its first-presentation barrier. A later-card refusal,
  renderer/service failure before that barrier, or supervised output-peer loss
  enters reverse-card rollback. X topology, WM work areas, pointer bounds, input
  quarantine, and protocol authority reconcile around that barrier. Hardware
  hotplug similarly republishes output capabilities only after replacement
  scanout presents. Preparation failure and session completion retain bounded
  affine-owner cancellation and cleanup. The bundled physical proof starts its
  output role from the committed policy proposal that first places both proof
  surfaces; it does not wait for a redundant scene-echo cycle. If the supervised
  dual-role process restarts, the output service synchronously pauses acceptance
  before spawn, then installs the returned PID and resumes negotiation. A new
  process therefore cannot race the old assignee identity.
- Exact root-space viewports survive runtime adoption and rollback, so ordinary
  extended-output composition cannot collapse back to origin zero. DMA-BUF
  Present resolves its authority-owned sources once, builds one native plan per
  head of every intersecting output, and retains one output frame identity until
  the cross-output join completes. Software Present, retained scene, focus,
  chrome, floating outline, resume, and hardware-cursor projection use the same
  complete output set; the old secondary-output marker frame is not used for
  presentation.
- A head's committed `OutputHeadMapping`, target generation, scale, transform,
  and refresh now flow directly into its `HeadRenderTarget`. Composition,
  damage, cursor, flat fallback, and startup projection consume that head-local
  mapping; an IPC `Fit`, `Cover`, or `Exact` change no longer falls back to one
  session-global mirror policy. Initial configuration is normalized into the
  same protocol-neutral mapping before the native owner is built, and authority
  snapshots preserve the mapping transactionally by opaque head identity.
- The passive frame handed to each backend exporter retains the complete plan
  identity: opaque head, Engine scene generation, committed target generation,
  mapping, and logical-content checksum. Whole-batch admission rejects stale,
  incomplete, duplicate, or cross-scene work before any per-head queue mutates.
  Topology candidate and rollback pools validate against their respective plan
  generations. Stable plan/queue records let physical gates prove the chain from
  native-size plan through queue and KMS without publishing connector identity
  above the backend.
- Native startup and resume are semantic and worker-first for singleton,
  mirrored, and extended outputs. Every native-size head frame is queued before
  export, all workers and affine framebuffer/mode owners must satisfy one pure
  prepare-all barrier, and each card-local output set enters one blocking atomic
  modeset. The old projected flat-CPU prefix is not an accepted physical
  baseline. Adoption retains every accepted owner before a bookkeeping failure
  can unwind through explicit cleanup; a pre-KMS failure drains or discards
  every queued worker command before clearing its passive content and damage
  state.

### Transitional Limitation

CPU-authority frames consume per-head plans in production. Initial empty-desktop
startup, the first authority cycle, explicit repaint, and resume/recovery all use
those semantic frames for their synchronous modeset; the projected flat-CPU
startup path is absent. Ordinary DMA-BUF, retained renderer-image, software
Present, compositor, damage, and cursor work fans out from the logical scene
before rasterization and joins across every applicable output.

The native scheduler has the prepare-all barrier, and live topology planning
creates replacement selections for enabled heads while representing omitted
connected heads as explicit disable effects. Candidate and rollback pools are
prepared before mutation; the live owner drives the per-card coordinator,
installs the accepted side, rebuilds logical runtimes, and rolls an accepted
prefix back in reverse order on failure. A previously disabled connected head
remains in the native model and has an explicit rollback-disable owner rather
than a fabricated framebuffer.

Core `PutImage` replay and the unequal-density mirror proof are complete. Signed
attempt `0025` selected distinct exact 1000- and 750-density authority rasters
for one logical generation with zero sampled fallback and complete per-head
retirement. Cross-drawable `CopyArea` remains a named fail-visible fallback and
is not active work unless a later retained workload reports that cause.

The mixed mirror-plus-extended `sophia_output_v1` cutover and its cable-loss
recovery are physically complete on their earlier signed candidates. The
code-side evidence defect is closed: Engine
classifies realized source and target extents per axis, lowering corrects the
class after retained-image realization, and the native renderer consumes that
same class while emitting output/head/scene-keyed evidence. Public chrome also
reconciles outer policy allocation and content geometry in one transaction, and
the post-commit topology path has deterministic coverage for forced repaint,
stale parked publication, transition overlap, and head-keyed startup readiness.
Signed source `3d19e2e67cfe2e43eb643d219be11a3251fe7176` now has a passing
three-head physical run: the public policy settled two surfaces across two
logical outputs, the extended head's exact keyed draw retired, post-policy
presentation released input quarantine, final health and cleanup were clean,
and visual confirmation passed. The proof is still not promotion-complete
on the current candidate: signed source
`66bc0dd71a40e249eb00cd98f6080cf0f6aa9c54` subsequently passed the
three-to-two-to-three head loss/return gate, and mirror members now advance at
their own refresh rates without releasing a buffer before the last scanning
head retires. That pacing
change affects both the two-head mirror and three-head mixed gates, so both need
fresh signed physical reruns. Signed source
`231847abefca878e2aa40794f902ac28468df447` produced verified mirror archive
`0006`: both independently paced heads presented one logical checksum, the
primary owned logical presentation, and the last head released the generation
before clean shutdown. The immediately following mixed run committed and
settled the three-head topology, then exposed an output-local Present phase
error. Output 2 submitted and retired Present frame 56 while output 1 still held
frame 55 for the same transaction. Output 2 then submitted ordinary topology
repaint frame 60. The ownership check consulted the transaction's in-flight
frame, which remains present until the entire cohort retires, and wrongly
treated frame 60 as a replacement for frame 56.

Submission ownership now consults the output's unsubmitted frame. Once one
output has acquired KMS ownership for its Present frame, later ordinary frames
on that output remain independent while other outputs finish the cohort. A
tagged Present with no matching unsubmitted reservation still fails closed.
Signed source `e946cc725bf731515a477c86e9a575554965418c` subsequently produced
and independently re-verified mirror archive `0007` and mixed archive `0001`.
The mirror run retained independently paced heads, primary-owned logical
presentation, and last-head release. The mixed run retained those properties
beside the extended output, exercised output-local Present ownership, and
finished with clean native and session health. This promotes the pacing slice.
Signed successor `eeef531a33564391765c1ec9fecddf6d06dcd4cc` independently
re-proved that complete display slice as mirror archive `0008` and mixed
archive `0002`. Both records bind Sophia binary digest
`9b60c57d7ffa2feb1a1ea00b8e24e24a9ecc90fa2d545ffb46737432f434c854`.

The mixed runner archives the raw log,
exact Sophia and reference-WM digests, signed source commit, and signed-tree
configuration behind a checksum and standalone verifier. The tty4 critical-
path runner orders the mirror run before the centered mixed run and refuses a
source change between them. The following protected Hagia gate found a separate
proof-harness defect: its restart wrapper depended on ambient variables and the
host `/tmp`, which the policy protection domain deliberately removes. The
replacement launches Hagia directly and lets the session owner request one
supervised replacement after committed action 66 produces an atomic private-
checkpoint inode change. It never reads Hagia's private checkpoint contents or
widens the Bubblewrap domain. The `eeef531a` display reruns passed, and the
following Hagia attempt exposed only a stale operator instruction: the guide
still named an old protocol shortcut after physical matching moved into the
public shortcut owner. Its correction changes neither the executable nor the
compiled policy, so the display archives remain valid while the Hagia gate
binds the corrected signed harness and the same executable digest. The next
Hagia run completed every interaction and broker claim, then correctly failed
its per-head aggregate because focus-only output actions left DP-2 blank. The
guide now moves Kitty to DP-2, waits for a nonzero native-head submission, and
moves it back before the focus actions. This adds the missing physical content
claim without weakening independent per-head completion. The first run of that
guide proved the movement and submitted 11,499 nonzero pixels on DP-2, but its
watcher expected `nonzero_rgb_pixels=` while the producer's Rust debug record
uses `nonzero_rgb_pixels: `. The guide, final verifier, and regression fixture
now match the producer form. The executable slices and exit criteria stay
ordered in `todo.md`.

Signed successor `9ca384a9ffb2e392b584092e64054c2d1f9fc833` then produced and
independently verified Hagia archive `0004` with the same Sophia binary digest
as mirror archive `0008` and mixed archive `0002`. Output 1 completed with 72
nonzero exports and output 2 with seven. The move-to-output submission occurred
between actions 5 and 6, exact text reached a kernel page flip in 15 ms, and
native health and teardown were clean. This completes the display-evidence
dependency for the protected metadata-broker row. Tier-0 indicator display,
work-area reservation, and target-resolved capture are now implemented and
covered deterministically. Signed archive `0005` proves their fullscreen,
restart, pointer-activation, per-output presentation, and cleanup path. The
critical path now moves to the minimum metadata-backed shell rendering model.

### Target

- Split logical scene snapshots from physical render targets.
- Derive per-head plans, damage ledgers, native targets, and compositor raster
  work before rendering.
- Replace all primary-derived and flat-output mirror composition paths with the
  common per-head planner used by mirrored and extended outputs.
- Extend authority-owned native variants to the remaining server-rendered X11
  operations. Bounded `PutImage` is implemented; cross-drawable `CopyArea` with
  explicit source-generation dependencies remains, and neither moves X semantics
  into Engine.
- Enforce prepare-all mirror cohorts, joined retirement, and explicit sampling
  evidence in deterministic and physical acceptance gates.
- Add opportunistic scanout cloning beneath per-head composition. This is
  normative target behavior that is not implemented. When every head of one
  logical output has an equivalent `HeadCompositionPlan` — the entire plan
  record except head identity and content checksum: mapping, native size,
  transform, density class, scene generation, and committed target generation,
  with identical modes including refresh and all heads on one card — the
  backend may compose once and scan the shared framebuffer out on every
  eligible head's plane, subject to an atomic `TEST_ONLY` probe. Content
  identity is guaranteed by compositor determinism, not checked at runtime:
  a checksum comparison is incoherent for cloning because clone mode performs
  one composition, so it could arm the optimization but never detect the need
  to revoke it. Per-CRTC cursor planes and gamma/CTM/degamma properties stay
  head-local beneath the shared framebuffer. Framebuffer release joins every
  scanning head's retirement through the existing presentation cohort. The
  decision is backend-private and re-evaluated only through
  `OutputTopologyTransaction` events, so strategy switching happens at
  configuration cadence with first-presentation barriers; no policy client,
  `sophia_wm_v1` peer, or `sophia_output_v1` role can observe or select the
  strategy. Any field later added to the plan record is equivalence-relevant
  by default, so unconsidered head-local state disables cloning rather than
  wrongly preserving it. Render copy — the per-head composition path this
  document specifies — remains the universal fallback and the semantic ground
  truth; the optimizer may decline a clone, never a topology.

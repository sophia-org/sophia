# Sophia Shell Interface Direction

**Role:** direction and experimental-contract note for `sophia_shell_v1`.
**Status (2026-09-18):** experimental revision 8 includes r5 content, r6
indicators, r7 native launcher and r8 persistent catalog. Production admission,
input and three-component service exist; lifecycle and native acceptance are
not complete. The [capability map](native-desktop-capabilities.md) separates
current paths from gaps. The surveys below are historical requirements,
not an up-to-date unsupported-feature list. The interface is not stable.

The accepted [presentation/execution decision](notes/decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md)
keeps renderer-neutral images on the shell wire and explicit direct GPU access
as separate startup permission. No custom kernel, mandatory GPU bridge or
Vello dependency is required by Sophia. The
[paired Lom/Sophia plan](notes/plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md)
owns remaining integration. The surveys below retain historical design evidence.

The experimental role schema is `protocol/sophia-shell-v1.kdl`. This note
records how broader shell vocabulary will be derived, the external evidence
that method draws on, and where the work sits in the roadmap. Revision 1 is a
falsifiable title-only slice, not a compatibility promise. `docs/architecture.md`,
`docs/sophia-policy-ipc.md`, and `docs/compositor-graphics.md` remain
authoritative wherever this note appears to disagree with them.

The [Sophia Native Protocol Family](sophia-policy-ipc.md) supplies the common
frame, negotiation, epochs, complete-transfer discipline, explicit outcomes,
recovery, and evolution rules. This note derives only the shell-specific facts,
candidates, and authority constraints. It does not define a shell-only
transport, library requirement, or alternate Engine entry point.

## The Decision

### Descriptor And Content Shells

Maintain both architectural models. Narthex remains the independent descriptor
reference; a separately granted content capability lets a shell rasterize its
own widgets. [Content shells](content-shell.md) collects that model's behavioral
requirements, beginning with a panel and anchored popout.
It owns the content behavior; this direction note retains research
provenance and feasibility questions. The existing normative contracts still
take precedence over both.

The session's operator policy must explicitly permit content before the shell
can negotiate it. Content permission is distinct from executable selection and
does not grant foreign pixels, ambient input, application execution, or WM
authority. Arbitrary artwork adds visual impersonation risk. One admitted shell
may combine content and descriptor capabilities; this is not a proposal for
multiple native shell clients or an automatic promotion of X11 panels.

### Scripting Boundary

[Scripting Sophia](scripting.md) defines generic session-owned command routing
for replaceable desktop clients. A shell does not serve a script-facing
endpoint, acquire WM authority, or export its broker-issued activations as
general command tokens. Script invocation also does not authorize disclosure
of the shell's descriptors to that caller.

Generic shell command discovery and invocation are future extensions requiring
negotiated vocabulary, explicit outcomes, and the existing recipient,
generation, and presentation checks. The current descriptor/candidate and
activation-acknowledgement messages provide no generic shell command catalog.
The experimental `sophia msg` CLI and control endpoint implement policy actions
and confirmed WM restart; generic shell commands remain unimplemented.

### Vocabulary Development

Derive capabilities from retained shell workflows and prove them with
independent clients. Lom is now the driving downstream content adapter. Its
Xilem/Masonry/Vello stack supplies workload evidence, never protocol authority.
The earlier Quickshell audit and Noctalia survey remain sources of requirements.
Narthex remains the maintained, independent descriptor client.

Sophia's normative architecture, common protocol contract, role semantics, and
schema own the interface. No reference client's object model, dependencies, or
private APIs may become requirements for another implementation. The complete
content workflow must be exercised by Lom and an independent C client using
the same published contract without Lom or Sophia libraries in the C client.

### Frontend And Toolkit Independence

`sophia_shell_v1` is Sophia's native shell-role interface over the common Unix
IPC transport. It does not run through X11 or require a Wayland connection.
Application frontends translate their own protocols into Engine transactions;
shells communicate through a separately admitted role endpoint. A future
Wayland or native application frontend can use the same Engine boundaries
without redefining the shell contract. X11 remains the sole active application
authority and the daily-driver priority; adding another frontend is separate
product work.

The wire carries generic capabilities, bounded proposals, opaque identities,
and broker-approved presentation data. XIDs, Wayland objects, Qt classes, QML
trees, and graphics-API objects do not become shell wire types. Other languages
and toolkits can implement the contract using ordinary IPC without linking
Sophia or Quickshell libraries. This independence does not claim portability
of the current Linux session implementation to every operating system.

Engine retains physical input, authoritative geometry, presentation, and GPU
composition. The blind WM retains spatial policy. A future content shell may
rasterize its own widgets under a separately admitted capability; its toolkit
adapter owns local widget behavior and the translation of authorized shell
events. Engine still selects the presented target and controls disclosure and
capture. Content authorization grants neither foreign pixels nor WM authority.

The [reference-client audit](shell-reference-client-audit.md) bounds the first
panel-and-popout workflow. Preparation establishes that workflow and the
downstream build; the [content-shell proposal](content-shell.md) now specifies
its behavior. The subsequent content ADR and schema own the actual bytes;
neither reference toolkit supplies protocol authority or implicit runtime grants.

## The First Experimental Slice

The revision-1 account below records the original experiment. The independent
shell is now Narthex (`narthex --serve`). Its revision-2 tab extension is specified
in [tabbed layouts](tabbed-layouts.md): Hagia proposes opaque group facts, Sophia
supplies recipient-local descriptors, and Narthex confirms complete candidates.
Sophia owns GPU composition, presentation, hit testing, and action validation.
The CPU content implementation described above is later work; this slice is
retained as descriptor history, not its current implementation status.

Revision 1 carries one complete, bounded descriptor snapshot from Sophia to a
separately protected shell. Each row contains an opaque slot, sanitized label,
trust and attention state, exact generation, and a broker-issued action narrowed
to the shell connection. It does not disclose a `SurfaceId`, coordinates, icon,
PID, path, class, namespace identity, or raw input.

The shell returns ordering, selection, and visibility as one complete candidate.
Engine alone resolves slots to private surfaces, lays out and renders the list,
publishes hit targets, captures physical input, and creates an activation from
the exact presented target. Prepared and presented outcomes are separate. A
shell disconnect retains the prior pixels but immediately revokes capture,
queued activations, and the recipient epoch.

The bounded TLA+ model checks that distinction through disconnect, broker
revocation, queue saturation, stale candidates, and acknowledgements. The
shared golden corpus is decoded independently by Rust and Nim, and the protected
cross-process proof runs both C and Hagia clients through presentation,
activation, and complete withdrawal.

The same slice now runs in a normal Hagia session when its shell profile is
enabled. Sophia launches `narthex --serve` in its own Bubblewrap domain,
and `session:window-switcher` asks it for a candidate drawn only from current
presented policy-managed descriptors. Engine renders and
publishes the exact targets. A click returns to the shell for acknowledgement,
then crosses the broker's current issuer check before the WM receives an
ordinary focus request. Shell loss clears capture and reconnects at a new
epoch; retained pixels remain inert until replaced. Core and admitted XI
explicit pointer grabs now participate in Engine's application lease
arbitration, and the compiled profile enables the switcher. Signed installed
evidence, previews, icons, work-area reservations, and the larger display-list
vocabulary remain out of revision 1.

## Why A Driving Client

`sophia_wm_v1` was specified against independent Rust, C, and Hagia clients.
That diversity kept the interface honest. It had to be language-neutral, wide
enough to carry a real window manager, and narrow enough to stay metadata-blind.

Narthex and the independent C proof exercise the descriptor lifecycle. Rich
content has no comparable implemented client yet. Specified in isolation it
fails in one of two directions:

- **Too narrow.** It cannot carry a real desktop, every shell falls back to the
  X11 compatibility path, and the native interface becomes decorative.
- **Too wide.** It re-exposes the titles, classes, PIDs, and view identities
  that `docs/sophia-policy-ipc.md` forbids, and the authority separation
  becomes nominal.

Earlier xmobar X11 evidence remains a useful presentation and
work-area probe, but it is deliberately minimal: static text, no hit targets,
no popups, no animation, no desktop surfaces. Specifying a shell interface
against xmobar would produce the too-narrow failure.

Noctalia is a suitable driver for three reasons. It is feature-complete across
the shell surface area, so it exercises the full vocabulary rather than a
corner of it. It is externally developed and not Sophia-aware, so it cannot
quietly assume Engine internals the way an in-tree client would. And its
existing protocol usage is a finite, enumerable list, which converts "what does
a shell need?" from a design question into a reading exercise.

## What Noctalia Is

An independently developed Wayland desktop shell in C++, built with Meson,
surveyed at `~/src/noctalia`. It provides bars, panels, dock, launcher,
notifications, lock screen, idle behavior, OSDs, theming, wallpapers, desktop
widgets, and multi-monitor shell surfaces.

Measured scale, for judging what a port or an extraction would cost:

| Subsystem | LOC | Relationship to the display system |
| --- | --- | --- |
| `src/shell/` | 120,631 | shell logic; ~109 files name Wayland types |
| `src/ui/` | 28,333 | widget layer; display-system neutral |
| `src/render/` | 17,778 | scene graph and GLES2 backend |
| `src/dbus/` | 15,544 | neutral |
| `src/config/` | 14,433 | neutral |
| `src/system/` | 13,607 | neutral |
| `src/compositors/` | 12,435 | per-compositor IPC behind virtual backends |
| `src/theme/` | 11,932 | neutral |
| `src/scripting/` | 11,520 | neutral |
| `src/wayland/` | 10,808 | the display-system binding |
| remainder | ~34,000 | mostly neutral services |

Total is roughly 291,000 lines across 1,197 files. The display-system binding
proper is under four percent of that. Roughly 84 files outside `src/wayland/`
name core surface types directly, and about 197 name any generated Wayland
type. Those call sites are the mechanical cost of introducing a platform seam.

Noctalia renders with GLES2 over EGL. Its only Wayland coupling in the render
path is `wl_egl_window` creation, resize, and teardown in
`src/render/backend/gles_render_backend.cpp`. Cairo and Pango appear only to
rasterize glyphs into A8 textures, not to draw the interface.

## The Rendering Fork

Noctalia draws its own pixels. The current descriptor interface lets Engine
render and hit-test the chrome while the shell submits bounded proposals and
opaque actions. `docs/compositor-graphics.md` describes the broader direction:
a bounded immutable display list of visual intent, lowered by Engine into its
own primitives and cached textures.

The descriptor revisions cannot carry that renderer's output. A future content
capability must bridge the gap without importing toolkit semantics. There are
two distinct integration paths.

**Path A — X11 compatibility client.** Noctalia keeps its renderer and runs
through the Sophia X Server Frontend, obtaining pixels through EGL on X11 and
the Mesa DRI3/Present path. This requires no new interface. It is reachable
with today's architecture, and the Kitty entry in
`docs/x11-compatibility-matrix.md` establishes precedent for the GPU transport:
ARGB and sRGB FBConfig selection, direct contexts, depth-32 DRI3 import, and
Present submission. But Path A grants no redacted presentation feed, has no
tray or XEmbed, approximates layer placement with override-redirect windows and
`_NET_WM_STRUT_PARTIAL`, and is precisely the shell-as-X11-application model
the native interface exists to replace.

**Path B — native shell client.** A client targets `sophia_shell_v1` and submits
bounded visual intent and, once specified, its own raster content. Engine
validates and composites it. A toolkit may retain its rasterizer behind generic
content resources; rewriting that toolkit into an Engine display-list emitter
is not required. The cached-texture and compositing-operator rules below govern
which work stays client-side.

An X11 compatibility probe can establish ordinary Qt or GLES transport behavior.
It does not prove native shell admission, content transport, or lifecycle.
The native adapter needs its own implementation and evidence.

**Noctalia is a useful specification driver for Path B regardless of whether
Noctalia itself is ever ported.** Its value here is evidentiary, not
operational. The scene graph enumerates what a real shell draws; the protocol
bindings enumerate what a real shell needs to know. Both are needed to specify
the interface, and neither requires shipping the port.

## Evidence: The Display-List Delta

`docs/compositor-graphics.md` proposes an initial display-list vocabulary and
requires that "new primitives require demonstrated compositor use." Noctalia's
scene graph is demonstrated use. Comparing the two:

| Proposed Sophia primitive | Noctalia equivalent | Status |
| --- | --- | --- |
| Solid rectangles | `RectNode` | covered |
| Rounded rectangles | `RectNode` | covered |
| Rounded or gradient borders | `RectNode`, linear-gradient program | covered |
| Analytic rounded-rect shadows | rect program | covered |
| Image textures | `ImageNode` | covered |
| Cached text textures | `TextNode`, `GlyphNode` | covered |
| Clips, opacity, placement, stacking | `Node` | covered |
| Offscreen effect groups | `EffectNode` | covered |

Noctalia additionally carries nodes with no counterpart in the proposed
vocabulary:

| Noctalia node | Purpose | Proposed disposition |
| --- | --- | --- |
| `InputArea` | hit targets | **admit** — the hit-target vocabulary the interface already reserves |
| `WallpaperNode` | desktop background | **admit** — session-owned surface class, not novelty |
| `ScreenCornerNode` | screen-corner masking | **evaluate** — cheap analytic primitive, plausibly general |
| `SpinnerNode` | indeterminate progress | **evaluate** — general UI need, but expressible as animated texture |
| `GraphNode` | sparklines and history graphs | **refuse as primitive** — client-rasterized texture |
| `CountdownRingNode` | timer arcs | **refuse as primitive** — client-rasterized texture |
| `AudioSpectrumNode` | audio visualizer | **refuse as primitive** — client-rasterized texture |
| `FancyAudioVisualizerNode` | audio visualizer | **refuse as primitive** — client-rasterized texture |
| blur program | backdrop blur | **defer** — Engine-owned effect, not a shell primitive |

This delta is the direction's first concrete output. It is the kind of finding
that first-principles design does not produce, because nobody guesses "audio
spectrum" while enumerating a compositor primitive set.

## The Novelty Risk

The driving-client method has a governance failure mode that must be named
before the work starts. `docs/compositor-graphics.md` states that "one-off
visual novelty is not sufficient reason to expand the stable Engine boundary."
A real shell will demand exactly that novelty. Four of Noctalia's nodes above
are visual novelty by that standard.

Letting a driving client expand the primitive set on demand turns Engine into a
general-purpose UI toolkit, which the architecture explicitly refuses. The
method therefore requires a pressure-release valve, and the natural one already
exists in the vocabulary: **client-rasterized image textures.**

A shell that wants an audio visualizer rasterizes it and uploads a texture.
Engine composites the texture without learning what it depicts. Novelty stays
client-side and the primitive set stays small. The
[graphics contract](compositor-graphics.md) governs primitive admission: scene
sampling stays in Engine, and recurring analytic operators may also qualify
through bandwidth preservation and resolution independence. A toolkit's demand
for a novel widget does not itself justify a new Engine primitive.

## Visual Style And Effect Contract

The shell owns visual policy; Engine owns visual authority and execution. A
future shell revision may choose colors, artwork, semantic effects, and bounded
transitions for shell content derived from facts that role may already hold.
Engine validates the proposal, resolves every effect against the session's
admitted capabilities, clocks its transition, computes damage, performs any
scene sampling or offscreen work, and presents the result atomically.

The public shell wire never becomes a shader or renderer-plugin ABI. Effect
proposals name negotiated semantic capabilities with bounded parameters,
generations, and explicit legible fallbacks. Shader source, SPIR-V, GL objects,
native handles, and arbitrary uniform blocks do not cross. Specialized
renderer implementations are trusted build-linked providers selected as
packaged desktop-profile inputs; their private Rust interface evolves with the
renderer and is not frozen with `sophia_shell_v1`.

The semantic capability names and bounded parameters are part of the public,
language-neutral role schema. A shell author can choose an admitted effect and
fallback from any language using ordinary IPC without implementing, linking,
or even knowing the renderer provider that lowers it. The private Rust provider
trait is an Engine implementation seam, not an SDK and not a second public
contract. Most visual identity remains ordinary shell policy, display-list
content, cached artwork, and semantic intents; authoring provider code is only
for a genuinely new trusted renderer operation.

This also preserves the WM/shell separation. `WmChromePolicy` remains the
frozen revision-3 WM's narrow metadata-blind styling surface. A desktop family
that wants richer WM-branded visuals supplies them through a separately
authorized companion shell or, after evidence, a capability-gated WM extension.
It does not move metadata or compositor state into blind spatial policy.

The [content contract](content-shell.md#visual-style-effects-and-pacing) collects
the pacing and reuse requirements. Engine owns the animation clock; a shell
declares admitted transition intent or submits new content generations under
backpressure. Cached content is the initial transport experiment. Its bandwidth,
damage, power, and lifetime costs must be measured before selecting a transport
extension.

### DMA-BUF Handoffs Are A Candidate

A shell-owned DMA-BUF transferred by Unix-domain FD passing is one candidate
for high-frequency client-rasterized content; it is not a settled shell
contract. Admission requires evidence for allocation ownership, fencing,
damage, resource bounds, fallback, renderer failure, and measured bandwidth and
power. The existing client DMA-BUF path does not prove this distinct boundary.

The open cost is that texture upload has different damage, bandwidth, and
power characteristics than an analytic primitive, and an animated visualizer
uploading every frame is the worst case. Quantifying that cost is a
prerequisite to relying on the valve.

## Derived Capability Requirements

Noctalia binds 25 Wayland protocols. Each is a requirement statement about what
a complete shell needs. Grouped by the Sophia authority that would own the
capability:

### Surface placement and stacking

| Noctalia dependency | Shell need | Owner |
| --- | --- | --- |
| `wlr-layer-shell-unstable-v1` | anchors, exclusive zones, four-layer ordering, keyboard-interactivity modes | `sophia_shell_v1` |
| `xdg-shell` | popups, positioners, grabs | `sophia_shell_v1` |
| `hyprland-focus-grab-v1` | dismiss-on-click-outside for menus | `sophia_shell_v1` |

Exclusive-zone negotiation is the load-bearing piece and has no X11 analog
beyond strut approximation. It is the single largest gap.

### Output geometry and scaling

| Noctalia dependency | Shell need | Owner |
| --- | --- | --- |
| `xdg-output-unstable-v1` | logical output geometry and names | Engine projection |
| `fractional-scale-v1` | per-output fractional scale | Engine projection |
| `viewporter` | buffer-to-surface scaling | Engine (display list) |
| `wlr-output-management-unstable-v1` | output configuration UI | session/config domain |

Engine already owns outputs and work areas. Most of this is projection, not new
authority.

### Presentation data

| Noctalia dependency | Shell need | Owner |
| --- | --- | --- |
| `ext-workspace-v1` | workspace list, occupancy, focus | **indicator descriptor** |
| `ext-foreign-toplevel-list-v1` | taskbar and dock entries | metadata broker |
| `wlr-foreign-toplevel-management-unstable-v1` | activate, close, minimize | broker plus opaque actions |

This group is redaction-critical and is where the interface most easily goes
wrong. It corresponds to the existing roadmap item for a bounded redacted
status feed.

**Superseded in part.** The first row is now answered by
`docs/sophia-indicator-descriptor.md`, and not by a broker. Workspace state
originates in the policy process, not in Engine and not in any metadata the
broker can see, so a broker has no upstream source for it. Policy attaches
indicators to its layout proposal and Engine republishes them after commit.

The correction matters beyond one row. This table originally assigned all
presentation data to one owner. There are two, with different trust properties:
policy-authored structure is blind-safe by construction, while broker-supplied
identity needs real sanitization. Keeping them separate means a status bar never
requests identity at all.

### Session services

| Noctalia dependency | Shell need | Owner |
| --- | --- | --- |
| `ext-session-lock-v1` | lock surface with input isolation | session service |
| `ext-idle-notify-v1` | idle timers | session service |
| `idle-inhibit-unstable-v1` | inhibit during media playback | session service |
| `xdg-activation-v1` | launch and focus handoff | session capability |

### Capture and transfer

| Noctalia dependency | Shell need | Owner |
| --- | --- | --- |
| `wlr-screencopy-unstable-v1` | screenshots, region capture | portal |
| `ext-data-control-v1`, `wlr-data-control-unstable-v1` | clipboard history | portal |

Sophia's portal model differs structurally from data-control. A clipboard
manager under a portal grant is not a drop-in translation and needs its own
design pass.

### Target-resolved input

| Noctalia dependency | Shell need | Owner |
| --- | --- | --- |
| `text-input-unstable-v3` | on-screen keyboard input method | Engine plus broker |
| `virtual-keyboard-unstable-v1` | on-screen keyboard injection | Engine, gated capability |
| `cursor-shape-v1` | cursor over shell surfaces | Engine (already protocol-neutral) |

Virtual keyboard is synthetic input injection and needs an explicit capability
grant. It should not be reachable from an ordinary shell authorization.

The ratified pre-schema contract is
`docs/target-resolved-input.md`. Engine resolves physical input against the
applicable output's last-presented interaction snapshot. Discrete actions are
coordinate-free by default, continuous targets use one paced replaceable value
slot, and exceptional coordinates require an independently issued capability
bound to one local region, precision, rate, target generation, and authority
epoch. Stable non-recyclable target identity, presented ownership and
occlusion, device/contact-bound per-seat capture, modal scope, and
precommitted hover/pressed alternatives define the boundary without adding
widgets, styling tokens, or a reactive property system to Engine.

Application coexistence is also a prerequisite, not an ambient pointer-boundary
rule. Frontend grabs become Engine-visible profile-scoped route leases;
ordinary scope exit waits for release acknowledgement, while lock and other
security transitions revoke input immediately and discard queued old-epoch
events. The current committed-scene application hit test and frontend-local
cross-boundary grab behavior must be repaired before this shell path ships.

### Display control and compatibility

| Noctalia dependency | Shell need | Owner |
| --- | --- | --- |
| `wlr-gamma-control-unstable-v1` | night light, color temperature | Engine |
| `ext-background-effect-v1` | backdrop blur behind surfaces | Engine effect |
| XEmbed (not Wayland) | system tray | retained-workflow admission only |
| `dwl-ipc-unstable-v2`, `org-kde-plasma-virtual-desktop`, `hyprland-toplevel-mapping-v1` | per-compositor workspace and window IPC | not applicable |

The last row is Noctalia's compositor-specific backend layer. Under Sophia it
is replaced wholesale by the redacted presentation feed, which is a
simplification rather than a port.

## Authority Constraints The Driver Must Not Relax

Recorded here because a driving client creates continuous pressure to relax
them:

- The shell receives only broker-approved presentation facts, including the
  current bounded sanitized labels. Raw titles, classes, PIDs, paths, XIDs,
  namespace identities, and portal payloads stay out regardless of what the
  driving client's widgets would like to display.
- Shell authority cannot set application placement or focus. Dock and taskbar
  activation is an opaque action submitted for adjudication, not a focus call.
- WM authority cannot acquire shell metadata. Hagia's blind spatial-policy
  projection stays blind; Narthex or a content shell is separately authorized in
  a different protection domain, with no ambient IPC or shared writable state
  that recombines the roles.
- Engine retains composition, physical hit-testing, physical input, capture,
  and cursor authority. Future content shells may rasterize their own widgets
  and perform local widget hit testing only on authorized input. Precommitted
  alternatives and content updates remain bounded proposals; Engine owns the
  animation clock and presentation schedule.
- The shell endpoint is distinct from `sophia_wm_v1`. Endpoint credentials do
  not prevent same-process collusion. Sharing an executable, repository, or
  language is permitted only through separately supervised protection domains.

A shell that cannot be built within these constraints is evidence about the
constraints, and that evidence belongs in the research log before any boundary
moves.

## Reservation And Action Coordination

Tier-1 exclusive zones are one logical presentation transaction, not a shell
commit followed by an unrelated WM reaction. The ordered identity chain is:

```text
shell candidate + reservation generation
    -> Engine work-area generation
    -> WM snapshot generation and connection epoch
    -> exact WM projection
    -> one coherent logical presentation bundle
```

Engine may prepare the shell candidate and request WM policy concurrently, but
promotion requires the candidate to be ready and every reservation,
generation, and shell/WM epoch to match. Supersession, disconnect, timeout, or
malformed policy rejects the incomplete bundle and preserves the prior
presented shell, work area, and application projection together. Normal
desktop components may therefore stop progress but cannot expose a half-new
desktop. Lock, session takeover, and other security surfaces follow their
separate preemptive authority path and never depend on shell or WM
acknowledgement.

The earlier Tier-0 prototype avoided this cycle by reserving a fixed indicator
strip before the WM projection. That automatic strip is removed from the normal
session. The user's selected shell supplies panel UI and proposes its reservation;
publishing policy indicators alone creates no bar or reserved space. The content
proposal therefore keeps reservation changes in the coherent presentation bundle
rather than relying on a built-in strip.

Opaque actions are capabilities, not globally meaningful integers. Each is
bound to an issuer role/authority and revocation epoch, recipient role/epoch,
operation class, and optional target slot/generation. The receiving authority
deduplicates activation identity within its epoch. A broker-issued taskbar
action cannot be interpreted as a policy or session action even if their wire
integers collide. Concrete records and limits remain schema work.

## Open Questions

1. Does a display-list shell interface carry a full desktop at acceptable cost,
   or does per-frame list submission for animated content force a buffer path?
   This is the question that decides whether Path B is viable at all.
2. *Partly answered.* Content-addressed cached textures are the first path to
   model and measure; descriptor passing remains deferred. Whether the shared
   transport can carry even that shell texture traffic remains
   sequencing-critical.
   `docs/sophia-policy-ipc.md` limits one frame to 64 KiB,
   uses begin/chunk/end transfers for anything larger, permits one transfer in
   flight per direction, and describes a bytes-only wire with no file-descriptor
   passing. A 1920x40 bar at ARGB8888 is roughly 307 KiB, or five chunks for a
   single full upload. Continuous content animation has not been shown to meet
   the required pacing and bandwidth bounds through this path.
   The selected first experiment uploads content once and references it by
   handle thereafter. It preserves the bytes-only wire but does not help
   genuinely per-frame content. A descriptor-passing side channel may be
   reconsidered only after that failure is measured. Strictly analytic display
   lists remain rejected because they remove the novelty valve and force the
   toolkit outcome the architecture refuses.
3. What is the damage, bandwidth, and power cost of the client-rasterized
   texture valve under a continuously animating widget?
4. *Partly answered.* Exclusive zones use the exact generation chain above and
   never grant the shell direct application placement. Layer ordering and
   keyboard-interactivity modes still need a driving-client-derived vocabulary.
5. *Partly answered.* Workspace and layout structure is settled by
   `docs/sophia-indicator-descriptor.md`: policy-authored slots, blind-safe by
   construction, carried on the commit. What remains open is the dock and
   taskbar half, which needs per-window identity from the broker. Icons are the
   hard case: an application icon is close to an identity disclosure, and no
   structural property makes it safe the way policy blindness makes labels safe.
   The offline descriptor reference now proves a title-only recent-window list
   with opaque activation; it deliberately preserves icon tokens without
   rendering them and does not choose ordering or recency.
6. *Answered.* Engine owns the animation clock. The shell declares bounded
   transition intent and submits content generations; it does not drive frames.
7. *Answered for descriptors.* Revision 4's [launcher](application-launcher.md)
   assigns search and ordering to the shell, drawing and input to Engine, and
   catalog policy and execution to the session. It grants no arbitrary launch
   authority to the shell. A later custom-content launcher still needs the
   [identity and activation contract](content-shell.md#a-later-custom-launcher)
   that arbitrary artwork makes necessary.

## Roadmap Placement

`todo.md` owns execution order. CP-14.3 remains the X11 development-session
priority. CP-15.1 audits the native protocol-family lifecycle; CP-15.2 provides
one family conformance surface. Shell stabilization remains behind both gates.

The admitted parallel preparation tranche establishes the Quickshell fork,
build baseline, and [panel-and-popout audit](shell-reference-client-audit.md).
It can refine requirements without adding Engine behavior or changing wire
records. The build's success or environmental blocker must be recorded; a
blocked build does not become a completed milestone.

A later content prototype needs separate admission with a named workflow and
exit gate. The behavioral proposal does not bypass that gate. The prototype must
turn the [content contract](content-shell.md) into a modeled generic wire boundary,
publish schema and corpus evidence, and exercise it with Quickshell and an
independent non-Qt client. Existing descriptor clients remain supported. It
cannot borrow the stable WM interface's status or reopen frozen WM records to
accommodate a toolkit.

### Reference Workloads And Evidence

The Noctalia survey above remains research provenance for native shell needs,
including compositing, metadata, and authority-specific services. Its scene
graph is useful input, but a native client need not expose its toolkit's tree
or replace its own rasterizer with Sophia primitives. Cached client content
provides the intended escape hatch for ordinary widget drawing.

Quickshell supplies a different reference: a reusable QML shell framework with
existing window backends and a Qt Quick renderer. The downstream fork tests
whether a generic shell boundary can support that rendering model. Narthex
continues to prove the confined descriptor tier. A separate non-Qt content
client will test whether the richer contract can be implemented from its
published specification alone.

Classical X11 desktops and X11 Quickshell panels remain application-frontend
workloads. Their compatibility results cannot establish native shell
conformance. Conversely, a successful native shell prototype cannot establish
X11 application compatibility or physical presentation quality.

Transport feasibility remains open: measure cached content, changed bytes,
damage, allocation lifetime, and frame pacing before choosing any transfer
extension. Shared family framing is retained; a toolkit-specific channel or
private Engine ingress is not an alternative to specifying the boundary.

## Non-Goals

- This is not a commitment to port Noctalia. Noctalia is a specification input.
- This is not a commitment to run Noctalia under Sophia in any form.
- X11 shell applications are compatibility workloads; they do not implement
  the native shell role. Noctalia has no admitted implementation tranche.
- This does not expand the display-list vocabulary. It records a candidate
  delta and a proposed disposition for each entry; expansion follows the normal
  demonstrated-use process.
- Preparation does not implement rich content, add an application frontend,
  port a full desktop shell, or install a replacement live shell.
- This does not alter `sophia_wm_v1`, Hagia's blindness, or any existing
  authority boundary.

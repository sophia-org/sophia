---
id: 6tewvlbh
date: 2026-09-08
kind: plan
tags: [plan, rendering, x11]
---
# Universal device negotiation across Sophia clients

## Scope and exit

Provide a universal device-negotiation solution equivalent in purpose to
Wayland DMA-BUF feedback, adapted to Sophia's native X11 architecture. Device
and buffer compatibility must follow capabilities and resource identity rather
than executable names, browser flags, or user-maintained launch recipes.
There must be no supported-application registry anywhere in the implementation.
Users and developers should receive the same negotiated behavior through normal
startup, a shortcut, a catalog entry or an independent terminal command.

niltempus explicitly promoted this work to top priority on 2026-09-08. `t069` owns
its design and implementation; its ordering and status live in
[todo.md](../../../todo.md). The Chromium launch adapter in `t068` remains a
bounded compatibility measure and does not satisfy this task's exit.

## Design and acceptance

- Trace advertisement, allocation, import and presentation across Sophia's
  existing DRI3/EGL/GLX paths. Identify which device and format/modifier facts
  every client can consume, including independent video-allocation paths.
- Review niri/Smithay DMA-BUF feedback and relevant X11 references. Borrow
  capability and lifecycle semantics without adding Wayland to Sophia or
  assuming that a server advertisement controls all client device selection.
- Keep Engine protocol-neutral, frontend translation authoritative, and the WM
  blind. Engine must contain no application-specific adapters, executable-name
  recognition, browser switches or toolkit exceptions. It consumes generic
  device identity and buffer capabilities. Protocol translation stays in the
  frontend; the existing temporary Chromium workaround stays in the CLI.
  Specify device identity, compatible format/modifier sets, topology changes,
  stale evidence and bounded failure before changing interfaces.
- Define a performant cross-device strategy where direct import is unavailable;
  measure any copy or conversion path. Do not count a crash-driven software
  browser fallback as successful negotiation.
- Prove behavior for unclassified executables and multiple client/toolkit
  families, launched both through Sophia and independently from a terminal.
  Cover both same-device and incompatible cross-device buffers on the two-GPU
  host, plus device loss and recovery with bounded resources and no stale reuse.
- Retain exact source, binary, device and buffer evidence, deterministic
  conformance tests, and physical accelerated-video acceptance. State any
  client/protocol limitation explicitly instead of claiming universal coverage
  from a supported-name list.

## Approved implementation contract

The 2026-09-09 implementation keeps the end-to-end exit above. Server-side
conformance, transfer tests and an adapted browser are intermediate evidence;
`t069` stays open until normal unmodified clients work without device overrides.
A client-internal failure before submission remains an explicit boundary, even
if its eventual resolution requires an upstream client change.

Each authenticated connection pins an immutable device bundle: one originating
render device, its measured import capabilities and its allocator. Installing a
successor changes new connections only. Backings keep their originating bundle
through publication, free, cross-connection use and final release. Device loss
changes availability, never an old connection's screen modifiers. The frontend
retains at most sixteen live bundle generations and preserves its GLX capability
catalog across replacement.

DRI3 1.3 device hints remain advisory and confer no device authority. Screen
modifiers stay fixed per connection; window preferences may change with exact
surface and output-topology generations. Present reallocation feedback requires
client opt-in, an actual copied completion and proof that another advertised
layout would permit a flip with every other eligibility condition satisfied.
A format refusal by itself is insufficient. Reallocation advice is suppressed
until the window's preference generation changes.

Render-device membership is compared separately from connector topology. An
identical startup event replay causes no output reconstruction. Keep the chosen
client device while it remains healthy, prepare replacements outside the owner
loop and install only an acknowledged current generation. Pending preparation
and renderer refresh obligations survive a bounded deferral.

Rendering always tries the actual incoming descriptor on the destination first.
Successful source-device hints may order fallback candidates but never bypass
that attempt or establish provenance. Cross-device transfer uses persistent
source contexts and at most three reusable internal linear bridge buffers,
charged to the renderer's existing memory budget. GPU completion and release of
internal exports precede reuse. Destination images exported to other consumers
remain immutable allocations: an external descriptor can outlive the renderer's
local reference count.

Worker shutdown must not block the owner on a full channel or an unfinished
thread. Retired workers remain in a bounded lifecycle registry; replacements on
the same exact device identity wait for their predecessors to finish. No claim
is made that a thread can cancel an unresponsive driver call.

Validation proceeds through protocol and lifetime tests, source-backed
contrary-path review, real same-device and cross-device pixel/performance probes,
and the normal-launch physical gate. Signed implementation checkpoints may be
pushed before physical acceptance; installation and acceptance are separate.

## Native feedback model and reference boundaries

On 2026-09-09 niltempus directed the implementation to use XLibre/yserver as X11
references and native Wayland for stronger negotiation patterns. Adapt those
patterns within Sophia's existing authority boundaries. No Wayland server or
Xwayland dependency is introduced.

The [native DMA-BUF feedback contract](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/stable/linux-dmabuf/linux-dmabuf-v1.xml)
separates one main import device from optional preferred allocations for a
surface. Feedback arrives as a complete replacement; the client retains a
composition fallback and decides whether to reallocate. The corresponding
Sophia mapping is:

| Native feedback pattern | Sophia owner and X11 contract |
| --- | --- |
| Main device and measured import formats | Backend prepares one device/allocator/capability bundle; frontend pins it per connection and answers DRI3 Open/GetSupportedModifiers. |
| Atomic feedback replacement | Session publishes a complete surface/topology-generation snapshot; frontend replaces only a valid current snapshot. DRI3 screen modifiers retain their lifetime guarantee. |
| Surface-specific scanout preference with composition fallback | Frontend intersects per-output preferences with the connection's measured screen formats. Actual import and atomic scanout validation remain authoritative. |
| Device identity precedes tiled-layout preference | Retain server-observed render-node identity. Equal modifier numbers or a client's hint alone do not prove same-device compatibility. |
| Reallocation only when beneficial | Keep Present SuboptimalCopy behind its sufficient alternate-layout proof and preference-generation suppression. |
| Client consumes the selected device | Existing X11 clients must use the DRI3 capability in the relevant allocation path; server feedback cannot redirect an independent client-internal allocator that never reads it. |

The reference audit used Smithay
`13738f8f2cc18224c229e7e8309ccdaa34e92e2a` and niri
`dd75865f547f0eac0e9b6c4d86d2cd00c0744252`. niri's
`src/backend/tty.rs::surface_dmabuf_feedback` intersects renderer and plane
formats, then restricts cross-device scanout preferences to LINEAR. Its
`src/niri.rs` selects surface feedback from actual render-element states.
Smithay's `src/wayland/dmabuf/mod.rs` builds immutable feedback and sends changed
surface snapshots without repeating identical ones.

The admitted next correction applies niri's conservative device rule to
Sophia's existing window preferences. Capture exact server-owned render-node
identity outside authority locks. Revalidate held card and render descriptors
against their current node identities before and after physical sysfs mapping;
node-number reuse must not associate an old KMS group with a new device.
Tiled preferences require a matching live
connection and output identity; absent or different identities retain only
LINEAR when already measured by both sides. Client hints may restrict that set,
never establish the identity proof. Queries perform no discovery and screen
capabilities remain unchanged. Regressions cover equal tiled modifier numbers
on different GPUs, absent/conflicting hints, node replacement, stale topology
and old connections retained across bundle replacement.

This is an allocation-preference correction, not the missing counterfactual
flip proof and not a repair to Chromium's early media selection. The
[native Wayland comparison](../investigations/uqnx2t2b-brave-gpu-restarts-after-va-buffers-fail-gbm-import.md#native-wayland-and-x11-comparison-on-2026-09-09)
shows that Chromium already propagates compositor feedback to its GPU child on
Wayland. Its X11 path lacks the corresponding propagation. The overall normal,
no-override launch exit remains unchanged.

## SuboptimalCopy gate

The 2026-09-09 contrary-path review found no sufficient counterfactual proof
that a different allocation would flip. The installed `99103568` XR24 comparison
now supplies that narrow prerequisite, as recorded in the
[physical acceptance](../investigations/qfg6mjp2-atomic-test-history-is-not-alternate-layout-flip-proof.md#installed-framebuffer-comparison-on-99103568).
Signaling still requires all conditions below. Present reallocation advice is
optional, and unavailable proof must preserve ordinary completion. The five
tracked obligations are:

| Condition | Implementation seam | Evidence and exit |
| --- | --- | --- |
| Client opt-in | Socket Present dispatch and pending frontend transaction | Preserve bit `0x8` per exact accepted transaction. ForceCopy (`0x2`) suppresses advice even with opt-in. Test ordinary clients, invalid options, cancellation and simultaneous transactions. |
| Actual copied disposition | `live_session/presentation.rs`, `LivePresentBufferDisposition` | Existing ownership is exact. Regression must restrict advice to `Copied`; `Retained` and `Flipped` remain `Flip`, with existing idle ordering preserved. |
| Different layout would permit a flip | Backend allocation and atomic validation | Prerequisite for signaling. A failed TEST_ONLY has no attributed cause. Establish a sufficient proof on the exact candidate and output generation, with an available alternative allocation. |
| Every other eligibility check passes | `DirectScanoutVerdict`, exporter atomic validation | Require the counterfactual proof above through exact retirement and current frontend revalidation. Unrelated geometry, assignment, bandwidth, synchronization or policy refusal must suppress advice. |
| No repeat per preference generation | Frontend per-window allocation state | Record the last signaled exact surface/preference generation. A later ordinary Copy is allowed; a second SuboptimalCopy in the same generation is not. |

A differential TEST_ONLY with an otherwise equivalent candidate allocation can
supply the missing evidence. IN_FORMATS membership can prove that the original
layout is unsupported, but an alternative's membership alone does not prove
that it would pass atomic validation. Preserve that distinction in any evidence
type; do not name a necessary condition as a sufficient one.

Framebuffer creation may reject a layout before any original atomic request
exists. The installed AMD control reproduces that case. A distinct proof arm
must retain the actual explicit AddFB2 failure after successful plane imports,
retry it against the current owner, and test the existing same-format fallback.
The shared production request builder must establish that substituting that
fallback framebuffer preserves every intended display property. A passing test
and unchanged commit/retirement remain mandatory; neither EINVAL nor plane-table
membership alone is sufficient. Diagnostic stages must distinguish this proof
from two actual atomic tests.

The proof task also resolves three open questions: whether atomic failure errno
is retained anywhere useful; whether the live preferred modifier set is nonempty;
and whether the cached direct-scanout test resets on buffer/layout change.
Treat the review's negative searches as bounded observations until those paths
are traced. Tests must cover stale candidates and topology changes as well as
one successful alternate-layout proof.

The physical comparison session must admit direct scanout at owner startup
(`SOPHIA_ENABLE_DIRECT_SCANOUT=1`) and record routed completion clocks
(`SOPHIA_LIVE_VISUAL_PROGRESS=1`). Ordinary installed sessions leave direct
scanout disabled until its physical gate is promoted. Verify this precondition
before interpreting an absent paired-test record; enabling diagnostics or
changing the probe's environment cannot enable the owner's direct path.
Normal-session allocation/preferences and Copy/Idle checks remain useful,
separate evidence.

The direct-enabled `5110bb7d` session demonstrated 120 linear Flip completions
for each of XR24 and AR24 on an isolated secondary output. The corresponding
compressed allocations each completed 120 Copies. These separate runs do not
prove equivalent atomic requests. The same run exposed missing transport from
backend tracing into daily capture; the CLI must preserve the two approved
atomic/layout record types before an absent witness can be interpreted. The
[live control investigation](../investigations/qfg6mjp2-atomic-test-history-is-not-alternate-layout-flip-proof.md#direct-scanout-controls-and-missing-trace-transport-on-5110bb7d)
records the precise evidence and repair.

The [atomic evidence investigation](../investigations/qfg6mjp2-atomic-test-history-is-not-alternate-layout-flip-proof.md)
records the bounded TEST_ONLY report implementation and the live plane-format
query. The cache covers an eligibility episode, not an exact candidate; actual
test reports therefore remain absent on cache bypass. Error and canonical
request retention establish test history without attributing a refusal to a
layout. A passing equivalent alternative, correlated through fallback ownership
and current device/output state, remains the prerequisite for reallocation
advice. XLibre's table-based heuristic does not replace this requirement.

The current implementation retains strict per-head format observations and
an original allocation through its exact fallback renderer request. Normal
allocation preferences keep their existing parser and ordering. The source
expires after one second; a newer offer or owner/topology transition invalidates
it even when the scene trace or restored target generation is numerically equal.
On a quiescent card turn, a matching-format fallback can supply an existing
alternative framebuffer for a fresh comparison. A creatable original uses two
comparable TEST_ONLY requests; an explicit AddFB2 refusal uses the distinct
framebuffer proof above and one alternative TEST_ONLY request. Temporary
cleanup has an independent bounded retry owner and remains serviced while idle.
Probe admission costs at most one comparison per second per output, with no additional
render or pixel copy.

The comparison now follows its accepted alternative through exact physical
retirement and committed copied-transaction settlement. Unknown tables, implicit
layouts, different-format fallbacks and changed requests remain inconclusive.
The ordinary render can prefer the source fourcc, with pre-draw allocation
fallback preserving normal rendering. A successful XR24 result still does not
prove AR24 layout support. Native identity and submission context are invalidated
on owner/topology changes, including rollback to a previous numeric generation.

These observations are not a reusable flip permit. Window preferences publish
independently measured XR24 and AR24 rows,
intersected with the connection's pinned import capabilities. An unavailable
bundle offers no preferences. The completion join now compares a retired pair
against the owner's current native/placement context and acknowledged snapshot,
then the frontend's current exact Present subject and effective membership.
Native context identities do not repeat across reconstruction or rollback.
Contention, changed state and unavailable devices suppress the optional match
without delaying ordinary Copy. The native path conservatively withholds evidence
for multi-output joins, and the frontend does not substitute parent preferences
for child presentations. Installed `99103568` supplies one complete XR24
framebuffer-rejection/alternative-test/retirement/current-preference chain on
output 2. AR24 completed ordinary rendering but remains unproven. This accepts
the t070 prerequisite without generalizing the proof to other layouts or outputs;
only then may the signaling obligations proceed.

The frontend completes those observations with a fresh, nonblocking decision at
the exact copied Complete. Client opt-in without ForceCopy and current proof are
both required. One claim belongs to the exact surface in its accepted preference
generation; ordinary Copy does not reset it. Unknown or duplicate Complete does
not claim it, and failed or partial subscriber delivery cannot restore it. The
session records the resulting mode while preserving actual ownership counters,
Idle ordering and displayed-frame pacing. No protocol bit or application identity
crosses into Engine.

Deterministic acceptance covers the wire option, full proof rejection matrix,
actual emitted mode, accepted-generation replacement, window reuse, contention,
duplicate feedback and partial queue delivery. Installed signaling acceptance
remains separate: an opted-in generic probe must receive SuboptimalCopy for one
exact proven transaction, no repeat in that surface/preference generation, and
ordinary completions when permission or proof is absent. Reuse the controlled
output and identity checks from t070; do not count a fixture as physical evidence.

A generic explicit-layout DRI3 probe supplies real XR24/AR24 allocations without
changing their metadata. Its bounded evidence reader joins the exact tested
source, retired transaction, current native/preference generations and the
client's received completion, including recorded opt-in for SuboptimalCopy.
These tools prepare physical acceptance; small
composed-window feedback and private allocation checks do not satisfy it. See
the [probe contract](../../../tools/probes/README.md#explicit-dri3-layout-probe)
for run boundaries and the retained evidence requirements.

The [installed fb8fe8be acceptance](../milestones/urqkkdzp-exact-copied-present-evidence-gates-reallocation-advice.md#installed-acceptance-on-2026-09-10)
now establishes the XR24 signaling gate: one received hint and a later proven
Copy in the same generation, plus no-opt-in, ForceCopy, direct LINEAR and
ineligible-geometry controls. This closes the narrow t071–t074 obligations;
the broader normal no-override exit remains unchanged.

## Implementation checkpoints

The [reallocation-advice checkpoint](../milestones/urqkkdzp-exact-copied-present-evidence-gates-reallocation-advice.md)
records the completion decision, deterministic evidence and remaining installed
signaling gate after the accepted XR24 proof.

First replace invented DRI3 modifier responses with measured renderer import
capabilities and refuse legacy exports that cannot represent the backing's
actual offset or stride. Keep import and allocation/export capabilities distinct.
Then add bounded renderer-owned multi-device import and transfer where direct
sampling fails, with exact buffer identity and synchronization. Acceptance needs
real device and pixel evidence; an advertised overlap alone is insufficient.

The initial independent audits establish a boundary: Chromium can fail to import
its own media buffer before submitting any buffer to Sophia. Its preliminary
GPU identity comes from PCI enumeration, while its X11 importer uses DRI3 Open.
Neither standard server feedback nor server-side transfer can intercept that
client-internal failure. This task must not claim to fix that path through server
capabilities; retain it under `t068` with the source evidence and explicit limits.

The [first capability checkpoint](../milestones/f24uuvwu-measured-dri3-import-capabilities-and-exact-legacy-exports.md)
records measured DRI3 negotiation and exact legacy exports, its retained
validation, and the transfer and physical-acceptance limits.

The [cross-device capture checkpoint](../milestones/j11gjkh1-generic-cross-device-image-capture-and-worker-recovery.md)
records submitted-buffer transfer, worker and inventory lifetime repairs,
offscreen pixel evidence, and the remaining frontend migration boundary.

The [first-buffer regression investigation](../investigations/09ywbgwt-measured-modifiers-exposed-an-rgb-stride-check-on-compression-metadata.md)
records a real Mesa client rejected before Present because descriptor validation
applied RGB image geometry to compression metadata. Repair `009498fb` retains
descriptor bounds and passes real GLX/EGL GPU pixel checks. niltempus confirmed
Kitty and Quickshell visibility and responsiveness in the installed release;
this accepts that regression repair without closing the broader exits above.

The [implicit-modifier investigation](../investigations/n5i1x7iv-implicit-dma-buf-exports-used-the-wrong-drm-modifier-sentinel.md)
records a second generic defect: legacy DRI3 exports preserved the FD but
substituted a non-ABI modifier, producing black pixels in a real EGL consumer.
The corrected export passes private pixel checks; Brave's logged initialization
failure and physical video acceptance remain separate gates.

The later [default-visual investigation](../investigations/g930kzbe-default-x-visual-excluded-rgba-pixmap-configurations.md)
traces the surviving initialization failure to alpha-zero GL configurations
on the default X visual. The XLibre-compatible correction separates native X
depth from GL color bits and passes default-visual texture and window pixel
tests. niltempus confirmed normal video on installed `b43d23d0bb15` on 2026-09-09.
That accepts the white-video repair with the existing CLI device override;
it does not satisfy this plan's requirement for generic device negotiation
without an application adapter.

The [connection and renderer refresh checkpoint](../milestones/szr8j0rg-connection-pinned-device-negotiation-and-bounded-renderer-refresh.md)
records immutable frontend bundles, DRI3 hints and window preferences, bounded
source inventory refresh, pooled internal transfers, and full repository plus
2,400-frame offscreen pixel validation. Its remaining physical and client-internal
limits preserve the end-to-end exit above.

The [controlled no-override run](../investigations/uqnx2t2b-brave-gpu-restarts-after-va-buffers-fail-gbm-import.md#controlled-no-override-acceptance-on-2026-09-09)
on installed `76ed2fddf31a` reproduces a client-internal import failure after two
video-frame callbacks. The matched override control sustains 903 callbacks with
VA decoding and no GPU restart. This leaves the normal-launch exit unmet and
pins the required next device-selection repair to the client boundary.

## 2026-09-25 evidence reconciliation

Review against accepted `9ee301e74ef22a065dcb112dc1873c7e458dfec4` preserves the
normal, unmodified-client exit. Connection-pinned bundles, cross-device transfer
and the accepted XR24 reallocation controls are evidence for their named server
boundaries. They do not establish the no-override client allocation path.

The retained `76ed2fdd` comparison failed inside Chromium before submission;
the matched override control sustained 903 video callbacks. This is historical
evidence with its original identity, not a fresh verdict about current browser
versions. No new normal-launch or device-loss acceptance was performed by this
review. A later workspace or native-protocol-family pass cannot replace those
missing client/device observations, and a server-only transfer change cannot
repair an allocation that the client never submitted.

## 2026-09-25 device-loss audit and D1 repair

An audit of accepted `caca7e37` found one in-scope server defect in device
loss and recovery.

- **The defect (D1).** The frontend kept its configuration, and through it the
  configured generation-1 device bundle, for its whole lifetime. After loss
  and supersession, the startup provider and its render descriptor stayed
  open. It also permanently held one of the sixteen generation slots.
- **The repair.** Both configured construction paths now hand that bundle over
  exactly once: `XServerFrontend::bind` and `bind_exclusive` through
  `over_listener`, and `run_x11_core_socket_server_once_config_traced_with_idle_timeout`.
  Only the registry, connection pins and backing owners hold a generation
  afterwards. Release is not immediate when the last lease ends: the registry
  collects a superseded, unleased generation at the next install. The
  controls observe exactly that.
- **Evidence.** Real-frontend controls with a provider lifetime witness fail
  first, pass after the change, and fail again when the handover clones
  instead of takes. Leases and the sixteen-generation bound are unchanged.

These findings remain under contract review and are unchanged:
- **D2.** A connection admitted while the current bundle is already lost is
  pinned to it, is offered DRI3, and every Open refuses.
- **Capacity retry.** It has no deadline or record.
- **Seat-wide failure.** One unopenable seat device fails every preparation.
- **Driver reset.** A reset that keeps the node identity is not detected as
  loss.
- **Session-fatal edges.** Several acknowledgement paths end the session.

The normal-client exit remains a client-internal boundary. The current
upstream proposal is recorded in the
[Brave investigation](../investigations/uqnx2t2b-brave-gpu-restarts-after-va-buffers-fail-gbm-import.md#upstream-patch-proposal-at-92802374-2026-09-25).
No device-loss physical acceptance was performed, and `t069` remains open.

## Connections

The [Brave GPU investigation](../investigations/uqnx2t2b-brave-gpu-restarts-after-va-buffers-fail-gbm-import.md)
records the cross-device failure and current CLI adapter. The
[GLX pixmap investigation](../investigations/uffn76nu-glx-pixmap-exports-need-coherent-backing-and-reply-ordering.md)
records related export and lifetime repairs. Preserve the boundaries in
[architecture](../../architecture.md) and the current
[client launch contract](../../configuration.md#client-launch-adapters).

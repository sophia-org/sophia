---
id: id869143
date: 2026-10-09
kind: investigation
status: investigating
tags: [investigation, x11, portals, namespaces]
---
# X11 drawable readback and an operator capture path

## Question

Why did screenshot tools fail against a working accelerated X11 client, and
what capture interface should an operator or agent use to verify a GUI?

niltempus relayed the RAWmakase session's report on 2026-10-09. Recording this
scope does not promote capture implementation ahead of the output recovery
critical path.

## Evidence

### Reported observation

Claude reported RAWmakase 0.2.3 running for 20 seconds without errors in a
1266x1398 window on the installed Sophia session, `DISPLAY=:77`:

- `xwd -root` and `xwd -id <window>` failed with `BadRequest`, major opcode
  133 (`XKEYBOARD`), minor opcode 3 (`XkbBell`), leaving empty files.
- `scrot -u` produced a black 2542x1398 image. That size matched the focused
  Brave window, not RAWmakase; this is not a readback result for RAWmakase.
- `xdotool` reported XTEST unavailable. `xdpyinfo` listed 16 extensions,
  including MIT-SHM, RENDER and XKEYBOARD, without XTEST.

The application used its CLI renderer for inspection. These are the relayed
observations, not a reproduction by this investigation. No raw capture or
protocol trace was supplied with the report.

### Source check

Source inspected at `95e10f60879897eaad1b191dfe64c67b7e0ead3e`. The XKB
decoder, image readback entry, injection configuration and extension discovery
files are unchanged from installed Sophia `22b124c88`.

1. `crates/sophia-x-authority/src/wire/extensions/xkb.rs` has no Bell request
   arm. Minor 3 reaches `UnknownOpcode`; the reported error names that missing
   request, not GetImage. The installed `xwd(1)` manual documents bells before
   and after a dump and `-silent` to suppress them. An explicit-window
   `xwd -silent -id <window>` is a useful follow-up to separate this error
   from pixel readback; it has not been run here and does not promise pixels.
2. Core GetImage and MIT-SHM GetImage both call
   `crates/sophia-x-authority/src/image.rs::read_drawable_image`. It reads
   CPU backing through `runtime/drawing/image_ops.rs`, composites inferiors
   within the caller's namespace, and zero-fills absent backing. It does not
   read Engine's composed frame. The wire tests in `tests/x11_wire/` cover
   image data and namespace-private root readback; the timed Present test in
   `tests/support/timed_present_wire.rs` distinguishes presentation raster
   from core drawing backing. Accelerated presentation therefore does not
   imply corresponding pixels are available to GetImage.
3. XTEST is implemented. `src/dispatch.rs` lists and advertises it only for
   `XTestAdmission::Admitted`; Session installs an injection policy only with
   `--admit-xtest` (`crates/sophia-session/src/live_session/run.rs`). Its absence
   is consistent with an unadmitted connection, not missing implementation.
   The reporting process's admission was not independently inspected.

## Finding and resolution

The blanket statement that Sophia cannot read X11 pixels is false. There is
a missing XKB request and a real gap between CPU drawable readback and a usable
capture of accelerated content or the composed desktop. The black image is
consistent with that gap; its exact producer and RAWmakase's own drawable
readback remain unmeasured.

An operator-authorized capture interface is wanted. A small CLI client should
use the same scoped portal/provider contract as other clients, so an agent can
request a single image and check its target, dimensions and frame identity.
The capture task must audit accelerated same-namespace drawable semantics
separately from privileged composed-output capture. Giving GetImage global
Engine access is not the portal implementation.

The capture scope and exits belong to [t046](../plans/queue-16-portals-and-confined-applications.md#t046).
The [t303 headless backend investigation](jweorh0z-headless-sophia-validation-and-capture-with-vkms-writeback.md#t303)
can supply a virtual-device proof, but VKMS is not a prerequisite for exposing
capture on the running native desktop. XTEST admission remains a separate
input permission; capture does not enable it. The later multi-listener group
admission work remains in [t142](../plans/ooy00zjd-socket-directory-and-frontend-multiplexer-architecture.md).

## Namespace prerequisites for capture (2026-10-09)

niltempus asked whether namespaces should be locked down before t046, then
requested this reference for later. The recommendation is to prove admission
and confinement before shipping the capture executor. Portal design and
device-free tests can proceed alongside that work. The broader Plan 9 namespace
composition investigation need not finish before a bounded capture slice.
This records recommended sequencing, not a task promotion or queue reorder;
monitor recovery remains the active implementation priority.

The relevant task and note map is:

| Task | Owning note and scope | Relationship to capture |
| --- | --- | --- |
| t133 | [Admission security investigation](1pv291te-namespace-and-client-admission-security-gaps.md) and [pidfd proposal](../plans/esnqxpqw-pidfd-and-namespace-admission-optimizations.md): peer identity, launch origins, descriptor transfer, process lifetime and admission bounds. | Reconcile the threat model with current code and prove the identity relied on by grants. |
| t142 | [Socket directories and live frontend groups](../plans/ooy00zjd-socket-directory-and-frontend-multiplexer-architecture.md#phase-2----t142-several-listeners-one-synchronous-frontend-tier-2): multiple listeners, credential admission and confined launcher mounts. | Prove that a confined group cannot reach the trusted endpoint and cannot acquire another group's authority. |
| t275 | [Plan 9 namespace model](kcfh2hdg-adopting-the-plan-9-namespace-model-in-sophia.md): service views, bind/mount/union semantics, inheritance and revocation. | Broader design; reconcile future recipes with existing grants without making all composition semantics a capture prerequisite. |
| t045 | [Confined daily-driver promotion](../plans/queue-16-portals-and-confined-applications.md#t045): application grants and recovery. | Daily-use acceptance follows the required working transfers and confinement proof. |
| t046 | [Portal integration](../plans/queue-16-portals-and-confined-applications.md#t046): narrow authorized transfers and their executors. | Deliver one scoped window/output capture through the same operator and agent interface. |
| t033 | [Role protection defaults](../plans/queue-13-authority-and-lifecycle-hardening.md#t033): protection for blind spatial/output roles and behavior without bwrap. | Related host-containment policy; do not confuse role isolation with application resource namespaces. |
| t113 | [Confined desktop services](../plans/1sxw3fyj-native-desktop-protocol-gaps-after-the-three-component-baseline.md#t113): per-service status and effect permissions. | Keep service access scoped instead of granting an unrestricted host bus to make confinement usable. |
| t060 | [Namespace pointer queries](../plans/queue-11-parallel-production-readiness.md#t060): installed menu-placement and drag acceptance. | Related namespace-correctness acceptance, separate from capture authorization. |

Recommended sequence after monitor recovery: reconcile t133 with production
admission, implement and qualify t142, deliver a bounded t046 capture slice,
then qualify the daily confined group under t045. The existing clipboard
transfer controls support t142; this sequence does not require completing all
of t046 before group isolation can be tested.

The baseline already includes namespace-keyed resource checks, registry
admission/revocation and clipboard controls. t141 completed the socket-directory
foundation; its launcher mount and trusted-path exclusion proof moved to t142.
The [cross-namespace root-readback leak](8xgoow54-a-root-readback-showed-one-namespace-anothers-windows.md)
was repaired and must stay covered when adding capture. t135's sandbox-backend
evaluation is recorded in completion history; replacing Bubblewrap is not a
prerequisite for this sequence. Task status remains in
[todo.md](../../../todo.md) and [completion history](../../../done.md).

The older t133 notes need source reconciliation before implementation. At
`8b8ac27b5`, `LiveXAdmissionPolicy::admit` checks the peer UID and admits to its
configured namespace before collecting ancestry for launch-origin metadata.
An ancestry lookup failure therefore is not itself the namespace-admission
failure described in parts of the old investigation. The pidfd proposal and
t275's evaluated design are proposals requiring validation, not security proof.

Before enabling capture, require an admitted requester and recipient, a bounded
target and payload, live generation/permission checks at execution, cancellation
and revocation, and refusal while locked, including pending transfers. Missing
executors must report unavailable rather than successful execution. These are
the [existing t046 exits](../plans/queue-16-portals-and-confined-applications.md#t046)
and [portal authority boundaries](../decisions/xa78u03g-keep-broker-and-portal-file-authority-and-custody-separate.md#portal),
not an ambient screenshot permission for every same-UID process.

## Validation and remaining work

### t313

Close XkbBell compatibility independently of capture. Decode the defined
request, validate its fields and implement its XKB semantics with an explicit
policy for audible output. Do not accept every unknown minor or suppress
protocol errors to make a screenshot command exit successfully.

Exit: generic wire controls for valid and malformed requests, both byte
orders and the selected bell/device behavior; an independent Xlib client
completes a bell plus GetImage sequence over known CPU pixels, with the same
pixels when the bell is suppressed. Record accelerated readback separately.
This fixes the reported error without claiming a composed capture path.

Task status lives in [todo.md](../../../todo.md). This investigation used source
and local documentation only: no live X request, input injection, screenshot,
build or test run. Capture acceptance still needs known-pixel checks on CPU and
accelerated clients, the selected output/window identity, and the portal's
denied, revoked and locked cases.

## Connections

- [Portal contract](../../namespaces-and-portals.md#portal-contract): scoped,
  generation-bound grants and separate screenshot/recording kinds.
- [WM freeze surface](../../wm-v1-freeze-surface.md#brokers-and-portals): capture
  is a separate authority and does not add pixels to blind WM policy.

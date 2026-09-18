---
id: k2d9l42p
date: 2026-09-17
kind: concept
status: draft
tags: [concept, shell, architecture, security]
---
# Native shell components compose through explicit scoped grants

**2026-09-18 status pointer:** the proposal below preserves its original scope.
Explicit bar/launcher/dock admission is now implemented; the
[capability map](../../native-desktop-capabilities.md) records its limits and
remaining acceptance. The original single-client description is historical,
not the current configuration contract. This note does not authorize additional
roles or automatically accept all of the proposed design.

## Intent and current boundary

Let users combine a bar, launcher, dock and other desktop components from
independent developers, or choose one integrated shell offering several features.
Neither arrangement should require a private protocol or a Lom coordinator.

This is a proposed direction, not an implemented multi-client capability or an
accepted wire amendment. Sophia currently admits one native shell client. That
client may combine content and descriptors. An ordinary X11 panel coexisting
with it is not evidence that two independently admitted native shells work.
The [current composition contract](../../desktop-composition.md) remains authoritative.

A native component is a separately admitted client with its own protection domain,
connection and resource lifetime, plus explicit grants for the functions it serves.
An integrated client can hold several grants; a modular setup distributes them
among independently selected clients. The same public contract serves both.
Splitting code into modules inside Lom alone does not establish this boundary.

## Capability examples

These describe needs, not new mandatory role names or approved capability bits.

| Component | Presentation and information | Authorized requests |
| --- | --- | --- |
| Bar | Output-scoped edge reservation and permitted workspace indicators | Activate an advertised workspace action |
| Launcher | Temporary overlay, authorized application catalog, scoped keyboard focus | Launch a catalog entry through the existing Session policy |
| Dock | Edge presentation and a bounded, permission-filtered application/window feed | Launch an entry or activate an authorized window action |
| Notification center | Admitted notification source and transient/persistent presentation | Invoke an authorized notification action or dismiss it |

The dock feed and notification service are not assumed implemented. They need
contracts owned by their respective authorities; a shell role does not create
those services or confer arbitrary window inspection. Existing follow-ups t043
and t046 retain their own scope rather than being silently included here.

## Authority stays in Sophia

- Operator configuration names the clients and permitted capabilities. Effective
  grants are the intersection of implementation support, client request and
  operator policy. Reaching an endpoint or claiming a role grants nothing.
- Roles are convenient permission presets, not unrestricted authority. A launcher
  does not obtain global keyboard capture; a dock does not obtain all metadata.
- Session admits and supervises clients; the existing owning authorities keep
  placement, reservations, input, application launch and WM policy decisions.
  The shell requests effects through those boundaries rather than executing WM
  policy or bypassing application admission.
- Metadata disclosure is capability- and subject-scoped. Action handles are bound
  to the receiving grant/publication and cannot be replayed by another component.
  Preserve the blind WM's separation from shell metadata.
- GPU execution remains an independent startup resource grant. Multiple components
  do not justify broad device/sysfs exposure or imply an aggregate VRAM quota.
  Sophia remains renderer-independent and uses stock Linux facilities.

## Composition, input and lifecycle

All components use the existing content/descriptor presentation machinery. Sophia
arbitrates edge reservations and transient placement; client names, connection
arrival order and highest requested z-order are not authority. An overlay cannot
cover secure UI or take input merely because it submits pixels.

Input belongs to the exact committed presentation and current grant. An admitted
launcher may receive a bounded, revocable focus lease for its active overlay.
Closing it restores a still-valid previous focus target or uses the documented
safe fallback. Pointer capture, outside dismissal, cross-output behavior and
stale-target suppression must work across component boundaries. Keyboard
shortcuts remain defined in the WM configuration; role routing only identifies
the admitted recipient of the resulting request.

Disconnect/replacement revokes that component's input, actions and active
reservations. Outstanding render consumers retain their storage until actual
retirement. Other components continue serving their own grants; they must not
reconnect merely because their neighbor failed. New admission gets a fresh
identity, and old replies, resource releases or completion witnesses cannot act
on a replacement. Replacement must not overlap exclusive ownership without an
explicit handover transaction.

Per-component bounds must be nested inside Session-wide byte, record, surface,
reservation, action and scheduling bounds. Adding clients cannot multiply an
unbounded allowance. Use the established control-credit/FIFO and retirement
contracts; one slow consumer must neither starve input nor erase another owner's
cleanup obligation. Admission failure must not partially grant a component.

## Developer and user experience

The protocol remains language-neutral, versioned and explicit about unsupported
capabilities. Rust helpers are conveniences, not a prerequisite. Provide a small
independent non-Rust client and conformance corpus rather than requiring another
developer to reverse-engineer Lom or copy its private state machine.

A user should be able to select an integrated shell or explicit component
executables and permissions. Optional shell packages may provide presets, but
preset syntax, reload behavior and fallback policy remain design work. No choice
between package-oriented and component-oriented UX was settled in this discussion;
the design must preserve both deployment shapes without making a package mandatory.

Steady-state performance should reuse output facts and retained UI state, wake
only affected consumers, and bound per-client records/bytes/work each turn. Shared
compositor scheduling should preserve unaffected displayed content. Do not add a
broker hop or duplicate renderer merely to label clients as components. Measure
multi-client latency and memory against the integrated setup with the same
workload; publish numeric budgets before acceptance, not after seeing results.

## First product and deferred extensions

Start with a persistent Lom bar plus a separate native launcher. Prove temporary
keyboard focus, input dismissal, independent crashes/restarts and per-output
placement while the bar continues operating. Also retain the integrated-shell
path and standalone Narthex compatibility.

A replaceable dock, notification center, wallpaper service and accessibility
integration are later consumers of the same admission model, with their own
metadata and interaction contracts. Lock screens, security prompts, screenshot
access and unrestricted global input are not ordinary shell-role permissions.

## Connections

- [Implementation and acceptance sequence](../plans/ptil1ejw-modular-native-shell-components-and-independent-launcher-critical-path.md) owns task criteria; todo.md owns status.
- [Shell lifecycle retrospective](t972gtpa-prove-the-shell-lifecycle-before-spending-operator-time.md) explains why joined production ownership tests must precede attended runs.
- [Content capability design](../decisions/6ndjwffd-content-capability-design-for-sophia_shell_v1.md) supplies the existing presentation/input/resource lifecycle.
- [GPU execution decision](../decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md) keeps rendering permission separate.
- [Application launcher](../../application-launcher.md) supplies the existing catalog and launch authority; modularity does not replace them.

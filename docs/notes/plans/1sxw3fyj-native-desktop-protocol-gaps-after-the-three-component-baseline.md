---
id: 1sxw3fyj
date: 2026-09-18
kind: plan
tags: [plan, shell, protocol, developer-experience]
---
# Native desktop protocol gaps after the three component baseline

The [audit](../investigations/vle7mt47-native-desktop-capability-audit-separates-contracts-from-client-ui.md)
and [developer map](../../native-desktop-capabilities.md) separate platform
contracts from UI choices. [todo.md](../../../todo.md) alone owns task state.
These candidates do not authorize implementation or new desktop applications.
A minimal independent probe is sufficient; change a reference UI only when
needed to prove a generic contract.

## t109

**Additional component admission and presentation.** Owner: Session config and
admission, Engine presentation/input. Depends on t104/t105/t106. Specify how a
non-catalog provider obtains bounded persistent or transient presentation
without impersonating a bar, launcher or dock. Reuse allocation, presentation,
input, reservation and retirement owners. Raising MAX_SHELL_COMPONENTS alone
or exposing arbitrary z-order/focus is not the solution.

Exit: capability/grant model and version/config migration; provider multiplicity,
quota sharing and placement arbitration; a minimal non-catalog provider beside
current clients; denied/foreign grant, reservation conflict, stale focus,
replacement, output loss and backpressure controls while another client
progresses. Secure UI stays outside ordinary roles. Background is t049, portal
payloads t046, general text t111. Native acceptance is separate.

## t110

**Idle observation and inhibition.** Owner: Session/input authority. Specify
bounded subscriptions/inhibitors with explicit grants, clock semantics, seat
scope, expiry, revocation, disconnect and suspend/resume. Expose idle transitions,
not raw input. Inhibition grants neither unlock nor power authority.

Exit: language-neutral contract, independent minimal client, deterministic
fake-clock controls for multiple subscribers, denied inhibitor, expiry,
backward clock, replacement and teardown. No idle daemon/settings UI required.
Lock remains t034.

## t111

**General native text and input methods.** Owner: Engine/input authority and
Session focus admission. Depends on t106. Specify text/edit/focus beyond the
launcher-only opening model: composition, commit/cancel, cursor/selection,
scoped input-method access. Preserve exact presented focus and consumed-input
barriers; launcher packets must not become arbitrary key capture.

Exit: negotiated contract and independent minimal text client; Unicode,
composition, stale edits, focus replacement, output loss, cancellation, bounded
text and backpressure controls. No production keyboard/IME UI, toolkit
dependency or unrestricted injection is included.

## t112

**Native accessibility boundary.** Owner: Session authorization and client
semantic disclosure/input owners. Specify opt-in semantics and permitted
actions, distinguishing standard accessibility adapters from Sophia wire.
Never infer semantics from raster pixels or imply screen/global-input access.

Exit: subject/recipient scope, redaction, bounds, current presentation, action
authority and revocation documented; independent producer/consumer probes for
denial, stale semantics/actions, replacement and bounded updates. No screen
reader product required. Text/IME interoperation uses its declared contract.

## t113

**Confined desktop-service integration.** Owner: Session/portal admission and
service-specific adapters. Specify access to existing audio, network,
power/battery and status-item/tray services. Inventory actual service APIs
before selecting adapters; do not invent a competing service protocol or
mount an unrestricted session bus. Separate status, effects, credentials and
privileged authentication.

Exit: per-service policy/effective evidence and failure semantics; minimal
probes with fake service peers cover denied calls, owner change, revocation,
bounded subscriptions and lost replies without effect replay. Distinguish
existing Session commands from new service grants. Portal prompts remain t046,
secure takeover t034. No settings/tray/authentication UI or power daemon is
required. Promotion must name the first bounded service slice.

## Ordering and shared acceptance

Close existing popout/lifecycle obligations before broadening their consumers.
t109 is the first new presentation foundation; notification/background provider
joins retain t046/t049. t110–t113 are independent contract candidates, not an
implied sequential desktop build. Do not duplicate these tasks in client repos.

Every promoted implementation needs bounded malformed/stale/denied tests,
cross-client failure isolation, exact release, documented wire and an independent
consumer where wire changes. Physical checks need a separate authorized workload
and exact source identity. This plan allocates no wire bits or message numbers.

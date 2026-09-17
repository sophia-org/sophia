---
id: ptil1ejw
date: 2026-09-17
kind: plan
tags: [plan, shell, architecture, native-components]
---
# Modular native shell components and independent launcher critical path

## Scope and exit

Support both an integrated shell and independently admitted native components,
starting with Lom's persistent bar plus a replaceable launcher. Use the same
public language-neutral contract, existing content/descriptor lifecycle and
Session-owned launch policy. No private Lom coordinator or display bridge.

The operator has approved implementation of the Bemenu/native-component path.
That is not evidence of current multi-client support. Task status and order live only in
[todo.md](../../../todo.md). The sequence below is its own critical path; it does
not replace the current Lom daily-driver path or authorize input-branch imports.

The exit is an independently implemented launcher operating beside the bar under
separate grants, with bounded cross-client scheduling/resources, correct focus
and dismissal, and isolated replacement/failure. Preserve the integrated client
and existing Narthex path. A clean tested implementation and an attended exact
release are separate milestones.

## t104

Implementation started with the [launcher contract proposal](../decisions/f64wqfh2-independent-native-launcher-admission-and-presented-input-contract.md)
and Bemenu CPU raster checkpoint `51fae2b` (strict C build, device-hidden
raster controls, Clang UBSan and three discriminating compiled negatives). The [ownership audit](../investigations/faon4eja-native-component-ownership-audit-and-reusable-c-wire-boundary.md)
and revision-1–6 C wire foundation are implemented. The
[revision-7 wire checkpoint](../investigations/v7m2c9ra-native-launcher-wire-contract.md)
adds explicit native allocation/candidate/focus/input/activation vocabulary with
independent Rust/C decoders; runtime negotiation still caps at revision 6. Model
controls and live ownership integration remain open; this is not a shipped
native-launcher capability.

**Decide the multi-component contract.**

Audit every single-client assumption in Session supervision, negotiation, content
stores, indicator publication, descriptor roles, application catalog delivery,
input capture, renderer accounting and endpoint selection. Turn the
[component concept](../concepts/k2d9l42p-native-shell-components-compose-through-explicit-scoped-grants.md)
into an ADR and versioned protocol/configuration proposal.

Settle and document before runtime implementation:

- Component/admission/grant identities, supported role multiplicity, exact output
  scope, revocation and replacement ordering; no first-client-wins ownership.
- Explicit operator selection and permission presets; rejection of conflicting
  exclusive providers; startup/refusal/reload/fallback behavior. Automatic fallback
  must not widen authority or silently launch an unselected component.
- Wire negotiation, endpoint/admission strategy, control request routing and
  action correlation across clients. Preserve the existing one-client profile as
  a compatibility case; do not assign packet numbers ad hoc in implementation.
- Edge-reservation arbitration and transient layering, keyboard focus leases,
  outside-click consumption and cross-component input transfer. Define anchoring
  only through authorized handles, never another client's private identity.
- Per-client and aggregate budgets, service fairness and explicit failure outcomes.
  Retiring storage remains charged after revocation; no fictitious GPU quota.

Model conflicting admission, stale completion, focus/capture transfer and revoked
cleanup. Define numeric bounds and measurable acceptance budgets here. Design
review is about the contract; it is not native evidence or permission to alter
unrelated WM/input work. Existing t022/t023/t039 protocol-family work remains
owned separately; reuse its compatibility and conformance results.

Exit: a decision-complete ADR, schema/config proposal, threat boundaries and
formal/fixture scenarios. Remaining product decisions are resolved explicitly.

## t105

**Admit and supervise independent component owners.**

The [shared-store and compositor-identity foundation](../investigations/r3b9n7cf-native-component-storage-and-compositor-identity.md)
is implemented. Session still owns a single live shell; this does not complete
t105 or enable a second client. Both epoch fields will be globally minted.

Typed `session.shell-component` selections now validate bounded independent IDs,
exclusive roles, paths and default-denied per-component GPU policy. Mixed legacy
selections refuse. The live startup parser explicitly refuses this unimplemented
admission mode before resolving a legacy fallback. This is configuration-boundary
progress, not two-client admission; the common live transport owner remains next.

The subsequent shared transport path now borrows one registry for real socket
resource/lifecycle service, preserves exact pre-launch reservations and custom
wire limits, and leaves legacy callers on a delegating compatibility wrapper.
Five private-socket controls and four compiled mutations join that path to actual
retained bytes. The [ownership record](../investigations/r3b9n7cf-native-component-storage-and-compositor-identity.md#shared-transport-checkpoint-over-bef4fd7a)
states the limits. Session still needs its bounded supervisor inventory, globally
minted attempt identities and asynchronous/fair negotiation/service before the
independent profile can be enabled. No native readiness follows from this seam.

Runtime now offers retained, nonblocking protected negotiation with bounded
visits, exact reservation cleanup and partial reply ownership. The blocking
compatibility API drives the same policy and state transitions. Session must
still join this API to the component supervisor and rotating service; the
independent profile remains refused. See the [negotiation record](../investigations/r3b9n7cf-native-component-storage-and-compositor-identity.md#retained-negotiation-over-a2c8e67c).

A prepared Session connection owner now holds both transports and the common
registry, burns global attempt identities, preserves exact successor/neighbor
ownership and rotates bounded negotiation visits. Content/action/indicator
helpers accept the same borrowed service view. This owner is not yet installed
in the live loop; supervisor and native-completion joins plus revision-7 role
semantics are still required. The [Session ownership record](../investigations/r3b9n7cf-native-component-storage-and-compositor-identity.md#session-connection-ownership-over-35153520)
distinguishes the private-socket controls from live admission.

Configuration checkpoint evidence: `.artifacts/bemenu-component-profile/`.
Device-hidden tests pass 118 config tests and 468 Session library tests, with
fourteen Session tests ignored. Four new config groups cover typed independent
selections, Session-only fragment disclosure, malformed/conflicting profiles and
legacy preservation. One new Session control exercises the real startup parser's
explicit refusal with nonexistent component executables and both legacy override
forms. It does not launch a protected child. Strict affected Clippy and the
repository layout gate pass. Two separately compiled mutations (duplicate role
admission and implicit GPU permission) fail their exact one-test controls.

The first local runner also attempted the `desktop_profile_probe` example without
its required arguments; its usage refusal is retained separately and is not test
coverage. The corrected runner selects only Cargo test executables. The raw
source-layout audit reports existing debt; the canonical layout subcommand checks
that ledger and passes. Neither result is a native or two-client acceptance claim.

Implement the accepted Session/config/transport design with one owned grant and
lifecycle per admitted client. Validate the complete operator profile before
launch. Enforce requested/supported/permitted capability intersection, exact peer
identity and independent GPU permission. Bound the client inventory and aggregate
credits before admitting any client.

Route subscriptions, requests, replies and revocation by exact client/grant;
never rely on a globally current shell pointer. Preserve all accepted work and
credit custody through backpressure and partial writes. Reconnect/replacement
must not reuse an old authority identity. No new rendering or allocation ledger
that competes with the existing resource owners.

Exit: private-socket production admission tests cover two clients, denied and
conflicting providers, cross-client spoof/replay, saturation, disconnect and
replacement. The surviving client's grant remains stable. Legacy single-shell
negotiation and intentional capability absence still pass.

## t106

**Compose placement and input across clients.**

Join independent component grants to the real reservation, presented-target and
native retirement owners. Implement the settled reservation/layering policy and
launcher focus lease. WM configuration continues owning shortcut bindings;
Session resolves the authorized component for the requested operation.

Test bar plus launcher on two logical outputs, unequal scale/negative origins,
outside dismissal, Escape/close, stale target and cross-output release, focus
restoration, active applications and topology change. Component removal withdraws
its interactive presentation and reservation without discarding render consumers
or another component's state. Preserve all-head timing/provenance distinctions.

Exit: actual shared production intake, projection, queues and retirement helpers
are exercised with simulated device completion, including held old consumers,
full queues, missing acknowledgements, revocation and a surviving peer. No test
may substitute an unrelated host-only policy implementation. Native completion
remains separately labelled.

## t107

**Deliver developer examples and a joined conformance gate.**

The public C foundation now includes atomic typed application-catalog assembly
(kinds 114–116) with an independent Rust-golden payload reader. Content/focus
lifecycle, protected independent admission and Bemenu integration remain open.

Provide a minimal independent non-Rust launcher client and optional Rust lifecycle
helpers using the same schema. Document admission, supported/denied capabilities,
focus acquisition/dismissal, catalog/launch outcomes, reconnect and resource
release. Supply actionable bounded diagnostics and explicit failure results.

Compose the production Session orchestration and real protocol clients in a
headless gate. Test integrated and split configurations, repeated two-output
interactions, a slow/crashed component, retained old resources, exact release,
quiet unaffected outputs and aggregate memory/credit bounds. Freeze repetition,
service budgets, latency definitions and mutation controls from t104; identify
all supplied policy/native facts. Never substitute queue admission for execution
or presentation, or suppress missing outcomes from latency samples.

Exit: independent implementation agreement, malformed/negative corpus, behavioral
mutations and an exact-source device-hidden canonical pass. The first launcher
may be a conformance example; this task does not silently schedule a full launcher
product redesign in Lom or Narthex.

## t108

**Accept the modular setup and document user selection.**

Provide explicit configuration examples for integrated shell and Lom bar plus
independent launcher, with selected provider identities and permissions. Preserve
a known working single-shell rollback. Any installation and hardware run remains
operator-attended and separately authorized.

On the exact installed release, verify both monitors, WM-bound shortcuts, launch
results, focus restoration, click dismissal, replacement/crash isolation, VT and
normal logout. Require stable surviving grants, correct reservation withdrawal,
resource/credit quiescence and exit 0. Compare integrated versus split workload
latency/memory against the budgets fixed before the run; report all rejected or
missing intended outcomes. Capture source, binaries, effective grants and timing
provenance. An unresolved retiring owner or unsupported platform is a refusal or
open acceptance item, never a clean result inferred from worker join alone.

Exit: retained attended acceptance, reproducible user configuration and developer
instructions, and current documentation updated to state the capabilities
actually shipped. Completion does not imply dock/notification/lock support.

## Native launcher store progress

The [wire/store record](../investigations/v7m2c9ra-native-launcher-wire-contract.md#native-store-ownership-over-795a3d60)
now records actual native allocation and candidate ownership, ordered catalog-row
binding through renderer handoff, aggregate byte/response accounting and retained
resource isolation from the bar. This is storage preparation only: revision-7
live admission, two supervised services, presented focus/launch authority and the
Bemenu backend remain on the critical path. The ten store controls use supplied
renderer transitions and do not qualify a physical run.

## Dependencies and boundaries

Sequence: t104 → t105 → t106 → t107 → t108. t106 also depends on existing anchored
content and lifecycle work t099/t100. t108 depends on current baseline daily-driver
acceptance t081. Concept/design work may proceed without delaying that baseline.

The content design, application-launch catalog and WM-owned keybindings are
reused; metadata disclosure or additional service families require their own
contracts. Existing t043/t046 own broader status/desktop-operation and portal
work. New dock/notification tasks should be filed only against concrete consumers,
not preallocated as part of this first two-component implementation.

## Connections

- [Component architecture concept](../concepts/k2d9l42p-native-shell-components-compose-through-explicit-scoped-grants.md).
- [Pinned integration retrospective](../concepts/t972gtpa-prove-the-shell-lifecycle-before-spending-operator-time.md).
- [Current Lom critical path](1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md).
- [Native WM and shell product](queue-14-native-wm-and-shell-product.md).
- [Current desktop composition](../../desktop-composition.md) and [launcher](../../application-launcher.md).

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
independent Rust/C decoders. The isolated component owner now negotiates revision 7
for explicitly reserved native launchers; legacy/live startup remains revision 6
and still refuses the independent profile. Model
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
(kinds 114–116) and typed native-launcher records (six inbound kinds and five
outbound encoders), plus immutable resource transfer/reply codecs (kinds 165–171),
checked against independent Rust golden bytes. Native input
text is borrowed; encoding/decoding grants no lifecycle authority. Content/focus
lifecycle and live independent admission/Bemenu integration remain open; isolated
protected component supervision has separate headless coverage.

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
resource isolation from the bar. The component owner now selects the native
storage profile from the configured role and negotiates revision 7 on its private
socket. Native resource, pacing, allocation and candidate records share one
bounded visit and the existing response FIFO. Live admission and two supervised
services, presented focus/launch authority and the Bemenu backend remain on the
critical path. The ten store controls use supplied
renderer transitions and do not qualify a physical run.

The next transport slice retains actual candidate Presented metadata for exact
focus installation, bounded semantic input/ACK receipts and a pending Enter tied
to its issued revision. Closing and timeout disarm that exact opening while
keeping notification credits in the same FIFO. These are private socket controls
with supplied renderer completions; physical keyboard/capture routing, idle
deadline visits in Session, application admission and Bemenu remain to be joined.
See the [focus ownership record](../investigations/v7m2c9ra-native-launcher-wire-contract.md#native-focus-and-input-over-6c65ccad).

The [activation ownership slice](../investigations/v7m2c9ra-native-launcher-wire-contract.md#native-activation-queue-ownership-over-c274f7ad) now joins private request intake to the actual Session launch queue, with retained exact response credit and independent ACK state. Worker verification/execution, dual protected supervision, live placement/input and the Bemenu client remain required before `lom-test` readiness.

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

### Presentation lookup must name the component grant

The backend presentation query now requires the exact ContentGrant as well as
output and candidate generation, and Session passes the pending obligation's
grant. Equal candidate numbers from different connection/content epochs cannot
borrow another component's presentation result. The actual intake/lowerer/owned
queue fixture checks each changed epoch independently after simulated completion.
All 34 device-hidden lifecycle controls pass at
`.artifacts/bemenu-presentation-grant-final`; the Session native-feature compile
check passes. The first adapted reconnect fixture incorrectly queried the old
grant after presenting its successor; its retained failure led to correcting the
query to the successor identity, without weakening the expectation.

This is a prerequisite, not multi-component composition completion. Runtime still
stores one shell frame and one presented input binding per output. Those owners
must support simultaneous grant-keyed panel and native-launcher content with
explicit ordering, independent retirement and capture, before the live Session
join can be enabled. Protected component supervision exists separately; the
legacy live owner still owns its own compatibility registry. Do not bypass that
join by starting a second independent global budget. No hardware, native display,
profile installation, M3 import or physical-run readiness is established here.

### Component composition and presented input over 78596093

The runtime now retains separate output/layer owners for the panel and native
launcher. Session chooses the layer; client epoch or target numbers do not select
stacking order. A grant cannot occupy both layers on one output. Queue refusal
restores only the attempted layer and its exact retirement claim, preserving the
other component's actual pixel source. Presentation lookup remains grant-specific.

Presented input is now a back-to-front list built from the actual presented image
identities. A prepared launcher is absent from that list. Each binding names its
grant even when it has no actionable targets; old displayed pixels retain their
old transform with authority revoked if the current owner no longer matches.
Unknown/stale pixels consume input conservatively rather than exposing a target
behind them. Adding another component preserves an unchanged binding's
presentation epoch and target continuity. Session routes once through the shared
Engine stack reducer, which selects the visible topmost binding and retains the
original capture identity through release/cancellation. Reused target and
allocation IDs from another grant cannot inherit it.

Device-hidden evidence: 152 backend library tests pass, including actual two-role
intake, Engine lowering and queued ownership with simulated native completion.
The expanded coexistence control checks both presented grants, unchanged panel
continuity, no prepared launcher input, topmost launcher activation and independent
rollback. Eleven existing Engine capture controls and seven new stack controls
pass. Reversing stack traversal in an isolated compiled mutant fails the exact
expected-grant assertion; original source is restored. Evidence is retained in
`.artifacts/bemenu-component-coexistence-final`,
`.artifacts/bemenu-component-input-stack-final` and
`.artifacts/bemenu-component-stack-mutation`.

This is shared composition/input plumbing, not live launcher readiness. The live
Session still needs the common component connection/registry owner wired into
panel service, native opening/catalog/focus/activation service, exact component
close/removal and process cleanup. The dedicated Bemenu executable and its private
protocol tests do not substitute for that join. The `lom-test` profile/harness and
fresh exact-source contained canonical gate remain required before an attended
run. No installation, hardware/display access or M3 import was performed.

The affected Session library also passes 468 controls with 14 ignored in the
private namespace (`.artifacts/bemenu-component-session-exact`); strict affected
Engine/backend/Session lib-and-tests Clippy and the layout-only gate pass. The
first Session runner omitted its compile-time fixture path and produced nine
missing-file failures. The corrected runner provides a private symlink to the
captured source, not a host source mount. Nested protection still reports an
absent private loader cache in these logs; no successful protected child launch
or native process admission is inferred from this library result.

### Borrowed panel service over 01fe9ecd

`PanelComponentService` retains per-attempt panel state without a socket,
process supervisor or separate content registry. Its content/presentation,
indicator publication and action service delegate to the same implementations
used by the live legacy owner. It requires the exact borrowed connection grant,
refuses native-launcher-role connections and cannot upgrade a negotiated grant
to discrete input. The actual dual-connection registry fixture drives indicator
publication through this service, checks exact received frames and unchanged
snapshot suppression, and verifies that neither the neighbor nor a replacement
attempt can be serviced with stale panel state. Protection evidence in this
fixture is supplied; no protected process is launched.

Integration also exposed a targetless-content gap: `project_render_bundle` built
occlusion allocations only while visiting actionable targets. A rendered empty
launcher result could therefore have pixels but no input occlusion. The retained
baseline control fails on the actual resource/candidate-store bundle with zero
targets (`.artifacts/bemenu-empty-target-baseline`). Projection now derives those
rectangles from rendered placements independently of targets. This fixes shared
surface geometry, not a special Bemenu pointer rule.

The shared-service and projection evidence is in
`.artifacts/bemenu-borrowed-panel-final`; native startup/focus/removal and live
Session scheduling remain to be connected. The explicit component profile stays
refused until that join and cleanup are complete. No physical-run readiness,
installation, native display activity or M3 integration follows from these
service controls.

Final scoped gates: 468 Session library tests pass (14 ignored), all six
component-connection controls pass, strict runtime/Session lib-and-tests Clippy
and layout pass. The targetless projection control previously failed with an
empty allocation list; the repaired real bundle retains its exact logical and
pixel rectangle. Nested-loader-cache diagnostic scope remains as recorded above;
these are device-hidden service/geometry controls, not protected-child proof.

### Native content service over ae8a6765

`NativeLauncherContentService` borrows the common component registry connection
and binds its state to that exact grant. It requires the negotiated native role,
uses the transport's current opening/revision and checks the supplied catalog
identity. The bounded native transport visit services the existing resource,
allocation, demand and candidate stores. Session resolves parentless role-3
allocations within the selected output, preserving opening provenance and scale,
with zero reservation. Edge placement centers along the other axis; margins and
oversized geometry refuse rather than escape the output. Resize generations are
checked instead of saturating.

The panel and native service now call one `submit_bundle` implementation for
actual bundle projection, runtime admission, Prepared and retained presentation
obligations. Session selects the trusted Shell versus Launcher layer. Actual
presentation still comes from the exact grant/output/candidate runtime query;
no focus is inferred from allocation, candidate assembly or Prepared.

Evidence `.artifacts/bemenu-native-service-final`: 471 Session library tests pass
with 14 ignored; seven component-connection tests pass, including a real local
socket native allocation through this service, exact reply geometry, absent
focus before presentation, and refusal of the replaced grant. Three pure
placement controls cover edges/scale, malformed identity/geometry without ID
minting and resize exhaustion. Strict Session lib/tests Clippy passes; the
layout-only check passes after placing controls in the existing content test
module. The initial layout failure is retained separately; no checker or debt
exception was changed. The nested private loader-cache warning retains the
previous qualification: supplied protection evidence is not a protected child.

This remains a component service checkpoint. The new socket control sends no
pixel candidate and does not execute a native frame or application. Close must
still remove only the exact opening's content, wait for actual source consumers,
and dispose its allocations before reopening. Live Session component process,
catalog/focus/keyboard/action service and shutdown orchestration, the `lom-test`
profile and fresh exact-source canonical validation remain required. Explicit
component configuration remains refused; no physical readiness, deployment,
GPU/display/VT activity or M3 integration is claimed.

### Exact component removal over f34c878c

The backend removal entry names output, trusted layer, grant and candidate. A
stale close refuses before changing the successor. An exact close immediately
revokes interaction in both retained admission and current presented bindings;
old pixels remain occluding. A candidate still awaiting its first presentation
keeps its existing obligation and returns pending. No missing snapshot is
interpreted as successful removal.

Once the exact candidate has presented, removal offers a fresh retained
composition without that component. Returned queue refusal restores its actual
source owner, keeps interaction revoked, and leaves the neighboring panel
unchanged. Another same-grant candidate cannot rearm that closing admission.
The caller retains an exact removal receipt and observes a newer presented
projection with no pixels/binding from the grant; enqueue alone does not settle
it. This receipt establishes displayed absence only. It says nothing about held
pixel consumers, worker/device cleanup or ResourceReleased.

`.artifacts/bemenu-component-removal-checkpoint` records 153 backend library
tests passing in the device-hidden namespace, strict backend lib/tests Clippy
and layout. The new actual-intake/Engine/queue control uses simulated completion:
close before first presentation waits; revoked pixels consume clicks; queue
refusal retains both actual component sources; retry preserves the bar's exact
presentation; a receipt remains pending until replacement completion; an old
close cannot revoke the successor. The control holds a real source lease after
receipt completion to keep that evidence distinct from resource release.
The compiled interaction-revocation mutant fails behaviorally and its isolated
source is restored (`.artifacts/bemenu-component-removal-mutant`); it was run
before the additional closing-admission guard. Initial fixture failure from a
missing allocation rectangle and the later compile correction remain retained,
not counted as positive gate evidence.

Session still must own/retry that removal receipt, cancel unsubmitted opening
work, retain pending candidate responses and settle allocations/resources before
reopening. This backend boundary is not the completed close protocol or live
component owner. The full launcher integration, fresh canonical gate and
`lom-test` readiness remain open. No KMS/GPU/display action, install, push or M3
import occurred.

### Native close cancels unsubmitted protocol work over 1e85cce4

The actual transport close FIFO path now settles pending allocation proposals
and unsubmitted candidate-store work before queuing Closed. Pending allocation
requests receive their reserved refusal; standing demands/unused permits receive
Cancelled permits; incomplete assemblies and accepted-but-unsubmitted candidates
receive Cancelled outcomes. Each response transfers the existing store credit;
close does not fabricate spare output capacity or discard an owed response.
The exact current transport opening is validated before this path, and stored
candidate opening/catalog/output provenance is checked before candidate mutation.

Submitted candidates remain non-cancellable. Their actual leases and remaining
Prepared/Presented obligations survive close. Active allocations also remain
owned: Session still must observe pixel removal before disposing them. Input is
already disarmed by the retained closing state; a late Presented cannot recreate
focus after Closed. The close/deadline/input APIs now borrow the registry mutably
because their deadline path can settle these store obligations; affected fixtures
were updated without changing their assertions.

Device-hidden evidence in `.artifacts/bemenu-native-close-final`: 14 runtime
library controls, 26 native transport controls and 10 native content controls
pass. New private-socket controls cover standing demand, unused permit,
incomplete assembly, pending and submitted candidate, pending allocation,
wrong-opening refusal without accounting change, no duplicate replies, and
late exact Prepared/Presented with a retained real pixel lease. These tests
supply protection and renderer completion; they are not protected-process,
Session owner-loop or native display execution. Runtime/Session strict
lib-and-tests Clippy and layout pass.

The compiled cancellation-omission mutant fails the exact new socket control:
it receives Closed where the permit cancellation must precede it. That proves
the permit branch is discriminated, not an independent mutation of every later
phase. The disposable source was restored; evidence is in
`.artifacts/bemenu-native-close-mutant`. Initial borrow-signature compilation
errors are retained separately and are not test failures of the final source.

Close/reopen is still incomplete at the live Session boundary: it must retain
and service the removal receipt, drain late input/resource records, settle active
allocations and consumers, and prevent a fresh opening until those obligations
permit it. Component process/catalog/focus/launch/shutdown integration and the
final exact-source canonical plus `lom-test` harness remain required. No live
endpoint, device, VT, installation, push or M3 import occurred.

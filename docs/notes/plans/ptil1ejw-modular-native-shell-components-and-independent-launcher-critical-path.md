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

### Permit reply correlation over b08cee0b

Following the close/reopen path exposed an actual Session/Bemenu integration
mismatch. Bemenu requires the permit reply to echo its demand transaction, but
both Session content services discarded that transaction and minted an unrelated
server transaction for the grant. Panel and native services now pass the exact
owned demand transaction to `grant_content_demand`. Server-originated output
publications retain their own checked serial source; answering a demand does not
consume that counter.

The actual shared-registry/private-socket native service fixture sends demand
transaction 913, deliberately distinct from the server counter, and asserts both
the reply transaction and an unchanged server counter. Device-hidden Session
471 PASS/14 ignored and component-connection 7 PASS, strict Session Clippy and
layout are retained in `.artifacts/bemenu-native-permit-transaction`. The compiled
old-behavior mutant fails the reply identity assertion and its isolated source
was restored (`.artifacts/bemenu-native-permit-mutant`). This is allocation/demand
service evidence without a native frame or protected child, not physical
readiness or a general wire revision change.

The same inspection found Bemenu's permit receiver currently accepts only grant
and timeout states, not the Cancelled state emitted by normal close. That client
repair and handling of late in-flight Begin/chunk/end records remain required
with the Session closing/removal owner. They are not silently classified as
malicious traffic or covered by the current passing server controls. Live join,
reopening, fresh canonical and `lom-test` readiness remain open.

### Bemenu cancellation counterpart

Signed Bemenu `69cc9b2` accepts exact standing/granted permit cancellation and
retains an already-sent candidate until its own outcome. Duplicate/wrong permit
IDs refuse. The actual upload state remains Resident before late candidate
rejection and ReleasePending afterward; no local cancellation fabricates byte
release. Device-hidden full `make check-sophia EXTRA_WARNINGS=-Werror` passes in
`.artifacts/bemenu-cancelled-permit-final`; restoring the prior receiver with the
new controls fails behaviorally on kind 177 in `bemenu-cancelled-permit-baseline`.
This closes the client receiver mismatch recorded above, not the server's late
record drain or live Session close/reopen orchestration. Both repositories remain
uninstalled; this checkpoint is local and unpushed. Physical readiness is still
pending the remaining live join, cleanup and exact canonical/harness gates.

### Bounded late-content drain over 9f46b1a5

The transport retains the exact last closed opening and exposes a bounded
content drain only while no new opening is active. It shares the actual native
record decoder, inbox, stores, aggregate output budget and resource transition.
The visit is limited to 32 records/the negotiated lower limit and 64 KiB payload;
possible terminal credit is checked before removing a request. It cannot grant
an allocation, permit, candidate, focus or launch.

A Begin which crossed close in flight receives one Cancelled candidate outcome.
The real store's monotonically consumed generation suppresses duplicate Begin
terminals; known chunk/end tails consume no new obligation. Unknown future tails
refuse. Late exact-opening allocation requests get Stale refusals and new demands
get Cancelled permits. Existing submitted candidates and live allocations remain
owned. Immutable resource transfers/retirement keep using their existing owner
and credit rules, allowing cleanup to progress after Closed.

`.artifacts/bemenu-late-content-final`: 14 runtime library, 27 native transport
and 10 native content controls pass device-hidden; runtime/Session strict
lib/tests Clippy and layout pass. The new socket control sends Begin/chunk/end
before close but defers intake until afterward, checks one exact terminal and
no candidate admission, repeats the records without duplicate response, then
checks late allocation/demand refusal. A real held resource lease keeps retiring
bytes at eight and withholds Released until its actual drop; the allocation is
still deliberately retained. Supplied protection and completion remain fixtures,
not protected-child or native display evidence.

The compiled duplicate-guard mutant produces an extra terminal and fails the
no-response assertion (`.artifacts/bemenu-late-content-mutant`); its disposable
source is restored. No kernel backpressure or general protocol fairness proof is
claimed. Input ACK/activation draining is separate, and this API intentionally
refuses while another opening is active: Session must join the exact close,
removal, resource/allocation settlement and reopen boundary, rather than treating
a temporarily empty inbox as peer acknowledgement. Those live owner duties,
component startup/shutdown and the final canonical/`lom-test` gate remain open.
No device, display, VT, install, push or M3 import occurred.

### Closed input visits over 52a4a732

The closed-input visit reuses the production ACK decoder and typed activation
request/response owner. It polls I/O once with the existing 64 KiB bound and
services at most 32/the negotiated lower number of records, choosing ACK or
activation in their relative FIFO order. Exact closed-opening validation prevents
using it on an active successor. Valid late ACKs are stale; newly handed late
activations receive an owned Stale outcome with their original transaction and
complete identity. No launch callback runs in this path.

An activation previously handed to Session is not reclassified or guessed.
Its owner must finish the actual decision; an already recorded Admitted outcome
remains Admitted. Zero serviced records therefore does not establish complete
close quiescence. Peer EOF reports NotConnected without disposing the registry
grant. The shared `native_launcher_closed_opening` observation means only that
Closed transferred to the FIFO, not peer receipt, pixel absence or resource
settlement.

Device-hidden `.artifacts/bemenu-closed-input-checkpoint`: 14 runtime library,
30 native transport and 10 native content tests pass; runtime/Session strict
lib/tests Clippy and layout pass. New actual-socket controls cover both ACK/
activation orders, exact Stale response, retained unknown outcome followed by
its caller-supplied Admitted response, duplicate finish refusal, and remote EOF.
That supplied decision proves response custody, not application execution.
The compiled Stale-to-Admitted reply mutant fails the expected outcome assertion
and its disposable source is restored (`.artifacts/bemenu-closed-input-mutant`).
This is a false-reply control, not a reproduced launch after close.

Session still must combine these visits with the retained presentation/removal
receipt, allocation invalidation, resource consumption and reopen gating. The
live component process/catalog/keyboard/launch/shutdown join, exact canonical and
`lom-test` readiness remain open. No display/device/VT run, installation, push or
M3 import occurred.

### Session retains the exact native close through pixel removal (2026-09-18)

`NativeLauncherContentService` now retains the serviced opening, last submitted
candidate, close transaction/reason and backend removal receipt across visits.
Close is recorded before fallible transport work; contradictory retries refuse.
Closing service drains the existing bounded late-content/input paths, observes
real pending presentation, and uses the existing exact backend removal API.
A receipt remains owned until actual replacement presentation confirms absence.
Neither a true pixel-absence result nor an empty drain permits reopening or
releases allocations. This tranche intentionally retains the close afterward;
allocation invalidation, actual resource settlement and live owner integration
remain required next work, not completed lifecycle claims.

Evidence: `.artifacts/bemenu-session-close-owner/` records device-hidden Session
471 PASS / 14 ignored and seven component-connection tests PASS, strict Session
Clippy and layout. The extended private-socket control refuses wrong opening and
changed close transaction, repeats the no-submitted-pixels transition, retains
the actual active allocation and grant, and refuses open service afterward.
That control does not execute the submitted native removal branch, a protected
child, GPU/KMS, or live close/reopen. Existing backend removal evidence retains
its separate scope. No current canonical, publication or physical readiness is
claimed by this checkpoint.

### Close resource settlement inspects actual owners (2026-09-18)

Session can now invalidate the exact closing opening's allocations after its
pixel-removal predicate succeeds. It validates the full allocation set before
mutation, leaves queued invalidation responses in their existing store/FIFO,
and retries only allocations still active. Saturation preserves the close.
`closed_native_owners_settled` inspects actual resource/candidate/allocation
owners and response credits, native response/input obligations, buffered input
and the partial-write FIFO. High-water identities remain retained. It neither
resets the grant nor manufactures ResourceReleased.

A locally settled result is deliberately NOT a peer-close barrier: old records
may still be in the peer outbox or socket. Session retains the closing state and
still refuses open service. Safe old-opening dispatch during same-connection
reopening remains required before enabling that transition. Live owner wiring,
composed submitted-removal coverage, canonical validation and lom-test readiness
remain open.

`.artifacts/bemenu-close-settlement/`: device-hidden runtime 14 + native transport
30 + native content 10 + Session 471 + component connections 7 PASS, 14 Session
ignored; strict Runtime/Session Clippy and layout PASS. The actual private-peer
resource control invalidates allocations, retains a real pixel lease, observes
retiring bytes and false settlement, then drops the lease and consumes the real
Released response before settlement becomes true. Session repeats invalidation
service with one transaction allocation total and still refuses open service.
The disposable compiled resource-check omission fails that held-owner assertion;
source restoration and logs are in `.artifacts/bemenu-close-settlement-mutant/`.
These tests do not execute native rendering, a supervised child or hardware.

### Same-connection successor with late closed-opening records (2026-09-18)

The live native parser now retains the last closed opening as refusal provenance.
Old allocation requests and Begin records use the existing high-water/terminal
owners; consumed tails cannot enter the new assembly. A tail of the actual
current assembly still takes its ordinary path. Late demand for an absent old
allocation is cancelled without replacing the successor's demand/permit; a
cancel tries exact current ownership first, with only known consumed identities
accepted as stale afterward. Refusals use the same bounded record/byte visit
and reserved aggregate response capacity. Resources continue through the real
connection-scoped store, not an opening-local fake inventory.

Session `reopen` waits for exact pixel absence, no pending presentation, and the
actual local-settlement predicate before publishing the successor. Publication
refusal retains the old close; successful FIFO transfer clears only the obsolete
presentation/close state and preserves connection-wide counters and facts.
Late input continues to use the existing exact binding/activation validation.
The retained tombstone is refusal authority, not permission to accept arbitrary
older identities or assume the peer stopped sending. Unknown/future malformed
records still refuse. Live Session orchestration remains unjoined.

`.artifacts/bemenu-native-reopening/`: device-hidden 533 PASS / 14 Session ignored,
strict Runtime/Session Clippy and layout PASS. A real private-socket control owns
a successor permit before receiving the old allocation, demand, Begin, chunk,
End and cancellation. It verifies exact refusals, intact successor permit, then
assembles/submits the successor and supplies Prepared/Presented to finish the
fixture. An initial fixture omitted those terminal completions and correctly
retained its submitted epoch; the corrected final run is `tests-final.log`.
Session's no-pixel fixture refuses reopening while an allocation remains, then
reopens after settlement and refuses the old close against its successor.
The compiled mutation bypassing only the old-Begin refusal fails with Stale;
restored-source evidence is in `.artifacts/bemenu-native-reopening-mutant/`.
These are component/socket controls, not protected-process/native close/reopen,
GPU/KMS, full canonical or physical acceptance. No publication or installation.

### Shared protected component launch construction (2026-09-18)

The legacy live shell and new `ShellComponentLaunch` use the same production
base-launch constructor: `--serve`, private read-only endpoint/config bindings,
explicit socket/config environment and process group. The independent plan
accepts an operator selection, requires an absolute executable and positive bar
allowance, omits the allowance for the application launcher, and applies the
existing exact per-attempt GPU policy after the real connection owner reserves
its nonzero grant. Direct mode without an admitted device refuses; denied mode
adds no device. GPU preparation/evidence is reused, not replaced by a Vulkan or
launcher-specific admission path.

This is construction for the live join, not an enabled component session. The
existing configuration refusal stays in place; selected component service,
catalog/input routing and final shutdown/error-carrier wiring still must be
connected before it can be lifted.

`.artifacts/bemenu-component-launch/`: device-hidden Session 471 PASS/14 ignored
and process-owner 3 PASS/2 ignored, strict Session Clippy/layout PASS. The new
controls reserve through the real process owner, inspect both actual launch
plans and deliberately refuse before spawn. They assert exact separate endpoint
and config bindings, read-only paths, MetadataShell-only role, no display env or
devices in denied mode, bar-only allowance, missing GPU admission refusal and
zero-grant refusal. Ignored protected-child controls were not run or relabelled.
No protected process, GPU device, native display, installation, canonical or
physical readiness evidence is produced by this construction checkpoint.

### Joined Session component owner, before owner-loop enablement (2026-09-18)

`ShellComponentSession` now owns selected launch plans, the shared process and
connection registry, exact negotiated role-service state, and the fixed revoked
grant inventory. It starts paused, takes explicit operator content/input policy,
refuses replacement while an attempt/process or cleanup is retained, and attaches
bar/native services only from actual successful role negotiation. Every service
borrow rechecks the exact connected attempt and presentation permission. Stop
records the exact grant before IPC revocation/process signaling; poll continues
reaping. Missing runtime retains claim cleanup. Shutdown is irreversible, and
final backend transfer refuses while processes or grant cleanup remain.

The joined owner preserves last presented panel bands while a successor has not
presented; a new connection is not itself a work-area update. This is source
integration with the existing borrowed services, not a new reservation policy.
The actual compositor owner loop still does not construct/use this owner. Its
catalog/input scheduling, per-component diagnostics, final error-carrier custody
and live shutdown must be wired before the configuration refusal is removed.
Successful supervised negotiation/role attachment and native close/reopen are
still integration gates, not inferred from the controls below.

`.artifacts/bemenu-component-session/`: device-hidden Session 471 PASS/14 ignored
and process-owner 4 PASS/2 ignored, strict Session Clippy/layout PASS. The new
control deliberately uses a nonexistent executable: initial pause burns no
attempt; failed startup retains supervisor until poll; missing runtime preserves
cleanup and attempt identity; actual runtime claim settlement permits a fresh
attempt; old stop cannot name its successor; shutdown cannot reopen; final
backend payload is retained on refusal and dropped once on settled transfer.
No actual native owner/worker is supplied. The initial fixture omitted the
Session-private endpoint parent and failed before startup; the final fixture
creates it as required. `tests-final.log` is the final positive evidence.
A compiled omission of only the pending-revocation start guard mints a successor
prematurely and fails the exact-attempt assertion; source restored, logs in
`.artifacts/bemenu-component-session-mutant/`. No protected successful child,
hardware, native run, canonical result, publication or readiness claim.

### Actual Session lifetime wiring before scheduling enablement (2026-09-18)

The production Session now prepares an optional selected component owner outside
`run_session_loop`, lends it through `SessionLoopResources`, and retains it and
its private endpoint parent in the actual `RetirementFailure` carrier. The
existing configuration refusal remains, so this does not enable component
startup. The common presentation-pause macro disarms components and settles
exact revoked claims; the real completion path requests shutdown before native
draining. The outer cleanup independently covers early loop errors, polls the
existing process supervisors within a terminal three-second deadline, and keeps
owners on timeout/error. This wait is terminal cleanup, never seat acknowledgement.

`component_lifecycle::finish` refuses disposal without native disposition. On
successful native/CPU/handoff disposal it collects actual component accounting;
an earlier loop error still retains the owner in the error carrier. Clean
success drops endpoints before removing their private parent. Terminal carrier
field order puts backend/CPU/handoff/error consumers before protocol accounting.
Per-role live scheduling, GPU/connection diagnostics, catalog and input routing
still precede removing the config refusal and the physical harness gate.

`.artifacts/bemenu-live-owner-lifetime/`: final device-hidden Session 473 PASS/14
ignored plus process-owner 4 PASS/2 ignored; strict Session Clippy, layout and
workspace formatting PASS. Shared outer-helper controls use an actual component
owner with failed nonexistent-binary startup and a headless runtime, plus supplied
native-disposition flags. They retain endpoint/owner on unresolved disposition
or prior error, and remove them on clean success. A second control passes the
actual endpoint owner through the real terminal error carrier. Neither induces
an owner-loop seat failure or supplies actual KMS/worker custody. Call-site and
drop-order claims are source integration, not physical evidence. The compiled
native-disposition guard omission fails owner retention; restored artifact is
`.artifacts/bemenu-live-owner-lifetime-mutant/` (before the final tuple-order-only
change; helper/control bytes unchanged). Formatting also normalizes the small
module/export ordering drift from earlier local component checkpoints.
No canonical/native run, installation, publication, or lom-test readiness claim.

### Bounded live component scheduling and bar service (2026-09-18)

The physical owner phase now services negotiated independent bars through their
borrowed production content, indicator, ACK and WM admission helpers, incorporates
presented work-area reservations, and issues pointer actions only to the exact
connected grant. WM completion errors retain their fatal/non-replay distinction;
ordinary peer service errors stop that exact attempt. Negotiation reconciliation
scans actual connected slots so an earlier role failure cannot lose a neighbor's
one-time successful negotiation event.

Launch selection attempts at most one ready role per visit, alternates selection,
and reserves a per-slot one-second retry delay before attempting startup. Paused,
shutdown, retained cleanup and process custody still block admission. The native
launcher role deliberately remains unready until catalog/execution and native
input are joined; the configuration refusal remains. This is live bar call-site
wiring, not a successful dual-component session or physical readiness claim.

`.artifacts/bemenu-live-component-scheduling/`: device-hidden Session 473 PASS/14
ignored, process controls 5 PASS/2 ignored, connection controls 7 PASS; strict
Session Clippy, layout and workspace formatting PASS. The scheduler control uses
actual failed executable attempts, reaping and exact runtime-claim settlement;
it proves role skipping, delayed retries and irreversible shutdown, not protected
successful startup. A compiled removal of only the retry-deadline exclusion
fails the same-instant retry assertion; restored disposable source and results
are in `.artifacts/bemenu-live-component-scheduling-mutant/`. No GPU/device,
native run, canonical gate, publication or installation was performed.

### Outer catalog worker and shared launch-queue custody (2026-09-18)

The shared Session launch queue now lives outside the owner loop and is borrowed
by its existing consumers. A new outer-owned component catalog service begins
its initial scan only for an explicitly selected native launcher while shell
presentation is available. It retains the actual worker before submitting the
bounded scan and stores the resulting immutable source snapshot; no connected
native grant is supplied in this initial scan phase. It therefore publishes no
catalog to a peer and authorizes no execution. A five-second initial-scan timeout
returns through the outer owner path. The existing component configuration and
native startup guards remain in place.

Terminal cleanup drains the real catalog worker without a display/process
execution environment. It rejects exact outstanding native verification before
joining, consumes at most one worker result per visit, and retains the service
on timeout/error. The outer Session error carrier owns both the catalog service
and the shared queue, including errors returned before ordinary completion.
This uses the existing worker and does not add a pool. The bounded terminal wait
is not used on seat acknowledgement. Existing queue call sites only change from
borrowing a local value to reborrowing the outer value.

`.artifacts/bemenu-live-catalog-owner/`: final device-hidden Session 474 PASS/14
ignored, native execution controls 4 PASS, catalog controls 4 PASS; strict Session
Clippy, layout and workspace formatting PASS. The live-helper fixture directly
prepares the internal selected profile (the public guard remains closed), scans
an empty catalog on the real worker, and proves successful bounded shutdown
cannot restart it. The two new terminal controls drain a real scan and an exact
socket-admitted verification with no execution environment. Existing execution
controls still run their private short-lived `/bin/true` child, not a GUI.
A compiled omission of shutdown result draining fails the bounded join control;
restored source and logs are in `.artifacts/bemenu-live-catalog-owner-mutant/`.
Initial compile/type and strict-reborrow failures are retained separately from
the final gates. Outer error-path placement is source integration, not an induced
native failure. Catalog FIFO publication, open/focus/input, connected execution,
close/reopen and the full physical harness remain unfinished. No hardware,
native run, canonical gate, installation or publication claim.

### Connected native catalog publication (2026-09-18)

The independent role scheduler can now attempt the native process once the
outer-owned source catalog is ready. Its actual connected borrow constructs an
exact-grant publication from that source and services it before any Opening.
The public configuration guard remains closed: native open/focus/input/execution
and close/reopen orchestration still precede physical enablement.

`NativeCatalogPublication` retains the validated bounded source plus exact
encoded remainder and transfers at most 32 records / 64 KiB per visit. Its
published accessor becomes available only after every record is FIFO-owned,
not after peer receipt. Connection epoch and content-grant epoch must both match.
A replacement connection creates a fresh publication; stale source provenance
never becomes an execution permission. The connected scan/publication phase
still has no native activation or process-execution effect.

The runtime now exposes queue-only `enqueue_async`, using the same aggregate
bulk record/byte budget as existing sends. Returned refusal precedes transfer;
there is no I/O after queue ownership. The producer retains its front through
refusal and removes that prevalidated front immediately after success, without
allocation or callbacks. Existing `send_async` delegates to enqueue then I/O,
preserving legacy behavior. This avoids retrying a catalog record whose old send
helper could have transferred ownership before returning an I/O error. Native
idle publication uses a separate bounded 64 KiB I/O visit. This is returned-error
custody, not a panic/unwind guarantee or a second output queue budget.

`.artifacts/bemenu-native-catalog-publication/`: final device-hidden Session 474
PASS/14 ignored, publication socket controls 2 PASS, existing native execution
controls 4 PASS, runtime library 14 PASS; strict affected Clippy, layout and
workspace formatting PASS. The 70-entry control saturates the actual FIFO,
retains the catalog front, then observes 32/32/8 exact records followed by Opening;
it asserts queue-only service performs no socket write and completion never
replays a record. A separate control refuses changed content-grant epoch with
unchanged connection epoch, wrong catalog epoch and disconnected authority.
These use actual private sockets and supplied protection; no supervised native
client or GUI is run. The compiled clear-front-on-refusal mutation fails the
exact wire-prefix assertion; restored evidence is in
`.artifacts/bemenu-native-catalog-publication-mutant/`. Saturation is deliberately
filled FIFO capacity, not observed kernel backpressure. No hardware, native
presentation, canonical gate, push, installation or lom-test readiness claim.

### WM request to retained native Opening and content service (2026-09-18)

The committed Session launcher request now queues an output-specific native UI
request when independent components are selected. It does not execute an app.
The outer catalog owner cancels untransferred requests on presentation pause or
connection replacement and expires requests still awaiting handoff after five
seconds. On the actual native connection it publishes current output facts,
then lends the retained catalog to the native content owner's Opening transition.
Opening IDs and transaction IDs have separate checked counters.

The native content service retains one exact request and, once constructed, its
exact transaction and Opening payload through queue refusal. It requires the
actual catalog publication's FIFO completion and exact grant before using its
generation. Current output identity comes from the same published content facts
as allocation validation. Success transfers Opening to the shared FIFO before
clearing the request. Existing active openings cannot be retargeted by another
request. A request during close uses the existing exact `reopen` boundary; close
orchestration itself remains unjoined. Output-facts backpressure defers service
instead of treating a healthy saturated peer as failed.

The connected caller then runs the existing native allocation/demand/candidate
service, observes actual runtime presentation, and retries focus using the
transport's retained Presented identity. Prepared grants no focus. A retained
focus-service flag is only scheduling state: the transport validates the exact
opening/revision/presented source before installing focus. Beginning close clears
that scheduling flag. Both large role payloads now have one connection-lifetime
box so the role enum stays compact; no per-frame service reconstruction occurs.

`.artifacts/bemenu-native-opening/`: final device-hidden Session 474 PASS/14
ignored, catalog/opening sockets 3 PASS, connection controls 7 PASS; strict
Session Clippy, layout and formatting PASS (`*-final2` logs). The new actual
socket control withholds catalog completion, fills the real aggregate FIFO,
retries Opening twice, drains it, and receives the original transaction exactly
once after catalog and output facts. It asserts no early focus and no active
replacement. It does not submit/render a native candidate or induce physical
WM input. Existing connection tests retain their prior supplied/headless scope.
The initial test forgot the separate I/O visit after output-facts enqueue and
failed at a read; this is not protocol failure evidence. Initial enum-size Clippy
failures are retained, followed by the compact role-owner correction. The final
compiled drop-request-on-refusal mutation fails the successful-retry assertion;
restored evidence is `.artifacts/bemenu-native-opening-mutant-final/`. That mutant
precedes only the final bar boxing; helper/control bytes match exactly. The first
mutation against the broken read fixture is not discrimination evidence.

Keyboard/pointer semantic input, execution, full close/reopen, successful
protected native-client composition and physical harness enablement remain
unfinished. The public configuration refusal stays closed. No canonical gate,
hardware, native display, push, installation or lom-test readiness claim.

## Native focus capture and shared input routing checkpoint

The Engine capture now accepts an exact native focus binding and emits semantic
text/navigation/Accept or an explicit Session dismissal command. It does not
invent a selected row. Consumed key/button sequences survive focus replacement
and revocation; the bounded sequence inventory refuses exhaustion. Session
continues using the real XKB composition path. Native pointer routing selects
only the exact focused content binding, allows its presented target resolver to
run first, and consumes otherwise unclaimed pointer events at a modal barrier.
It preserves application-owned sequences and ordinary cursor accounting rather
than routing a new press through the launcher to an application. Legacy input
refuses accidentally forwarded native commands explicitly.

Device-hidden evidence `.artifacts/bemenu-native-capture/`: Session 475 PASS,
14 ignored; existing launcher 4 PASS; native capture 4 PASS. The Engine library
contains zero unit tests and contributes no passes. Strict affected Clippy,
layout and formatting pass. The shared Session routing control uses actual XKB
and cursor accounting with a supplied native focus; it has no application
surface or native content target and is not a proof of those delivery paths.
The Engine controls exercise exact binding, Unicode, navigation, modal fallback,
replacement, revocation and capacity. A separately compiled mutation clearing
consumed sequences on native focus replacement fails the exact replacement/
revocation control; source restoration is recorded in
`.artifacts/bemenu-native-capture-mutant/result.json`.

This capture is not yet armed by the production component service. Live focus
synchronization, semantic input/ACK dispatch, pointer action admission, catalog
execution, close/reopen and protected child integration remain next. The public
configuration refusal remains closed. No canonical, hardware, native launch,
push, installation or lom-test readiness is claimed by this checkpoint.

## Native semantic input transfer ownership

The native content owner now holds at most 32 pending semantic inputs and 32 KiB
of text. Each retains the captured exact focus, transaction, kind, text and
monotonic queue time. Invalid/stale input refuses before admission; capacity
refusal leaves input with the caller. ACK intake and input transfer have separate
32-record service bounds. Receipt/FIFO saturation leaves the original front
owned for retry. Transport success transfers the event or Accept intent before
infallible removal; issuance time is the service time, not an expired capture
clock. Pending inputs older than five seconds or a backwards clock refuse without
retargeting or consuming them. Focus publication waits for pending transfers.
Close/disconnect and the live dispatcher still need to join this owner; it is
not yet armed by the physical owner loop.

`.artifacts/bemenu-native-input-owner/` records device-hidden Session 475 PASS/14
ignored and two actual private-socket input controls PASS, strict Session Clippy,
layout and formatting. The saturation control fills all sixteen actual transport
receipt slots, retains another sixteen inputs, then supplies exact ACKs and
receives the remaining original transactions/text once. The second control
rejects stale focus, invalid text and expired/backwards service while retaining
pending ownership. Presentation and protection are supplied by the existing
fixture; this is not supervised Bemenu, natural kernel backpressure or native
input acceptance. A launcher diagnostic about missing optional ld.so.cache is
retained in the log; the activated device-hidden test processes completed.
`.artifacts/bemenu-native-input-owner-mutant/` compiles a drop-on-refusal mutation;
the saturation assertion fails, and source restoration is recorded. No broader
canonical, hardware, publication, installation or physical readiness claim.

## Connected input service and exact close join

The connected catalog/content visit now runs the real input ACK/transfer owner
before content service, using CLOCK_MONOTONIC with checked conversion. The same
visit services an existing close before admitting a successor Opening: actual
pixel replacement precedes exact resource settlement. An unresolved close
returns for another visit without rebuilding or discarding its owner. This is
the existing close machinery exposed through one production-called transition,
not a new retirement state machine.

Beginning the first exact close cancels untransferred local semantic inputs only.
The transport keeps already-issued receipts and FIFO frames. A wrong opening
refuses before cancellation. The new real socket control issues sixteen records,
retains sixteen, refuses a wrong close without losing them, then cancels the
local remainder and receives all issued records before FocusRevoked and Closed.
The existing borrowed native content fixture now uses the same combined close
transition for settlement/retry, preserving its no-submitted-native-frame scope.

`.artifacts/bemenu-native-input-close/`: device-hidden Session 475 PASS/14 ignored,
input-owner 3 PASS and component connection 7 PASS; strict Session Clippy, layout
and formatting pass. `.artifacts/bemenu-native-input-close-mutant/` compiles an
omitted-local-cancellation mutant, fails the intended one-test assertion and
restores source. Supplied focus/presentation/protection remain explicit. The
optional loader-cache diagnostic remains in the activation log; actual hidden
executions complete. No real seat event, protected Bemenu process, native
replacement presentation, canonical gate or physical readiness is established.
Capture synchronization/dispatch and actual execution remain unjoined; the
configuration guard stays closed. No push, installation or hardware action.

## Physical capture dispatch and composition scope

The independent-component physical drain now synchronizes capture from the
currently connected native transport's exact focus, and routes its semantic
reports through the same native content dispatch method used by the socket
fixture. The dispatcher refuses stale outer output/presentation or binding
without an effect. It captures text in the bounded input owner and sends Escape
through exact retained close; close queue refusal retains that obligation. Input
owner overflow or another transport failure retires only the matching component
attempt. Transaction allocation is checked and shared with catalog publication.
No M3 or X-authority path changes.

Engine composition is reset when native grant, opening or output identity
changes, not on candidate/focus-lease refresh within the same opening. The real
XKB dead-key control observes an accented character across a candidate refresh
and an ordinary character across each ownership change. This preserves keyboard
state while preventing unfinished composition from crossing an opening.

`.artifacts/bemenu-native-dispatch/`: device-hidden Session 475 PASS/14 ignored,
native input owner 4 PASS and Engine native capture 5 PASS; strict affected
Clippy, layout and formatting pass. The new dispatch control produces commands
through actual Engine capture, uses the shared Session dispatcher and real
private transport, rejects stale/duplicate closed captures, and checks exact
text/Escape FIFO transactions. Focus/presentation/protection are supplied; it
does not run the physical owner loop or a protected Bemenu child. The compiled
omitted-presentation-check mutation fails that control; restored evidence is in
`.artifacts/bemenu-native-dispatch-mutant/`. The optional loader-cache activation
diagnostic remains retained separately from successful hidden test execution.

Still required before enabling configuration: pointer action dispatch and ACK/
activation service, connected catalog execution and child adoption, idle input
ACK/Accept deadline service into the retained close owner, protected integration,
full exact-source canonical and lom-test preparation. In particular queue drain
alone does not service a deadline after the last local input transfers. This
checkpoint does not claim complete input acceptance or native readiness. No
push, install, live display, GPU or VT action.

## Idle input deadline service

Every active connected native content visit now services the transport's exact
ACK/Accept deadlines even after its local semantic queue drains. A successful
transport timeout close is adopted by the existing pixel/resource close owner;
the next visit must still establish removal and settlement before reopening.
Transport errors remain exact-connection recovery, never evidence of absent
pixels. The deadline helper does not reissue a close after its owner is retained.

`.artifacts/bemenu-native-idle-deadline/`: Session 475 PASS/14 ignored and input
owner 5 PASS in the device-hidden harness; strict Session Clippy, layout and
formatting PASS. The new private-socket control transfers its only input, checks
no close before the negotiated ACK deadline, then receives the exact Timeout
close transaction at the deadline with no new input. Repeated service retains
that close. It supplies presentation/protection and time; it neither waits on a
physical keyboard nor proves native pixel removal. The compiled empty-local-
queue deadline bypass fails this test; source is restored in
`.artifacts/bemenu-native-idle-deadline-mutant/`. Optional missing loader-cache
activation diagnostics remain in the retained successful test log. No canonical
or hardware evidence, push, install or lom-test readiness. Pointer action and
connected catalog execution remain next; configuration stays guarded.

## Connected pointer action and launch admission join

Physical content activation now selects the exact connected component grant. Bar
activation retains its existing service; native activation uses the native action
ledger, shared transaction mint and catalog-owner time origin. The connected
native visit lends its exact FIFO-published immutable catalog and the actual
Session launch queue to the shared action visit: at most 32 ACKs, one retained
cancellation and 32 activation requests. Current presented targets come from the
runtime input projections, not a client-provided list. Admission remains Session
queue insertion and does not mean an application has executed.

`.artifacts/bemenu-native-actions-join/final3/`: device-hidden Session 475 PASS/14
ignored, native admission 13 PASS and native worker/execution 4 PASS. Strict
Session Clippy and layout/formatting pass. The new control uses the same connected
visit as production, actual private socket/action ledger and actual launch queue.
It checks one current pointer activation admission, no replay, and cancellation
of a removed target before a launch can be queued. It invokes the real Engine
continuity reconciler for its supplied projection; it does not run native display
publication or physical routing. Existing explicit process tests run only their
device-hidden short-lived /bin/true child, not a GUI application.

Two earlier fixture failures remain retained: its initial target lacked the
continuity token required by publication, and its cancel-kind assertion used 2
rather than protocol ActionCancel=3. Both fixture corrections preserve production
policy. The final compiled cancellation-omission mutation admits one forbidden
launch where zero is required and fails that queue assertion; source restoration
is recorded in `.artifacts/bemenu-native-actions-join-mutant/`. Optional missing
loader-cache activation diagnostics remain separate from completed hidden tests.

Connected worker execution, immediate exact child adoption, post-admission close
and revocation/replacement reconciliation remain next. Terminal worker results
must not close a new opening merely because an old verification completed.
Pending input after an admitted activation must receive an explicit disposition,
not accidentally become a connection failure. Configuration remains refused
until the full join/protected integration/canonical is checked. No push, live
endpoint, installation, GPU, native/VT run or lom-test readiness claim.

## Connected catalog execution and exact child adoption

The transport exposes its exact admitted opening read-only. Session dismisses
that UI through the retained close owner, cancelling only untransferred semantic
input; ordinary dismissal does not revoke the queued application. The admitted
outcome precedes revocation/Closed in the existing FIFO. The connection's grant
continues to authorize only the previously verified exact queue payload.

The connected component visit now drives the actual outer-owned catalog worker.
Before a visit can produce a child, Session reserves supervisor vector capacity.
A Started result moves immediately into ManagedSessionChild with its exact native
origin and transaction, then starts the existing first-window admission timer.
A missing/replaced connected grant revokes unexecuted queue owners before worker
service. A native component service failure explicitly revokes its grant before
stopping it. Already-attempted execution preserves the child's first-window
attribution. Worker completion does not decide which opening to close, so an old
rejected verification cannot dismiss a later opening. Unconnected visits still
drain worker results with no execution authority.

`.artifacts/bemenu-native-execution-join/final2/`: device-hidden Session 476 PASS/
14 ignored, input owner 5 PASS, native execution 4 PASS and native admission 13
PASS; strict affected Clippy, layout and formatting PASS. The new private Session
fixture runs the production catalog scan and connected execution/adoption method,
actual private protocol admission and real worker verification. It checks UI
close cancelling pending text without cancelling the launch, exact managed child
origin, no duplicate child, preservation after executed-grant retirement, and
revocation both while queued and while verification is pending. Only /bin/true
runs; the fixture supplies protocol presentation/protection and manually dispatches
the real Session queue. It does not run the compositor owner loop, protected
Bemenu, X window admission or a native display.

Two compiled mutations fail their exact controls: omitting grant reconciliation
leaves an undispatched launch queued; discarding the managed child's native origin
fails matches_admission. Sources are restored in
`.artifacts/bemenu-native-execution-{revoke,origin}-mutant/`. Initial mutation
launches were rejected before execution because the wrapper allows mounts only
below /work; corrected runs use a private alias to the read-only /work snapshot.
Those pre-activation failures are not test evidence. The first positive fixture
also caught the new close helper's invalid reason zero; it now uses the existing
Cancelled/UI-dismissal reason, explicitly distinct from grant revocation. Original
failure logs remain. Optional loader-cache activation diagnostics remain retained.

The public configuration guard remains. Still required: joined protected
Bemenu/bar protocol execution, final configuration/harness enablement, exact-source
canonical checks and physical-run preparation. This checkpoint makes no first
window, hardware, GPU, VT, install, push or lom-test readiness claim.

### Protected executable admission, 2026-09-18

Bemenu `36863b9` links the existing menu core into its optional native executable;
upstream shared/plugin targets are preserved. The old selected binary failed in
production protection before negotiation because its sibling libbemenu.so.0 was
not exposed. Do not fix that by granting its whole build directory. The repaired
binary negotiates native capabilities and the exact reserved content grant using
ShellComponentLaunch and ShellComponentProcesses in nested production bwrap.
The control receives an explicit test executable; no live display is contacted.

Device-hidden controls: five ordinary component process tests, the explicit
protected dual-Rust-peer lifecycle test, and the actual protected Bemenu
negotiation test pass. The dual-peer test retains old pixel owners across exact
stop/replacement and keeps the other process connected. It does not substitute
for actual Bemenu/bar content integration. The Bemenu test stops immediately
after negotiation; its resulting EOF may produce service code 4, so this is not
a graceful native UI-close or catalog/raster claim. Full Bemenu check-sophia with
warnings as errors passes, including relocated executable private-socket tests.
Strict scoped Session Clippy, layout and formatting pass.

Evidence: `.artifacts/bemenu-protected-join/summary.json`, `final-execution.log`,
`final/result.json`, old binary/refusal log `actual-before-2.log`, and
`.artifacts/bemenu-standalone-link/result.json`. Initial fixture setup failures
(missing private loader cache, then precreated endpoint directory) are retained
separately and are not runtime evidence. The successful runner generates its
loader cache inside the private namespace; it mounts no host /etc or devices.
The first direct-core build failed because Make expanded prerequisites before
the shared source-list definition; the definition now precedes the optional
include. That compiler failure is not a protocol control.

Still required before lom-test: actual protected catalog/content/close/reopen,
configuration and harness enablement, exact-source canonical validation. No
hardware, GPU, native presentation, installation, push or physical readiness is
claimed by this checkpoint.

### Actual protected client text replacement interoperability

The new explicit ignored process control uses the selected C executable and
real protection/negotiation, actual NativeCatalogPublication, allocation,
resource/candidate validation, focus and input ACK owners. Geometry and native
Prepared/Presented are supplied by the fixture; it is not native rendering or
the physical owner loop. The real Cairo upload contains two row targets; typing
app2 produces the exact ACK and a newer one-row candidate with different bytes
and selection 2, while the test still owns the previous pixel bundle.

This exposed a real client/profile mismatch: Bemenu advanced interaction_generation
with each offer, but both current Session content paths require 1. First upload
succeeded; the edited candidate was rejected stale and timed out. Bemenu now
keeps authority generation 1 and advances a separate target_counter. Candidate
and query revision remain independent. This fixes the client to the current
profile, without weakening the server validator or claiming a negotiated future
authority-generation scheme. Evidence in `.artifacts/bemenu-protected-join`:
`content-filter-execution.log` (original timeout),
`content-filter-fixed-execution.log` (pass), and exact source result manifests.
Bemenu's device-hidden gate passes; the single-field increment mutation compiles
and fails its targeted connection assertion in
`.artifacts/bemenu-interaction-mutant`. No live source mutation was used.

New readiness finding: actual confined rasterization reports missing default
Fontconfig config and unwritable cache locations. Resolve native font discovery
and cache ownership before declaring lom-test ready. Also still open: actual
close/reopen integration, combined bar/native service, configuration/harness and
exact-source canonical. Supplied protocol presentation proves neither scanout
nor native cleanup. No devices, display, installation or push in these checks.

Client correction is signed Bemenu `5f0a3e4`. Final scoped run:
`.artifacts/bemenu-protected-join/content-final-execution.log` and
`content-final/result.json`: eight passing parent controls (five ordinary,
three explicit protected controls); four ignored in the ordinary invocation,
three deliberately selected separately including the Rust-peer parent. Child
entry is exercised only through its parent. Scoped Clippy/layout/fmt pass. This
is not a new canonical run; the earlier canonical identity remains unchanged.

### Protected font setup and same-connection close/reopen, 2026-09-18

Bemenu `6a4b8e9` owns native-only Fontconfig initialization and a mode-0700 private
/tmp cache. It uses already-visible /usr system font roots, refuses an empty font
set and matches generic monospace without host /etc or user-font mounts. Cleanup
unlinks flat entries through its retained directory descriptor; symlink targets
are untouched, unexpected directories/failure are reported. The upstream plugin
retains its original behavior. Full device-hidden Bemenu gate and focused font
controls pass; two compiled mutations (empty-font admission, skipped cleanup)
fail. Actual protected catalog/text raster now emits no Fontconfig diagnostics.
This does not establish user-font admission or every distribution's font layout.

Bemenu `7d2d239` resets query/selection only after an accepted fresh Opening. The
actual protected test first reproduced a reopened menu retaining the one-row
app2 filter. After correction it receives the full two-row catalog again without
reconnecting. An invalid-opening mutation fails the query-preservation control.
The original timeout/mismatch evidence is retained, not replaced with a pass.

The real process control now runs with a separately protected Rust bar peer in
the same ShellComponentProcesses and aggregate content registry. Bar connection,
grant and exact byte lease survive native text/filter, close and reopen. The
launcher closure reaches actual Retiring bytes and cannot settle while the test
holds its renderer-facing bundle; dropping that actual owner permits settlement.
It then reopens, uploads a newer candidate with reset query, closes again and
settles before process stop. Final accounting is quiescent after both peers and
the bar lease end. Eight parent controls pass in the final scoped run, alongside
strict scoped Clippy/layout/fmt. Four ignored tests in the ordinary run remain
explicitly selected separately (three parents; child entry only through parents).

Evidence: `.artifacts/bemenu-protected-join/reopen-final/result.json` and
`reopen-final-execution.log`; `.artifacts/bemenu-native-fonts/result.json`;
`.artifacts/bemenu-fonts-{no-fonts,cache-leak}-mutant`;
`.artifacts/bemenu-reopen-{query,unvalidated-mutant}`. Initial Fontconfig-only
probe was noisy and is not the repair. Geometry/Prepared/Presented/Focus are
supplied to the real transport; this test does not execute Session native
projection/removal, a GPU worker/KMS or physical input, nor does the fixture bar
execute Lom. Requesting process stop can still race with EOF and report client
service code 4; no graceful client-exit claim is made. No new canonical, hardware,
install, push, or physical readiness claim. Remaining work is public profile and
harness enablement, compatibility/build identity and exact-source canonical gates.

### Public component selection, 2026-09-18

Session configuration now selects the implemented independent owners instead of
unconditionally refusing revision-7 components. Explicit components require a
normal enabled shell with a Sophia WM and content permission. A bar requires a
positive panel allowance; a launcher requires content input and a selected,
known application catalog. Launcher-only selection reserves no panel band.
GPU permission is per component: global direct permission is refused for this
mode. No absent executable is inspected or started during config preparation.

An explicit legacy `--shell-process` conflicts; the installed
`--shell-process-default` fallback is ignored. Component configuration also
ignores ambient `SOPHIA_SHELL_CONFIG`, using the separately selected per-role
configuration grants. Legacy single-shell selection retains its existing checks.
The role matrix exercises actual Session parsing with nonexistent application,
WM and shell paths, checks no startup applications and no fallback selection,
and covers missing catalog/input/panel, unknown catalog, absent WM, wrong mode,
global GPU permission and explicit legacy conflict.

Evidence is `.artifacts/bemenu-component-config`: the initial fixture had a KDL
separator error (retained); corrected role controls pass. The device-hidden
Session library run passes 476 tests with 14 ignored. No actual process launch,
GPU, display or native acceptance is inferred from configuration tests. This
replaces the public guard only; the `lom-test` wrapper still selects the legacy
single-shell profile and needs a separate explicit component profile/harness
join before a physical run. No canonical successor or physical readiness claim.

### Independent component launch evidence, 2026-09-18

The component owner now retains the GPU preparation result beside its exact
attempt key instead of discarding it. The read requires that same currently
Connected attempt; preparation/spawn alone, revoked attempts and altered grant
identities refuse. Successful negotiation reports role, slot, connection/content
epochs, negotiated revision, GPU policy and exact admitted device numbers. A
negotiation already revoked by a concurrent presentation pause is not reported
as current admission. Direct policy evidence is the existing validated launch
result, not proof of adapter selection or native rendering.

Process events distinguish failed supervision from a retired process whose
endpoint cleanup succeeded/failed. Endpoint release is not claimed as a normal
child exit or native cleanup. The structured recorder has a bounded, explicit
component/catalog/launcher vocabulary; it excludes query text, catalog strings,
paths, raw event Debug values and free-form errors. Launcher denial is an
intentional per-role GPU policy, not a failure of the bar grant.

Evidence: `.artifacts/bemenu-component-evidence/final/result.json` and
`final-execution.log`. A real protected Bemenu control negotiates, revokes/reaps,
settles and relaunches through ShellComponentSession, proving fresh exact evidence
and refusal before negotiation/after revoke/for altered and old grants. It is
GPU-denied and receives no opening or presentation; the direct-device positive
is source integration, not a hardware test. The first control failed because it
omitted final settlement after request_shutdown recorded its obligation; that
failed log remains separate. Final device-hidden runs: 476 Session library,
25 diagnostics, five ordinary component process controls and four explicitly
selected protected parent controls pass (510 total). Five ignored entries in the
ordinary process run include four selected parents and the child entry exercised
only through its parent; 14 library tests remain ignored. Strict affected Clippy,
layout and formatting pass. No new canonical, installation, push or physical
readiness claim. The component-aware harness/profile and exact final gate remain.

### Component-aware attended harness, 2026-09-18

`lom-test launcher` now selects independent Lom/Bemenu while preserving plain
`lom-test` as the established single-shell panel workload. It archives the exact
signed Bemenu source into the evidence directory for a fresh native-only build,
records source/config/binary hashes and verifies them before/after the session.
Generated KDL grants direct GPU only to the bar, selects the explicit
native-launcher-gate catalog and starts no applications automatically. The core
fixture admits registered xterm plus /usr/share/applications under trusted-host
policy. The real profile composer requires an existing WM application-launcher
key; no probe injects or replaces a binding. Current operator profile already
binds Super+Space. No user configuration was modified or program installed.

Presentation records now include their exact connection/content grant, preventing
the two clients' independently numbered candidates from satisfying each other's
checks. Component shutdown records quiescence only after actual backend disposal
and the aggregate inventory predicate. The smoke verifier requires stable distinct
roles, exact per-role GPU policy, WM/catalog preparation, two native presentations
per output per grant on two shared outputs, one supervised process-start outcome,
and final quiescence. The wrapper also requires normal exit, TTY/keyd recovery and
unchanged inputs. This is not a claim about visible application windows, focus,
placement, kernel timestamps or the separate panel latency workload.

Evidence: `.artifacts/bemenu-launcher-harness/final4/python-result.json` and
`final4-execution.log`. Device-hidden tests: actual generated-profile Session
preparation (1), Session library (476; 15 ignored), diagnostics (25), real WM
profile/binding composition (2); tooling suites (29 existing, 20 new/inherited
launcher controls). The 20-control suite is also selected independently in that
run; do not count duplicate executions as additional coverage. The real shell
verifier gate retains its 28 prior mutants. Commands/builds/VT/native results in
Python command tests are supplied effects; the generator and transcript verifiers
are actual production tools. Initial fixture errors (missing shortcut profile
identity, absent private loader cache and an outdated mocked signature command)
remain separate logs. No false pass is substituted for them. A frozen canonical
successor, candidate compatibility/build checks and physical readiness audit
remain before asking the operator to use the new command.

### Harness follow-up and frozen validation, 2026-09-18

Contained canonical `20bb3353` reports PASS (288 Rust groups, 3,519 passes,
zero failures, 36 ignored), but emitted the same new unused-helper warning during
test/Clippy builds. It is not the final warning-free readiness result. An existing
Session fixture also imports the composer example; it now calls the native-key
validator and asserts that its bar-only WM profile would refuse a native launcher
probe, while its ordinary panel composition remains accepted. No warning allowance
or gate relaxation was added. Scoped strict Session Clippy and the actual fixture
pass. The script now fixes Sophia/config-example output directories explicitly so
an inherited CARGO_TARGET_DIR cannot select an old binary at the expected path.
Numeric smoke-verifier input is length-bounded before integer conversion, with
an oversized-field negative.

`final5-execution.log` under `.artifacts/bemenu-launcher-harness` retains the
updated device-hidden positives and the prior fixtures' final successful scope.
The independent unchanged Lom `a317370` project gate passes in
`.artifacts/lom-native-launcher-readiness-rerun/result.json` (55 Rust controls,
16 tooling controls; strict Clippy/fmt/layout/dependencies). Its archived tracked
source hashes remain exact. That run reused this owner's disk-backed target cache
and is not a clean-build claim. The initial wrapper import-path failure occurred
before tests and remains under the unsuffixed artifact directory. Neither run
executed a real display, GPU, VT or installed launcher. Final canonical/build and
readiness manifests must identify their exact successor; no current-source
canonical claim is borrowed from 20bb3353 or a prior checkpoint.

### Exact operator-profile provider replacement, 2026-09-18

The operator's actual WM profile still declares Narthex and its private config.
The readiness audit caught that these incompatible legacy settings survived the
old per-key composition alongside the probe's new component set. The composer now
treats an explicit component selection as one provider set: replace inherited
legacy shell-client/shell-config and old component identities in the generated
probe, preserving WM/policy/bindings/applications and the original file. This does
not relax Session's rejection of an ambiguous user profile.

`.artifacts/native-launcher-readiness/profile-check/result.json` records a private,
device-hidden run of the actual release CLI, updated release composer and Session
preparation against the selected operator profile. Actual expansion/composition,
known catalog, independent role permissions and empty startup all pass. Three
real composer controls pass; restoring the old composition behavior in a separate
archive compiles and fails the new legacy-provider control for the actual schema
conflict (`.artifacts/native-launcher-composer-mutant/result.json`). The control
also covers replacing differently named old roles without retaining their private
configuration. Strict config Clippy/fmt/layout pass. The prior exact canonical
`19591816` completed PASS, but it predates this final provider-composition fix and
is not relabelled as validation of the successor. No user config was rewritten,
no live/native endpoint was contacted, and physical acceptance remains pending.

### First launcher preflight: retired collection regression, 2026-09-18

The operator's `20260918T105056Z` capture reached the protected GPU proof:
exact DRM identity selected the RADV discrete adapter and the first candidate
received the proof's synthetic presentation. Code 9 for the second candidate
was intentional. The actual failure was `content lease or backing survived
renderer release`, before native desktop takeover. This is not launcher or
native presentation acceptance.

The registry refactor made repeated exact-grant disconnect return without
collection once the grant was already retired. The legacy transport proof
disconnects while holding a render bundle, drops it, then disconnects again;
the second call therefore observed uncollected retired bytes. The single-shell
facade now explicitly collects after disconnect, including repeated calls,
without changing the registry's exact revocation or granting a stale caller
authority over another grant. Collection still requires real consumers to end.

The real private-socket candidate fixture covers both Presented and
RendererFailed, repeated disconnect with a held readable render bundle,
post-drop byte/backing/accounting quiescence, and repeated settled calls.
Ten device-hidden transport/component controls pass, including independent
grant and successor protection. Removing only the collection in an isolated
archive compiles and fails the new renderer-failure control with eight bytes
still retained instead of zero. Evidence: `.artifacts/legacy-disconnect-collection`.
This is returned-call resource ownership and accounting evidence, not GPU/KMS
execution. No new native run was performed for this repair.

### Persistent catalog connection ownership (2026-09-18)

The component catalog now retains bounded publications per exact connected grant.
Reconciliation consumes the complete connected inventory before revoking removed
owners; alternating service visits no longer imply replacement. The one existing
verification worker is visited with the exact pending or dispatch grant. A wrong
or absent connection is refused before polling its result. Transient publication
keeps its existing wire encoding; persistent publication selects revision-8
identity records and retains that mode through FIFO retries. Dock negotiation and
construction remain refused pending the candidate/action integration.

Device-hidden evidence in `.artifacts/dock-catalog-owners-v3` checks two real
private-socket publications, inventory reordering, duplicate-inventory refusal,
unrelated-peer removal, exact worker routing and real `/bin/true` child adoption.
The catalog and presentation used for activation remain supplied fixture facts;
this is not a revision-8 end-to-end launch or native dock proof. The final Session
library gate passes 477 tests with 15 ignored; strict library/test Clippy passes.
Earlier `dock-catalog-owners` and `dock-catalog-owners-final` runs accidentally
tested the baseline because their archive overlay was skipped. Their logs are
explicitly baseline-only. The corrected snapshot initialized its own repository
and compared every changed file to the candidate before execution.
Removing the exact connected-owner guard in a separate archive compiles and
fails the wrong-peer assertion. The positive source is retained separately and
was never mutated. Layout passes; no canonical or hardware acceptance is claimed.

### Persistent candidate store (2026-09-18)

The actual content store now accepts typed persistent catalog candidates under
the persistent profile. Begin binds the Session-supplied catalog generation;
End and render submission revalidate generation, connection epoch, available
unique catalog slots and current allocation identity. Only role-1 surfaces and
kind-3 targets are admitted. Legacy and transient entry points remain separate.
The renderer bundle carries passive catalog metadata alongside its real leases.
Consumed Begin generations cannot be reused after a terminal refusal.

Six device-hidden persistent controls plus 22 ordinary and 10 native-launcher
candidate controls pass; strict runtime library/test Clippy passes. Removing
catalog-generation equality in a separate compiled archive fails the pending
owner revalidation control. Evidence: `.artifacts/dock-candidate-binding`, final
`generation-check.log` and `catalog-mutant.log`. These tests supply catalog,
allocation and renderer completions. They prove store custody and refusal, not
socket negotiation, launch authorization, native rendering or dock readiness.
Transport/Session persistent service integration remains open and admission
remains refused; no hardware, installation or publication occurred.

### Persistent activation response owner (2026-09-18)

Typed persistent intake now reserves one aggregate control credit before removing
the exact request. Completion validates the full activation and transaction,
retains the first outcome across FIFO refusal, and clears the fixed-field pending
record only after enqueue. FIFO ownership retains the charge through the final
byte. Disconnect and fresh negotiation clear that connection's old obligation.
This is response custody, not authorization or permission to replay launch work.

Seventeen device-hidden runtime library controls pass, including three new
persistent response controls. Strict runtime library/test Clippy passes. A compiled
credit-omission mutant fails the aggregate-capacity assertion. Positive restoration
uses a fresh dedicated target: an initial shared-target retry reused the mutant
binary because of older archive timestamps, and is explicitly non-evidence.
Artifacts: `.artifacts/dock-catalog-response`, `fresh-positive.log`,
`fresh-clippy.log`, and `credit-mutant.log`. The fixtures supply negotiated state
and simulate FIFO drain; they do not exercise kernel backpressure or a revision-8
handshake. Dock service construction and negotiation remain refused pending the
presentation/action integration. No physical run or install was performed.
The combined candidate/response source also passes all 477 Session library tests
with 15 ignored in the same device-hidden fresh-target run; layout passes with
one documented external private-fixture mount, without changing the checker.

### Persistent candidate transport (2026-09-18)

Revision-8 assembly now reaches the existing candidate store through a bounded
transport visit (at most 32 records and 64 KiB of payload). It validates the
negotiated profile, exact grant and unique current output context before dequeue;
legacy/transient candidate families refuse. Submission rechecks the published
catalog while transferring actual resource leases. Allocation, Session action
authorization and enabled Dock negotiation remain separate unfinished work.

Device-hidden runtime library controls pass (20), including wire assembly through
real leased submission and exact original-transaction Prepared/Presented FIFO
responses. The controls supply negotiation, allocations and renderer completion;
they are not a dock connection or native presentation. Missing/ambiguous End
context and response saturation retain input; stale Begin settles one terminal.
The existing 22 ordinary, 10 transient and six persistent store controls also
pass. Strict runtime Clippy and layout pass. A separately compiled wrong-grant
mutation fails the expected pre-dequeue refusal; the positive archive/target was
not mutated. Evidence: `.artifacts/dock-candidate-transport`. Its initial check
failed because the fixture granted a permit under a one-record budget; the fixed
fixture admits its full limit at construction. No hardware or publication.

Persistent allocation service reuses the existing edge request/grant/release
owner, restricted to role 1. Popout and transient-opening requests refuse; a
release proposal does not discard its live allocation until Session completes it.
Device-hidden validation in `.artifacts/dock-edge-allocation/check.log`: 20 runtime
library, 11 ordinary allocation, 10 transient and seven persistent controls pass,
with strict runtime Clippy. The new control supplies Engine allocation decisions;
it does not exercise physical work-area reservation. Initial Clippy rejected a
redundant Boolean expression, replaced with an explicit profile match.

### Three retained component layers (2026-09-18)

The backend now has three Session-selected layers ordered Shell, Dock, Launcher.
Grant epochs do not select stacking. A dock no longer has to share the panel's
output/layer key; the existing exact grant, retirement, queue-refusal and removal
owners apply without a separate resource registry.

The new production-intake control retains three real pixel leases, supplies
native completion, checks stacking independent of epoch order, refuses a dock
replacement without altering neighbors, and verifies exact dock removal only
after replacement presentation. Bar and launcher presentation identities survive.
All 154 backend library tests and strict library/test Clippy pass device-hidden;
a compiled mutation aliasing Dock to Shell fails the new control. Evidence:
`.artifacts/dock-three-layers`. This is actual queue/projection ownership with
simulated completion, not worker/GPU/KMS or three supervised clients. Dock
negotiation remains disabled until the Session catalog-action join is complete.

### Persistent Session service and revision-8 admission (2026-09-18)

The persistent catalog profile now negotiates revision 8 with its exact capability
mask and explicit input grant. Session constructs a separate borrowed catalog
service for Dock, uses the shared resource/allocation/candidate owners, carries
the catalog binding through actual presentation, and submits to the Dock layer.
Catalog and input configuration are required before endpoint construction.

Issued actions retain typed indicator, transient or catalog provenance. Catalog
authorization checks the complete issued action, current presented target
continuity, publication generation and deadline before inserting an immutable
catalog origin into the existing launch queue. Admission consumes the exact
event independently of ACK; the transport owns response retry without replaying
the launch. Repainting an unchanged target does not invalidate an issued action,
but catalog replacement, cancellation and authority changes do.

Device-hidden validation in `.artifacts/dock-action-service/frozen-check.log`
passes 483 Session library tests, nine connection-owner tests and five process
tests, with 20 ignored across those suites. Strict Session library/test Clippy
passes. `runtime-check.log` passes 88 runtime/transport controls; layout passes.
Separate compiled exact-echo and replay mutations fail their intended controls.
Earlier failed fixture setup and Clippy runs remain retained, not passing evidence.

The three-peer test performs real bar/menu/dock negotiation and exact borrowed
service construction without a supervisor. Action tests use real sockets, typed
FIFO and the production launch queue, but supply protection and presented facts;
they do not execute queued applications or establish native presentation. No
full canonical gate, GPU/display run, installation or publication is claimed.
Provlita's retained GPU protocol client, the three-client physical wrapper and
their integrated gates remain before attended acceptance. Tasks remain open.

### Generic persistent client adapter (2026-09-18)

The shell-client library now offers bounded revision-8 catalog assembly over its
sole FIFO, exact catalog candidate enqueue using the existing ContentLifecycle,
and paired ACK/activation admission using reserved control capacity. Partial or
replayed catalogs never become current. A candidate enqueue refusal changes
neither the lifecycle watermark nor the output FIFO. The app still owns the
catalog-to-presented-view relationship; these methods grant no launch authority.

The device-hidden shell-client suite and strict Clippy pass in
`.artifacts/dock-client-catalog/socket-check.log`; layout passes. Six new controls
cover incomplete/malformed catalogs, a 128-entry real-socket publication spanning
the 64-record inbox, preserved wire order, candidate saturation and exact paired
response refusal/retry. Supplied welcome/presentation metadata is not supervised
client or native evidence. An initial fixture used a nonzero Limits transaction
and failed codec validation; its corrected run is retained separately.

Lom `2b79103` and Provlita `7816142` share the extracted client-side GPU owner;
both full project checks pass device-hidden. Sophia has no Vello dependency.
Provlita's native worker, output/resource scheduler and attended wrapper remain
unfinished. No physical readiness, install, push or canonical PASS is claimed.

### Three-component dock smoke preparation (2026-09-18)

`lom-test dock` now selects Lom, Bemenu and Provlita independently with explicit
top-24 and bottom-64 reservations. The profile is generated by Rust `xtask dock`,
preserves WM-owned bindings, and keeps application startup empty. The production
launch builder now passes the dock's own reservation thickness instead of omitting
the environment grant required by Provlita. The control inspects the actual launch
spec before deliberately refusing spawn; no protected GPU process is claimed.

The connection-owner fixture negotiates all three roles, holds an actual dock
pixel lease across close, continues bar/menu uploads, then checks fresh dock epoch
identity and final aggregate quiescence after all consumers release. This exercises
real private sockets/stores, not the complete three-client/native workload.

Successful application execution now records its exact catalog cause, grant,
output and event after adopting the actual child. The redacting diagnostic
recorder preserves those bounded fields and dock slot 2/role/failure records.
The host smoke verifier requires both-output presentation and actual menu/dock
launches, not client claims. Its negative controls cover missing, duplicate,
overflow, aliased, restarted and mismatched authority evidence. The real launcher
control substitutes build/VT/proof/session effects, executes the Rust profile and
verifier, and refuses failed proof, watchdog exit, empty evidence and overwrite.

Contained scoped evidence: `.artifacts/dock-smoke/final-check.log` has 525 passing
Rust tests and 21 ignored, including the generated profile through the real
Session parser. A missing-rg layout invocation is not a pass; the explicit-rg
rerun passes in `layout-check.log`. `launcher-check.log` adds the actual-script
control, strict xtask Clippy and 29 existing launcher/workload regressions.
Strict affected Session/conformance Clippy passes. Sophia and Provlita release
builds succeed with devices hidden; Provlita's config-only command succeeds.
Canonical validation is a separate frozen-source gate, pending at this checkpoint.

The [physical instructions](../../../tools/probes/dock/README.md) define a
90-second four-launch smoke. It is not the 40-action latency workload, restart
acceptance, or complete t107/t108/Provlita t005/t006 acceptance. No native run,
GPU enumeration, installation, M3 import or publication occurred in this slice.

The first frozen canonical attempt on `7a31de06` passed workspace tests but
failed the conformance-reader anchor guard. The dock reader now uses the shared
`record_after_marker` convention and tests tracing-decorated positive and fatal
logs. The checker/allowlist was not relaxed. That first FAIL remains retained;
the corrected signed successor requires its own canonical result.

### First attended dock repair (2026-09-18)

The first run reached the dock and application execution but failed native
acceptance. Four xterms encountered missing LookupColor; a later VT handoff
separately hit an outstanding seat lease. The [retained investigation](../investigations/d7k4q2vm-dock-smoke-exposes-color-lookup-and-seat-retirement-order.md)
records the source repairs, discriminating private-xterm/mutation controls,
Provlita vector tiles and strengthened child-exit evidence. The command remains
`lom-test dock`; use shell `exit` for the four terminal launches and keep a
VT-release test separate. No t107/t108 or physical acceptance is closed here.

## Output-bound catalog placement follow-up

The [dock placement investigation](../investigations/n8r3d6qp-catalog-launch-must-capture-the-clicked-output-workspace.md)
adds committed per-output WM bookmarks to the shared dock/launcher queue.
Workspace state stays WM-owned. Smoke acceptance now requires each supervised
launch's exact first surface to reach a matching committed output, in addition
to process start/clean exit and independent component lifetimes. Native
acceptance remains pending; private socket tests cannot close it.

The subsequent `20260918T165507Z` run recorded matching logical placement but
terminated on an oversized runtime observation batch. The [runtime batching
investigation](../investigations/chc18alj-completed-runtime-observations-must-be-chunked-without-dropping-work.md)
records the device-free reproduction and lossless bounded-decoding repair.
Three-component acceptance requires a fresh clean attended result; neither
placement matches nor deterministic tests turn that failed capture into a pass.

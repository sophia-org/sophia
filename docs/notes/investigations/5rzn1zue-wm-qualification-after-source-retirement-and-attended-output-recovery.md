---
id: 5rzn1zue
date: 2026-10-10
kind: investigation
status: investigating
tags: [validation, protocol, policy]
---
# WM qualification after source retirement and attended output recovery

## Question

What remains of t249/Hagia h006 after the WM became 9P-only and t310/h018
passed attended occupied-output recovery? niltempus authorized this audit
alongside the t133/t275 design on October 10. It is a source/evidence review,
not a new benchmark, device test, release or claim that a historical gate
qualifies current source.

Reviewed masters: Sophia `b0ca8280a1c27dbf7718aa99238aea62cf82d6d4`,
Hagia `16c1d43497bc3812e755d6813b9b94024ab4d05c`, niltempus
`dc57e532fdd030e6ea90ed0a68eae27f76e21300`. Their installed code pair is
Sophia `6fd66325b` and Hagia `ed2f30060`, release
`niltempus-d59a72c27a32a2c05e27`; subsequent master changes are documentation.

## Evidence

Paths below are under `~/.local/state/sophia/development-evidence/`.
This audit read the named reports and hashed those files. It did not reverify
every artifact in the older trees or rerun their producers.

| Evidence | Identity and result | What it establishes |
| --- | --- | --- |
| `hagia-wm-policy-env/pairing-be6e5888/run3/report.json` | SHA256 `7ff821d80a4ffb042fa440efb6728fdca3b61e7433dfa0908faa3ddc82c3844f`; Sophia `be6e5888d`, frozen Hagia `e8b56a3`, status pass. | Twelve owner cases and thirteen legacy cases on a hashed test overlay. Admission, CPU pixels and frontend acknowledgements include supplied facts; mirror completions are simulated. |
| `t249-release-overnight-48c387dd-r2/run/measurement-report.json` | SHA256 `6eea2e240d8998565d088fbd191a498eaebc0bd086ac554ecb0cd1b63a43dda4`; release Sophia `820853335`, Hagia binary `0419e09e…`. | Forty pairs refused; no latency or full-t249 acceptance. |
| Same run, `measurement-manifest.json` | SHA256 `5b9cc4a5e763454c71cfd9c60472463287492001abab3079bccd894500817b4c`; 80 runs. | Declared old IPC/files comparison workload, not an identical-current-source baseline. |
| `t310-topology-repaint-01/RESULT` | PASS at Sophia `6fd66325b`; 540 Rust summaries, 7,533 passed, 0 failed, 101 ignored. | Current full repository correctness gate; ignored independent-peer and physical gates remain distinct. |
| `t310-hagia-output-return-gate-01/RESULT` | `PASS nimble verify=0`, for the installed `ed2f30060` repair. | Hagia's local gate; not a rerun of the historical real-Hagia Session overlay. |
| `t310-two-output-attended-06/acceptance-01/ACCEPTANCE.json` | SHA256 `8737831f1036b6f7c3f35dca8935e41112fdd5e5ed0d5b9fe3ee54bea1661976`. | Exact installed occupied-HDMI migration/return, unchanged Sophia/Hagia processes, user-confirmed workspace labels and keyboard/mouse. No latency measurement. |

All forty measurement pairs have the refusal “different retained updates;
survivor percentiles are not equivalent work.” Overlapping additional refusals
are 21 file-p99 interval breaches, three p99 regressions above 2 ms and one p95
regression above 1 ms. These counts were recomputed from the preserved report;
they are not a new statistical analysis or a transport-causality conclusion.

## Finding and resolution

The task row and older plan still described opting into WM files while output
remained IPC. That is historical, superseded by the
[source-retirement decision](../decisions/twkn9fsp-retire-wm-and-shell-ipc-with-release-rollback-while-latency-qualification-remains-open.md)
and accepted output-file work. Current qualification must describe the current
9P-only SDK path and whole-release rollback. It must not restore retired product
IPC or claim that default selection proves performance.

Hagia's `tools/sophia_pairing/README.md` explicitly marks the old runner
historical and says it is not rebased. Its `compatibility.json` requires Sophia
`be6e5888d79f2e89bd7fdfbee3455f8aa7eb6390`, Hagia source
`e8b56a3195ac11f44e100bc4ed903c0d3cb72e9a` and executable SHA256
`e8221d1197b032e51c7fabe5dccc20e6c8e52342e8e8cd8a82940ad9063b86ad`.
Its private mount anchors and two-wire expectations no longer describe current
Sophia. Pointing it at master is a precondition failure, not a missing run to
queue. Preserve the fixture and evidence unchanged.

### Coverage that can be reused, and the exact gap

| Required family | Existing evidence/current source | Remaining qualification |
| --- | --- | --- |
| Profile/configuration/catalog startup | Historical protected normal Hagia owner case; current `policy_file_c_sdk.rs` and `policy_file_startup.rs`. | Current ordinary Hagia through protected Session launch, exact SDK/profile/capability identities. |
| Layout, focus, actions and session operations | Historical layout/behavior/action cases; current client tests and generic Session tests. | Map current capabilities to named tests; add current real-Hagia joins only where local/generic tests cannot establish the composition. |
| Timeout and stale proposal recovery | `policy_file_recovery.rs::protected_c_sdk_recovers_after_stale_and_timed_out_projections` uses the real protected connection and settlement owner, with supplied timeout outcomes. | Current Hagia checkpoint/recovery under the same conditions; do not label a supplied timeout as a real resize deadline. |
| Restart, profile replacement and rejection | Historical normal-Hagia checkpoint and rollback cases; current generic profile/epoch controls. | Current SDK-based Hagia through automatic and requested restart, rejected profile preserving the committed state, fresh request after recovery. |
| Source repaint, receipts, capture/release debt | Historical mirrored-owner join with simulated device completion; current backend and protocol tests. | Enumerate all currently negotiated capabilities, especially later overlay/chord/release changes; prove their real owner joins or name the missing case. Simulated receipt evidence stays labelled. |
| Revocation, slow peer and disconnect | Current `policy_file_custody.rs` tests real reactor ACK credit and waiting-read ESTALE; startup controls cover stop and wrong epochs. | Check that the external SDK/client pairing reaches the same terminal paths without leaked request or input authority. |
| Output loss and occupied-window restoration | Accepted t310/h018 release and exact process/owner trace. | Reuse within that scope; it does not establish every WM capability, throughput, reload or restart case. |
| Independent SDK/export | Hagia `tools/check_sdk_export.sh` runs two explicitly ignored supplied-stream tests in `policy_file_nim_peer.rs`. | Invoke explicitly on the selected pair; ordinary full-gate totals do not prove these tests ran. Its supplied admission/outcomes do not prove supervised production launch. |
| Transport cost and drag latency | Preserved refused 80-run release campaign. | New declared method with valid comparison/accounting and current-source measurements; CPU, allocations, copies, wakeups and round trips remain separately required. |

The current source references above were inspected, not executed in this audit.
They are reuse candidates with known limits, not newly passing results.

### Next bounded implementation slice

Build a new current-source lifecycle qualification suite in the owning external
integration tooling, starting with protected startup, profile rejection/rollback
and requested/automatic WM restart. Keep Hagia policy assertions in Hagia and
named-stack launch/release work in niltempus. Sophia owns only generic contract
and owner seams. Do not port the whole historical overlay or insert client-name
expectations into Sophia to obtain access to private owners.

Pin Sophia, ordinary Hagia, SDK, profile and binaries; list each expected test
and refuse missing or zero-test runs. A generic test-support seam or explicit
source overlay must be disclosed as such. Record which admissions, timeouts,
pixels and completions are supplied. Test rejection and disconnect controls
before asking for a new installed-session check. The remaining capability
mapping follows that working lifecycle slice rather than a broad new campaign.

### Measurement boundary before another campaign

The old runner's forty unequal survivor populations prevent its comparative
percentiles from answering the transport question. A successor must report all
offered/admitted/coalesced/settled identities and unresolved work; it cannot
filter to matching survivors after observing a run. Establish a small diagnostic
with reproducible accounting before any hours-long acceptance run.

Today's SDK-only Hagia cannot simply run on the retired IPC adapter. Comparing
today's full stack with an old whole release changes more than transport. Keep
two questions explicit: current control-path responsiveness against the declared
absolute budget, and a controlled transport comparison on a compatible fixed
pair. The latter needs a reviewed method before it can satisfy the original
relative-budget exit. A historical mechanism experiment may help explain a
cost, but does not by itself qualify current source.

The p95 +1 ms, p99 +2 ms and 60/120 Hz interval budgets remain unchanged. No
baseline substitution, accepted workload/coalescing change or threshold change
is implied by this audit. A necessary change of acceptance method must be
recorded prospectively, never used to relabel the old failure. Heavy measurements
get a quiet window and bounded resource settings under the coordination policy.

## Validation and remaining work

This slice checked source boundaries and decoded the retained reports. It made
no build, benchmark, device or live-session call. t249/h006 remain open; the
new lifecycle suite and measurement method are the next work. t250 retains the
attended daily-configuration exit and whole-release rollback; t289 keeps its
separate performance scope.

The parallel [admission boundary](../plans/esnqxpqw-pidfd-and-namespace-admission-optimizations.md)
and [namespace recipe draft](kcfh2hdg-adopting-the-plan-9-namespace-model-in-sophia.md#proposed-recipe-boundary-2026-10-10)
advance the observer design without waiting for every WM/shell qualification.
t315 retains its t249 prerequisite; t317/t318/t319 are not promoted into
implementation by this audit. t307 remains parked. No t310 cable acceptance
needs repeating to begin this work.

Documentation validation: changed local file links resolve, task IDs remain
unique across open and completed ledgers, and whitespace checks pass. zk's
repository-wide broken-link report is unchanged from the 25-note baseline;
none of this slice's notes adds a broken link. No task was closed by the audit.

## Current lifecycle implementation (2026-10-10)

The next bounded slice above is implemented outside Sophia. Hagia
`870bfc68b3d2d8c1239f9f853f63ddb6557bd04a` owns `tests/external/lifecycle.rs`;
niltempus `dbda3009b389ab4bd9aec6dce81fd4d5b10ce1c9` owns the Rust runner
`tools/wm_lifecycle.rs`. Sophia production and repository test sources are
unchanged. The runner archives Sophia `0f2ad2386baa063ab92d9567a145148f5232bccb`
and explicitly mounts that external test module under the launch/reload owner.
It is a disclosed test-source overlay, not a new public Session API.

The final three cases each ran exactly once and passed: protected ordinary
startup with profile/catalog/capabilities, accepted profile replacement followed
by real Hagia rejection and Session rollback, and automatic recovery after
terminating only the fixture child followed by requested restart. All successors
answer their queued scene and send a real post-checkpoint Dirty continuation.
Requested restart invokes its production owner directly, not the control socket.
The ordinary binary uses SDK `b2a254dcb792e5f9d66f78bdd73f645153504507`; its
SHA256 is `20581c96e823240460fa09e863f779de53a4851080cefabaa4e46b22172ff5f4`.

`t249-sdk-lifecycle-01/RESULT` is `PASS lifecycle=3 native=false`. Its verified
self-excluding manifest SHA256 is
`b5ac4542073194c5ba27200a292862eb77489624236549f2b40add43c9df23c6`.
`t249-sdk-lifecycle-controls-02` hashes its final test, fixture and mutant inputs
before/after and fails exactly the restart case when Hagia's checkpoint load is
disabled: the fresh projection deadline expires without the client's restored
Dirty. Its manifest is
`6bbfd791ec39a0f752b2bc63682bd3cd29b4f92df296378838294bdab9557aa5`;
controls-01 preserves the mutation/build. `t249-sdk-lifecycle-checks-01` records
overlay Session clippy with warnings denied and the runner's refusal/closure
controls. Development run 01 stopped on zero tests because the feature was
omitted; it remains a failed invocation, not qualification. Runs 02/03 remain
labelled development runs.

This is an **empty headless scene**. It does not qualify occupied model/focus
restoration, renderer pixels, native receipts or physical input. Protection is
supervisor evidence, not independent namespace inventory or 9P Tauth. The full
product/formal gates were not repeated for this external tests-only change.
No release was built or installed. The historical pairing runner and refused
measurement campaign remain unchanged. t249/h006 stay open; next is the
current capability-to-evidence map and its missing owner joins, followed by the
prospectively reviewed measurement method. The observer admission/recipe design
can continue within its previously admitted parallel scope.

## Current capability-to-evidence map (2026-10-10)

This follow-up inspected Sophia `b52a32dc3c26000bae9b6d656fc274710a90f9bd`
and Hagia `70595f2553bb21527f9cf9dfcdade8e7cfb2f743`. It ran no tests,
builds or live-session operations. The lifecycle evidence above remains pinned
to its own source pair; the intervening commits are documentation.

### Selection is not behavioral coverage

The 23 defined bits are in `crates/sophia-protocol/src/wm_rows.rs`.
Hagia's `src/sophia/wm_file_wire.nim::fileWire` requires bits 0–8 and 10;
profile activation adds 9, pointer-focus settings add 13, and output assignment
settings add 15/16. Its optional offer includes 11/12 and 14–22, minus anything
already required. Bit 13 is conditional, not offered unconditionally.

Session's `live_session/wm/public_policy/transport.rs` removes 18/19 from the
headless ceiling. `sophia-runtime/src/policy_capabilities.rs` then removes
dependent bits: 19 needs 18; 17 needs 14; 20 needs 1/6; 21 needs 20;
22 needs 19. Profile activation is supported only with the profile admission.
The runtime tests `selected_mechanisms_are_bounded_by_offer_ceiling_and_profile_admission`
and `each_dependency_is_removed_after_either_offer_or_ceiling_excludes_its_prerequisite`,
plus the three lifecycle/chord/held-capture tests in
`crates/sophia-runtime/tests/policy_capabilities.rs`, pin these rules.

The lifecycle's recorded mask is **3399679 (`0x33dfff`): 19 bits selected**.
Absent are 13 (profile did not request pointer focus), 18/19 (headless ceiling)
and 22 (dependency pruning). With a native ceiling and this same offer/profile,
source predicts `0x7fdfff`; requesting pointer focus too permits `0x7fffff`.
Those are computed expectations, not observed native negotiation records.

### Coverage by bit

In this table, **J** means a behavior exercised by the current three-case
external lifecycle run; **S** means current test source inspected, not rerun by
this audit; **P** means the separately recorded attended output acceptance.
A selected bit without a corresponding workload remains only negotiated.
Sophia paths prefixed `support/` are under `crates/sophia-session/tests/`;
Hagia paths are relative to its separate repository. Test filenames identify
reuse candidates, not a claim that every assertion was traced or executed.

| Bit / capability | Current owner and reusable checks | Joined evidence and remaining gap |
| --- | --- | --- |
| 0 BINDINGS, 1 ACTIONS | Session catalog/shortcut dispatch; Hagia `tests/tpolicy_model.nim` view-action cases; `support/chord_lifecycle_session.rs`. | J admits the catalog and shortcuts, but invokes no window action. Need a real action cycle through Hagia and Session settlement. |
| 2 MULTI_OUTPUT | Session output ownership; `support/policy_output_ownership.rs::partial_drag_projection_preserves_the_other_outputs_committed_content`; Hagia model cross-output movement. | P establishes occupied output migration/return in its exact release. J has one empty output; restart with occupied outputs is not covered. |
| 3 POINTER_INTERACTIONS | Session bounded gesture queue, `support/live_session/wm_session_tests/pointer_interaction.rs`; Hagia model interaction vocabulary. | S; no current external SDK drag/cancel join or measured drag latency. |
| 4 CHROME | Session configuration and Engine chrome; `support/policy_file_custody.rs::real_array_decoder_refuses_unnegotiated_chrome_before_semantic_delivery`; Engine `tests/chrome_layout.rs`. | J configuration admission, no chrome pixels or hit-test evidence. |
| 5 POLICY_DIRTY | Session cycle owner and Hagia checkpoint continuation. | J consumes real post-restore Dirty; disabled checkpoint restore fails the named restart control. Occupied state remains untested by that join. |
| 6 CONFIGURATION, 9 PROFILE_ACTIVATION | Protected launch/reload; Hagia `tests/tprofile_handoff.nim`; generic `support/policy_file_startup.rs`. | J real catalog, accepted replacement, rejected profile and rollback. This is the strongest current joined family, limited to empty policy state. |
| 7 SESSION_OPERATIONS | Session owns execution, Hagia sends intent only after settlement; Hagia `tests/tpolicy_wire.nim` agreeing/rejected expectation cases. | S and historical owner evidence. J sends no operation; need acceptance/rejection and exactly-once intent on the current pair without executing a real desktop operation. |
| 8 INDICATORS | Hagia projection, Engine strip/action identity; Engine `tests/indicator_chrome.rs::strip_layout_and_action_share_one_publication_identity`. | P workspace labels observed after return. No complete current SDK indicator/action identity join. |
| 10 LAUNCH_PLACEMENT | Hagia admission policy and Session layout; Hagia `tests/support/admission_focus.nim`; `support/live_session/wm_session_tests.rs::public_policy_admission_reconciles_to_the_engine_safe_extent_before_staging`. | S; J admits no surface. Include occupied initial management before recovery tests. |
| 11 TAB_GROUPS, 12 TRANSLATION_GROUPS | Hagia projection rows; `tests/twm_file_projection.nim`; Engine `tests/tab_chrome.rs` and `tests/translation.rs`. | S; no current external owner join for tab membership or translated occupied geometry. Wire round-trip tests alone do not prove rendering. |
| 13 POINTER_FOCUS | Session focus admission; `support/policy_active_focus.rs::pointer_focus_admission_uses_negotiation_and_never_activates_before_commit`; Hagia `tests/support/pointer_focus_policy.nim`. | Not selected in J. Needs a separately declared profile, then admitted focus and rejected/stale focus controls. |
| 14 LAUNCH_ORIGIN, 17 OUTPUT_LAUNCH_CONTEXT | Session origin registry; `tests/launch_origin.rs::delayed_child_freezes_origin_before_focus_and_source_placement_change`; Hagia `tests/support/launch_origin.nim`. | S; no launch in J. Test frozen origin through the current peer; do not treat process ancestry as authentication. |
| 15 OUTPUT_ACTIONS, 16 OUTPUT_POLICY_KEYS | Session output authority and Hagia output policy; `support/policy_combined_output.rs`; Hagia `tests/twm_file_arrays.nim` output-generation refusal and `tests/support/arrow_output_policy.nim`. | P covers physical topology return, not every output action or live mode/VRR transaction. J has no output service. |
| 18 SURFACE_INSTANCES, 19 PRESENTATION_ACTIONS | Session presentation owner, Engine retirement and Hagia presentation model; `support/policy_presentation_lifecycle.rs`, `policy_presentation_routing.rs`; Hagia `tests/twm_presentation.nim`, `toverview_adapter.nim`. | Absent in J. Generic tests and historical simulated completion are reusable, but no current SDK receipt-to-model-to-input join is qualified. |
| 20 ACTION_LIFECYCLE, 21 CHORD_ACTIONS | Session chord service; `support/chord_lifecycle_session.rs::a_chord_action_is_handed_off_as_a_cycle`; Hagia `tests/trecent_windows.nim`, `trecent_windows_replay.nim`. | Selected but not exercised in J. Need current real-peer begin/held/end/cancel, reconnect and credit/refusal joins. |
| 22 HELD_CAPTURE | Session presented-capture routing; `support/policy_presentation_routing.rs::a_held_capture_waits_for_a_held_application_key` and `a_new_owner_epoch_keeps_the_presented_held_rule`; Hagia recent-windows adapter. | Absent in J. Must join presented identity, chord ownership and release debt; an admitted proposal is not a presented capture. |

Base Snapshot/Projection, request/transaction/connection identities, settlement,
focus, restart and checkpoint custody do not each have a capability bit. They
remain required. `support/policy_file_recovery.rs::protected_c_sdk_recovers_after_stale_and_timed_out_projections`
uses a real protected C SDK connection but supplies timeout outcomes to the
settlement owner. `support/policy_file_custody.rs` tests actual reactor ACK
credit, bounded send and stop/ESTALE behavior. Neither is evidence that ordinary
Hagia has traversed every corresponding failure path on the current pair.
Hagia's `tests/tpolicy_wire.nim` uses a supplied wire: its expectation, receipt
and operation checks are local client evidence, not production transport runs.

### Next owner join: occupied settlement and recovery

Extend the external lifecycle fixture and runner, retaining the same repository
ownership and explicit overlay discipline. The smallest useful successor has
two opaque surfaces with different generations, a committed focus/placement,
and a real catalog action that proposes a distinguishable change. Prefer one
headless output initially; extra outputs are a separate extension, not a reason
to delay the first nonempty-state proof.

1. Commit initial management and an action through the ordinary Hagia process.
   Compare Session's committed layout/focus with the client's committed checkpoint;
   a proposal or an ACK is insufficient. No frontend pixels are claimed.
2. Stage another action, then force a stale scene and separately supply a timeout
   to the production settlement owner, following the generic C SDK boundary.
   Require the prior checkpoint and committed state to survive, a fresh request
   identity, and a later successful commit. Label the timeout as supplied; it
   does not qualify a real resize deadline.
3. Terminate only the fixture peer and exercise automatic and requested recovery
   with the occupied state. Require fresh connection/request identities and
   restored window membership, placement and focus after the actual Dirty cycle.
   A negative control must disable occupied checkpoint restoration or promote a
   refused candidate and fail on state, not merely on a missing log line.

The fixture must separate authoritative scene facts supplied by the test from
policy values actually produced by Hagia. Record that distinction with the
exact source, SDK, profile, binary, overlay and case list. Inspect the private
seam before implementation; if it cannot expose committed focus without
manufacturing the expected result, narrow the assertion or add a generic seam.
No live keyboard, GPU or session access is needed for this slice.

Then cover session-operation intent and terminal peer/credit paths, followed by
the native presentation/chord/capture join. That join needs its own declared
receipt and completion fixture; setting `native_scanout=true` is not native
presentation evidence. Keep simulated retirement clearly labelled and preserve
release-debt checks across reconnect, lock and lost heads. Existing local tests
should be reused, not copied wholesale into a client-specific Sophia suite.

The prospective measurement method remains required before a comparative
campaign. This map changes no budget, closes no task, and does not make a new
live release necessary. It advances the t249 prerequisite of t315 while leaving
the admitted t133/t275 observer design work independent, in alignment with
zsx0tk4k and the observer plan.

## Occupied settlement and recovery implementation (2026-10-10)

Hagia `0bbfe3cee3d1efa0b191098c70e64062ab8073b0` adds
`tests/external/occupied.rs` and extends the shared lifecycle helper.
niltempus `83023216817500f7595d5550f76d6b28016c99ba` mounts both external
modules and requires all five named tests. Its Sophia pin is
`9f52403be19346a49ad275481886de9a5d4015a3`; the mount and production code
are unchanged from the prior lifecycle pin. There are no Sophia production or
repository test changes, no Hagia production change, and no new installed release.

The clean signed-fixture run `t249-occupied-01` reports
`PASS lifecycle=3 occupied=2 native=false`. Its verified self-excluding manifest
is `7a340bac1e5b4dfc9cf4458162b6c5ab1fe4077afd8f1d31d5067dc0ce2946b7`;
ordinary Hagia binary SHA256 is
`2334a3754bdc59ea0f141a07a685c1c83aca1f1fa8a5ca86c9fb74213ad72538`.
SDK remains `b2a254dcb792e5f9d66f78bdd73f645153504507`. Both fixture modules,
source pins, profile, case list and per-proposal identities are recorded.

The two new cases use distinct generational surface handles and real catalog
actions. The settlement case first commits a focus change, then supplies a
timeout and separately advances the scene before refusing the staged proposal.
Committed layers, policy focus and checkpoint stay unchanged. The actual
recovery SceneChanged request does not replay the action; a later explicit
action has fresh request/transaction identities and commits successfully.
The restart case changes private column sizing, then exercises automatic and
requested replacement of the fixture-owned peer. Both restore the committed
policy projection, layout layers and checkpoint relationships, including focus,
window membership and column state. Only the epoch-scoped layer translation
identity is normalized for comparison. Each successor supplies its own Dirty.

**Fixture limit:** mapped authority facts and source-less layers are supplied.
The fixture completes layout directly through the commit owner; it bypasses
visual readiness, application admission, configure ACKs and rendering. Focus is
policy focus, not physical keyboard routing. Timeout is supplied, not a measured
resize deadline. Requested restart invokes the owner, not the public control
socket. This extends J coverage for occupied policy recovery; it does not qualify
native presentation, all capabilities, multi-output restart or latency.

Development records remain distinct. `t249-occupied-dev-01` exposed a fixture
checkpoint-read race: transport Ready can precede the peer's checkpoint rename.
The helper now waits for replacement and reads bytes/inode through the same
open file. Dev-02 compared cached pre-projection surface facts to a later fresh
snapshot; the setup now observes the newly committed geometry before restart.
Dev-03 expected the next explicit action before Session's owed recovery scene;
the test now checks that recovery scene explicitly. Dev-04/05 pass their named
development cases. None of these was a production repair or is relabelled as
the clean final run.

`t249-occupied-controls-01` binds the final test executable and fixture hashes
before/after both controls. The previously built no-restore mutant is reused
only after checking identical Hagia production trees; it fails on committed
placements/focus, before waiting for Dirty. A separately built mutant promotes
and saves refused candidates; it fails `refusal cannot promote client candidate`.
Both are exactly one named failure (exit 101), not compiler or timeout failures.
The verified manifest is
`6dcd65ec09d3bfffd2ef3c7cd9106eb0d7832bbfc8d2a13d0705180cc55eea60`.
`t249-occupied-checks-02` records strict Session overlay clippy, two runner
refusal/closure controls, runner clippy and formatting, all passing; manifest
`2855f118dd35575247f03425285c8fe1dfc1bca1395b79c973ac9a685beec5cb`.
The full Nim/formal/product gates were not repeated for external tests-only
changes. Source and build caches are excluded from these evidence manifests;
the declared source identities, overlay files, binaries and records are covered.

The operation/disconnect successor below covers the next intent boundary.
Slow-peer/credit handling and the native presentation/chord/capture join remain.
The prospective measurement method and original budgets remain outstanding.
t249/h006 stay open; this does not expand observer authority or reopen t310
physical acceptance.

## Operation settlement and disconnect implementation (2026-10-10)

Hagia `67da6edfb2c5932b59f828b452ab9bf4e17236fe` adds external
`tests/external/operations.rs`, reusing the occupied fixture's supplied facts.
niltempus `7eff07b728a41a7813bd29fb850999f222eb5d09` binds all three module
hashes and eight exact test names. Sophia is pinned to this investigation's
parent `ea64b1f027ace8b9064935d966697acfac37d004`; only documentation changed
from the previous pin, with identical mount and owner APIs. No client-specific
code entered Sophia and no production code, release or installed process changed.

`t249-operations-01` reports
`PASS lifecycle=3 occupied=2 operations=3 native=false` on clean signed Hagia
source. Its verified manifest is
`84e927141e8887cd9eb0ca66b7f317f9edf47aee0663110d5c139fca7dc83189`.
The new cases establish:

- A real admitted terminal action first commits its projection; Hagia saves its
  checkpoint, then sends the operation. The request names the activation serial,
  deliberately distinct from the projection request ID. Session returns the
  expected typed launch intent on acceptance and none on supplied refusal.
  The tuple stays local: neither the operation queue nor executor is invoked.
- Both operation outcomes permit the same peer's next fresh cycle without
  changing the committed layout or checkpoint. A refused action instead owes
  recovery SceneChanged; a fresh explicit action can subsequently send an
  operation.
- Terminating the fixture child while a projection or operation is unsettled
  exercises automatic restart. The new epoch clears pending intent and expected
  slot, preserves the checkpoint, restores occupied placements/focus through its
  own scene/Dirty exchange, and accepts a fresh explicit action. The obsolete
  proposal is discarded, not settled into the replacement owner.

`t249-operations-controls-01` compiles two server mutations in the development
archive. Accepting a refused operation fails “refusal must not return an
executable intent”; keeping pending operation across restart fails
“disconnected operation must lose its authority”. Each is one named runtime
failure, exit 101, with source restored afterward. Manifest:
`b597e4e71aa96e00019713d12af0e50f6a92d6c3fb09ab5b1df409f49be6a78f`.
The three external module files and overlay hash match the final run exactly;
Hagia's production `src`, `vendor` and `hagia.nimble` Git objects match too.

`t249-operations-checks-01` passes strict Session overlay clippy, then stops
after layout succeeds because Nimble tries to save metadata in the read-only
sandbox. Its manifest is
`5c60c5999c026ed8fdd0f3cb55de9581ac5c1f84049b185ce452c71758a9e1e8`.
The successor `t249-operations-checks-02` uses the layout script directly and
passes layout, two runner controls, strict runner clippy and formatting;
manifest `d822a3c6639d8e92d32d479c1e5b25727fa7a36d1164b492ef9144680889abd3`.
All manifests verify and covered records are read-only. The development run
is preserved separately from the clean signed-fixture run.

This extends joined bit-7 evidence through Session's typed-intent boundary and
lost-peer recovery. It does not prove application execution, exactly-once
external effects, exhaustive wire output, real resize deadlines, public
control-socket restart or physical input. Layout completion and occupied
authority facts are supplied; native receipts remain absent. Full Nim/formal
and product gates were not repeated for external tests only. t249/h006 remain
open for slow-peer/credit coverage, presentation/chord/capture owner joins and
the reviewed measurement method; original budgets and observer scope are unchanged.

## Connections

- [t249 plan](../plans/80blhke8-migrate-the-hagia-wm-role-to-admitted-9p2000-l-files.md#t249) owns the unchanged acceptance requirements.
- [WM reactor investigation](87juczar-wm-file-reactor-waits-behind-the-driver-command-channel.md) preserves the measured failure and scheduling correction.
- [Observer delivery plan](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#first-deliverable-and-dependency-order) separates prerequisites from the repository queue.
- [t310 acceptance](ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md) owns the current occupied-output evidence.

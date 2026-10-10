---
id: 5rzn1zue
date: 2026-10-10
kind: investigation
status: investigating
tags: [validation, protocol, policy]
---
# WM qualification after source retirement and attended output recovery

## Reboot handoff (2026-10-10)

Resume update: B and A below are integrated. Read the
[runner review](#post-reboot-runner-review-2026-10-10) and
[revocation results](#revocation-reaches-the-switcher-model-2026-10-10)
before starting another job, followed by the
[captured-disconnect results](#captured-release-debt-survives-replacement-2026-10-10).
The subsequent [matching-loss cancellation result](#matching-loss-cancels-the-presented-chord-2026-10-10)
extends the terminal-path coverage without closing t249.
The [pending-settlement abort result](#pending-settlement-abort-precedes-replacement-2026-10-10)
then checks the barrier before automatic replacement.
The [fourteen-case coverage reconciliation](#fourteen-case-coverage-reconciliation-2026-10-10)
supersedes the older three-case capability map and records the passing current
SDK/export gate. The [Manage settlement successor](#new-window-manage-settlement-2026-10-10)
then covers the first missing admission owner boundary, through AwaitingPixels.
The [CPU admission successor](#cpu-backed-new-window-admission-2026-10-10)
continues through Managed, production CPU intake and headless pixel checks.
The [mirrored receipt successor](#mirrored-receipt-consensus-2026-10-10)
joins backend-derived Presented and Withdrawn receipts to real Hagia policy.
The [source-only repaint successor](#source-only-preview-repaint-2026-10-10)
uses retired instance snapshots to inspect the preview's captured source generation.
The coverage baseline remains revisable without reducing the acceptance scope.
The old worktrees have been removed. The original
reboot checkpoint follows for provenance.

User requested a reboot checkpoint.
Root directed Claude wB:p2 to stop and preserve unfinished work.
Run `zk index --quiet` after reboot; root stopped its slow index pass to avoid
delaying shutdown. The Markdown handoff itself is committed independently. The installed
desktop is unchanged by this lane; do not install, reload or repeat physical
monitor tests as part of resuming these external tests.

Completed and pushed masters before this handoff: Sophia `5e8ecbc43`, Hagia
`08611fc9`, niltempus `74506a45`, all signed and clean. The ten-case presented
chord join and its two discriminating controls are integrated; its results and
limits appear below. Finished worktrees from that slice were removed.

Outstanding work is **B, runner resources, then A, revocation-to-model**:

- niltempus worktree `/home/niltempus/dev/niltempus-t249-runner-slots`, branch
  `test/t249-runner-slots`: signed baseline `ec3c4cfb3eab7104557a3e135ca38db71be59b37`
  is not merged. It separates builds from evidence and uses cargo-slot, but
  root review found two blockers: no independent build-root lease across run,
  reuse and cleanup; and protected-root ancestor overlaps were not refused.
  Signed WIP correction is `6fc1cf080283a722f349e1acbb80373344263249`, clean.
  It adds an external nonblocking build-root lock, two-way protected-path
  checks, and failure-record handling. Claude reports nine focused tests,
  strict runner clippy and busy-root/ancestor probes passing; root has not
  reviewed or independently qualified the correction. Documentation still
  needs its lease/protection update. No product run or new evidence package
  exists for this slice; scratch checks are reported development checks.
- Hagia worktree `/home/niltempus/dev/hagia-t249-revocation`, branch
  `test/t249-presented-revocation`, was clean at `08611fc9` on root's checkpoint
  inspection. A has not run. Do not interpret its existence as test completion.
- Sophia pin `/home/niltempus/dev/sophia-t249-rev-pin` is clean and detached at
  `f77244abe`, the existing runner's code pin. Newer Sophia commits are notes;
  do not move the pin casually or rebuild the installed desktop.

Claude's durable restart record is
`/home/niltempus/.claude/projects/-home-niltempus-dev-sophia/memory/project-t249-revocation-restart.md`.
At handoff he reports no active jobs, no build root and no held cargo slot.
The only probe lock is an unlocked job-tmp file; no resume dependency may rely
on that temporary file or its test binary surviving reboot.

First resume action: update B's docs, read Claude's restart record and inspect the runner
correction against `ec3c4cfb`. Require a stable external build-root lock shared
by runner, controls/checks and cleanup, busy refusal without mutation, protected
root overlap checks in both directions, and closed failure reporting. A pool
path is not lease proof; the cargo-slot wrapper holds that separate lock.
Review focused controls before A execution. Nothing unreviewed goes to master.

A's agreed boundary is one new `sdk_presented_revocation_closes_switcher`
case in a child module, reusing the first Held/Presented prefix without a Right
press or outstanding debt. Supply a completed frame without its stamp; observe
Revoked with the original identity, then the **first exact SceneChanged answer**
with no presentation and unchanged W1 focus. Only after that observation supply
the Withdrawn frame. Ended must restore chord credit without changing focus.
The omitted-Revoked archived-server control must fail a named semantic assertion
on that answer, never merely a timeout, disconnect or later retry. Do not allow
Withdrawn to mask the omitted Revoked. Disconnect/debt, cancel, AbortSettlement,
native retirement/pixels and latency remain separate open work.

A has one newly identified evidence limit: Session supplies no refusal-reason
record for this staged-projection rejection. The fixture may observe explicit
settlement without a proposal with transport healthy, but stale-generation
causation remains a source inference unless independently exposed. Root must
review how first_answer binds that settlement to the exact request before
execution; do not add production observability merely to make the test pass.

Resource rules: all Cargo via `cargo-slot sophia`, fixed pool only; no new
per-task targets, copied target trees or built binaries in evidence. Keep build
products in a marked scratch root outside evidence and retain it only until
controls/checks finish. Preserve old frozen evidence unchanged. Use at most four
jobs, nice 10 and no debug/incremental output. No whole-product gate is requested.
After review/integration, remove merged worktrees with `git worktree remove`.

Architectural direction is still
[zsx0tk4k](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md)
and [ernn0bkv](../concepts/ernn0bkv-plan-9-integration-points-for-sophia.md), with
[jsschoen](../plans/jsschoen-converge-public-roles-on-one-9p-core.md) the delivery
spine: t249 qualification plus admission/recipe design, then admitted composition,
portal grants and the observer capture CLI. t307/QEMU remains off the critical
path. Admission exchange and recipe review are in esnqxpqw/kcfh2hdg; no new
admission implementation was authorized by the test work.

## Post-reboot runner review (2026-10-10)

Resumed t249 at the checkpoint above and refreshed the zk index. Independent
development checks of niltempus `6fc1cf08` passed all nine runner tests, strict
runner Clippy and formatting. External `flock` probes confirmed busy run and
cleanup refusal without root mutation, protected ancestor refusal, and cleanup
manifest verification after lease release. These were scratch development
checks, not a product qualification package.

Independent Claude review found an uncovered interruption case: the runner's
close-on-exec lock is released if the runner dies, while GNU timeout places the
build/test supervisor in a separate process group and can keep using the
archive. A second run can then acquire the lock and replace that archive.
The earlier passing checks do not qualify this case. The correction requires
the supervisor to retain the lease through its work and a control that kills
the runner while the step remains alive. External controls must also avoid
calling the self-locking runner while holding the same lock themselves.

The accepted correction is signed niltempus `20ff4c0`. Only build/test
supervisors and the `git archive`/`tar` source writers inherit the lease;
read-only helpers retain close-on-exec behavior. Thirteen serial tests pass,
including three controls that kill the lease-owning parent while each writing
child remains active. Removing inheritance in a compiled scratch mutant makes
all three fail when the second lease wrongly succeeds, with exit 101. The
root's control cleanup observes child completion before deleting its scratch
roots. Runner Clippy, formatting and whitespace checks pass. Parallel test
execution exposed transient lock inheritance across unrelated fork/exec calls;
the documented suite is serial. This is a test-execution limit, not evidence
that the single-threaded runner drops a held lease.

`t249-runner-resources-01` preserves the incomplete 11-test development state
as STOP; its manifest is
`280237859b8d6f7a052129935481823cb7a1b9298ddd610c15ae88bf96f3088c`.
`t249-runner-resources-02` qualifies the corrected runner, manifest
`2572375df419942fdb9697e615514c0daeb4b8cc18c1e469a251a67ca9b8c3c0`.
Both manifests independently verify. Records contain source, logs, paths and
hashes, with no copied compiled artifacts. A separate reviewer control removed
scratch roots after its timeout but before confirming process exit; that
cleanup is not qualification evidence, and the reviewer subsequently confirmed
no surviving processes. The independently retained root controls supply the
qualification above.

No A case or product run had started at runner acceptance. Revocation's first
answer must bind to the captured request identity with subsequent cycle
admission disabled. Settlement without a returned proposal is observable;
its precise refusal reason remains a source inference. The t249/h006 exits and
the installed desktop remain unchanged.

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

This three-case snapshot is superseded for present coverage by the
[fourteen-case reconciliation](#fourteen-case-coverage-reconciliation-2026-10-10)
below. Its inspected sources and original findings remain historical evidence.

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

## Stalled live peer recovery (2026-10-10)

The read-only follow-up separated command/event queue pressure, response
deadlines, journal ACK credit, waiting reads/revocation and process restart.
They are different owners and bounds. The command and event queues have
capacity one. The response deadline is twelve seconds; sending against a full
journal has a separate four-second deadline. The journal allows 64 records
within 1 MiB. ACK consumption advances its floor; revocation answers pending
reads with ESTALE before closing.

Source review found generic coverage in `policy_file_custody.rs` for real
reactor ACK release, bounded full-journal send, stop while waiting for credit,
and ESTALE before close; driver, adapter and shutdown tests cover their own
queues and wakeups. SDK-local scripted tests cover held-event ACK ordering,
partial-event deadlines and terminal ticket custody. Those tests were inspected,
not rerun here. The previous external cases terminated the peer first. The
missing composition was the production response deadline requesting replacement
of a peer which was still alive.

Hagia `651763e2dff1102a0b7380b3347ce7fc19c44fea` adds external
`tests/external/slow_peer.rs`; niltempus
`e441c7fb6b8cd4738ebc3d34fbc51df48d664b85` mounts and hashes that fourth module
and requires the ninth exact test. Sophia is pinned to `65440ff5d`; the change
from `ea64b1f02` is documentation only, with the same mount blob and owner code.
There is no production-code or installed-release change.

`t249-slow-peer-01` reports
`PASS lifecycle=3 occupied=2 operations=3 slow_peer=1 native=false` with clean
signed fixture source. Independently verified manifest:
`537e570a65ce84623dbd00684f10ef304752a4bc1b25955a5cd7aabac961a390`.
The fixture proves its protected parent/child chain, records the peer's start
time and pauses only that process with SIGSTOP. It observes the stopped state
before issuing an action. The unmodified deadline then requests restart while
the supervisor reports no process exit and the peer remains stopped. The
captured reason is `WM file response deadline expired`. Committed layout,
policy and checkpoint remain intact. The new epoch clears request, operation,
deferred-command and staged authority; the old peer must disappear, with a
zombie still counting as present. The successor completes its scene/Dirty
recovery and a fresh explicit focus action.

Observed detection was 12051 ms and replacement 29 ms. These are one-run
observations, not latency qualification or proof of an owner-loop stall. The
actual path is `sophia_runtime::ProcessSupervisor::terminate`, not Session's
`terminate_session_child`. The fixture's same-start-time drop guard resumes a
surviving paused peer; the outer PID namespace and timeout bound failure cleanup.
`session-lines.txt` captures lines through deadline detection, not the entire
replacement/recovery lifetime.

`t249-slow-peer-controls-01` modifies only archived server copies. Ignoring both
Failed and the disconnected-worker indication hits the named forty-second
stalled-peer bound. Retaining `in_flight_request` across restart fails the
immediate stale-authority assertion, not a later Ready timeout. Each compiled
control exits 101 with one named failure, and not the other control's message.
Manifest `9172a63d50bb671fb8d3250e45898d7550122d261bc4e6c2eecf9935058ba4a3`;
the bound inputs match at start/end and the archive sources are restored.
The earlier unsigned development run is kept separately as
`t249-slow-peer-dev01`, manifest
`e0245cf81c6a9410c3231513b02358e27535699db343a200e753dbdfd06e1305`.

`t249-slow-peer-checks-01` verifies the two mutated production files against
the pinned blobs, and passes strict Session overlay/runner clippy, two runner
controls, formatting and Hagia layout. Independently verified manifest:
`f35280c03cd458794dce6b5124dd4cd4513419bed6d27faa145d66774a2db098`.
There was no test rerun or full product/Nim/formal gate for this external slice.

Occupied facts and layout completion remain supplied. This case does not
exercise AbortSettlement, a paused peer consuming ESTALE, ACK-credit exhaustion,
slow-but-progressing throughput, application execution or native presentation.
The headless request/response cadence does not naturally fill the 64-record
journal; whether presentation receipts can reach that bound requires a separate
reachability review, rather than an artificial headless flood. Next is the bounded
presentation/chord/capture owner-join design, including receipt credit and its
terminal paths. The measurement method remains separate. t249/h006 stay open.

One source-only question remains separate: a full Cycle command queue returns
an error from `poll_public_request`. The surrounding deferred-command and
receipt guards appear to prevent reaching that branch, but this slice does
not prove that invariant or reproduce a session failure. Any follow-up belongs
in generic Sophia queue tests, not a Hagia-specific production change.

## Presentation-join feasibility review (2026-10-10)

Read-only audit of Sophia `f77244abe`, Hagia `9fdf0b360` and niltempus
`3696e497b`; no new test, build or device evidence. The proposed tenth external
case would select the native capability ceiling without a device, drive real
Hagia chord Held/Ended and presentation actions, and supply head identities,
router time and completion stamps. Existing nine cases retain their headless
ceiling. Supplied completions cannot establish native retirement or pixels.

The first precondition failed by source inspection, before implementation.
Hagia's `recent_windows_presentation.nim` emits SurfaceInstances for the real
window previews. Runtime `validate_policy_presentation` requires each source
in both displayed and committed surface sets. The current occupied fixture
supplies source-less layout facts and never submits them to the visual runtime;
a fresh runtime has neither set. Its real publication would therefore fail
preflight with MissingSource. Generic Session presentation fixtures with empty
instance arrays do not close this gap. No publication was narrowed, validation
bypassed, worktree created or test result claimed.

The next authorized step is a read-only feasibility check for an existing
production CPU-intake path supplying three owned surface buffers with matching
identities. A positive case must establish those sources through ordinary
intake/commit, not direct mutation of runtime registries. A new driver,
cross-crate test API or application-launch framework is outside this slice.
If reuse is feasible, source lifetime, real Hagia recent-focus ordering and
preflight must be checked before the presented-input fixture is implemented.
A refusal-only case would be a separate obligation, not a substitute.

Receipt claims also need narrower wording. Hagia drains and ACKs receipts in
its wire loop, but Presented has no model effect; Revoked can close its
switcher. Empty Session queues and successful later cycles show progress,
not an observed ACK for a particular receipt. A source-only bound calculation
suggests one stopped-peer publication can yield at most three receipts per
output (48 at 16 outputs), plus a Cycle, below journal capacity 64. This depends
on the publication and phase assumptions and is not a general unreachability
proof. Keep generic journal-exhaustion evidence separate; do not construct an
artificial receipt flood or close ACK obligations from that arithmetic.

The intended controls remain completion-to-capture disabled in an archived
Session and Ended ignored in a Hagia mutant, each failing a named semantic
assertion. Dropping Presented receipts would not discriminate client model
behavior. Revocation-to-model, disconnect with held release debt, cancellation,
AbortSettlement, native stamps/pixels and latency remain separate open joins.

## Presented chord and capture join (2026-10-10)

The source-less precondition gap above is resolved using existing APIs. Hagia
`3960c83752743805cfbe3a17843296c6fef4947e` adds the tenth external case;
niltempus runner `beb125b744f0d18467c16b9bb5dc233ac45409e6` pins Sophia
`f77244abe`. No Sophia production code changed. One ordinary CPU production
cycle commits three owned buffers through `production_authority_batch`; no
runtime registry is directly populated and no alternative commit path is used.
The nine preceding cases retain their non-native ceiling.

The real Hagia Held publication fails with MissingSource on an empty runtime
and MissingHeads without a target, then passes unmodified preflight with those
sources and a supplied head. Before completion it has no receipt or actionable
capture. Supplied completion enables a production-routed Right action with the
exact presentation identity, creates release debt, and settles it on release.
Repeated completion produces no duplicate receipt. Hagia republishes, the old
action identity is refused, and receipts follow Revoked/Withdrawn/Presented
ordering. Ended returns chord credit and commits W3, derived beforehand from
checkpoint window order and focus and cross-checked against preview ordering.

All five manifests below independently verify under the development-evidence
root; these are their SHA256SUMS file hashes:

| Package | Result | Manifest |
| --- | --- | --- |
| `t249-presented-dev01` | Preserved STOP: final Ready preceded the last receipt flush. | `e541760f361bd384442187e71feb01baa6b985605d1b6930b6849d0ae7706d4d` |
| `t249-presented-dev02` | Unsigned development fixture, ten cases pass. | `baf738cd98c37a2852d9fa993d857e9bbe4584e69720524583fb8614d16361ce` |
| `t249-presented-controls-01` | Two compiled controls fail their named assertions; inputs restored. | `836e6638d74f264bb0b4ad91f169a5529821e6e2d48208939f312f1e02e4ff79` |
| `t249-presented-01` | Clean signed fixture, ten cases pass. | `32e64d8b45db223c74f0a1a0d3f902070c666a8341177810d479bf1d6b11b623` |
| `t249-presented-checks-01` | Overlay/runner clippy, three runner tests, fmt and Hagia layout pass. | `a2ef23105d82f86fbb0ff03621b73655b1a0801bba485f0b9bd5593fca85fcfa` |

The fixture correction drains receipts through bounded owner polls with new
cycles disabled. It proves transport progress, not a peer ACK. Three additional
scratch development attempts were disclosed; they are not qualification records.
The final fixture, overlay, glue and runner hashes match dev02. Production Hagia
source is identical across those builds; executable bytes differ, so no binary
reproducibility is claimed. Clean-run executable SHA256:
`7cff9718f8ed280ecd334975778a4cdc02bc91cee8636197e93c57563f21e695`.

F1 removes completion in the archived Session and fails at “presented switcher
must take the captured key”; W2 is a predicted later outcome, not observed.
F2 ignores Ended in an isolated Hagia mutant and fails at “released chord must
commit the switcher selection”: actual W1, expected W3. Both exit 101 with one
named failure. The original sources are restored and control inputs match.

Review compared the copied dispatch to production: the reached three arms use
the same owners, with fixture assertions replacing refusal logging. Other input
arms are rejected by the fixture. The shortened presentation service omits
availability/stopping and runtime-revocation branches and installs separately.
Pinned span hashes guard reference drift; they do not prove glue equivalence.
The 1,015-line test is larger than estimated but remains one disclosed fixture;
no production test API or driver was introduced. Release debt uses Debug plus
routing behavior because the owner has no public debt accessor.

This qualifies semantic joins with supplied CPU content, heads, stamps, time
and layout completion. Pixel admission/visual readiness is bypassed; displayed
membership is established by validation, not an independent set read. There is
no native retirement, pixel, physical-input or latency evidence. Presented has
no Hagia model effect. Revocation-to-model, disconnect with release debt, cancel,
AbortSettlement and receipt ACK reachability remain open. t249/h006 are not
closed. No full product gate, release, installation or live-session operation
was repeated. Future runs must use cargo-slot and keep build products outside
evidence under the updated global resource rules; frozen historical records
are preserved.

## Revocation reaches the switcher model (2026-10-10)

Hagia `22908aa070d14bc4226e65945f771426155ca944` adds
`sdk_presented_revocation_closes_switcher` in a child of the external
presentation fixture. niltempus `95c5c83` requires eleven individually listed
cases and binds all six fixture modules. Sophia remains pinned at `f77244abe`;
no production implementation, release pin or installed process changed.

The two presentation cases share their original prefix through Held. The new
case supplies completion without pressing Right or creating release debt,
then supplies a completed frame whose policy content remains visible but whose
publication stamp is absent. Session revokes the original presentation identity.
The first SceneChanged request is captured at admission; subsequent polling
disables cycle admission and requires the same epoch, healthy transport and the
exact request. Its answer must contain no presentation and preserve W1 focus.
Only after that answer commits does the fixture supply Withdrawn. Ended returns
chord credit and still preserves W1 in the proposal, committed state and
checkpoint.

The development run `t249-revocation-dev01` passes all eleven cases, including
the unchanged captured-Right behavior after extracting the shared prefix. Its
manifest is
`2b665e30cbc50d191a181760c90a01d478ab1c651593585914ef0d48c60d6b19`.
`t249-revocation-controls-01` omits only Revoked transport delivery in the
archived Session, retaining local revocation and withdrawal state. It fails
exactly one named assertion on the first answer, with exit 101; manifest
`df25107e836251b42c70dc82713219f658ae4c618a5540f80f06030b476b9e62`.
Both observe request 5, scene generation 2: the positive returns a proposal;
the control settles without one before any Withdrawn frame or retry. Neither
timeout nor disconnect satisfies the control.

Strict overlay Clippy initially rejected the fixture's large answer enum.
`t249-revocation-checks-01` preserves that STOP, manifest
`67a8e2b75c3407fd18c1b7d85b9d4b55594bebc6d26da69d443b36685bb85a4e`.
The signed fixture boxes the proposal value without changing the assertions
or owner calls. These development records precede that representation change.

The final signed run and repeated control bind identical fixture bytes:

| Package | Result | Manifest SHA256 |
| --- | --- | --- |
| `t249-revocation-01` | Clean signed fixture, all eleven cases pass. | `54aa50e50d2dcf44ab7bef253bdacb63f8df764a46173d20dbb7a5de4ace9b34` |
| `t249-revocation-controls-02` | Omitted Revoked fails the named first-answer assertion, one failure, exit 101. | `77cbf1cd6057d3676799a510afab167f0a0f5afc193b674a350278a7b4ee2d0b` |
| `t249-revocation-checks-02` | Restored Session overlay passes strict Clippy, including tests. | `439ba8df712112af0f38173c99e238e18abb201c5c2c3ecc1e4a68ced7f3d515` |

All manifests independently verify. The control holds the external build-root
lock and a cargo-slot lease, restores the archived server bytes, and confirms
the Hagia binary and all fixture modules are unchanged. The final run uses
the signed Hagia fixture above and niltempus runner `95c5c83`; production Hagia
source is unchanged from its baseline. Root also checked the updated runner's
thirteen serial controls, strict runner Clippy, Rust formatting, whitespace,
and Hagia's `nimble layout`. No full product gate was repeated. Binaries and
build caches stay outside evidence; paths and hashes identify them.

After controls and checks ended, the runner removed the marked build root
under its own lease. `t249-revocation-cleanup-01` passes and independently
verifies, manifest
`edd860bbe0be39366accaaf11eb17647e9d60cac41c86199c835a0236314e928`.
The stable sibling lock remains; cargo slots and frozen evidence were not
removed. Hagia `22908aa` and niltempus `95c5c83` are integrated and pushed to
their masters. Their merged worktrees and the detached Sophia test pin were
removed after inspection; the source identities and evidence remain recorded.

The observation is exact-request settlement without a returned proposal.
This staged-projection rejection exposes no refusal-reason record; the
same-generation presentation rejection remains a source inference. The
`stale_responses_delta=0` observation counts only scene-advanced rejections and
does not identify the reason here. Supplied content, heads, completion and
router time retain their earlier limits. Native retirement, source-loss/pixel
qualification, disconnect with release debt, cancellation, AbortSettlement,
receipt ACK reachability and latency remain separate work. t249/h006 stay open.

## Disconnect with captured release debt: source review (2026-10-10)

After integration, the next bounded owner join is automatic WM replacement
while a captured Right press still owes its release. This is a source-only
feasibility review of Sophia `f7f7e4141`, Hagia `22908aa` and niltempus
`95c5c83`, not a new passing case. The reviewed restart, capture and key-routing
files are unchanged from the existing Sophia fixture pin `f77244abe`.

The existing fixture can reach this boundary without a production test API.
Reuse `held`, supply completion and route Right down through the same capture
owner as the presented-chord case. Dispatch that captured action and obtain its
exact proposal, but do not commit it or release Right. Require no pending
layout settlement before terminating only the fixture's supervised peer; this
keeps AbortSettlement outside this case. Save the checkpoint and committed
layout before termination. Poll automatic restart with bounded waits until the
epoch advances, then discard the old proposal without settling it into the
successor.

`wm/public_policy/restart.rs` retains the public state object, calls
`presentation_capture.revoke()`, revokes presentation input and clears the old
epoch's chord ledger and router chords. Engine's `PolicyInputCapture::revoke`
clears action targets inside debt entries without deleting the entries.
`reset_chords` returns credits while retaining the router's physical key state.
The assertions belong immediately after replacement, before recovery cycles:
Right debt survives, old presentation authority and staged/in-flight work are
gone, chord credits are full, and committed layout/checkpoint are unchanged.
The old captured action must not enqueue a request in the new epoch.

After the successor becomes Ready, route the owed Right release using supplied
withdrawn projections. Require no policy action, no ingress and zero
`keys_suppressed_no_focus`, then require the debt to be gone. Route an additional
unowed Right release as a contrasting observation: with this fixture's empty
input focus it should reach the ordinary no-focus path. Zero ingress alone
cannot distinguish swallowed debt from an event dropped for lack of focus.
The generic Engine test `unbound_modal_key_release_remains_consumed_after_close`
already covers local revocation, but does not exercise real-peer replacement
or Session routing; it was read, not rerun. Recover the occupied checkpoint
through the replacement's own SceneChanged/Dirty exchange and exercise a fresh
action only after releasing the old physical keys.

The discriminating archived-Session mutation is to replace the restart call
`public.presentation_capture.revoke()` with
`public.presentation_capture = Default::default()`. It should fail the named
post-restart debt assertion; without that assertion it should expose the owed
release to the no-focus path. Merely omitting `revoke()` is not a valid negative
control for this keyboard case: keyboard capture stores `None` as its debt
target already. Pointer target invalidation needs a separate case. The new
case and this compiled mutation have not been implemented or executed.

This design preserves the existing supplied CPU content, heads, frame stamps,
router time and layout-completion limits. It does not establish application
delivery, native withdrawal, receipt ACKs, pointer debt, cancellation or
AbortSettlement. No builds, product runs, hardware access or installed-session
changes were made for this review. t249/h006 remain open.

## Captured release debt survives replacement (2026-10-10)

Hagia `ad3ec6601f145fd333576501fcb1b5cb2f8e7d35` implements the preceding
design as `sdk_presented_disconnect_preserves_release_debt` in a child of
the external presentation fixture. niltempus
`8095269602daac885d17f300141b57e4f300128d` lists twelve cases and binds all
seven module hashes. Sophia stays pinned at `f77244abe`. There is no production
code, release or installed-session change.

The case presents the real Held switcher, captures Right and obtains its exact
PresentationAction proposal without committing it. With no pending layout
settlement, termination of only the fixture peer triggers automatic replacement.
Immediately after the epoch advances, Right debt remains while old presentation,
staged request and chord authority are gone. Credits return, and the committed
layout and checkpoint remain unchanged. The old captured identity cannot enqueue
work in the successor. With supplied withdrawn projections, the owed release
produces no policy action, no ingress and zero no-focus suppressions; its
duplicate produces one no-focus suppression. The old Alt release emits no
Ended into the new epoch. The successor answers its own scene/Dirty exchange,
preserves W1, then a fresh routed Alt+Tab chord commits W2.

The unsigned development run `t249-captured-disconnect-dev01` passes all twelve
cases, manifest
`79ae83e40d9667b1fc331c127ef796c614c4641972f49a685cd147f37c89e0f1`.
`t249-captured-disconnect-controls-01` replaces only restart's capture revoke
with an empty capture in the disposable Session archive. It fails exactly
“disconnect must preserve captured Right release debt” immediately after the
epoch changes, exit 101 with one failed test; manifest
`b88ff3d9210a10a7d65020222ffcd3253ef40a8ffc90dc8f7e47ccf3ffcd026d`.
The archive is restored before `t249-captured-disconnect-checks-01` passes
strict Session overlay Clippy, including tests; manifest
`fe74c9fb9f6644405e8fbf2857bc694422dad6305aaea6ebc6c9daf4d177c832`.

The final clean signed-fixture run and its repeated same-binary control/checks
all bind identical fixture bytes:

| Package | Result | Manifest SHA256 |
| --- | --- | --- |
| `t249-captured-disconnect-01` | All twelve cases pass. | `93e47264e100d90e27e6eee1cbf57d8cda9cfa2ae87b7d27d9b970e1914d1135` |
| `t249-captured-disconnect-controls-02` | Cleared debt fails the named post-restart assertion, one failure, exit 101. | `ca182c476eb4558795392227835b1f5d877e32c8378968e18eeba9f866a3e8c1` |
| `t249-captured-disconnect-checks-02` | Restored Session overlay passes strict Clippy, including tests. | `f098edeb5518f72e352e05928941080cdd61832985d5d63c2cc45e6f06ebb7a2` |

All six manifests independently verify. Controls/checks hold the external
build-root lease and a cargo-slot lease; the final archived restart source was
also independently compared with the pinned Git blob. All seven fixture modules
match the signed source across all six packages. Hagia production source, vendor
and build definition are unchanged. Separate builds do not claim reproducible
binary bytes. Development checks also pass thirteen serial runner tests (one
internal helper ignored), strict runner Clippy, Rust formatting, whitespace
checks and Hagia's layout gate. No whole-product gate was repeated.

Hagia `ad3ec66` and niltempus `8095269` are merged and pushed. The runner removes
the marked build root after all controls and checks finish;
`t249-captured-disconnect-cleanup-01` passes with independently verified manifest
`c02a9c59f1d41eef6e4069df3eec75e64778ee4ba86336b144cc36842e844102`.
The stable sibling lock and cargo slots remain. Merged worktrees and the detached
Sophia pin are removed after inspection; evidence contains records, not binaries
or build caches.

Independent read-only review found no blockers. The positive run observes both
release-routing counters. The mutation stops at the debt assertion, so the
predicted later no-focus behavior with that assertion removed remains source
inference. Fresh-chord assertions check its event kind, Released terminal event
and W2 outcome, without explicitly correlating every event's chord token.
The runner's historical `presented_case` identity field names the original
case; the complete required set is bound by `tests.txt` and `overlay.sha256`.

This is keyboard debt with empty input focus and supplied content, heads,
completion, router time and layout settlement. It does not establish application
delivery, pointer debt, native withdrawal, source-loss/pixel qualification,
receipt ACKs, cancellation, AbortSettlement or latency. t249/h006 remain open.

## Matching loss cancels the presented chord (2026-10-10)

Hagia `7bca59cc04a6a0b37b86bad9d2ad88c05f8cf47b` adds
`sdk_presented_cancel_preserves_focus` in an external child module. niltempus
`066ec9373731c5697b27b795d00ab63304b64163` requires thirteen cases and binds
eight fixture modules. Sophia remains pinned to `f77244abe`; no production
source or installed release changes.

The fixture supplies the keyboard-matching true-to-false transition to
`LiveWmSession::observe_keyboard_matching`, then calls `service_shortcuts` in
the same order as the production owner loop. The real router emits one
Ended(Cancelled) for the token of the presented Held chord. The request's
activation serial, action and count must match that chord's ledger record;
the epoch and peer remain unchanged. Hagia's answer must close the switcher
and retain W1 before any Revoked or Withdrawn receipt can mask cancellation.
Chord credit returns; the committed focus and checkpoint retain W1. Only then
does the fixture supply withdrawal, restore matching and route the old Alt
release, which must emit no second terminal. A fresh opener has a distinct
token, and its matching Released terminal commits W2.

The unsigned development run `t249-held-cancel-dev01` passes all thirteen cases;
manifest `ec825ff5b1f4720906855e2c415d983bd3ded3b7ce932c7b7e0922e5279a2fae`.
Independent source review found no blockers. The repeated nonmatching call
observes no duplicate terminal; it does not discriminate the transition latch,
because cancellation over an already empty chord set also emits nothing.

The clean signed run `t249-held-cancel-01` also passes all thirteen cases,
manifest `cf64d878a25587083359c07a905c6a6fcc043a15dbc16396b3139b3dab6f5dd4`.
Its cancellation answer names epoch 1, chord token 1 and activation serial 7,
retaining W1 (`SurfaceId { index: 31, generation: 2 }`). The alternative
selection W2 was derived before the run as index 47, generation 5.

`t249-held-cancel-controls-01` rebuilds only the disposable Hagia archive with
the lifecycle handler's `released = cause.lifecycleReason == 1` changed to
`released = true`. The mutant still closes the switcher, but fails exactly
“cancelled chord must not commit the switcher selection”: actual W2, expected
W1, exit 101 with one failed test. This happens on the correlated Cancelled
answer before any revocation or withdrawal receipt; it is not a timeout or
transport failure. Manifest:
`957b74cc926432feaf7f10536e4f73861ea7293091c92e07669eb4f8635ff0ad`.
The adapter source is restored after building the mutant, and the ordinary
Hagia binary is unchanged. Binary paths and hashes identify both executables;
neither is copied into evidence. The experiment holds the external build-root
lease and a cargo-slot lease; its timeout supervisor also inherits the build
lease. Session source and fixture bytes are not mutated by this control.

`t249-held-cancel-checks-01` passes strict Session overlay Clippy, including
tests; manifest
`389b5324f9335a8f3ddfd2f34ae1ce4b3f7373f05abf0393dd3e2c50a2e93769`.
All four manifests independently verify, and all eight fixture modules match
the signed source across development, clean run, control and checks. The restored
Hagia adapter was independently compared with its signed Git blob. Development
checks also pass thirteen serial runner tests (one internal helper ignored),
strict runner Clippy, Rust formatting, whitespace and Hagia's layout gate.
No whole-product gate or performance measurement was repeated.

Hagia `7bca59c` and niltempus `066ec93` are merged and pushed. After all jobs
ended, runner cleanup removed the marked build root under its lease.
`t249-held-cancel-cleanup-01` passes with independently verified manifest
`5e67ee697e9e3be7c127f05ddd954a5946e770142d12009ce46e43bd066a4e28`.
Merged worktrees and the detached Sophia pin are removed after inspection.
The stable sibling lock, cargo slots and frozen evidence remain.

The trigger is supplied directly to the existing owner. This does not exercise
actual seat locking, routing-mode computation, lock keyboard reset, seat/device
removal or shortcut-registry replacement. There is no captured-key debt in this
case. Content, heads, completion, router time and layout settlement retain the
earlier supplied boundaries. Pointer debt, other cancellation triggers,
AbortSettlement, native/source-loss/pixel joins, receipt ACK/credit reachability
and latency remain separate work. t249/h006 stay open.

## Pending settlement abort precedes replacement (2026-10-10)

Hagia `168270dc5296db0f18dd1f4e0e11369f36f71e41` adds
`sdk_disconnect_aborts_pending_settlement` beneath the occupied fixture.
niltempus `c1702f3ae3a8195d79777b5264c94e8176dbc67c` requires fourteen cases
and binds nine modules. Sophia remains pinned to `f77244abe`. No production
source, installed process or release changes.

The real Hagia fullscreen action produces a proposal whose frontend
presentation state differs from the committed baseline. Production
`layout.stage` retains it pending presentation-state acknowledgements. The
fixture supplies a 30-second initial deadline and leaves frontend controls in
a local queue. This separates forced expiry from the normal short timeout;
it is not a timing measurement. Terminating only the fixture peer makes
`poll_public_restart` take AbortSettlement: transport is unavailable, the
worker and deferred command are gone, and the exact pending identity remains
in the old epoch with its deadline shortened to now. A repeated restart poll
still cannot advance the epoch or restart count.

The real `expire_pending` returns TimedOut for that transaction and settlement
identity with no applied surfaces. Applying the result preserves committed
policy, layout layers and frontend presentation state. Only the next restart
poll admits the successor. Its own scene/Dirty exchange restores the baseline,
and a fresh fullscreen action still toggles from the uncommitted state. That
last action uses the existing supplied layout completion, not a frontend ACK.

The unsigned development run `t249-abort-settlement-dev01` passes all fourteen
cases, manifest
`506954f70edd8e092b327976a951443f4358937a70b6d1cbd3499e6369b6a2d8`.
The clean signed-fixture run `t249-abort-settlement-01` also passes all fourteen
cases; manifest
`92cc01eb4febf19191948d2927bc85c0bad8684958ef8fe73158ef128f5a0909`.

Two compiled archived-Session controls discriminate the abort from replacement:

- `t249-abort-settlement-forced-control-01` removes `force_pending_timeout`.
  It fails “AbortSettlement must force the pending deadline” while the original
  deadline remains in the future. Manifest
  `2aae85a83a9124d532bb8c5661a99a6c18ef6af18ccb351a41ff779397cc6382`.
- `t249-abort-settlement-barrier-control-01` forces `settlement_pending=false`.
  It fails “replacement must wait for pending settlement”, observing epoch 2
  instead of epoch 1 before expiry. Manifest
  `b83f7d0c845a9da4bd9ea761fbfdfdc8a15ce01a15b79769611f81c463709194`.

Each exits 101 with one named failure, before a timeout can satisfy the case.
The archive is restored after each mutation; ordinary Hagia and fixture bytes
remain unchanged. Controls hold the external build-root lock and cargo-slot
lease, and the timeout supervisor inherits the build-root lock descriptor.
Independent source review found no blockers in the owner sequence or the two
planned control seams. Checkpoint equality after peer termination is a
consistency observation, not evidence of what a dead peer would do. The
discriminating non-promotion assertions cover the returned TimedOut result,
empty applied surfaces and unchanged committed reducer/layout state.

`t249-abort-settlement-checks-01` passes strict Session overlay Clippy, including
tests; manifest
`5a7f9e01ce83b902e70b5523a9f0b1b775364ca354d8d7c31eae6e6a3e3e70e7`.
All five manifests independently verify. All nine fixture modules match the
signed source across development, clean run, both controls and checks. The
restored restart source also matches the pinned Git blob. Development checks
pass thirteen serial runner tests (one internal helper ignored), strict runner
Clippy, Rust formatting, whitespace and Hagia's layout gate. No whole-product
gate or performance measurement was repeated.

Hagia `168270d` and niltempus `c1702f3` are merged and pushed. Runner cleanup
removed the marked build root after all jobs ended;
`t249-abort-settlement-cleanup-01` passes with independently verified manifest
`1e8542c44ea33a09189ed54b41042ece5d28e32d3ef1e724a13bbcd0a2d681c4`.
Merged worktrees and the detached Sophia pin are removed after inspection.
The stable sibling lock, cargo slots and frozen evidence remain; no binaries
or caches are copied into evidence.

The fixture chooses the owner interleaving directly; production expires the
layout in a later owner-loop phase. Rollback control emission is not inspected,
and the queued controls are never delivered or acknowledged. This qualifies
the restart/settlement ordering for a pending frontend presentation-state ACK,
not native rollback, application behavior or every resize/admission path.
Supplied occupied facts, initial deadline and completion retain those limits.
Other cancellation triggers, pointer debt, native/source-loss/pixel joins,
receipt ACK/credit reachability and latency remain separate. t249/h006 stay open.

## Fourteen-case coverage reconciliation (2026-10-10)

This review uses Sophia `9222b853a`, whose production inputs are unchanged
from the external runner pin `f77244abe`, and Hagia `168270dc`. The fourteen
qualified cases above are the current real-peer evidence. Read-only independent
review checked the capability declaration, external fixtures and Session
admission owners; it ran no tests. The older three-case table is not a current
gap list.

The later [Manage settlement result](#new-window-manage-settlement-2026-10-10)
adds the fifteenth case and supersedes the absence of `enqueue_manage` coverage
in this snapshot. Visual admission and launch-origin propagation remain distinct.
The [CPU admission successor](#cpu-backed-new-window-admission-2026-10-10)
adds backing-snapshot admission and rendering; Present retirement and launch
contexts remain distinct requirements.
The [mirrored receipt successor](#mirrored-receipt-consensus-2026-10-10)
adds retired-frame stamp and receipt consensus for one output with two mirror
heads. It does not cover multiple logical outputs or the native service tail.
The [source-only repaint successor](#source-only-preview-repaint-2026-10-10)
separates preview source capture from the still-open production scheduling join.

This is a revisable coverage baseline, not a frozen feature list or a reduced
exit. Every defined capability and explicit acceptance requirement stays in
scope unless a recorded decision assigns it elsewhere. Newly discovered
required paths extend this baseline. Conversely, a fixture limitation does not
automatically require a new product feature or every possible combination of
states. Local, generic, historical and attended evidence can be reused within
their actual scope after source-impact review.

**J** means a qualified current real-Hagia join, with the supplied boundaries
recorded above; **S** means inspected generic/local test source, not a new run;
**P** means the separately accepted t310/h018 attended evidence. A negotiated
bit alone proves no behavior. Source-only coverage is a reuse candidate until
its retained run and candidate compatibility are established.

| Bit | Capability | Evidence and uncovered boundary |
| --- | --- | --- |
| 0 | BINDINGS | J keyboard catalog/routing in occupied and presented cases; pointer bindings not joined. |
| 1 | ACTIONS | J focus, fullscreen, terminal intent and recent-window actions; not every action family. |
| 2 | MULTI_OUTPUT | P occupied output migration/return. External cases each use one output; no current multi-output presentation join. |
| 3 | POINTER_INTERACTIONS | S `wm_session_tests/pointer_interaction.rs`; current SDK drag/cancel and accounting diagnostic remain unqualified. |
| 4 | CHROME | J configuration admission; S Engine chrome layout. Hit-testing and pixels are not established by configuration. |
| 5 | POLICY_DIRTY | J occupied restart, disconnect and abort recovery through real restore/Dirty exchange. |
| 6 | CONFIGURATION | J startup/replacement/refusal. Reload while occupied or presenting needs impact/coverage review. |
| 7 | SESSION_OPERATIONS | J accepted/refused/disconnected typed intent. External execution remains excluded; it cannot be inferred from intent. |
| 8 | INDICATORS | P workspace labels; S Engine publication/action identity. No complete current SDK indicator/action join. |
| 9 | PROFILE_ACTIVATION | J accepted replacement and rejected-profile rollback on the empty scene. |
| 10 | LAUNCH_PLACEMENT | S generic admission tests. No external case calls `enqueue_manage`; relayout-populated surfaces do not qualify admission. |
| 11 | TAB_GROUPS | S Hagia projection and Engine tab-chrome tests; no current external membership/owner join. |
| 12 | TRANSLATION_GROUPS | S Hagia projection and Engine translation tests; no current external translated-geometry join. |
| 13 | POINTER_FOCUS | Not selected by the fixture profile. S `policy_active_focus.rs`; a declared enabling profile and accepted/refused focus path remain needed. |
| 14 | LAUNCH_ORIGIN | S `tests/launch_origin.rs` and Hagia policy tests. Frozen origin through current peer admission remains unjoined. |
| 15 | OUTPUT_ACTIONS | P topology return; S combined-output tests. No output service or output-action transaction in the external fixtures. |
| 16 | OUTPUT_POLICY_KEYS | P topology return, with its exact identities; broader output-key/action composition needs review. |
| 17 | OUTPUT_LAUNCH_CONTEXT | S origin tests; current peer launch-context composition remains unjoined. |
| 18 | SURFACE_INSTANCES | J switcher publication/preflight with supplied CPU sources; overview instance/region and backend completion joins remain separate. |
| 19 | PRESENTATION_ACTIONS | J keyboard-scope action identity and Presented/Revoked/Withdrawn ordering. Pointer targets and all-head consensus are not covered. |
| 20 | ACTION_LIFECYCLE | J Begin/Held/Ended Released/Cancelled and return of credit. Opener refusal/capacity paths need generic-evidence and reachability review. |
| 21 | CHORD_ACTIONS | J exact chord/cause correlation, replacement and fresh chord; no claim for every cancellation trigger. |
| 22 | HELD_CAPTURE | J captured keyboard debt, replacement and owed-versus-duplicate release. Application-held keys, protected bypass and pointer debt remain unjoined. |

The capability table is insufficient by itself. The plan also names these
non-bit requirements, which must survive any shortening of the work list:

| Acceptance requirement | Evidence and remaining boundary |
| --- | --- |
| Snapshot/projection identity, checkpoint and refusal settlement | J occupied commit, supplied timeout/stale refusal, restart, real response deadline and forced pending abort. Native rollback and frontend ACK delivery are not claimed. |
| Source-only repaint | Existing backend/local tests and historical owner evidence require exact mapping; no current external SDK source-only repaint case. |
| All-head receipt consensus | Historical simulated mirror completion is labelled; a current production-owner join is not established by single-output receipts. |
| Protected/application capture and release debt | J keyboard debt with empty input focus. Application-held-key wait, protected bypass, pointer debt, lock and lost-head transitions need mapping to actual owner evidence. |
| Reconnect with reused numeric identities | J epoch replacement and rejection of old action identity, but unchanged surfaces. Reused surface index with a new generation is a distinct gap. |
| Revocation/debt independent of 9P reply credit | S `policy_file_custody.rs` ACK-credit, bounded-send and stop/ESTALE controls. Healthy receipt drainage does not prove exhaustion behavior; establish SDK reachability and local revocation behavior before adding a stress case. |
| Launch contexts and new-window admission | S generic origin/placement checks. Current occupied fixtures supply `admission: None`; neither management settlement nor origin propagation is joined. |
| Read-only inspection and malformed/truncated controls | Retained captured-tool result `t249-wm-inspect-fcc3ff91` and separate admitted inspection evidence are reuse candidates. Preserve Submitted/custody versus semantic-outcome labels; do not equate captured records with admitted live observation. |
| Independent SDK/export | The explicit two-test run below passes on the selected pair; ordinary full-suite totals exclude these ignored tests. Supplied admission/outcomes remain distinct from protected ordinary launch. |
| Measurement and resource costs | Historical forty pairs remain refused. Current accounting diagnostic, reviewed comparison method and full latency/cost campaign remain required. |
| Signed candidates and isolated integration | Slice identities and controls are retained; final affected gates and evidence review bind the chosen candidates before completion. |

Receipt/debt capacity latches, registry replacement, lock reset and reload while
presenting are source paths to assess against these requirements, not newly
invented features. t250 still owns attended daily-configuration acceptance and
whole-release rollback. That separation does not waive t249's explicit
production Session/backend joins: synthetic completion cannot be promoted into
native retirement or pixel evidence. No new installation, live reload or device
operation is authorized by this reconciliation.

### First uncovered admission boundary

`occupied.rs::populate` and `presentation.rs` observe surfaces with
`admission: None`, then call `enqueue_relayout`. Production new-window handling
instead calls `LiveWmSession::enqueue_manage` from `owner_loop/authority.rs`.
That preserves a Manage source through admission-extent synchronization,
settlement, retry/unmanaged state and restart rearming. The existing occupied
cases remain valid for their stated scene/recovery scope; they do not prove
this path.

The next bounded fixture should introduce a new managed surface against an
occupied baseline through that owner, observe a real Hagia placement, and bind
successful admission to committed Session state and the peer checkpoint.
Queue duplicate behavior must be tested at its actual queued/in-flight phase,
not assumed after commit. The mutation must fail a named admission-behavior
assertion; merely checking that a proposal carries the Manage enum is not a
discriminating semantic control. Review whether the shared commit helper reaches
the admission owner before choosing its settlement method. Launch contexts and
restart of unsettled management build on this boundary.

### Measurement preflight finding

Read-only review of the frozen Hagia measurement fixture found that its
`Capture::offer` increments a coalesced counter and drops the replaced ticket;
the capture has no per-coalescing replacement record. The report retains
settled/failure/unresolved identities and refuses different survivor sets, but
cannot reconstruct the replacement chain from explicit events. This supports
the original refusal; it is not a new passing interpretation of those runs.

A successor diagnostic must retain each offer's identity, scheduled/enqueued
time, admission decision, each replaced-to-replacement edge, dispatched request
and transaction, terminal outcome and unresolved work. Validate conservation
and unique terminal dispositions, retain enqueue lateness, and keep coalesced
updates out of settlement latency. Do not fabricate settlement at replacement
time or silently replace the declared metric with time-to-latest-state.
Normal checkpoint persistence, workload/coalescing policy and supplied frontend
boundaries must be explicit and identical where comparison is claimed.

Current-only absolute responsiveness can be diagnosed without a legacy peer.
The relative +1 ms p95/+2 ms p99 exit still needs a prospectively reviewed
compatible comparator after source retirement. Neither current-versus-old
whole releases, a historical mechanism experiment alone, matching-survivor
filtering nor a closed-loop workload substitution resolves that requirement.
No hours-long campaign should start before the small diagnostic proves its
accounting and the comparator/method is recorded. Thresholds remain unchanged.

### Current SDK/export result

`t249-sdk-export-current-01` runs Hagia's unchanged `tools/check_sdk_export.sh`
on Sophia `f77244abe3063255c57c283a32eafe1666d6e684`, Hagia
`168270dc5296db0f18dd1f4e0e11369f36f71e41` and vendored SDK
`b2a254dcb792e5f9d66f78bdd73f645153504507`. Both explicitly ignored
`independent_nim_supplied_stream_startup` and
`independent_nim_supplied_stream_cycle` pass: two passed, zero failed.
The gate first checks that both names exist. Each peer exits successfully and
its executable hash is checked before and after execution.

The actual SDK PolicyWire callbacks exchange startup/profile, configuration,
snapshot/projection, operation and Presented identity through the production
file export. Admission, configuration and semantic outcomes are supplied;
this is not another ordinary-Hagia policy, protected-launch or native result.
The cycle's receipt assertion observes consumption in the SDK peer, but does
not exercise credit exhaustion or native receipt generation.

Execution uses cargo-slot's fixed Sophia slot, four build jobs, serial tests,
nice 10, no incremental/debug output, and bubblewrap with network, host devices,
host process visibility and user-runtime sockets hidden. Sources are read-only;
the temporary peer/cache live in private `/tmp` and are removed by the gate.
No binary is copied to evidence. The package binds source records, command,
orchestration, identities, logs and case records. Its independently verified
self-excluding manifest SHA256 is
`a763489e02509357daa955042e3f7b3f6520f50de490f6793696b447915d3dd0`.
Retained input bytes match the source after execution, and the complete vendored
SDK manifest verifies. The identity record's `sdk` field contains the raw signed
commit object; its Git object hash equals the manifest's revision above.
This closes the selected-pair export rerun gap, not t249/h006. No full repository
gate, measurement, installation or running-session change occurred.

## New-window Manage settlement (2026-10-10)

Hagia fixture `270cd419c414a89decc479bfeaad4c221258ebbc` adds
`sdk_occupied_manage_settles_admission` in `tests/external/manage.rs`, a child
of the occupied fixture. niltempus runner
`14dcf34ddd12eb28774099881d133d0f819189e1` binds fifteen cases, ten modules
and a fifth production-glue span. Sophia stays pinned to `f77244abe`.
Hagia's later `5239a8f` changes only README wording: Session retains the returned
placement, while the checkpoint assertion compares focus, not geometry.
Fixture and production bytes are unchanged by that clarification.

Against the occupied two-surface baseline, a supplied authority batch requests
a third PolicyManaged top-level through a real presentation intent. Session's
admission owner enters PolicyPending, and its own `next_unmanaged_surface`
selects that surface. `enqueue_manage` admits one request and refuses queued
and in-flight duplicates. Real Hagia places and focuses the new window.

Unlike the occupied fixture's supplied direct completion, this case calls
production `layout.stage`. It checks ControlPending and the exact AdmitSurface
and SetPresentationState commands dispatched to a local channel by the
production control queue. Supplied Delivered answers pass through that queue's
correlation and the admission/presentation acknowledgement owners. A wrong
transaction and repeated answers are refused. The deliberately repeated ACK's
UnexpectedAcknowledgement is a local correlation control, not peer transport
failure. The two copied completion-dispatch arms are independently reviewed
and bound to `owner_loop/session_control.rs`, hash
`244a9c1ab6d74142f86a3288be001cecf1452d8f5980b872140e619563e1ffe5`.

`resolve_pending` commits the exact settlement, then the real peer checkpoint
is awaited separately from transport Ready. Session must no longer offer the
surface for management, must retain the returned placement and focus, and must
agree with the checkpoint's new window/focus identity. Admission remains
AwaitingPixels: no CPU/native candidate or visual completion is supplied.

The clean signed-fixture run `t249-manage-01` passes all fifteen individually
named cases, manifest
`039f0491bcec44ccd28dd0f006aab49df16d8d89eb7d0f2d9c475c92d5422a9b`.
`t249-manage-dev01` also has fifteen passing case logs, but correctly ends STOP
because two final comment edits arrived after its source snapshot. Its manifest
is `6f4b76bf10a69463abe8b615b672908cdb247dc6b5de94bc65a528a1f4b25524`.
That development record is retained separately and is not promoted.

`t249-manage-control-01` changes only the archived Session's `enqueue_manage`
source from Manage to Relayout. The peer still receives SceneChanged and returns
the same placement; staging and acknowledgement assertions pass. The first
failure is “committed Manage answer must end the owner's admission request”,
with `Some(SurfaceId { index: 59, generation: 1 })` instead of None. It is one
named semantic failure, exit 101, in 0.12 seconds; neither a deadline nor an
enum assertion supplies the discrimination. Manifest
`6d7ff2e2ddc6eec28ebaad71a59371c0bc0c19b1ba2a2314df2d8ba6b5991816`.
The source is restored and independently equals the pinned Git blob; the
ordinary Hagia executable and all ten fixture modules remain unchanged.

`t249-manage-checks-01` passes strict restored Session overlay Clippy, including
tests; manifest
`96f5c56f023a2ca609b9fc16f2b693f1ddabcbd0ac49077e1835d31bfffa307d`.
All four run/control/check manifests independently verify. All ten modules in
the clean run, control and checks match signed source. Development checks also
pass thirteen serial runner tests (one internal helper ignored), strict runner
Clippy, Rust formatting, whitespace and Hagia's layout gate. The full Nim/formal
or product gate was not repeated for this external-test-only addition.

Hagia `5239a8f` and niltempus `14dcf34` are merged and pushed. After all jobs
ended, the runner removed the marked build root under its lease.
`t249-manage-cleanup-01` passes, independently verified manifest
`6df7ac52249670bab4065892653f91209e81d4808341eaa135dbd88f1d5acc05`.
The merged source worktrees, detached pin and scratch executables are removed
after inspection; cargo slots, the stable sibling lock and frozen records
remain. No compiled artifacts are copied into evidence.

This qualifies management-request settlement, not complete launch visibility.
Authority facts and frontend answers are supplied; no real frontend delivery,
map/viewable transition, pixels, Managed state or safe-extent reconciliation
is proved. No follow-on relayout is claimed: restaging AwaitingPixels can
re-enter admission control. Launch origin and Manage across restart remain
separate. The explicit backend gaps include the presented-policy install branch
in `wm/presentation.rs::service_presented_policy` and admission completion in
`native_retirement.rs`; they cannot be assigned to t250 merely because the
current fixtures bypass them. Current control-path latency remains t249;
attended/input-to-photon acceptance remains separate. t249/h006 stay open.

## CPU-backed new-window admission (2026-10-10)

Hagia `6247086eb2c417f0f8af30e58a3fc898b023708f` adds
`sdk_occupied_manage_admits_cpu_candidate` in `tests/external/manage_pixels.rs`.
niltempus `799ae9588169f1f51545ded3cb8f65d5d982575f` binds the sixteenth case
and eleventh fixture module, including the final input-change comparison.
Both commits are signed. Sophia production remains pinned to `f77244abe`;
no production source or installed process is changed.

A supplied authority Request for a third window arrives with a 173-by-111 CPU
backing snapshot and no software Present. Production intake quarantines that
exact transaction and records BackingSnapshot evidence. Real Hagia places and
focuses the window; Session reconciles its content size to the retained safe
extent and stages the exact candidate. The same production control queue and
admission/state acknowledgement owners as Tier 1 consume supplied Delivered
answers. The existing fifth glue hash binds those two dispatch arms.

The CPU backing branch in `layout/commit.rs` reaches Managed without Present
retirement. A following authority turn calls `projected_batch` and
`production_authority_batch` directly, releasing the retained frame into the
ordinary `LiveProductionVisualRuntime` CPU cycle. The test checks generation 1,
buffer identity and Session's committed geometry. Read-only
`LiveProductionCpuScene::presentation_layers` exposes the exact retained bytes;
the last composed frame also contains the expected RGB value at the window's
centre. A second, differently filled CPU frame must reach generation 2 without
being quarantined again. Only after that behavior does the test inspect Managed,
planning removal and released recovery extent. Both frames' retained bytes and
composed interior pixels are checked. The real peer checkpoint agrees on focus;
the checkpoint does not provide the geometry oracle.

The development run `t249-manage-cpu-dev01` and clean signed-fixture run
`t249-manage-cpu-01` each pass all sixteen individually invoked cases.
Their independently verified manifests are respectively
`b5302f41ba310dd1a004c7676d0e3214f7033b12ba29525b0423c77a3df47bad`
and `de9742750fcf0aeb6494f32a1fc767a808c00e8d707efbdf64e553c22d8ca634`.
The normal CPU case completes in 0.21 seconds in the clean run; this is a test
duration, not a latency result. `t249-manage-cpu-checks-01` passes strict Session
overlay Clippy including tests, manifest
`7fc0fe21a26fee6cc21cdbce5034809dabe10192168a8af911b0364dc0a7360b`.
Development checks pass thirteen serial runner tests with one internal holder
ignored, strict runner Clippy, Rust formatting, whitespace and Hagia's layout
gate. The full Nim/formal and product gates are not repeated for external tests.

Two compiled archive-only controls fail at their intended first assertions:

- `t249-manage-cpu-control-managed-01` skips the CPU branch's `mark_managed`
  transition and its planning/extent cleanup. The first admitted frame still
  reaches production and passes the byte/pixel checks, but the second is
  quarantined. “a managed surface's next frame must reach production” reports
  generation 1 / handle 611 instead of generation 2 / handle 612. One failure,
  exit 101, 0.18 seconds; manifest
  `6feb167f4b2b2505a1decb86c83488e64fcbd3e8193aa17fd907a9c81111b881`.
- `t249-manage-cpu-control-extent-01` omits `set_recovery_extent` inside the
  synchronization owner's Update arm. Omitting only the Manage caller would
  be masked by the intake caller. The first failure is “Session must reconcile
  admission to the safe candidate extent”: 638-by-718 instead of 173-by-111.
  One failure, exit 101, 0.12 seconds; manifest
  `1cfc8764e2eb5ecf841fcc77e6ba1eb92ba600c8bff65e9db22424252e85b361`.

The controls hold both resource leases, refuse leftover mutations in either
source file before starting, restore their changed source and bind the ordinary
Hagia executable. All five run/check/control manifests independently verify;
all eleven recorded modules match signed source. After both controls, all
1,616 archived crate production-source files independently match the pinned
Git blobs. Independent read-only review finds no blockers in the fixture design,
runner or control seams.

Hagia `6247086` and niltempus `799ae95` are merged and pushed. With all jobs
finished, leased cleanup removes the build root. `t249-manage-cpu-cleanup-01`
passes, independently verified manifest
`28b2fbe961e268a2e9709e4c1653b6b93f0cd45fce14d6b21e823b8c96c00ef3`.
No compiled artifacts are copied into evidence. The merged worktrees, detached
pin and scratch executables are removed after inspection; the fixed cargo slots,
stable sibling lease and frozen records remain.

This is CPU backing-snapshot admission, not native presentation acceptance.
Frontend map/viewable facts and control answers are supplied. CPU software
Present and DMA-BUF Present admission still require the AwaitingRetirement
path and its production completion join. The presented-policy backend install
branch, native receipt generation and all-head consensus remain t249 gaps;
they are not deferred to t250. Launch contexts, admission across restart and
the other coverage-matrix requirements remain open. Tier 1 stays necessary:
with pixels, `mark_managed` removes planning state, so replacing Manage with
Relayout no longer supplies its no-pixel management-request discriminator.
t249/h006 remain open and measurement requirements are unchanged.

### Existing seam for the next backend join

The pinned backend already exports `session_policy_presentation_fixture` under
`test-support`, which Session enables for tests. Its
`SessionPolicyPresentationFixture` in
`crates/sophia-backend-live/tests/support/session_policy_presentation_fixture.rs`
owns a mirrored composition target and accepts the real runtime and CPU scene.
It exposes target heads, queues retained composition frames, simulates submission
and individual head completion, and calls the production input-publication owner.
The backend derives the publication stamp from the retired frame lists; the
caller does not supply a receipt or input stamp. This is the existing candidate
for the real-Hagia all-head receipt join, without a new test API or pin change.

An initial read-only audit overlooked this export while inspecting the adjacent
`SessionContentFixture` and proposed extending that bridge. Root found the
existing policy fixture at the same pin; that extension proposal is withdrawn.
Actual device completion remains simulated, and the concrete native-selection
tail of `service_presented_policy` still needs its own coverage accounting.
It cannot be waived as physical-only acceptance merely because the headless
fixture calls the installation owner directly.

## Mirrored receipt consensus (2026-10-10)

Signed Hagia `74c9729def30f77392c276d1a1c9826542f38b3f` adds
`sdk_presented_receipts_require_all_heads`. Signed niltempus
`51fb4a6670e88b88d6567fc68a69622e314a629b` binds the seventeenth case and
twelfth overlay module. Production Sophia remains pinned to
`f77244abe3063255c57c283a32eafe1666d6e684`; all five copied-owner glue spans
are unchanged. No production API or backend fixture extension is needed.

The real Hagia Held prefix supplies the CPU scene and initial installation.
The existing `SessionPolicyPresentationFixture` then queues production retained
composition and simulates submission and individual completion for two mirror
heads. Production code derives the publication stamp and input projection from
the actual retired display lists. No receipt, stamp or completed projection is
supplied in this phase. After only the first head completes, Session must emit
no Presented receipt. After both complete, it emits exactly one receipt for the
installed publication; republishing the same completed frames emits no duplicate.

A real Ended(Cancelled) exchange closes Hagia's switcher without changing focus.
Session's changed installation branch emits Revoked before replacement retirement.
The first replacement head alone must not cause Withdrawn while the other still
carries the publication. Completing both produces exactly one Withdrawn for the
original receipt identity, and republishing does not duplicate it. Behavioral
receipt assertions precede raw projection diagnostics, so the controls fail on
the semantic obligation rather than an implementation detail.

The development run `t249-consensus-dev01` and clean signed-candidate run
`t249-consensus-01` each pass all seventeen individually invoked cases:
`lifecycle=3 occupied=5 operations=3 slow_peer=1 presented=5 native=false`.
Their independently verified manifest hashes are respectively
`35c31e6c707b0ab016cc3594f7876103fd53b073a478f5ba2e5a00c68e9767ed`
and `bd59cc169c6850e3ff9c41718220bef4bf0f47dd32d0196367ec7d68d5ee4fe8`.
The clean consensus case takes 0.20 seconds, a test duration rather than a latency
measurement. `t249-consensus-checks-01` passes strict Session overlay Clippy,
including tests, with manifest
`2ff535ad59fe90c8ce16882ab21bd747f9df49fb378aa004162cd5afc7e6271f`.
Development checks also pass thirteen serial runner tests with one internal
holder ignored, strict runner Clippy, Rust formatting, whitespace and Hagia's
data-layout gate. The full Nim/formal and product gates are not repeated for
external-only test changes.

Two compiled archive-only controls change the unique presented-head collection
in backend `production_visual_runtime/projection.rs`:

- `t249-consensus-control-presented-01` keeps only the primary head. Its first
  failure is “a publication retired by one mirror head must not be Presented”:
  one failure, exit 101, 0.16 seconds. Manifest
  `1eed2482e0797a08db9be6a64d9baba4329050b6b4be10c7536d7c1dacbc1147`.
- `t249-consensus-control-withdrawn-01` truncates only when the primary has a
  retired stampless frame. The entire presentation round and Revoked pass;
  replacement then fails “a replacement retired by one mirror head must not
  withdraw the publication”: one failure, exit 101, 0.19 seconds. Manifest
  `19cd006ae773d9beae3d3036c16e0d387a024b6cec645a739ac734cfe6f23555`.

Both controls use the clean run's exact case argv, hold the build-root and cargo
slot leases, compare the mutated file with its pin blob before starting, and
restore it afterward. They preserve the ordinary Hagia executable and all
twelve mounted fixture files. All five run/check/control manifests independently
verify, and every recorded fixture matches signed source. After both controls,
all 1,616 archived crate production-source files independently match pinned Git
blobs. Read-only independent review finds no blockers in the fixture, runner,
documentation or control seams. Hagia `74c9729` and niltempus `51fb4a6` are
merged and pushed.

With jobs finished, leased cleanup removes the build root:
`t249-consensus-cleanup-01`, independently verified manifest
`fd583770791cb71ffebaaba5e11b8eaf313b8d984549a58dd1a4257bd7928cbc`.
Records live under `~/.local/state/sophia/development-evidence/`; no compiled
artifacts are copied there. Fixed cargo slots and the stable sibling lock remain.

The bounded result has one logical output with two mirror heads. Worker/device
submission and completion are simulated. The inherited prefix initially installs
against one supplied head; the case subsequently validates the publication against
the target's heads, but does not claim that the first installation used them.
The closure does exercise the changed installation branch. The GLUE-bound service
mirror calls the production revalidation, withdrawal and receipt owners, but
does not execute `service_presented_policy`'s concrete native-selection tail.
That owner ordering remains a t249 gap. Physical display behavior, native Present
admission and multi-output consensus are not established here. t249/h006 remain
open, and measurement requirements and budgets are unchanged.

### Source-only repaint oracle identified

Read-only follow-up identifies a direct preview oracle at this pin: each retired
display list records `CompositorSurfaceInstance::source_generation`, resolved
from committed content during capture rather than supplied by the WM. The current
external fixture exposes publication instance identities but not these per-head
retired instances. A small WM-neutral test-support readback could expose the
existing fact, allowing a case to require generation 2 on both heads while the
policy publication and receipt identity stay unchanged and no WM cycle begins.
This would require a reviewed Sophia test-support commit and runner pin update;
it is a design, not implemented evidence. The proposed archive control clamps
the resolver's unique source-generation assignment to 1. Explicit fixture
recomposition would still leave production source-only repaint scheduling as a
separate native-service join, and the mirrored target supplies no preview pixel
readback. The frozen historical overlay remains unchanged and was not rerun.

## Source-only preview repaint (2026-10-10)

The generic test-support readback in signed Sophia
`8ce7c40effce25251856d52d3fb234532ca9e921` exposes each mirror head's retired
`CompositorSurfaceInstance` values through the existing
`SessionPolicyPresentationFixture`. It copies the frame's stored instances;
it does not recompute them from the latest source or requested publication.
An unretired head contributes an empty list. The only crate/Cargo difference
from the previous pin `f77244abe` is eighteen lines in this test-support file.
Production source and all five independently rehashed GLUE spans are unchanged.

The external `sdk_presented_source_repaint_preserves_identity` case begins with
real Hagia's held switcher and backend-derived Presented on both mirror heads.
Each retired preview initially names source generation 1. One previewed surface
then receives a supplied second CPU backing frame through authority observation,
`projected_batch`, `production_authority_batch` and the ordinary CPU cycle.
The fixture explicitly requests retained recomposition and simulates both head
completions. Session must retain the original receipt identity and publication
stamp, emit no new receipt, and admit no WM cycle. Each retired preview tuple
must keep its instance identity and source, changing only the repainted source's
captured generation to 2 on both heads. Separate diagnostics inspect the retired
application layer and the CPU scene's retained bytes.

Signed Hagia `e539e69d9eac19542edf2ff431e934b0b1bdf52b` owns the external
case; signed niltempus `cb9ddebc9a2abb0d5a73271961bfd99da9ee1614` binds all
eighteen cases and thirteen modules to the new Sophia pin. The development run
`t249-repaint-dev01` passes eighteen individually invoked cases, manifest
`ff4a6e665753ac5a091da27d10d97c6cbca3aef070aa334623e98d0bcc313e23`.
The repaint case takes 0.22 seconds; this is a test duration, not latency evidence.
The source-layout audit, formatting, whitespace, Hagia data-layout gate, strict
runner Clippy and thirteen serial runner tests (one internal holder ignored)
also pass. The full Nim/formal and product gates are not repeated for this
test-support and external-fixture change.

The first clean signed-candidate run `t249-repaint-01` also passes eighteen
cases, manifest
`802ecd579de83022fa1e933b32debab0c69e24fa2937ae49fb052a08bb0501cb`.
`t249-repaint-checks-01` passes strict overlay Clippy, including tests, manifest
`501db71c4b3243e660de4c9dd4fc8d9a680185fe28bd080313d01e777b078a13`.
These records precede the following health-check tightening.

The peer's `remote=116` log line also occurs during older qualified fixtures'
shutdown, including startup-only cases. `Lifecycle::stop` drops the transport
worker before terminating the child, and `NinePStop` explicitly revokes and flushes
reads with ESTALE. The repaint case previously had no health poll between its
Presented drain and shutdown. Hagia follow-up `1a2902e` adds a case-local poll
without admitting work, then checks healthy transport, unchanged epoch and
request count, empty queued/in-flight work and a supervisor poll with no exit.
It runs after the preview and byte assertions and before the final record and
teardown. This observes health at that boundary; it is not a policy round trip
or a guarantee against a later exit. The shared shutdown helper is unchanged.

Final signed Hagia `1a2902e1739f4abf31933429a5900dceb8bba60b` passes all
eighteen cases in `t249-repaint-02`, including the pre-teardown health check.
Its independently verified manifest is
`c2d8a0ab683dc73dadce802c170608e7c053c2b53a06e92da13df57c0df736cf`.
The repaint case again takes 0.22 seconds. The final control uses this run's
exact argv and thirteen fixture copies, rather than the earlier candidate's.

The original control prediction was wrong. In `t249-repaint-control-01`,
clamping the resolver's captured source generation to 1 compiles, but the
production composition-plan guard refuses `StaleInstanceSource` at the second
`target.queue` (`presentation_repaint.rs:211`) before a repaint frame can queue.
The experiment expected the later readback assertion, so its result remains
STOP, preserved unedited with manifest
`2fb40801b26a00a3d5469ae34842af84281dbef2aa14b8f4df2617e3d330c9b3`.
Restoration was independently verified. The corrected one-site control requires
that exact production error, exact queue site, exit 101 and one failed test.
It establishes fail-closed rejection of stale source capture on this target path;
it does not independently test the retired-instance assertion.

`t249-repaint-control-02` passes that corrected expectation, one failure in
0.18 seconds, manifest
`042d62490622609f9883e2f499011fb272f3ad5372caeb5cb829aa981d945b09`.
The independently reviewed compound `t249-repaint-control-03` additionally
clamps the comparison's committed generation in `composition_plan.rs`, bypassing
that guard for the deliberately stale resolver output. The first failure is now
“retired previews must sample the repainted source”: both heads retain generation
1 for the changed source instead of 2, after the no-cycle and presentation-identity
checks pass. One failure, exit 101, 0.18 seconds; manifest
`5670263593b9b93a7751adc4c7e195d9722a8fd9a714c19a126b17cb66061b23`.
This is explicitly a two-site oracle control, not a claim that a single resolver
regression can bypass the production guard. The CPU-scene damage validator is a
separate defense and is not qualified by this target-retirement control.

Both controls bind the final ordinary binary and all thirteen fixture modules,
hold the build-root and cargo-slot leases, and restore their archived production
files. The compound script checks both original files against the pin before
mutation and restores both on failure as well as success. After the controls,
all 1,616 archived crate production-source files independently match pinned Git
blobs. The runner documentation follow-up `38e2ae3` distinguishes the one-site
refusal from the compound readback control; its runner executable is unchanged.

`t249-repaint-checks-02` passes strict Session overlay Clippy, including tests,
on the final fixture and restored sources; manifest
`be323782bdbad75077c14a2431626a0226aa473591ed03c3619bc0844a2d0b96`.
All eight run/check/control manifests independently verify, including the
preserved STOP record, and each package's thirteen fixture modules match its
own signed candidate. Independent review finds no blockers in the readback,
runner, fixture, health check or either control boundary. Hagia `1a2902e` and
niltempus `38e2ae3` are merged and pushed.

After all jobs finish, leased `t249-repaint-cleanup-01` removes the build root;
its independently verified manifest is
`0b8bc8b23753164a1befdc3708e9355ca3c7d576166a489b649eac12bd20304c`.
The nine records remain under `~/.local/state/sophia/development-evidence/`;
no compiled artifact is copied there. Fixed cargo slots and the stable sibling
lease remain. Finished worktrees and scratch executables are removed after
inspection. No installed desktop component is rebuilt, installed or reloaded.

This remains one output with two mirror heads and supplied CPU sources/device
completion. The inherited scene bypasses admission: these supplied surfaces are
not a proof of managed frontend admission. The target does not rasterize preview
pixels. Recomposition is fixture-requested, so the native service's decision to
schedule source-only repaint remains a t249 production-owner gap. Neither that
gap nor Session's concrete native-selection tail is deferred to physical-only
acceptance. No performance, physical display or complete t249/h006 claim follows.

### Next admission boundary identified

A read-only audit finds that a software-Present admission case need not supply
the typed retired record. The existing CPU cycle's without-native branch calls
`settle_unframed_software_presents_without_native`, which generates a correlated
`LiveProductionRetiredSoftwarePresent` from the committed submission. Its frame,
native-submission and timing fields are zero; this is headless copy settlement,
not native target retirement. The public runtime drain can provide that record
to Session's `record_native_software_present_retirement`, whose admission
completion should let a following source frame escape quarantine. This is a
source-backed design, not a result. A case must distinguish record correlation
from Session's admission transition and label its supplied frontend answers.

The production Session loop currently obtains those records through its native
service report; a fixture-driven drain/record call would not qualify that service
selection. The audit found no exported target fixture for native software-Present
submission/retirement. Whether a live no-native Session can accumulate undrained
headless records requires a reachability assessment before calling it a defect.

## Shared-core readiness split (2026-10-10)

After the source-repaint evidence closed, niltempus approved implementing a
smaller prerequisite for the remaining role migrations. The
[t323 plan](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#t323)
owns that gate: request lifetime, bounded credit-independent local revocation
and release handling, reconnect with reused identities, and the existing
eighteen-case regression baseline. The
[sequencing record](../milestones/dueqd6r0-separate-shared-core-readiness-from-full-wm-qualification.md)
preserves the approval, conditional promotions and unchanged full exits.

The software-Present design above remains a t249 follow-up, but is no longer
the next implementation slice. Neither that source audit nor the Plan 9/rio
comparison establishes a passing gate. Existing generic core tests are to be
mapped to retained evidence before more Hagia cases are added; any new join
must exercise the production owner and identify supplied external facts.
The full capability matrix and all dated results above remain unchanged.

### Initial t323 evidence map

The initial audit reads Sophia `83bb9c2b9` (the source-repaint documentation
successor) without modifying production or fixtures. The following tests were
read, not run during this audit. The
[runtime retirement mapping](1lty2tzb-what-ipc-code-remains-after-the-desktop-moved-to-9p2000-l.md#runtime-wm-ipc-retirement-t269-2026-09-28)
records earlier executions and candidate identities; those historical runs
require impact review before use for the final readiness candidate.

| Boundary | Existing tests or retained join | Limit for t323 |
| --- | --- | --- |
| Shared request lifetime and pressure | `sophia-9p/tests/pipeline.rs` and `waiting_and_bounds.rs`: reply/flush races, tag and fid reuse, bounded outstanding requests/reply bytes, waiting reads, revocation and full-output ordering | Shared protocol evidence; not Session's local input or presentation behavior under transport pressure. |
| WM staging, replay and epoch custody | `policy_file_custody.rs`, `policy_file_replay.rs`, `policy_file_startup.rs`: driver-issued permits, partial staging, replay without decode, wrong epoch and snapshot/QID lifetime | Generic export semantics; opened snapshots and live action authority need distinct expectations. |
| Snapshot atomicity and record validation | `policy_file_atomic_cycle.rs`: credit precedes encoding; byte-credit refusal, stop and expiry spend no snapshot QID or event. `snapshots_pin_metadata_and_qids_continue_across_epochs` and `wm_file_arrays.rs` cover immutable snapshot identity and truncated records. | Shared-core design obligations included in t323; source mapping still requires a current run or reviewed reuse. |
| Actual journal ACK exhaustion | `real_reactor_services_ack_to_release_a_blocked_whole_event_send`, `stop_wakes_actual_reactor_waiting_for_ack_credit`, `actual_reactor_send_without_ack_has_a_bounded_deadline` | These use the actual reactor and filled journal. Stop/progress is not a joined proof that Session revokes a presentation and consumes release debt while credit is absent. |
| Local capture revocation | `physical_policy_routing_keeps_release_debt_when_action_queue_forces_revocation` | Fills the owner action queue; presentation stamps are supplied. This is not journal ACK exhaustion or 9P reply-byte exhaustion. |
| Receipt retention pressure | `policy_presentation_revocation_survives_full_lifecycle_delivery_queue` | Supplies receipt storage pressure and checks local revocation despite transport unavailability; it does not exhaust an actual reactor's credit. |
| Reused surface index | `a_stale_click_does_not_retarget_a_reused_surface_index` | Generic stale-click refusal; it does not combine peer reconnect and real Hagia model recovery with that generation change. |
| Repeated presentation identities across epochs | Engine `reconnect_cannot_reuse_an_old_action_when_publication_and_target_ids_repeat`; Session `policy_presentation_reconnect_rejects_old_identity_at_enqueue_and_settlement` | Generic Engine/Session refusal; neither is the real-peer reused-surface-generation join. |
| Real Hagia reconnect | Retained `sdk_presented_disconnect_preserves_release_debt`: unsettled action discarded, exact old identity refused, owed and duplicate release distinguished | Uses unchanged surfaces across restart. Reuse of a numeric surface index with a new generation remains a missing join. |

The first implementation target is therefore the production Session
revocation/release path with independently verified transport pressure. State
which capacity is exhausted (reply bytes, journal ACK credit or owner command
queue); do not use one as a proxy for another. Require local revocation and the
owed release before restoring peer progress, with a bounded failure path and a
control that distinguishes the claimed behavior. The other missing join
replaces a surface generation across peer recovery and rejects the predecessor's
authority before accepting a fresh action. These are gate obligations, not
results or a claim that all remaining generic evidence has been revalidated.

Pausing the peer does not itself establish journal ACK-credit exhaustion.
Session's receipt flush observes the capacity-one command slot through
`try_command`; a full slot can have several transport causes. A fixture must
independently establish journal state before claiming that bound is exhausted.
Otherwise its claim is limited to command-slot pressure while transport is
stalled. Reply-byte exhaustion needs its own construction. Independent
read-only review agrees with this distinction and the two missing joins;
neither reviewer nor root ran tests during this documentation audit.

The audit independently verifies every `t249-repaint-02/SHA256SUMS` entry;
the manifest still hashes to
`c2d8a0ab683dc73dadce802c170608e7c053c2b53a06e92da13df57c0df736cf`.
Its retained RESULT is the eighteen-case PASS. This is integrity verification
of the existing run, not another execution or readiness-gate acceptance.

## Connections

- [t249 plan](../plans/80blhke8-migrate-the-hagia-wm-role-to-admitted-9p2000-l-files.md#t249) owns the unchanged acceptance requirements.
- [WM reactor investigation](87juczar-wm-file-reactor-waits-behind-the-driver-command-channel.md) preserves the measured failure and scheduling correction.
- [Observer delivery plan](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#first-deliverable-and-dependency-order) separates prerequisites from the repository queue.
- [t310 acceptance](ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md) owns the current occupied-output evidence.

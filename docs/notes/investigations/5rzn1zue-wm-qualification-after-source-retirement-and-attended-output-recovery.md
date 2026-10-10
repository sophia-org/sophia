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

## Connections

- [t249 plan](../plans/80blhke8-migrate-the-hagia-wm-role-to-admitted-9p2000-l-files.md#t249) owns the unchanged acceptance requirements.
- [WM reactor investigation](87juczar-wm-file-reactor-waits-behind-the-driver-command-channel.md) preserves the measured failure and scheduling correction.
- [Observer delivery plan](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#first-deliverable-and-dependency-order) separates prerequisites from the repository queue.
- [t310 acceptance](ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md) owns the current occupied-output evidence.

---
id: v45ldzp0
date: 2026-09-25
kind: investigation
status: investigating
tags: [session, shell, validation]
---
# Two component desktop acceptance separates package preparation from attended retirement

## Question and scope

Prepare t101/t081 acceptance for the [authorized Lom/Bemenu desktop](../milestones/a3j8o6g0-desktop-acceptance-retargeted-to-lom-and-bemenu.md).
This slice starts at `1fbaf4cbc81ef3629176aea30b758ce51018d887` on
`session/t101-two-component-acceptance` in `sophia-borders`. The first checkpoint
changes tests and this investigation only; the approved bounded CLI correction
is recorded below. Queue, primary plans and peer queues remain with
the director. Narthex is a separate reference/rollback session, never a third
simultaneous provider. No dock or new tab service is included.

## Read-only installed artifact inventory

Observed September 25, not evidence of what a running process loaded:

| Artifact | Identity |
| --- | --- |
| Sealed release | `/opt/sophia-niltempus-desktop/releases/niltempus-af4bb8a48c69542dc1d8` |
| Sophia source / binary SHA256 | `9ee301e74ef22a065dcb112dc1873c7e458dfec4` / `821feac5cd0c82d235e17c287be8986363f08341a635c4aa40eb8322de2b2d13` |
| Lom source / binary SHA256 | `ad349869a8ce3f9eff77c6b91465b80cb54198b6` / `48f85c5c3d6b5c6bc55a371406b5b6b125d231cf242ab4cfb9c4cce5edec1919` |
| Bemenu source / binary SHA256 | `7d2d2399e945c1126458aeb4d01feb8978339949` / `f11656c81373a03c782ba6ba1447e6d529c08aea6b7a3dc40360b3158d4bc825` |
| Hagia source / binary SHA256 | `97ed593e7b6829054f7143cf621ae5fd789bded6` / `b4060baff520b2d34cb772caa6475e6835fd35a2c00e843d0ff6c1a6161570d7` |
| Sealed desktop profile SHA256 | `b8ace3a87f174af19edf9ab203779270b6a84b2acba2a8b4177300b0ab7198d4` |
| External `~/.config/lom/config.kdl` SHA256 | `a0c88fc211df32888b8ec19df06dd839f925adcbdb5e0fc7a73b71a22db754fa` |

Source identities come from `desktop-manifest.json`; the listed binary/profile
hashes were independently read from files. The manifest reports signature `N`
for the Lom source tip, `G` for the other three; do not silently upgrade that
claim. This inspection is not a complete release verifier run.

The source personal profile refers to older `desktop-releases/20260918-d444eba2`
paths. The sealed profile rewrites them to this release's Lom and Bemenu. It
selects panel/bar, GPU direct, top reservation 24, and menu/application-launcher,
GPU denied. The external Lom KDL remains mutable and outside the release.
The installed entrypoint selects the sealed profile but uses the user-owned
`~/.local/state/sophia-niltempus-desktop/development/hagia`. That file currently
matches the packaged Hagia hash and its own `hagia.json`; both must be pinned
independently at a future run. No process inspection, IPC or device access was
used. Installed Sophia predates the t100 reconnect budget repair.

## Existing owners and uncovered joins

| Boundary | Reusable evidence / limit |
| --- | --- |
| Parser and role/provider selection | `sophia-config/tests/shell_components.rs` already rejects duplicate identities/roles, simultaneous legacy selection, malformed paths and conflicting reservations; session `shell_component_config.rs` checks normal mode, catalog/input prerequisites and fallback omission. These are configuration checks, not binary admission. |
| Private launch assets | `ShellComponentLaunch::new` canonicalizes selected private config paths. It does not parse Lom KDL or authenticate executable bytes. Per-attempt GPU identity revalidation is a later owner. |
| Offline CLI preflight | `commands/config/session_preflight.rs` checks legacy shell and WM executability and delegates policy to Hagia. It does not iterate independent components or check their private assets. The ignored executable regression records this gap; no production repair is included. |
| Personal sealed inventory | The personal installer's `release.go::verifyRelease` compares full file hashes/modes and required executables. This is separate from CLI profile acceptance, mutable external KDL, and the selected development WM. No new competing verifier is introduced. |
| Profile reload | `component_launch_reload.rs` tests actual terminal-command reload and provider preservation, stale action removal, held command continuity and absent-provider refusal. It does not establish an installed reload. |
| Component reconnect | [t100 evidence](vup982br-retained-panel-pixels-can-block-fresh-component-admission-after-disconnect.md) uses real component transport/service/backend settlement, retained source leases and independent consumer ownership, supplied initial PendingPresentation and simulated native completion. It proves neither a protected Lom/Bemenu restart nor GPU retirement on this installed release. |

New Session fixtures compose the selected two-role parser result with the
existing launch-plan and outer preparation owners. With no GPU coordinator the
requested direct panel is refused, the denied menu endpoint is prepared, and
no process, grant attempt or work-area band is invented. This deliberately
cannot count as a complete desktop startup. The private-file control uses a
clearly identified denied-policy copy to isolate canonicalization without a
GPU device. A comment-only file resolves successfully: that is path validation,
not acceptance by Lom's config parser.

## Proposed attended phases, not execution authorization

Freeze exact sources/locks, sealed manifest and binary hashes, selected profile,
external Lom KDL, development WM and separate rollback artifact before any run.
Validate private KDL with the pinned Lom's offline checker and WM policy with
the pinned Hagia. Establish component artifact validation separately from the
current CLI preflight gap. Record intended direct/denied policy separately from
actual protected launch/grant evidence. Repeat artifact hashes at the end;
process identity must come from launch evidence, not file existence.

| Phase | Observations and measurable exit |
| --- | --- |
| Startup | Exactly bar and launcher, distinct slot/connection/content epochs, no legacy shell/dock. Every admitted output gets native panel pixels and its reservation. Missing direct permission must be a named refusal, not silently counted as success. |
| Stable workflow | Per output: empty-focus styling, workspace activation, clock/calendar open/action/dismiss and parent loss; Bemenu query/Escape/reset, real launch and focus return; Hagia navigation/overview. Record workload counts before starting. No component restart is allowed inside the stable measurement interval. |
| Stable latency/storage | Existing panel budget is 10 s warmup plus 60 s, 20 state-changing actions per output on two outputs; ACK p95 <=50 ms/max100 ms, native p95 <=150 ms/max300 ms. Require complete causal identities/all-head kernel completion, no discarded failures, samples every5 s with gaps <=6 s, bounded warmed inventory and final zero ownership. These are the existing panel workload bounds, not newly approved universal launcher/calendar/overview budgets. |
| Replacement | Separate run/phase: replace each component independently, retain the neighbor's exact epoch/grant and continued service; stale action/completion cannot affect replacement. Old pixels/reservation may persist without input authority until actual replacement retires. Measure bounded retry/refusal and settled inventories; do not demand universal recovery while consumers pin the explicit t100 saturation zone. |
| Normal exit / rollback | Actual worker join, claims/source credits and copied native consumer ownership each drain at their own owner. Confirm physical completion and normal exit, then separately test the pinned Narthex rollback. Timeout cleanup is failure evidence, not normal shutdown. |

`run_current_lom_panel_gate_tty4.sh launcher` is existing attended two-component
smoke, not this entire matrix. It builds artifacts, uses a GPU preflight, has
90 s normal exit and 110 s watchdog, and must not run under this preparation
authorization. Its launcher verifier demands stable identities/two generations
on both outputs and exactly one started application; a process-start record is
not proof of a useful visible/focused window. The old one-grant panel workload
verifier requires one grant and final shell shutdown; it cannot directly accept
a combined transcript with multiple component epochs. Neither verifier should
be loosened to disguise restarts. A future component-aware measurement join
must distinguish per-role samples from aggregate final accounting. No such
validator or production instrumentation is added here.

CPU source storage/backing credit is one per accepted resource, not copied head
buffers or driver VRAM. Native frame copies can retain leases independently;
physical all-head completion is a separate observation. A below-cap sample,
synthetic Presented, helper completion, or successful GPU preflight cannot
replace those exits. Existing teardown evidence remains narrow and unchanged.

## Validation and handoff

Focused evidence lives in `sophia-borders/.artifacts/t101-preparation`.
The first Session run passed 2/2 with native-session, device-hidden, jobs2/nice19
on the exclusively allocated t027 cache. No client process was started by these
Session controls. The CLI gap control invokes the real CLI with a fixture WM
policy checker, not Hagia or either shell. Its expected failure is recorded
separately from passing tests; it is not desktop progress.

At the first checkpoint production, plans and queues were unchanged. Broader t100, t097 and physical
acceptance exits remain open. Lom t007/t008/t009/t010 need scoped peer evidence;
removing Lom t020 from this desktop's prerequisite does not close that peer.
Calendar stays required, while new tabs/switcher service does not enter this
slice. Subsequent artifact-validator or measurement-owner changes require
review of the established gap first.

The real CLI red failed as intended: missing component executables and KDL
returned exit0 with `status=accepted policy=validated`. The ordinary preflight
suite passed7/0 with that one red explicitly ignored. The formatted Session
rerun passed2/0. These totals do not count the red as accepted package behavior.

Signed fixture checkpoint `7328e35cbfc9581d267049583cbb4bae157b9b6b` preserves
that first red. The follow-up runs all eight invalid artifact cases: missing or
non-executable Lom/Bemenu, each under validated and deferred policy exits.
All eight incorrectly returned success (`preflight-eight-red.log`, test exit101).
These cases keep the private KDL path present so that artifact refusal cannot
be masked by a missing config. The intended correction is bounded to the
existing CLI: check each component with `require_executable` before either
policy exit, and canonicalize selected private paths to match the existing
launch-plan existence contract. Do not impose a new regular-file/readability
rule, parse KDL, authenticate hashes or claim race-free launch authority from
this check. That proposed correction was subsequently approved by the director.

## Approved CLI correction

After signed red checkpoints `7328e35c` and `24b48372`, the existing CLI
preflight now visits every parsed component before either policy exit. It
reuses `require_executable`; errors name the parser-bounded ID and fixed role
token. Optional private paths are canonicalized only, as at launch. There is
no client config parser, new API, GPU check, component execution or change to
runtime launch authorization. Legacy shell/WM checks and WM-only policy
handoff remain intact. A directory or opaque readable/unreadable object is
not reclassified by a new file-type/readability rule: canonicalization is the
actual existing boundary, not a promise the client can consume the asset.

The eight artifact cases now refuse before the fixture WM policy checker runs.
Missing private config refuses in both branches. Present opaque config and a
symlink to it pass in both branches; component executables contain a sentinel
write and exit99, and the sentinel stays absent. The validated branch still
passes only WM policy and cleans its private staged file; the deferred branch
does not execute its selected WM. These tests use fixture scripts, not actual
Hagia/Lom/Bemenu binaries.

The focused CLI suite passed10/0. A compiled mutation replaced the component
iteration by `iter().take(0)`; the artifact test failed in0.11s with all eight
invalid cases accepted again. `mutation-bypass-loop.log` and
`mutation-source-hashes.txt` retain that control; the original initial and
expanded red logs remain separate. The source was restored before final checks.
This closes the demonstrated preflight omission, not exact package identity,
current-process identity, TOCTOU-free launch, or attended acceptance.

After restoration, the real CLI suite again passed10/0 and the native-session
preparation controls passed2/0. Strict CLI preflight-test Clippy and native-session
test Clippy passed. Formatting and diff checks passed; no broad owner suite or
main/native-family gate was run in this slice. Evidence names:
`preflight-restored.log`, `session-corrected.log`, `clippy-corrected.log`,
`clippy-session.log`, and `layout-corrected.log`.

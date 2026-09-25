---
id: 1m3z9q0j
date: 2026-09-13
kind: plan
status: active
tags: [plan, shell, gpu, lom, daily-driver]
---
# Lom and Sophia portable GPU shell critical path

📌 [Pinned retrospective: prove the shell lifecycle before spending operator time](../concepts/t972gtpa-prove-the-shell-lifecycle-before-spending-operator-time.md).

## Scope and exit

Deliver one protected Lom shell with workspace/active-output indicators, clock,
calendar popout and preserved descriptor launcher/switcher behavior. It must
work on supported stock Linux installations under the accepted
[presentation/execution decision](../decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md).
No custom kernel, mandatory GPU bridge, Waybridge, Vello dependency in Sophia,
or second native shell is on this path.

The CPU content implementation and Lom's persistent client are the starting
point. Production GPU admission is implemented behind a default-denied profile,
while content input and native acceptance remain closed/incomplete;
writing this plan is not evidence of an admitted desktop. Preserve previous
model, fixture and incident evidence under its exact source identity.

The exit is a paired, exact-release daily-driver acceptance, with bounded
protocol/storage obligations and honest direct-GPU availability limits. The
operator still authorizes installation and attended runs separately. Offline
validation uses the device-hidden wrapper, not merely cleared opt-in variables.

## Repository ownership and sequence

`sophia:tNNN` and `lom:tNNN` are repository-qualified identities. `depends:` in
each todo.txt file refers to that repository; `peer:lom/tNNN` or
`peer:sophia/tNNN` links counterpart work. Peers share evidence where appropriate
but do not inherit each other's completion. Stable IDs and status live in each
repository's task files; this plan and
[Lom's plan](https://github.com/sophia-org/lom/blob/master/docs/notes/plans/pf4er77j-lom-daily-driver-critical-path.md)
own scope and exits.

| Step | Sophia owns | Lom owns | Integration exit |
| --- | --- | --- | --- |
| 1 | t097: explicit launch grant, compositor charge boundary | t003: admitted device selection and asynchronous renderer | Protected launch can prove/refuse the requested resources on stock Linux |
| 2 | t098: exact presented targets, action dispatch and settlement | t004: view bindings, TEA action intake and acknowledgements | A real private protocol exchange activates only the exact presented pill |
| 3 | t099: popout composition, precedence and dismissal | t005: calendar state and complete panel/popout candidates | Open, action, dismissal and parent-loss transitions settle coherently |
| 4 | t100: output/epoch/recovery lifecycle | t006: per-output scheduling and reconnection | Fresh state after every transition; no stale action or retained obligation |
| 5 | t101: shared client workflow support and package verification | t020: combined descriptor workflows; t007: package/configuration | One shell preserves desktop controls and has verified rollback |
| 6 | t081: native-shell milestone and evidence | t008: attended matrix; t009: soak; t010: promotion | Exact installed candidate earns both repositories' acceptance claims |

Steps 1 and 2 can develop independently using contained fixtures. Step 3 needs
step 2; step 4 tests can begin earlier but must validate the combined path.
Descriptor compatibility can develop in parallel with those implementations;
packaging and native acceptance wait for all of them. Existing higher-priority
X11/input work retains its ownership and queue order. Before implementation or
integration, coordinate shared files, use separate disk-backed build targets,
and reconcile with the current master; this plan imports no unfinished branch.

For each server/client seam: specify the state/identity and terminal outcomes,
update lifecycle models under the evidence policy, retain negative controls,
implement behind the gate, and test the real protocol/production owner path.
A model pass does not substitute for code, routing or physical evidence.

## Task details

### t097

Replace the old hard-quota admission placeholder with an explicit default-denied
startup GPU grant. Implement the successor ADR's target profile and refuse the
old `gpu-memory-bytes` form with a migration message; never turn an old quota
request into direct-access consent. Keep CPU-only content clients independent
of GPU permission and keep content itself default denied.

Use the existing typed profile/protection-domain launch owners. Establish exact
child, device and grant identities before exposing one render node and releasing
client execution. Record effective permission, driver inputs and refusal reasons;
no display, input, KMS or unrelated device mounts. Missing support, stale grants,
ambiguous selection and failure midway through launch must clean up without
publishing a content grant. A reconnect cannot inherit predecessor rights.

Close compositor-owned upload/backing accounting before production admission:
reserve known peak overlap before accepting obligations, retain charges through
native retirement, and refuse saturation without losing prior accepted work.
Reconcile it with the existing CPU pool instead of inventing a second unbounded
ledger. Neither this ledger nor per-import limits count all driver VRAM.

Exit: device-hidden policy/launch negative tests and models pass; a separately
authorized contained GPU proof verifies actual selected-device access, exclusion
of other devices, matching Lom adapter, and retirement/backing accounting.
Retain a capability/prerequisite record for an unmodified distribution kernel;
report untested driver combinations. A fake cgroup/controller fixture or a bool
set to true cannot satisfy the gate. No main compositor startup is needed for
the isolated proof. Modelled recovery cannot promise a stalled kernel returns.

Implementation checkpoint: the profile migration, exact typed render-node bind,
connection-epoch grant, device-replacement revocation, and conservative
content-backing credit are implemented with deterministic tests and the
`ShellGpuLaunchAdmission` model. This does not close t097: the isolated hardware
proof, other-device exclusion, Lom adapter match, and native retirement evidence
remain separately authorized and unrun.

The next candidate adds a production-policy isolated runner and a dedicated
tty4 native gate. The isolated runner launches Lom through the same GPU policy,
mounts one render node and no display, card or input device, verifies a real
nonuniform 256x24 candidate, then deliberately settles it as RendererFailed and
requires lease/backing cleanup. The native gate uses the tracked Minimal
workspaces-and-seconds configuration and requires two presented generations per
output. These are runnable gates, not results: neither has been executed on
hardware, and t097 remains active until their retained evidence passes.

The first attended native attempt on `adc25c43` at
`.artifacts/lom-panel-native/20260913T200334Z` failed before any content output
facts or presentation. The protected shell negotiated repeatedly but exited
after each grant (704 grant epochs in 20 seconds). The proof profile carried an
unnecessary terminal binding while Hagia rejected and restarted its
configuration 389 times; the reduced record did not retain the rejection reason,
so the binding's causal role is unproved. The watchdog restored the TTY, keyboard
mode and keyd. This is failed hardware evidence, not native acceptance and not
proof of Lom's process-exit cause because the diagnostic-mode runner
intentionally discarded role stderr.

The successor gate removes the unrelated application and binding, requires the
bounded protected GPU/content proof to pass before graphics takeover, and reads
native verdicts from the structured event file rather than the wrapper log. A
failed GPU or content prerequisite now reports the client's boundary error and
stops before Sophia acquires the display. No successor hardware result has
been recorded yet; t097 remains active.

The protected preflight on `61c37741` at
`.artifacts/lom-panel-native/20260913T202120Z` stopped before graphics takeover
because no enumerated Vulkan adapter passed Lom's grant selector. This confirms
the fail-closed boundary and does not distinguish an empty adapter inventory
from a missing or mismatched optional Vulkan PCI-bus identity. The successor
candidate carries host-observed PCI vendor/device identifiers as a bounded
fallback inside the already-single-render-node domain and records bounded
adapter counts if selection still fails. Its causal role and the native bar
remain unproved until a new attended result is retained.

The successor preflight on `bb97c561` at
`.artifacts/lom-panel-native/20260913T204021Z` enumerated one adapter but zero
non-CPU Vulkan adapters. Source and binary inspection established that the
protected domain omitted `/sys`, while RADV's libdrm discovery requires the
selected render minor's sysfs DRM and PCI identity. Adapter matching therefore
never ran; carrying PCI fields could not restore an adapter the driver had
discarded. The repair generates an immutable one-device discovery projection,
preserves the actual render-minor basename and makes Lom authorize only an exact
`VK_EXT_physical_device_drm` render-major/minor match. This explains the observed
enumeration failure; a later driver initialization failure remains possible and
requires the separately authorized protected hardware proof.

A frozen-source review of that repair found two evidence-boundary defects before
another hardware attempt. The proof verifier accepted missing DRM identity,
missing or non-Vulkan backend facts, and a zero grant epoch because absent fields
compared equal. It now requires every authoritative field exactly once, parses
bounded canonical device numbers and a nonzero epoch, and positively requires
Vulkan plus an allowed non-CPU adapter type. The same review found that the
generic protection builder could snapshot a replacement render node after
Session validation and bless it as its baseline. Session now passes its validated
device, inode and `rdev` identity into the launcher, which refuses disagreement
before constructing the domain and checks again before spawn. These checks narrow
the startup handoff; they do not claim atomic hotplug pinning or hardware success.

The attended `d00f43f6` / Lom `4340aa0` run at
`.artifacts/lom-panel-native/20260913T214837Z` passed the protected GPU preflight
and selected the RX 7900 GRE through exact Vulkan DRM `dev_t` identity. The
operator saw the bar and updating time. The session was not usable: pointer
motion was observed once but never routed, Hagia's configuration was rejected
492 times, the WM restarted 492 times, and the shell transport failed 82 times.
This is GPU-discovery progress, not native-session or input acceptance.

Source inspection tied the policy restart storm to the proof profile. Hagia
advertises all seven session-operation slots, including the application
launcher, while the terminal-free gate supplied no application catalog and
Sophia correctly rejected missing slot 7. The repaired gate selects an empty
trusted catalog and an explicitly empty startup list. A production-stage test
loads those exact KDL documents, proves slots 1 through 7 are present, commits
Hagia's complete configuration, and proves removing slot 7 is rejected. The
diagnostic now retains the bounded missing slots and catalog generation, and
the native verifier requires exactly one committed configuration. This does
not yet attribute the shell transport failures or prove pointer recovery; both
remain outcomes for the next separately authorized attended run.

### t098

Carry content target tables through production projection to the exact native
presentation. Prepared frames cannot publish targets. Bind dispatch to grant,
output, allocation, candidate, target and interaction generations; clip against
the actual presented allocation and enforce security/occlusion precedence.

Implement the existing discrete-input wire lifecycle, including bounded queues,
activation acknowledgements, duplicates, cancellation, timeout and revocation.
Route workspace intent through the existing authorized indicator action token;
do not turn a widget index into a capability or reuse unrelated toplevel actions.
Do not synthesize a general pointer stream for a toolkit.

Implemented in the t098 source tranche: the profile has an independent,
default-denied `content-input` choice; native retirement publishes the exact
candidate target table; Engine owns capture and release suppression; and the
Session action ledger keeps content acknowledgement separate from WM admission.
Lom derives the target rectangles and retained TEA meanings from the same
Masonry layout used to render the candidate. Deterministic reducer, transport,
projection and model gates cover this source boundary. Native pointer acceptance
remains part of t008/t081 and is not inferred from those headless checks.

Exit: model controls and deterministic production-owner/private-socket tests
prove one correct action and reject unpresented, stale, duplicate, revoked,
old-epoch, wrong-output and changed-meaning targets. Delayed rendering cannot
block action settlement. Test queued, routed and client-observed outcomes as
different facts. Keep descriptor and independent wire corpus tests passing.

### t099

Finish the already-specified allocation/candidate popout path in the live owner.
Use acknowledged parent-allocation physical coordinates and presentation epoch;
do not reconstruct integer logical anchors from fractional scaling. A complete
candidate carries the panel/popout resources and matching targets, with Engine
coverage, reservation and precedence rules applied together.

Outside dismissal is Engine-owned and consumed without coordinates or
click-through. Timeout triggers coherent withdrawal by Engine rather than
waiting indefinitely for client cooperation. Parent loss, output loss, security
takeover and revocation invalidate targets immediately, retaining any referenced
pixels until their independent leases retire.

Exit: owner-path tests cover open, target action, replacement, dismissal,
timeout, rejected placement, parent/epoch loss and backing retirement. Include
the ADR's fractional-scale anchor and negative-margin cases. This integrates
existing allocation machinery; it does not invent a new popup protocol.
Exercise the full panel/popout lifecycle with the independent C client as well
as Lom through the protected conformance host; shared Rust code alone cannot
prove the cross-language contract. Keep native input/presentation separate.

#### 2026-09-25 production-owner audit

The live allocation resolver already places popouts from acknowledged physical
parent geometry. Three additional controls in
`crates/sophia-session/tests/support/metadata_shell_popout.rs` cover all four
panel-edge tie-breaks, signed margins at integer scale two without rounding an
odd physical anchor, and refusal of oversized, out-of-parent and missing or
wrong-output parents without consuming an allocation identity. These call the
production resolver with supplied parent facts; they do not establish a
presented parent, fractional output support or a native presentation. The full
content-owner test module passes eleven controls. Reversing the away-from-panel
tie-break produces the intended assertion failure.

The remaining gap is integration, not another placement formula. The legacy
`ContentActionLedger` issues activation and cancellation, but has no
outside-dismiss issuance. `PresentedContentBinding` retains allocation geometry
without the panel/popout role and parent relationship needed to select a popout
for an outside press. Implementing dismissal therefore needs an exact presented
popout identity through projection, consumed press/release handling, bounded
withdrawal and independent retention of submitted pixels. Coordinate that input
seam with overview work rather than changing its shared routing concurrently.

The independent `bindings/c/tests/sophia_shell_content_client.c` explicitly reads
a named frame corpus; it is not a live admitted client. The protected
`shell_content_conformance_host` runs a supplied client's `content-proof` mode,
accepts one panel and deliberately reports renderer failure while checking
resource release. It does not present a panel/popout pair or exercise dismissal.
Both are useful existing checks, but neither satisfies this row's joined
cross-language lifecycle exit. t099 remains open.

The subsequent development checkpoint adds passive presented-popout metadata,
threads it through content projection, and reports an outside press with exact
grant/output/candidate/presentation/allocation identity. A red Engine control
returned `Pass` for that press. The capture repair consumes the press, retains
its release debt after withdrawal, refuses dismissal from revoked or
unpresented bindings, and prevents activation of a lower component. Engine
capture/stack tests and native-session all-target compilation pass. This is
explicitly unfinished: the Session report still needs delivery to the action
owner, deadline-driven coherent withdrawal, stale-candidate rejection and
retained-source retirement controls. It is not an acceptance or merge candidate.

The next slice wires that report to the real action FIFO. An outside dismissal
has zero target/action fields and no coordinates, and repeated presses retain
one event and deadline. A matching ACK records receipt; it cannot prove that the
popout disappeared or renew the withdrawal deadline. Expiry subtracts the
popout's images, targets and allocation from the owned content frame. The parent
keeps its original protocol Presented identity; the replacement still needs
actual native presentation before parent input becomes current again. A refused
queue restores the frame, keeps input revoked and retains the withdrawal for a
later owner turn. Expired replacement proposals and candidates naming an
invalidated allocation cannot resurrect the popout.

The private-socket action suite passes 21 controls with one existing ignored
case. Backend lifecycle controls pass 38 tests, including withdrawal before first
presentation, queue refusal/retry, parent preservation, delayed replacement and
final resource-credit collection after an independent lease ends. The full
device-hidden native-session library passes 586 tests with 18 ignored; the
retained Hagia t080 binary exercises real pregraphics policy rejection. That
optional fixture now explicitly admits its terminal/browser bindings so it
reaches policy validation. Strict session Clippy and workspace checks pass.
Logs are in the development worktree's `.artifacts/t099-*` files. These are
deterministic controls with simulated device completion, not native acceptance.
The independent C/Lom full popout lifecycle and the final signed-candidate gates
remain required before closing t099.

The fractional placement follow-up found a second rounding step in allocation
validation: a popout at exact physical x=4 with logical width=2 and scale=5/4
was rejected because its rounded logical placeholder x=3 implied width=4,
instead of the required three pixels from the exact physical origin. The
runtime regression failed with `Malformed` before the repair. Popout extent
validation now quantizes from a zero local origin; panel endpoint quantization
is unchanged. The owner uses the acknowledged parent's scale and generation,
including signed margins, rather than a newer output descriptor's scale.
The production resolver's fractional result is accepted by the real allocation
store, and negative margins at 3/2 and 7/4 retain their physical anchors.
Runtime allocation tests pass 11 controls, owner tests pass 13, and strict
native-session Clippy passes. Evidence is in
`.artifacts/t099-fractional-{red,runtime,owner,clippy}.log` in the development
worktree. These supplied rational parent facts do not establish live fractional
output admission or hardware presentation.

A further bounded-queue control filled all sixteen action slots before an
outside press. It failed because no withdrawal deadline survived notification
refusal. The ledger now reserves local dismissal obligations independently of
wire action capacity; repeated presses keep the first deadline. If no action
was emitted, no event number is consumed and no cancellation is owed. The local
obligation still reaches the existing withdrawal path at expiry. Its storage is
bounded by the sixteen-allocation ceiling; exhausting that storage reports an
error instead of silently forgetting a withdrawal. The private-socket action
suite passes 22 controls with one existing ignored case. Red and green evidence
is in `.artifacts/t099-dismiss-capacity-{red,green}.log`. These controls establish
deadline retention and wire admission, not a new native presentation result.

The independent live C client now speaks content records over the protected
socket without Sophia headers or codec libraries. The joined backend fixture
compiles the session's exact `project_render_bundle` source, rather than a
second mapping, and uses production intake, composition, queue and retirement
owners with the existing supplied device-completion target. Both C and Lom's
`content-lifecycle` diagnostic pass four complete candidates: panel, anchored
popout, action replacement and the surviving panel after dismissal. Actual
Engine pointer capture chooses the popup action and outside-dismiss identity;
the release remains consumed after removal. A stale parent epoch is refused,
queued content has no Presented receipt, parent loss revokes input, and the
last independent lease must end before resource release. Both peers close
without acknowledging a deliberately wrong action epoch. The guarded tests
require an explicitly supplied executable and are wired into
`tools/check_shell_protocol.sh`; the old corpus reader remains a separate check.
Evidence: `.artifacts/t099-{c,lom}-lifecycle.log` and
`.artifacts/t099-c-content-host.log`. The complete device-hidden session library
passes 589 controls with 18 existing ignored cases. This is deterministic
paired acceptance; device completion remains simulated and Lom's real calendar
service and attended native acceptance remain separate peer exits.

The first native-family run on `8ce095a1` retained a real fixture failure:
Hagia's matrix builds the session with `atomic-scanout-live`, whose default
session mode differs from `native-session`. The policy-rejection fixture now
explicitly selects normal mode before admitting its launcher bindings. The
exact older feature configuration is checked with hidden devices; this changes
the fixture, not the desktop default. The failed family report remains at
`/home/niltempus/dev/sophia/.artifacts/native-family-t099-8ce095a1/report.json`.

#### 2026-09-25 accepted t099 integration

The director reviewed the complete exit against signed candidate
`7116ad7de1a8f9d2c1fc15a53cf2a895e66d5bb8`, merged and pushed as signed
`aff26bac8180932b88430dab8139f6d9e423545f`. The merge tree is identical to the
gated candidate. Main returned clean to master and the exclusive gate window
was released.

Layout and all eight device-hidden native-family phases passed. The report is
`/home/niltempus/dev/sophia/.artifacts/native-family-t099-7116ad7d/report.json`;
the adjacent `t099-7116ad7d-paired-identities.json` records the four repositories
and binary hashes. Frozen Hagia `50336ce6` and Narthex `50b9014` isolate this
acceptance from rendering-foundation development. Signed Lom diagnostic
`97b6f63` supplied the protected independent lifecycle peer. Affected authority
library/wire suites passed 1,242/548 tests. The inherited t220 layout extraction
and explicit normal-mode fixture correction are included; the original
`8ce095a1` red reports remain retained under their actual identity.

These results satisfy Sophia t099's deterministic production-owner and paired
client exit. Lom t005 remains open for its own service/native criteria; its
diagnostic branch was not promoted to Lom master. Native input/presentation
acceptance remains separate. No live installation or reload occurred. Shared
source ownership is released to t245, whose generic presentation lifecycle must
preserve local withdrawal, parent receipts, stale-allocation refusal, consumed
release obligations and independent source/backing retirement.

### t100

Complete production topology/output-fact publication, fresh reconnect grants,
pending and submitted retirement, reservation/work-area coherence, and bounded
failure recovery. Exercise rapid output removal/addition, scale/allocation
replacement, disconnect during every phase, security revocation, and client
restart with old rendering retained. Keep protocol/control progress independent
of client GPU scheduling; a slow output must not starve another output's
control obligations.

Exit: deterministic tests drive real production owners and private transport
across each transition with exact old/new identities, no duplicate settlement,
no reuse before final release, and bounded retained bytes/metadata. Account for
terminal feedback undeliverable after peer loss. Preserve unrelated input and
indicator behavior; a synthetic host-only encoder is insufficient evidence.

The attended `20260913T221559Z` run on Sophia `826cff1b` and Lom `4340aa0`
committed the WM configuration once, and the operator observed a moving cursor
and workspace pills. The panel appeared on output 1 only. Each fresh content
grant prepared and presented candidate generation 1 on output 1, then the shell
exited before output 2 and reconnected; 99 transport failures accompanied about
100 shell epochs. Lom had incorrectly allocated candidate generations per
panel, while r5 uses one grant-wide namespace because CandidateChunk and
CandidateEnd omit output. Output 2 therefore repeated generation 1 after output
1 advanced the grant watermark and was rejected stale. The client repair uses
one service-wide counter and a two-output real-socket regression. Native
multi-output stability remains unaccepted until a later attended run shows both
outputs without reconnects. The same run's `motion_observed` record agrees with
the operator's moving cursor. `motion_routed` specifically records delivery to
a client surface; no application target or admitted discrete panel input was
present, so its absence is expected and does not identify a diagnostics gap.

The next attended run at
`.artifacts/lom-panel-native/20260914T005638Z` used Sophia `d8264884` and Lom
`d09e100`. Its protected GPU preflight again selected the physical Vulkan
adapter and completed two sequential renders. In the native session Sophia
prepared output 1, then exited with `native submit did not retain its content
identity`. The compositor advanced `pending_content` into renderer ownership
only inside an optional `ScanoutExportPending` report arm, although the worker
can start on a tick whose report carries no submit record. A later accepted KMS
submission then found the renderer-owned identity slot empty and correctly
failed closed. The first bounded repair observes the worker transition
independently of that optional report on singleton and mirror paths, moves the
exact identity once, and still rejects absent or competing ownership. It also
reports the selected ownership slot and worker state if the invariant can still
fail. Deterministic tests pin both the state transition and its position before
optional report handling; the next attended run remains the causal test. It
must also establish stable two-output presentation and action delivery, which
the source repair alone does not prove.

The attended `20260914T011705Z` run on Sophia `56fc17a2` and Lom `d09e100`
confirmed that repair's native effect: the 20-second session recorded no
runtime fatal and repeatedly prepared, presented and completed content on both
outputs. The strict verifier still rejected the run because Lom acquired 21 GPU
grant epochs and logged 20 stale `ResourceBegin` outcomes. Its two slots per
output were numbered `[1,2]`, then `[3,4]`, making first use `1,3,2`; Sophia's
grant-wide resource high-water correctly refused the newly introduced 2 after
3. Lom `fec727d` assigns all output-primary IDs before any alternate ID and pins
first use `1,2,3,4` in the real-socket two-output lifecycle. Pointer clicks in
the rejected run reached chrome or had no current content target, and no
presented-content action was accepted. A successor attended run must show one
stable grant, repeated generations on both outputs, and the exact action path;
the source fix is not that evidence.

The successor run at `.artifacts/lom-panel-native/20260914T012810Z` used Sophia
`feca59c8` and Lom `fec727d`. No stale `ResourceBegin` remained, and both outputs
again reached native presentation, confirming the grant-wide resource ordering
repair. The shell still restarted: its first epoch timed out waiting for
`ResourceReleased` after output 1 candidate 3 replaced candidate 1; its second
epoch timed out waiting for `Presented` after output 2 candidate 4 was prepared.
The first timeout has a deterministic lifecycle cause: the CPU framebuffer
reuse cache retained the old output damage snapshot, and with it the old content
resource lease, solely while waiting to recycle a busy composed allocation.
The repair detaches source leases from that conservative damage baseline while
keeping the copied framebuffer in the bounded reuse pool. The second timeout is
not valid evidence of a presentation bug: the seat suspended 206 ms after
candidate 4 was prepared because the operator left the test VT, and no native
presentation was possible afterward. Source review did find an independent
obligation gap: a byte-identical accepted shell candidate could be suppressed
as an unchanged retained scene and never receive its distinct retirement. The
repair tracks output-local shell candidate retirement debts so pixel equality
cannot satisfy them. Headless controls prove old-resource release while the
copied framebuffer remains in flight and distinguish fresh candidate retirement
from ordinary identical-scene suppression. Review of the suspended candidate
also exposed a lifecycle boundary: content service could acknowledge Prepared
in the owner-loop pass that then quiesced native presentation, and a topology
replacement could otherwise orphan an output-local retirement debt. VT and
topology quiescence now revoke and pause the shell connection before detaching
native ownership, clear retirement claims only after that revocation, and
negotiate a fresh grant after an active native owner returns. Content intake is
suppressed while the seat or native frame service is unavailable, and topology
rebind refuses to discard any remaining claim for a removed output. A removed
output regression pins that refusal and the explicit post-revocation cleanup.
The next attended gate must remain on TTY4 until the command itself returns;
native stability and workspace action delivery remain unaccepted until it
passes without timeout or grant replacement.

The attended successor at
`.artifacts/lom-panel-native/20260914T101231Z` used Sophia `a98863ce` and
Lom `fec727d`. Exact RADV `drm_dev_t` admission passed, both outputs repeatedly
reached native presentation, and output 1 presented its clock-refresh candidate
3 after its initial candidate 1. Lom then timed out waiting for
`ResourceReleased` for candidate 1's resource, reconnected, and repeated the
same sequence. The renderer worker's buffer-age history was the remaining
owner: a successful mixed render cloned the complete output damage snapshot,
including every `ContentResourceLease`, into each GPU buffer slot's semantic
history. Those pixels had already been copied into the slot, so the history
needed node, generation and geometry but had no storage claim. The repair stores
the same conservative lease-free snapshot used by the CPU framebuffer reuse
pool. A focused regression uploads a real shell resource, records the completed
GPU-slot write, retires the resource and requires immediate `ResourceReleased`;
restoring the raw snapshot makes that regression fail. This source repair does
not turn the failed run into acceptance. A new attended run must show the old
resource release, no `ResourceRetiring` timeout, a stable grant and continued
presentation on both outputs.

#### In-progress generic lifecycle repair

The implementation checkpoint adds a shared owned native batch queue. All
outputs are validated before queue admission; a later-output refusal returns
the offered frame owners and preserves the previous queue. A queued frame
that owes a distinct retirement cannot be replaced by a newer ordinary repaint.
That obligation follows its native content through pending, rendering and
submission. Ordinary repaint service skips a protected output and retains its
repaint request while other outputs progress. Retained head plans also keep
their logical checksum, so unchanged outputs avoid redundant work; equal pixels
never waive a new candidate's distinct retirement.

Device-hidden integration now drives actual `set_shell_content`, retained
projection, Engine planning/lowering, the owned queue, pending Mixed frames,
output frame history and persistent scanout custody. Worker copy, framebuffer
cleanup and flip completion are simulated. A 1,000-cycle two-output control
keeps CPU and worker history caches alive, captures 2,000 Engine activations,
holds one real old byte consumer while newer work proceeds, and observes one
exact release when that consumer ends. Its four resource IDs stay within 96
source bytes between uploads and 128 bytes during upload overlap; owned resource
credits reach zero after teardown with historical metadata still alive. These
are fixture bounds, not production GPU residency or latency measurements.

Session action cancellation retains its response credit beyond the deadline
until the FIFO owns Cancel. Cancel requires no acknowledgement and cannot
retract an admitted WM effect. A private socket control exercises the actual
ledger-to-FIFO transfer. Candidate selection also skips outputs waiting for
presentation rather than hiding another output's ready candidate behind them.

Development evidence is retained in
`.artifacts/shell-lifecycle-dev/native-batch-progress.json`. The mutation controls
restore early resource release, lost retirement protection, equal-pixel
retirement suppression and premature cancellation collection; each must fail
its corresponding regression. These results do not close t098/t100: the
mirrored native adapter and full Session/FIFO/WM/client chain still need joined
integration coverage, followed by frozen-source review and a device-hidden
canonical gate. No new hardware run, native acceptance or release promotion
is implied.

Checkpoint validation: affected library tests passed (backend 130, runtime 8,
Session 440 with 13 ignored, shell-client 4), as did strict Clippy on backend,
runtime and Session libraries/tests and formatting. Eight production-queue
controls include reconnect reuse of candidate numbers: an old grant's pixels
cannot establish a new grant's Presented identity. A pending same-grant frame
keeps the exact previous presented targets until its own retirement. These are
scoped device-hidden development checks, not a current-source canonical pass.

The next bounded extraction shares the native installation validator with the
production-queue fixture. The native adapter captures current owner, head and
target identity independently of the queued payload before transferring any
head. Two-head controls retain both real source owners through a second-head
identity, cleanup-capacity or retirement-obligation refusal, then release once
after both consumers end. An identity-check mutant fails the control. The
device-hidden backend library suite passes 122 tests with this extraction;
this adds installation-validation coverage, not native mirror completion.

Mirror physical custody, cohort flip and primary logical timing now pass through
one shared completion function called by the native adapter. Six controls use
actual boxed copied buffers and simulated completion/cleanup. They cover a
delayed sibling while the primary advances, secondary-first completion,
predecessor cleanup failure, stale/wrong native identity, group-only poisoning,
an older cohort completing after a newer abort, and wrong supplied cohort
identity. Poison cannot mint a new cohort Presented; an already-terminal
Presented remains intact. A supplied cohort must match the current output,
frame, head set, primary and prepared target before custody moves. The native
completion report independently preserves outstanding cleanup state.

The two review findings and their source-level closure are recorded under
`.artifacts/mirror-completion-review/`; six-test positive and compiled poison,
cohort and physical-guard mutants are retained under
`.artifacts/shell-lifecycle-dev/`. This is completion-boundary integration, not
execution of the worker, mirror installer or KMS. Joining those boundaries to
the actual intake/copy path and the Session/FIFO/WM/client chain remains open.
The reviewed completion checkpoint passes 590 affected-library tests with
13 ignored and strict affected-library Clippy in the device-hidden wrapper.
This is scoped validation, not a canonical or physical acceptance result.


The next checkpoint joins the production composition installer and shared
mirror reservation policy to actual `set_shell_content`, Engine lowering,
owned deferred generations, `PendingRenderedFrame::Mixed`, copied backing
custody and shared mirror completion. The two-output 1,000-cycle workload now
also runs with two physical heads per output. Only device/copy/completion
operations are simulated; this does not exercise a real renderer worker,
exporter or KMS. The full Session/FIFO/WM/client action chain remains open.

An ordinary retained offer may suppress a mirrored neighbor only when every
current head has exact displayed native identity and the same frame/checksum,
the live group is converged, and actual queue/exporter/worker/prepared/submitted
owners are idle. Pending cleanup conservatively disables suppression. Required
identical content still mints a distinct frame. Shared validation and lifecycle
reservation precede the first owner transfer; returned refusal retains the
whole generation. No panic/unwind retention claim is made.

Device-hidden evidence: 134 backend library tests pass, including the mirrored
1,000-cycle test, whole-owner reservation refusal/retry, lagging heads, exact
unchanged-neighbor preservation and deferred ordinary change-back. The latter
asserts equality with both actually displayed checksums and preserves the
lowerer's scene identity. Strict backend Clippy passes with the integration
target's required `libinput-events` feature; the incomplete-feature invocation
is retained as a failure, not a pass. Scoped logs are under
`.artifacts/shell-lifecycle-dev/mirror-*`. No native run or full-tranche source
acceptance follows from these controls; frozen review and broader validation
remain separate gates.


The frozen mirror checkpoint `9ee3b6c834e873e3cbac09197325282ce227d3ff`
received independent scoped source approval: 134 backend library tests and
strict backend Clippy passed, and reservation, required-identical and
queue-idle mutations each failed behaviorally. This does not broaden the
simulated-copy/device evidence into worker/exporter/KMS acceptance.

The next action checkpoint joins a real private socket, generic
`ContentLifecycle`, paired ACK/indicator outbox, Session action ledger and the
shared WM admission queue. Production now delegates publication validation,
checked serial minting, output scoping and queue-capacity admission to the same
borrowed owner used by the fixture. Both ACK-first and activation-first service
orders pass; unpublished/wrong-output/capacity refusals preserve the queue.
Prepared activates no targets, Action-before-Presented is rejected, and Cancel
produces no ACK. Restoring an ACK prerequisite or bypassing publication checking
makes the corresponding control fail after compilation.

Presented and committed policy publication remain supplied fixture inputs;
linking the WM queue result to the ledger is still performed by the fixture.
The physical owner loop, admission outcome under backpressure, actual policy
execution and subsequent matching presentation are not established by this
checkpoint. Next: retain the exact indicator outcome after WM admission until
FIFO transfer succeeds, without replaying the admitted cause.

Private lifecycle and action fixture bodies now live in `tests/support`, with
narrow owner-access mounts documented in the style guide and exceptions. The
corrected device-hidden development wrapper generates a private loader cache
and preflights its explicitly mounted `rg`; missing-cache and missing-tool
attempts remain retained failures. Relocated affected-library validation passes
591 tests with 13 ignored. These are scoped checks, not a canonical release or
attended acceptance result. Logs: `.artifacts/shell-lifecycle-dev/client-wm-*`.


The subsequent response-owner slice makes Session consume indicator requests
through the typed transport admission path. Before inbox removal, one exact
request owns an aggregate control credit. The transport records its first
completed outcome before fallible encoding/FIFO admission, refuses a conflicting
completion, and retries only that outcome from `poll_io`. Pending requests
cannot be delivered to WM again. FIFO ownership clears the plain pending record
without another allocation, callback or I/O; partial writes retain the frame's
credit through the final byte. Reconnect/disconnect invalidates the old owner.
Content and descriptor-only peers both account for the reservation.

Three external private-owner controls exercise record/byte credit, partial
FIFO drain, exact result retention, and no readmission. The post-reservation
refusal control deliberately reduces a fixture limit: it is a defensive
returned-refusal test, not an observed kernel-backpressure schedule. Two
compiled mutations (missing credit and dropped refused result) fail. The real
private-client roundtrip now receives the exact Accepted outcome after shared
WM queue insertion. The owner loop's complete execution and policy/native
causality remain open. Scoped validation: 602 affected-library passes,
13 ignored, strict affected Clippy and layout pass, no hardware. Evidence:
`.artifacts/shell-lifecycle-dev/indicator-response-*`.

The next owner-loop extraction calls one shared activation service from both
`physical_input_phase` and the private-client fixture. It owns typed request
intake, publication classification, descriptor-only event high-water, linked
presented-action validation, actual WM queue admission, ledger completion and
exact transport response. Poll failures retain the previous recover-at-caller
behavior; WM/finish failures still propagate. One sampled monotonic elapsed
time now covers this bounded decision; ACK service samples its own time.

Five private-client controls cover both ACK orders, unknown/stale/capacity and
duplicate outcomes with exact WM call counts, direct-mode snapshot/high-water,
and empty-intake no-replay. The same production borrowed WM queue is used for
the accepted roundtrip; native Presented and committed publication are still
supplied facts. Bypassing publication or linked-ledger checks compiles and fails
the intended call-count assertions. Session library: 445 passes, 13 ignored;
strict affected Clippy and layout pass. This is shared decision orchestration,
not an execution of the entire physical owner loop or WM peer/native pipeline.
The exact earlier signed `2e569301` separately passed device-hidden canonical
`cargo xtask check` at `.artifacts/offline-check-2e569301/report.json`;
that canonical result excludes this extraction. No hardware ran.

The causal-evidence follow-up returns the WM queue attempt's actual activation
serial and policy connection epoch from the shared admission owner. Session
records that receipt only after updating the ledger and handing the exact
response to transport ownership. Accepted issuance and exact validated ACKs
carry `CLOCK_MONOTONIC` microseconds, connection/grant/event and presented target
identity. Invalid, duplicate and late ACKs emit no validated-ACK record. The
policy settlement record binds the same policy epoch/activation serial to the
reducer's resulting indicator generation and explicit outcome; queue admission
is not called policy execution.

The structured recorder has a record-specific numeric/enum allowlist with
bounded integer parsing and payload rejection. The real private-client control
checks that the returned receipt names the serial actually inserted into WM's
queue; sanitization controls cover the new vocabulary and malformed fields.
These are the first half of the causal chain. Native timing must still bind the
originating render revision/candidate to an exact completion witness, and the
workload verifier must reject missing or ambiguous joins. No latency acceptance
is inferred from these diagnostic records or their fixture values.


The native timing slice emits a candidate-to-native binding only after the
actual owned queue admits its fresh retirement frame. Binding includes the
native owner incarnation, output/frame and every head/target generation.
Completion is a separate observation of exact displayed custody: every current
head must display that same native frame, and a mirror must have nonfailed
convergence. The timestamp is the maximum associated head completion time.
This does not change primary-driven logical publication or input activation.
A lagging sibling leaves all-head timing unavailable; a later unrelated frame
cannot supply the missing witness. Capture must retain the first exact record
rather than infer timing from Session polling or FIFO enqueue.

Head completion now retains the timestamp provenance from the native reducer.
Evidence distinguishes kernel UST from local observation fallback and flags
missing kernel timestamps. A fallback may describe real completion, but cannot
silently satisfy an exact kernel-completion latency gate. Controls cover both
mirror completion orders, exact and mismatched displayed identity, missing and
duplicate heads, and mixed kernel/fallback provenance. The bounded recorder
captures both native records even with ordinary tracing disabled. These are
simulated completion/source and recorder controls, not KMS or latency results.
Originating Lom revision joins, complete workload accounting, numeric latency
limits and the exact-source release/attended gate remain open.


The next evidence slice records the exact enqueued indicator states without
labels, adds target identity to issuance/ACK evidence, and has Lom name its
captured revision with exact grant/candidate/presentation identities. The new
`tools/probes/lom_workload/verify.py` rejects missing, contradictory or unrelated
joins across issuance, validated ACK, WM serial, committed indicator state,
originating Lom candidate and all-head native completion. It checks every
sample and independent per-output nearest-rank p95 and maximum limits. Its
synthetic transcript/CLI controls are registered in the existing Lom verifier
gate. These controls execute no Session, policy peer, renderer or KMS pipeline.

The verifier explicitly reports `scope=causal_action_latency`. It does not yet
close refresh-rate evidence, memory plateau/teardown credit reclamation,
process stability, recovery or the attended launcher. Budget values in its
fixtures are test inputs, not a recovered numerical operator approval. Workload
limits must be fixed before the native attempt; the proposed numerical defaults
are not yet ratified. t081/t098/t100 and the paired Lom acceptance stay open.

The accounting follow-up reads the existing active/retired stores and transport
inventory without a shadow registry or release claim. It reports resource IDs,
transfers, candidates, allocations, demands, permits, storage/backing charges,
reserved response records/bytes and retained input. A partial output write
retains its whole record charge; observations neither drain responses nor
collect owners. Explicit collection still depends on actual consumer release.
These are protocol/storage charges, not process RSS or GPU residency.

Device-hidden controls cover 37 resource/candidate/private-socket cases and 11
runtime library cases, including unchanged accounting through enqueue refusal,
credit transfer and partial write. Strict runtime Clippy and layout pass. One
control deliberately ends the renderer's consumer while leaving its submitted
candidate unresolved: the snapshot must remain non-quiescent. This does not
prove normal Session shutdown resolves that obligation. Connecting bounded
normal exit, exact terminal settlement and post-cleanup accounting remains
required; a watchdog exit or destruction of the accounting owner is not evidence
of successful reclamation. No canonical release or native acceptance is claimed.

Native binding evidence now also carries the existing current render target's
`mode_refresh_millihz`. Each physical head must name a stable mode of at least
60 Hz, including mirrored siblings. The workload verifier rejects absent or
invalid rate evidence and reports the actual recorded rates per head. This
uses no default and changes no mode or scheduling policy. It qualifies mode
selection only: observed cadence, VRR behavior and driver health are separate
from the mode's nominal rate. Synthetic transcript and recorder controls do not
establish any native hardware rate.

The final-shutdown follow-up closes shell admission before native draining.
After the owner loop and its runtime/CPU scene have ended successfully, Session
requires quiescent native custody and explicitly joins the current exporters'
and mirror groups' renderer workers within two seconds. Only then does it drop
the remaining native owner, settle exact submitted identities in disconnected
stores, collect actual consumers and emit the final accounting snapshot.
Worker destruction alone was insufficient: its existing nonblocking fallback
can retain an unfinished thread in the bounded worker registry. The explicit
join covers current owned workers, not a global claim about historical workers
or driver allocations. Failure and timeout retain unqualified shutdown status.

Device-hidden controls cover real thread completion, a blocked payload
destructor with an independently progressing neighbor, repeated join/panic
results, live-epoch refusal, exact disconnected settlement and an independently
held pixel consumer. Mutations reporting an unfinished thread as joined,
skipping the candidate terminal and discarding retained consumers each fail
their intended control. These compose shutdown boundaries; they do not execute
KMS teardown or establish a normal native exit. Reconnect-time settlement remains
separate. The launcher still needs bounded normal exit, workload/plateau checks
and strict verification of this final snapshot before attended readiness.

Workload inventory now uses the existing five-second bounded owner-loop sampler
to observe actual content stores, aggregate transport charges and their immutable
negotiated ceilings. The verifier brackets the complete action window, rejects
sampling gaps over six seconds, checks four total reusable slots and pixel bytes
bounded by twice the exact two-panel sizes, and refuses warmed resource-ID growth.
It also checks negotiated and workload-specific candidate/allocation/queue bounds.
All final ownership/credits must still be zero. This measures protocol storage;
RSS and driver allocations are separate populations, and sampled plateaus do
not substitute for production admission or the retained-cache 1000-cycle controls.

#### 2026-09-15 native crash and queue/startup repair

The operator's `20260915T223914Z` capture ran Sophia `82342681` with Lom
`7e3b4cc7`. GPU preflight passed, but Session exited 1 in KmsSubmit with
`composition output already owns a distinct retirement`. Output 2 had presented
while output 1 still owed its exact retirement; a WM update preceded the fatal
ordinary Scene admission. A separate earlier topology attempt rolled back with
`native output topology first-frame coverage is incomplete`. The capture does
not establish that rollback caused the later fatal. The preserved report and
19 hashed original inputs are in
`.artifacts/lom-native-failure-20260915T223914Z/`. This failure supersedes the
candidate's earlier readiness claim; it is not evidence of a GPU driver crash.

The repair separates replaceable ordinary Scene admission from forced topology
and explicit retirement requests. A blocked ordinary output returns no new
frame ID and retains one output-local repaint obligation. Service recomposes
current retained sources after protection clears, without another external
event; other outputs progress. Actual queued/submitted owners are unchanged.
Ordinary CPU entry points still offer outputs independently: do not generalize
the topology whole-batch guarantee to an entire CPU cycle. Deferred retry does
not establish Session CPU visual-progress or native latency correlation.

Topology admits its entire first-frame batch into the real owned queue before
arming. Arming compares those owned frames with returned IDs and independently
current native owner/head/target identities, rather than demanding exporter
pending state before service is allowed. Native shell construction is prepared
without launch; the startup transaction and execution must both settle before
negotiation. First success retains `ready` epoch 1; only later successes report
`reconnected`. No exception was added to the single-grant acceptance check.

Device-hidden regression evidence is in `.artifacts/shell-lifecycle-dev/`:
`repaint-red.log` reproduces the original exact fatal; `repaint-backend.log`,
`repaint-retry.log`, and `repaint-session.log` cover output-local deferral,
changed WM outline retry without another event, invalid second-output refusal,
owned topology coverage and private socket startup negotiation. The startup
fixture supplies protection evidence; it is not a protected-child/GPU launch.
`repaint-mutations.json` records compiled behavioral negatives. Existing actual
owner/1000-cycle fixtures retain simulated device completion scope. Canonical
validation and native acceptance remain separate gates; no new hardware run is
implied by these controls. Keep t100 and t081 open until their full exits pass.

#### Attended e29 follow-up: configuration ownership

Keyboard bindings remain in the selected WM desktop profile. The Lom gate must
not introduce its own Super+number list. Its recorded profile is composed offline
from the operator's expanded WM profile plus explicit probe overrides for shell
content, the test catalog and empty application startup. Policy, shortcuts and
application commands remain from the WM profile; the probe refuses replacements
of policy or shortcut definitions. An explicit Lom executable selection replaces
only the test shell. Original configuration files are never rewritten.

The `sophia-config` `desktop_profile_probe` example uses the production parser
and source renderer; Session reloads its output as ordinary KDL. Controls retain
custom chords, included bindings, literal command arguments and named application
references while proving empty startup. The actual Session staging control uses
that composed profile and a registered workspace action: the complete operation
catalog commits and the missing-slot catalog refuses. These are offline controls,
not proof that a physical Super chord was delivered or switched a workspace.


The final-exit repair keeps the visual runtime, CPU scene, native retirement
slot and renderer-image handoff outside the owner loop. The production final
transition is shared with a fixture using simulated drain/join/disposition
operations and real retained byte owners. A revoked seat cannot call drain;
failed disposition requests worker shutdown but retains the runtime and scene.
The terminal error carries those owners and the prior error. Exact completion
precedes handoff release and final shell accounting. An explicitly headless
profile returns no native receipt; missing ownership in a native profile still
refuses. Join success alone cannot establish revoked scanout disposition.
Resume borrows the retained renderer-image handoff; each import receives only a
duplicate of its plane descriptors. The original handoff is released after the
entire resume succeeds, and stays in the outer error carrier on refusal. Ordinary
socket-descriptor controls establish duplication lifetime, not DMA-BUF import.
Successor admission also requires the prior exact completion even when the
retiring slot is empty; absence alone cannot replace a live owner's identity.

Author device-hidden controls cover retained ownership through an unrelated
error, later continuation, no duplicate shutdown request, revoked no-drain,
missing/old completion and a successor owner. The compiled drain-after-revoke
mutation fails its no-call assertion. These controls do not execute native
KMS or establish an attended shutdown result. Frozen review, canonical validation
and native acceptance remain separate pending gates.

Frozen b85 review found two retirement integration gaps despite its canonical
PASS: the old pre-return completion path could still call native cleanup after
revocation, and revoked detach replaced the output runtimes before their affine
custody was inspected. The successor gates the actual completion effect groups
on retained seat authority and keeps the existing output-runtime owners during
detach while invalidating input projections separately. Renderer-image clear
also requires output custody disposition, not merely a detach report.

Resume and topology rebind reject unresolved suspended custody before constructing
replacement output state, enabling workers, importing images or assigning a new
set. Session also checks retained runtime disposition before constructing a
replacement native owner. This is retain-and-report behavior: an unresolved
revoked owner can refuse recovery and remain in the terminal error carrier.
It does not introduce a revoked-resource release protocol or establish successful
VT resume. The adopted real-runtime Arc control covers retained displayed custody
through repeated revoked detach; shared completion-gate effects are simulated.
Frozen review and a new exact-source canonical run are pending for this successor.

### t101

The daily component login exposed a
[launch-reload provider capability regression](../investigations/c9di7qpg-independent-shell-providers-were-omitted-from-desktop-launch-reload-validation.md).
Its deterministic repair preserves provider ownership during terminal changes;
installed acceptance must observe successful reload and the new terminal without
restarting the shell components. This does not close the broader t101 gate.

Daily application failures also require
[private bounded stderr and launch records](../investigations/j4rp6twa-application-stderr-needs-a-private-bounded-owner.md).
Deterministic process/storage checks and an installed application launch are
separate evidence; neither recovers output discarded by an older release.

Support one combined content/descriptor shell in the shared client boundary,
including bounded message demultiplexing and terminal outcomes for every
negotiated workflow. Lom currently consumes content and indicators; receipt of
other negotiated records must not fill an unserviced queue. Keep the original
descriptor switcher/launcher trust and activation rules. No second Narthex
connection and no custom raster launcher authority is implied.

Package exact Sophia and Lom commits, dependency lock, binaries, profile and
private KDL assets. The verifier must distinguish requested `gpu "direct"`,
actual launch evidence and granted content/input capabilities. Retain the
selected policy client identity and a known Narthex profile/artifact for rollback
in a separate session. Coordinate existing packaging tasks rather than marking
them complete from this narrower artifact.

Exit: combined-client fixtures preserve descriptor controls through content
updates and reconnect; exact package and profile validation pass with identity,
missing-resource, old-quota and permission mutation negatives. Document the
authorized install/run/rollback procedure. No GUI, signing prompt, session
restart or install follows automatically from a passing offline verifier.

### t081

Use the paired package to prove actual native panel pixels/reservation on every
admitted output, empty-focused-output styling, workspace activation, clock,
calendar interaction/dismissal, descriptor launcher/switcher behavior and
recovery. Tie input to exact native retirement, not `Prepared`, renderer
completion or a synthetic `Presented` record. Narthex is tested as rollback,
not simultaneous native-shell coexistence.

Lom t008 owns the attended matrix and t009 the measured workload/soak; this task
retains the Sophia-side evidence and incident links. Record source, installed
binary hash, effective profile/grants, device/driver, topology, refresh and
observation duration. Define workload and latency/resource acceptance budgets
before running. Report action and frame p50/p95/p99, upload/readback/copy costs,
idle wakeups, queue depth and retained bytes. Check that old generations drain
and warmed storage does not grow with repeated actions, not merely that one
sample is below a limit. Report driver/device incidents under direct-access
limitations, never as guarantees the architecture cannot make.

Exit: attended matrix and soak pass for the exact candidate with a tested
rollback; failures reopen the relevant task. GPU or simulated-client evidence
alone cannot close this task. Broader desktop acceptance tasks retain their own
exits. Lom t010 records promotion only after both repositories agree on evidence.

### t102

Candidate only; paired with Lom t016. Measure the full CPU-byte path before
promoting memfd or DMA-BUF transport. A promoted design must retain immutable
acceptance, exact target pairing, synchronization, recipient isolation,
cross-device/format behavior, finite overlap accounting and final release.
Compare end-to-end latency and bandwidth on the same workload. Do not claim
zero-copy from a handle transfer. Lom t017 first measures reuse of existing
immutable resources/placement tables; r5 already supports tiles. Deltas remain
separately specified future vocabulary.

### t103

Candidate only; paired with Lom t019. A GPU bridge requires a concrete isolation
or performance need the direct path cannot meet. Select a bounded job/adapter
contract, enforceable accounting owner, scheduling and failure model, a second
independent client/renderer proof, and measurements before implementation.
Opaque commands, a process boundary or one job at a time do not establish hard
GPU-memory isolation. No custom kernel, mandatory Vello API or new display
server is admitted by this candidate row.

## Connections

The [implementation record](../../lom-content-implementation.md) owns historical
claims. [Content shells](../../content-shell.md), the
[wire ADR](../decisions/6ndjwffd-content-capability-design-for-sophia_shell_v1.md),
[target-resolved input](../../target-resolved-input.md), and the
[successor GPU decision](../decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md)
own the contracts. [Sophia tasks](../../../todo.md) and
[Lom tasks](https://github.com/sophia-org/lom/blob/master/todo.md) own status.

## 2026-09-16: output-targeted actions and globally numbered workspaces

The `20260916T102653Z` attended capture remains immutable. It contains twelve
issued/acknowledged/admitted actions split across both logical outputs and no
shell transport failure. Operator evidence reports DP-2 clicks changing DP-1,
DP-2 staying on workspace 1 and flashing. Source inspection found the output
was validated at shell admission but discarded into a legacy WM Action;
Hagia then executed against the active output. Affected-output ordering is
coverage and cannot repair that loss.

The repair adds capability-gated explicit output/generation action requests,
plus capability-gated opaque configured output keys. Hagia owns assignments:
DP-1 key 1 → numbers 1–3; DP-2 key 2 → numbers 4–6. Keyboard numbers select the
current owning output in one policy proposal. Bar actions retain their exact
output, independent of coverage order. Both protocol additions preserve old r3
layouts; unnegotiated clients receive neither extension. Lom's existing generic
label/action path requires no monitor-specific logic.

Checkpoint schema 18 retains keys and preferred ownership across changed runtime
handles and restart. Existing fallback migration retains global numbers on
unplug; reconnect restores preferred views. Initial legacy migration preserves
view/window identity and refuses ambiguous membership. Established assignment
or Session key changes are refused rather than silently renumbering live state.
Configuration, including all Super bindings, stays in the selected WM profile.
The gate now builds exact signed Hagia source and records its commit/hash.

Validation: the affected Rust run passed 1,568 tests (14 ignored); the explicit
real-Hagia socket control passed separately, as did the connector/key mapping
control. Strict affected Clippy and layout passed. Rust/C/Nim wire controls,
177 Hagia model controls, 108 other Hagia controls and 29 launcher/workload
controls passed. A compiled first-output retargeting mutant fails the real-Hagia
control; restored source passes. Final canonical results are recorded against
the exact signed commit, separately from these scoped runs. These checks do not
establish physical connector
mapping, eliminate the reported DP-2 flashing, or close the 40-action workload.
No VT, hardware, install or live reload is part of this implementation slice.
The next attended run must separately verify two disjoint number sets, exact
clicked-output changes, global Super+number selection, steady per-output
indicators, no flashing, and clean resource retirement.

The prepared next-run profile is `~/.config/hagia/lom-workspaces.kdl`, copied
from the selected WM profile with only six assignments and two keys added.
`~/.local/bin/lom-test` selects it by default while respecting an explicit
`SOPHIA_DESKTOP_PROFILE`. Original desktop profiles were not rewritten and no
live reload occurred. The exact profile source/hash and launcher backup are
retained under `.artifacts/workspace-repair/`.

Regeneration exposed a stale WM-generator shell-schema check (revision 4 versus
the existing revision 6) and C corpus coverage missing existing extension rows.
The generator now validates the full existing r6 message set; the independent
C codec covers all sixteen current record rows, including the new policy key.
No archived revision-3 bytes or archive digests were changed.

## 2026-09-16: workspace-switch bar continuity

The subsequent immutable `20260916T114128Z` capture has fifteen issued,
acknowledged and admitted clicks (six on logical output 1, nine on output 2),
zero transport failures/runtime fatals and native exit 0. The operator observed
correct mouse/keyboard switching and disjoint workspace labels, but a flash of
both bars on switching. The complete 40-action workload was not performed;
its verifier remains failed for the action count, not accepted by inference.

Source inspection found a separate composition omission: the policy-update CPU
callback rebuilt surface chrome, tab bars and outlines locally, omitting shell
content and descriptor overlays. At 11:42:19.209Z the capture queues frames 26/27
with the empty-scene checksum on both outputs; at .254Z retained composition
queues the bar-bearing successors. Those are queued-frame observations, not
independent pixel/scanout measurements. They corroborate the reported flash but
do not substitute for attended verification of the repair.

Policy-cycle capture and retained/ordinary repaints now use one complete,
output-local composition builder. The captured CPU inputs keep actual owned
shell sources while coordinator/output borrows run; historical metadata cannot
stand in for those leases. The application → decoration → shell → descriptor
overlay order and output confinement are preserved. Native queue/retirement,
input and revocation semantics are unchanged.

Controls exercise the production capture/builder, Engine lowering, real owned
queue and simulated completion while no replacement shell frame arrives. Both
bars survive repeated empty-workspace cycles and refused queue admission. A
second control preserves application/shell/overlay ordering and retains the
captured source until exact resource release after it drops. Restoring the
shell omission compiles and fails the continuity control. This is not execution
of the physical owner loop or KMS. Existing delayed/mirror/1000-cycle controls
remain distinct evidence.

Lom separately compares resolved output-local visual state. A revision-only
interaction update reuses its existing immutable resource through a new paced
candidate; it does not perform GPU render/readback/upload or retire that resource.
Targets still change only at exact Presented. Candidates requiring fresh native
retirement still receive it, including identical raster candidates. Candidate
logs add `raster_source=rendered|reused`; this is client work evidence, not a
native completion timestamp. No shell-wire change or Lom-specific compositor
policy was added. The next authorized `lom-test` must establish no visible flash,
continued click/shortcut correctness on both monitors, complete workload
outcomes, stable connection/resource ownership and clean shutdown.


### Click continuity follow-up (2026-09-16)

The attended `20260916T230302Z` capture on Sophia ff1c78fc / Lom 72624d3
contained 26 captured button sequences, 19 activated actions, and seven releases
without activation. Every one of those seven crossed a new presentation on both
outputs. The old Engine equality required the candidate/presentation numbers to
remain unchanged. Diagnostic sanitization dropped cancellation status and several
identity fields, so the retained record alone is not an exact cancellation-reason
trace. The source and deterministic regressions establish the redraw mechanism.

The repair gives each continuously presented equivalent target a server-owned
non-reused token, assigned at production projection publication. Release names
the current exact presentation; queued actions retain their original identities
across equivalent updates. Incompatible transitions still invalidate and suppress
release. Lom separates button lifetimes from indicator publication revisions.
Neither a pixel checksum nor the latest model authorizes capture continuity.

Evidence is retained under `.artifacts/click-continuity`: Engine field/ABA controls,
actual backend intake/lowering/queue/projection with simulated completion over
1,000 two-output refreshes, 1,000 Session socket/client/shared-WM actions on one grant (500 per output)
with supplied presentation facts, and Lom socket/raster/registry tests. These are compositional
headless controls, not one full physical owner-loop/KMS execution. Mutations and
exact-source gates are recorded with their own logs. No native run is performed
by this repair; t081 and Lom t008/t009 stay open.

The capture also exited 1 with `window allocation frontend disconnected` and a
Drained retirement receipt. Its cause is separate and unestablished by this slice;
click continuity is not a shutdown repair or clean-session acceptance. The next
authorized attended matrix must count all 40 intended switches (20 per output),
include holds across clock refresh, and retain shutdown outcome independently.

### Allocation publisher shutdown follow-up (2026-09-17)

Attended capture `20260917T000849Z` used Sophia 855138f9 and Lom a317370.
All six captured presses activated, were acknowledged and joined to committed
WM actions (four on output 1, two on output 2). One sequence on each output
crossed a new presentation and still activated. This is six-action evidence,
not the full forty-action workload or a latency acceptance result.

The separate exit-1 cause is now localized: quiescence started at monotonic
509947275 msec, frontend drain was observed at 509947346, and the owner-loop
allocation publisher subsequently attempted a new update on the closed service
channel. It reported `window allocation frontend disconnected`; outer native
retirement nevertheless completed as Drained and shell inventory reached zero.
The captured quiescence reason was stripped by the old diagnostic allowlist, so
the shutdown trigger is not inferred from that missing field.

The shared quiescence entry now irreversibly stops optional allocation
publication before requesting frontend drain. Pending metadata acknowledgements
are cancelled locally, never promoted to Applied; the previous applied witness
remains available to outstanding presentation comparisons. The stopped publisher
never queries native allocation preferences or sends another update. Active
channel loss and a failed drain request remain errors. Accepted authority work,
coordinator work, CPU/native progress, actual frontend join and retained native
owner disposition still determine completion independently.

Evidence in `.artifacts/allocation-shutdown` covers the production publisher and
shared shutdown entry using real channels, including a pending acknowledgement
already disconnected or already queued, repeated shutdown, active channel failure,
queue-full generation accounting and quiescence with outstanding work. Native
facts are supplied only to the existing quiescence reducer; these tests do not
run the complete owner loop, frontend workers or KMS. Compiled negatives and
exact-source canonical results are retained separately. No live action is part
of the implementation. Native exit-0 acceptance remains pending; t081 stays open.

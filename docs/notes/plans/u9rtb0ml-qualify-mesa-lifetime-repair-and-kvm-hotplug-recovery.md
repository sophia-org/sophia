---
id: u9rtb0ml
date: 2026-10-09
kind: plan
tags: [plan, rendering, topology, validation]
---
# Qualify Mesa lifetime repair and KVM hotplug recovery

## Resume progress (2026-10-09)

The newly reported installed-session crash has interrupted qualification.
The [incident account](../investigations/kdleagg3-kvm-output-and-usb-loss-returns-the-desktop-to-greetd.md#installed-session-renderer-failure-interrupts-qualification-2026-10-09)
records `renderer_retained_buffer_missing`, preserved logs and the intentionally
stopped integration gate. Niltempus identified workspace switching. A separate
late-render repair is signed as `bacfdb207` and merged into public master as
`22b124c882be0044fb5fceb7c9bdf5d3c6d6f0f0`. Both the repair and merged master
passed the full isolated CPU gate; independent source and merge reviews found
no blocking issue. The merged gate is `205-workspace-render-recovery/gate-02`,
14:17:33–14:26:39Z, with unchanged source pins. Desktop integration `39fdba80`
pins that exact public revision. Release `niltempus-e3e6a9c375a1bfa4c7bc` built
and passed checksum, source-identity and profile checks in
`205-workspace-render-recovery/release-02`. Niltempus installed it and confirmed
normal login. The read-only `installed-01` check binds the running binary and
session manifest to `22b124c88`; durable presentation-deferral records are
present, with no diagnostic storage errors. Workspace-stall recovery and
physical lock/hotplug acceptance remain separate. No 202 guest has run. The
display attempt/completion correction is signed as `1e63d9c71` and passed
42 focused checks before the full gate was stopped while compiling.

201's unchanged runtime passed the control-only `201-02-controls` successor:
64 orchestration cases, five binding refusals, 168 decision cases and capacity
checks. The original 201 run failed two mistaken fixture expectations and is
preserved as failed. The successor manifest is
`b9d84497b69fe4dbb26f8e189cd9e5787bf08de13376e763c76f221ef7935fb7`;
run records are `55c5339f7e8b8de068f28872620fa1bff04a2b892dd4488dfc6352d3b5df7998`.
Claude froze 201 with READY-201 and manifest prefix `e76032e4`; fresh host
preflight had not begun when both lanes stopped. The declared host kernel for
any later 202 is 6.18.55_1, with the guest still pinned to 6.18.54_1; such a
comparison is not host-identical to 194. Review the crash before resuming.

The first isolated run of 198-03 passed all 105 controls and all fourteen
mutants. Retrospective 194 files match 198-02 byte for byte. Both source checks
passed; the frozen package manifest is
`9619ca6a276318c8e9c9d77f0fbfb5da23a992ebb8fb95dcc83e8f5f3187ac28`.
`198-03-run-records-codex` declares the exact wrapper, independent review and
one-run result, under manifest
`fe33d10715d224bac4ec9ca34bf2a8d6bd6870e77b45a05f8f7aa9b05d4b2431`.
The old Claude run-record manifest has nineteen valid payload entries and
one invalid self-entry; it is preserved with that construction error recorded.
The removal-tail mutant's replacement kill test was chosen after the survivor
was seen. Its mechanism was independently reviewed; this limits independence
of that control selection, without changing the runtime classifier.

Lock implementation `006434bed` now has a passing full isolated gate on signed
`f1effad0daf38e7a9ad4eaef20fb2214dea2ac27`. Signed `3539415dc` moved internal
Session coverage diagnostics into `docs/session-lock-diagnostics.md`, restoring
the provider contract byte for byte without editing SDK snapshots. The only
subsequent source correction reorders one import. Gate 02's formatting failure
and gate 03's overlong test socket path are preserved. Gate 04 used a shorter
private target and passed in 13:00:50–13:07:22Z, with unchanged source pins and
a clean tree. Its manifest is
`c2517d2c837bce6a140311d50d7b01964b02be326732c68c4f343efa9aef63bb`.
Native pixels and physical lock retirement remain unproved by this CPU gate.

The isolated recovery worktree is `~/dev/sophia-recovery-resume`, branch
`candidate/t306-recovery-resume-20261009`. Signed merges `75af47276` and
`497464309` combine the reviewed hotplug candidate with the current lock work
and accepted t310 startup repair. The native resume conflict preserves the
production lock-test boundary, restored-image result and pending-image cleanup.
Notebook conflicts retain the newer accepted-startup account. This combined
candidate still needs its gate and guest qualification; it is not installed.
Claude owns preparation of 201 around the qualified classifiers. No 202 guest
has run. The shutdown checkpoint below remains the historical handoff.

## Resume interrupted by normal-login failure (2026-10-09)

After reboot, niltempus reported that moving the main monitor from DP-1 to
DP-2 prevented normal login and requested robust output handling. That request
promoted t310 ahead of this plan. The installed strict profile refused the
missing DP-1; this was not new evidence of the retained-image fault.

The [t310 investigation](../investigations/ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md#t310)
records the signed startup repair, passing isolated gate and installed release
`niltempus-19efce64b00ae803d566`. niltempus confirmed normal login; read-only
checks bind the running session to Sophia `838d5b16a`, with DP-2 enabled and
DP-1 absent. Its scope is startup and explicit reload; automatic hotplug
recovery remains unqualified. All packages and remaining
gates in the shutdown checkpoint below are preserved. The new release does
not include the unqualified master lock implementation.

## Shutdown and resume checkpoint (2026-10-09 UTC)

Niltempus requested shutdown after the lock implementation was signed. Work
stopped; this is a handoff, not acceptance of t306 or t307. Claude confirmed
that his lane has no background work and that 198-03 remains unrun. The root
gate had already exited on its own; no shutdown signal or replacement run
was needed. No build or guest is left running in either lane.

The implementation is signed commit
`006434bedcb723c2b5d3eafb8c4ecf596ab4c4a5` on `master` in
`/home/niltempus/dev/sophia`. The preceding notebook reconciliation is
`f913a9f0b`. The checkpoint documentation is committed after that implementation.
These local commits are durable; this checkpoint does not claim a push or
an installed release. The last recorded installed release remains
`niltempus-087445319affcb9bfc53` (Sophia `19403a511`); the new lock work has
not been installed.

Preserve these clean worktrees and their evidence identities:

| Worktree under `~/dev` | Branch | HEAD |
| --- | --- | --- |
| `sophia-t306` | `t306-wm-variant` | `254204a743a248c871db65968109bbfad3ccbead` |
| `sophia-context-freshness` | `investigate/t307-context-freshness-20261009` | `9dee187ae0645c1a61268ad3f374545a4f1c9c68` |
| `sophia-hotplug-candidate` | `candidate/t306-hotplug-20261008` | `3fc8b449590049f1661da1896436f5e87f529b6a` |

All package names below are relative to
`/home/niltempus/.local/state/sophia/development-evidence/t306-01/`.

- Lock work: `200-lock-publication-characterization` retains focused checks,
  mutants, source snapshots and `gate-01`. That full gate ran
  11:35:02–11:35:09Z and exited 1 on
  `C SDK contract drift: .../docs/sophia-lock-files.md`, before full validation.
  Its source pins passed and the tree stayed clean. Its retained manifest is
  `b41a3a44f3022d5ac3141c4d53e2cfdcdca909be9019a1505169e16141fdd442`.
  This is a failed prerequisite, not an interrupted or passing gate.
- Premise work: `198-03-premise-callback-classifier` is source-only, awaiting
  review and its first CPU run. Its source-input manifest is
  `a8edd8c6ed571a3e20d4d7742882c1848ce03f455a4223859cfa8c6caa94231b`.
  Both failed predecessors and their logs remain untouched. Durable launch
  and result-hash records are in `198-run-records-claude`, manifest
  `c390bbf4126bf4cfc1fc3594147dad183591dd2461c7369ef6fcf5007645e38c`.
- Pixel policy: `199-pixel-diagnostic-classifier` is CPU-qualified and frozen
  under `a977bfeed5a3d4f31749e60bc0b5f55b9ba1bc983929a9a95b686b0e3491b717`.
  This does not qualify a new guest result.
- Runner work: the two temporary 201 drafts were copied byte for byte to
  `201-mesa-pair-observable-runner/draft-source/`, with `SHA256SUMS`.
  `201-decide_next.py` is `d0aa533c754a9c32bb596fe8ca31a0eae74e4c0261717b9d10adcf241cc447fb`;
  `201-control_pixels_decision.py` is `03528248d8bb6a0f97fd7d7e0b95324d47d19f76e5e0c7dd9ddb0513c4a471bd`.
  Both are unreviewed and unrun; there is no assembled or approved 201 runner.
  No necessary draft now depends on `/tmp` surviving reboot.

On return, read this checkpoint and the two linked investigations before
resuming the admitted plan. Inspect `git status`, the task rows by stable ID,
and the package manifests. Coordinate the CPU lane with Claude; old Herdr
pane IDs `w9:pT`/`w9:pX` are historical and must be rediscovered if the session
changes. No previous exact-argv guest GO carries over to an unreviewed successor.

For part 3, first reconcile the lock diagnostic documentation with the SDK
contract boundary. Both C and Rust SDK snapshots compare
`spec/sophia-lock-files.md` with the authoritative document byte for byte.
Preserve their immutable snapshot provenance: either use the normal upstream
contract/snapshot update workflow or place internal diagnostic documentation
in its appropriate Sophia-only contract. Do not bypass the gate or edit a
vendored snapshot in place. Then sign the correction and run a fresh full
gate with a new evidence directory (`gate-02`); `gate-01/run-gate.sh` retains
the prior exact launcher. Its private target is
`~/dev/sophia/target/lock-characterization-200`. Review any formatting failure
against the source baseline; do not silently bundle unrelated reflow.

For part 1, independently review the narrow 198-03 control delta, then authorize
one bounded isolated CPU run with its declared launcher and fresh output.
Require all retained controls, the baseline and all fourteen mutants to pass
before freezing it. Its runtime core is unchanged from 198-02. Only after
that qualification should 201 be assembled around premise4, handle6 and the
existing 186 images, with fresh runner/decision/binding controls and review.
The reserved future series is 202. Retrospective 194 analysis remains separate
from a new paired experiment; no new guest is authorized by this checkpoint.

After reboot, refresh host, kernel, device, seat, binary, process and source
prechecks before any hardware work. Claude reports the iGPU seat1 rule is
installed and takes effect at reboot; the iGPU probe is paused. Revalidate
the resulting seat and render-node mapping rather than reuse the previous
`renderD128`/PCI assumptions. This is not permission for VFIO or device rebinding.
Parts 2 and 4 still require their own candidates, reviewed packages and exits;
the physical KVM check remains attended and separate.

## Scope and authority

This records the four-part plan niltempus authorized with “Implement the plan”
on October 9. It joins the bounded Mesa comparison in t307 to the lock and
hotplug qualification in t306. It does not promote the t310 monitor-policy
candidate, authorize an iGPU passthrough action, or accept either task.
Task state and execution order remain in [todo.md](../../../todo.md).

The [sampling investigation](../investigations/r2m9cx6v-static-retained-dma-buf-images-can-sample-black-before-hotplug.md)
owns rendering diagnoses and results. The
[KVM investigation](../investigations/kdleagg3-kvm-output-and-usb-loss-returns-the-desktop-to-greetd.md)
owns topology, input and lock results. Numbered directories under
`~/.local/state/sophia/development-evidence/t306-01/` retain raw logs, source
snapshots, exact run contracts and manifests. Those artifacts support the
notebook; they are not another work queue or the sole record of a decision.
Frozen artifacts and their original verdicts remain unchanged.

## 1. Qualify the Mesa comparison contracts

Resolve both known contract gaps before another guest series. The screen
premise must distinguish LOOKUP from INSERT and REMOVAL callbacks using live
descriptor generations, source-bound ordering and the same boot's trace.
Only the sibling loader's lookup against the producer winsys counts toward
sharing. Maintenance callbacks cannot establish a hit or miss. Unknown or
ambiguous assignments refuse; the bounded contract allows at most 16 proven
winsys generations across the process.

The pixel classifier may recognize the one exact known Mesa diagnostic, at
most once per arm, solely between validated `sibling_alive` and
`after_sibling_drop` records. It reports driver health separately. A diagnostic
never counts as healthy rendering, and foreign or misplaced text still refuses.

Require independent source review, retained refusal controls, meaningful
mutants, runner controls and a frozen package before the one-shot series:
observer, private original Mesa, then private patched Mesa. Keep the existing
test, matched images, bounds, infrastructure checks and no-replacement rule.
Advance requires all of these original-arm results:

- observer passes and infrastructure is clean;
- same-boot screen premises establish the expected original topology;
- separate arm preserves pixels and AddFB handle without a diagnostic;
- shared arm has `LOST_AFTER_SIBLING_DROP`, the named composition assertion,
  test exit 101, and zero or one recognized shared diagnostic.

The patched guest qualifies only with its expected same-boot screen premises,
both arms `PRESERVED`, test exit 0, clean infrastructure and no diagnostics.
Invalid evidence, observation errors, AddFB loss and unreached premises do not
substitute for the original negative control. A stopped or inconclusive series
gets a disposition, not an automatic replacement. Retrospective analyses of
194 cannot change its frozen verdicts or satisfy this future comparison.

## 2. Test the narrow Mesa repair in Sophia's production workload

Use the matched private original and patched Mesa builds with one frozen,
reviewed Sophia integration candidate. The workload is one virtual card,
two heads, per-head workers, Sophia's generic WM fixture and a static DRI3
client that Presents once. It contains no hotplug intervention.

Declare and freeze the comparison before running it: original, patched,
patched, original, four boots, with no replacements and a stop on
infrastructure failure. Require both patched boots to reach readiness and
retain the independently expected pixels, and at least one original boot to
reproduce the relevant failure. If the original never reproduces, the outcome
is limited to successful patched runs; it does not establish a repair effect.

The context-level result from part 1 is a prerequisite for interpreting this
workload as the cache patch's effect. It does not itself identify the cause of
the original Sophia black frames. Keep the patch narrow and retain the matched
build provenance; do not install private Mesa or patched QEMU into the host.

## 3. Characterize lock custody and prove cover retirement

Exercise real LockPublication, LockFileCustody and SessionLockFrames through
candidate publication, output removal and return. Distinguish provider
revocation from Session's pending retirement. Cover recovery by presentation
of the old image, absorption of the stale outcome, no pending candidate,
new lock, reconnect, unrelated output, and wrong receipt identity.

Topology tests must use the production rebind/resume boundary with controlled
device facts, rather than assign the runtime output list directly. Require
locked loss/return, loss during Locking, mirrored heads and all-output absence.
Mutants that clear the cover during rebind or resume must fail the relevant
tests. CPU characterization does not prove native device retirement.

Add a passive topology- and lock-epoch coverage record only after every
current head has retired a cover frame. Qualify its binding, duplicate
suppression and refusal during suspension or incomplete coverage. This is
diagnostic evidence for the locked guest; it does not change lock policy or
replace the existing Locked transition.

## 4. Qualify hotplug and prepare an attended rollout

Integrate an explicit reviewed repair scope on current master, preserving the
installed baseline. The raw t306 diagnostic branch is not an install candidate.
Use Rust/xtask for maintained tooling. Carry the corrected bounded endpoint
collector and refuse recorded unreaped processes even if a later scan finds
none. Correct display attempt/completion ordering before guest qualification.

Freeze fixed qualification runs for managed-head return, all-head return,
keyboard return, combined loss/return and locked all-return. Use a 60-second
session for the combined case with bounded host collection. Require independent
pixel and input-routing proof, complete endpoints, lock coverage for the
returned topology and no automatic reruns. Startup instability remains a
refusal; a later good retained frame does not silently discharge its obligation.

Gate the exact signed candidate, prepare its release and concrete install and
rollback commands, and keep the attended physical KVM check separate. Physical
acceptance must verify restored displays, keyboard shortcuts, pointer routing
and lock/unlock on niltempus's devices. Neither CPU tests nor virtual guests
close that final obligation.

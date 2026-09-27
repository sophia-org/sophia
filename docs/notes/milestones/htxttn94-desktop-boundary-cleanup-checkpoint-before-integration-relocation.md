---
id: htxttn94
date: 2026-09-27
kind: milestone
status: recorded
tags: [milestone, tooling, policy, validation]
---
# Desktop boundary cleanup checkpoint before integration relocation

## Result or change

This records an implemented slice of the
[t263 desktop boundary cleanup](../plans/9cd1ie0x-publish-independent-sophia-client-libraries.md#repository-boundary-cleanup-before-the-installed-session).
It does not close t263 or establish installed-session readiness.

Signed Sophia commit `be6e5888d79f2e89bd7fdfbee3455f8aa7eb6390`
makes desktop discovery and WM policy validation independent of a particular
client. Profile validation requires an explicit checker or explicit deferral;
required validation cannot silently become deferred. Session exports only the
generic `SOPHIA_WM_POLICY_*` names. Shell selection is explicit, and generic
panel fixtures replace product fixtures in configuration tests.

Personal installer commit `29f0480` supplies its own Hagia policy adapter and
requires `policy=validated`. Both preparation and login check the WM's environment
contract. The personal WM executable remains user-owned so the intentional
reload workflow is preserved; an incompatible older binary is refused.

Signed Sophia commit `a4217fc7` removes sibling checkout requirements from two
archive-verifier self-tests. Their synthetic client identities use real signed
Sophia objects, retaining signature checks and invalid/unsigned identity controls.
The frame-fed fixture uses a temporary local clone because its archiver requires
a `.git` directory and the test may run from a linked worktree.

## Evidence and decisions

Evidence is under `~/.local/state/sophia/development-evidence/bemenu-files/`:

- `boundary-preflight.log`: 12 CLI preflight tests pass.
- `boundary-preflight-launcher.log`: generic checker and PTY refusal controls pass.
- `boundary-config-tests.log`: configuration discovery and profile tests pass.
- `boundary-session-config-native.log`: 52 session configuration tests pass with
  `native-session` enabled. The earlier `boundary-session-config.log` ran zero
  tests and supplies no coverage.
- `boundary-installer-probe-tests.log`: personal installer tests pass, including
  capability-probe refusal and strict validation verdicts.
- `boundary-g1-matchers.log`: matcher self-test passes.
- `boundary-g1-frame-fed.log`: initial failure is retained; the temporary fixture
  did not satisfy the archiver's `.git` directory requirement.
- `boundary-g1-frame-fed-local-clone.log`: corrected fixture and negative controls
  pass. The production archiver was unchanged.

The independent Bemenu relocation was gated before its Sophia deletion in
`5f5871127`; its source, binary and evidence are recorded in the linked plan.
Later external relocations still require their own complete input bindings and
gates before Sophia entry points are removed.

## Paired WM and generic launcher follow-up

Hagia's generic environment branch was merged locally as signed
`20ef21300901e2cdb9e6fc448674edba556a4d32`, with the exact tested tree of
`5569345b8aa89fb16f7c65d508f90c2aa81df7a2`. The independent legacy-pin suite
passed 474 tests. The separate pairing `run3` passed all 38 phases against
Sophia `be6e5888` plus the recorded Hagia-owned test overlay. Its report is
`hagia-wm-policy-env/pairing-be6e5888/run3/report.json` under development evidence.
The frozen Hagia executable came from `e8b56a3`, SHA-256
`e8221d1197b032e51c7fabe5dccc20e6c8e52342e8e8cd8a82940ad9063b86ad`.

The launch records list configured generic environment keys; they do not
capture the child's environment. The checkpoint recovery case demonstrates
that real Hagia used the new checkpoint path. Candidate and activation support
is evidenced indirectly by the protected startup and profile recovery cases.
The earlier `run` failed because its target was hidden by the sandbox's `/tmp`
mount. `run2` failed because a legacy fixture depended on removed implicit shell
selection. Both failures are retained. The corrected fixture supplies an
explicit shell, preserving the policy profile under test.

Sophia `ecb3209c` replaces the joined Bemenu evidence test with a contract
launcher. The focused native-session test passes under device-hidden execution,
retaining pre-negotiation refusal, exact connection identity, stopped-key
refusal, reaping and revocation settlement assertions. The log is
`bemenu-files/boundary-joined-launcher.log`. The other two Bemenu tests remain
until external tests cover both wires and their production Session path.

## External host-check seam

`sophia session check-host` provides the bounded invocation and verdict check
for an operator-selected host-policy executable. It contains no desktop process
names. Exit observation leaves the leader unreaped through TERM, the two-second
grace and KILL, preventing process-group number reuse during cleanup. Stderr
controls are escaped; executable symlinks and the trusted checker's ability to
escape its process group are documented.

The focused CLI suite passes 9/9, including successful leader exit with a
background child that holds no output pipe, timeout cleanup, output overflow,
exact verdicts and constrained override behavior. Logs under `bemenu-files/`:
`boundary-check-host-run2.log` and `boundary-check-host-clippy-final.log`.
The original `boundary-check-host.log` remains failed (8/9): its stderr assertion
did not account for Rust's error-return Debug formatting. The assertion was
corrected without changing production behavior. Formatting and diff checks pass.
These tests open no display or input device and do not claim a live host-policy
decision.

The retained session launcher now calls the required checker after its
TTY/runtime/VT checks and before input preparation or service changes. The DRM
guard uses the same seam and retains its ambient-display refusal. It requires an
absolute prebuilt `SOPHIA_BIN`; force overrides only known active-session evidence,
never missing policy, malformed output or timeout. Product process-name lists
have left these two guards. External callers must supply their host checker.

The wrapper gate passes launcher safety 20/20, real CLI refusal on a disposable
PTY 2/2, and guard/recovery 2/2. The watchdog regression, focused clippy,
formatting, shell syntax and diff checks pass. Logs are
`bemenu-files/boundary-host-wrappers.log`, `boundary-host-wrappers-clippy.log`
and `boundary-host-watchdog.log`. The supplied clear checker in recovery and
watchdog fixtures is test data, not a host detection result. No graphics or
input device was opened by these checks.

The retained `check-launch` preparation helper had the same reap-before-signal
ordering as the original host checker. It now observes exit with WNOWAIT and
leaves reaping to cleanup after the last group signal. The preparation argument
tests pass 4/4 and input/staging tests 8/8, including timeout group cleanup;
focused clippy, formatting and diff checks pass. Evidence:
`bemenu-files/boundary-preparation-wnowait.log` and
`boundary-preparation-wnowait-clippy.log`. The external recipe migration must
preserve that ordering in copied preparation helpers.

## Public conformance seams and E2 relocation

The public seams at signed Sophia `d20faf3709ae21d94491f7a628ac9a4a86619cdf`
provide a strict record reader, five conformance-host binaries from an exact Git
checkout, and a parameterized protected shell content proof. Their focused gates,
the fragmented-intake mutation control and the offline host-install proof pass.
Evidence is under `seams-bcd/gate-c6ecebfca`, `seams-bcd/gate-d20faf370` and
`seams-bcd/install-c6ecebfca5bc4e76aa7e64165dc78e25b4af0d19`.

External integration commit `995f941` pins that exact Sophia revision and binds
the moved dock/workload checks and GPU verifier to those public interfaces.
Its post-format E2 run3 passes check-pins, check-provision, 16 workspace tests,
clippy, formatting and 47 verifier mutations; one live test remains ignored.
Logs are under `integration-e2/`. Provisioning used a private Cargo home outside
both source trees and a copied registry seed. No crates were downloaded; run2
refreshed the crates.io index and obtained Sophia from a local Git route. Run3
needed no network. Earlier provisioning, missing-dev-dependency and formatting
failures remain recorded.

Sophia removes the five Lom gate/verifier scripts, product probes and fixtures,
dock profile/transcript code and its xtask entry points in the same merge as the
new public proof interface. Generic panel fixtures, launcher output-protection
assertions, content/SDK tests and direct-scanout fixtures and archive verifier
remain. Named product recipes and their assertions continue in the external
repository. This relocation establishes no new GPU execution, native display
or physical acceptance result. The combined cleanup tree still needs its gate.

## G2 runtime evidence and retained Session setup

External G2 run6 at `e6939c7` passes both runtime-level Bemenu wires with the
same `52a6e309` artifact recorded above. Both report two openings, three
candidates, one edit and activation, an unchanged neighbouring bar and a retired
held lease. The logs are `integration-g2/live1-bemenu_files-run6.log` and
`live2-bemenu_ipc-run6.log`. Font/device isolation and scripted Session decisions
remain the test boundary.

The first Session aggregate smoke failed. Two migrated tests exposed stale
setup in Sophia's retained shared helper: it added the menu after starting the
bar, but `ShellComponentConnections::add_with_transport` has refused additions
after the first start since `06fa46b88`. The helper now adds both roles first;
all negotiation, pixel, neighbour, stop and settlement assertions remain intact.
This is a setup repair, not a successful live-test result. The joined Session
test separately timed out during negotiation; its cause remains unexplained.
`AcceptTimedOut` covers accepting and completing the hello, so it does not by
itself prove that the peer never connected. The ignored fixture child was also
invoked directly in that failed run and is excluded from the top-level rerun.
All failures remain in `integration-g2/live3-bemenu_session_ipc-run6.log`.
That repair did not remove Sophia's two retained Bemenu tests.

External Session run7 at signed `31f5e511c925183b1e87cbcee61166f990e5b293`
passes all three real-Bemenu Session tests, excluding the fixture child entry.
The final offline gate at that exact head passes pin/provision checks, 18 tests
(seven ignored), clippy, formatting and 47 verifier mutations. The lock digest
`07997d25baa4ba38d2ed4c468c267e0ad9b8ce8cde536574fbf50910cdc1fb33`
matches the provisioning marker. Evidence: `integration-g2/*-final.log` and
`live3-bemenu_session_ipc-run7.log`. The earlier timeout remains unreproduced and
unexplained; the proposed cold-start diagnostic was cancelled before code was
written, and no handshake deadline changed.

With that external coverage green, Sophia removes the two retained Bemenu tests
and their product fixture. The generic joined-launcher test and the other
contract-derived process tests stay. Focused process tests pass 7/7, with three
explicit namespace/child entries ignored in the ordinary invocation
(`bemenu-files/boundary-g2-removal.log`). No cold-start reliability or physical
acceptance claim follows from this relocation.

The retained generic joined-launcher test also passes explicitly with devices
hidden (`boundary-g2-removal-joined-run3.log`). The first invocation paired a
private PID namespace with the host procfs and timed out during isolation; the
second corrected procfs but encountered the first run's PID-named scratch path.
Both failed logs remain. The passing invocation mounts matching procfs and a
private tmpfs, without changing the test or production code.

## Remaining work

The [t263 plan](../plans/9cd1ie0x-publish-independent-sophia-client-libraries.md)
and [t252 acceptance plan](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
retain the full exits. These slices have no combined full-workspace gate,
physical acceptance, publication or installed-release claim. The final
candidate must combine the remaining relocations, matching
external clients and installer inputs, then pass the affected gates before
release preparation.

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
The wrapper migration remains separate; these tests open no display or input
device and do not claim a live host-policy decision.

## Remaining work

The [t263 plan](../plans/9cd1ie0x-publish-independent-sophia-client-libraries.md)
and [t252 acceptance plan](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
retain the full exits. These slices have no combined full-workspace gate,
physical acceptance, publication or installed-release claim. The final
candidate must combine the remaining relocations, matching
external clients and installer inputs, then pass the affected gates before
release preparation.

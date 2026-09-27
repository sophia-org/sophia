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

## Remaining work

The [t263 plan](../plans/9cd1ie0x-publish-independent-sophia-client-libraries.md)
and [t252 acceptance plan](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
retain the full exits. This checkpoint has no full-workspace gate, new-name
Hagia launch evidence, physical acceptance, publication or installed-release
claim. The final candidate must combine the remaining relocations, matching
external clients and installer inputs, then pass the affected gates before
release preparation.

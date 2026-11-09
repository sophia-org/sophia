# T4 worker-native-submit feasibility checkpoint

Status: design probe only. No Rust rendering change, build, installation,
native launch, qualified CPU saving or end-to-end implementation proof.
Source anchor: `2c77581d8`. The [modeling brief](modeling-brief.md) maps the
current boundaries; [coverage](brief-coverage.md) names every configured check.

## Result

The simple T4 change is insufficient. A render worker cannot submit silently
and retain today's result-channel, output-detach and shutdown ownership rules.
The source requires three new mechanisms:

1. A submission authority/capacity claim serialized with effective revocation,
   seat changes, target replacement and other submissions on the same head.
2. A retained submission owner that survives failed reply delivery, facade
   detach and worker exit, including uncertain ioctl outcomes.
3. Completion readiness and exact durable ownership established before the
   worker can submit, so a page flip racing ahead of result consumption is kept.

The owner-to-worker Render request stays. The owner still processes exact
completion, SFT terminal publication, predecessor retirement and failure.
Only the intermediate successful-result → owner → submit handoff is targeted.
Refused submissions still need notification because they produce no flip.

```text
CURRENT
owner --Render--> worker --buffer/result wake--> owner --submit--> DRM
                                                  <--completion--

PROPOSED, NOT IMPLEMENTED
owner --Render + reserved submission obligation--> worker --submit--> DRM
  |                 shared exact custody                    |
  +--card readiness already armed <-------completion--------+
  +--failure notification / retained uncertain ownership-----+
```

## What was checked

Using the repository-pinned TLC jar (SHA256
`e22f8ffb4bacdea0a871f444dd94fe5fb0d8013b3388ae39e82e26f852c735d5`):

- Protected abstract safety model: 717 generated / 296 distinct states,
  complete search, six invariants pass.
- Seven negative controls produce their expected invariant violation:
  unlocked authority check, lost reply, worker stop, late ledger, late card
  coverage, early release and foreign completion.
- The protected design's progress probe fails: after a valid submission claim,
  a worker can stall while withdrawal is requested. Effective withdrawal cannot
  pass the held gate. Fairness on enabled submit/withdraw actions does not fix
  an action disabled by the stall. This is an unresolved design obligation,
  not a claim that the current owner-side ioctl has stronger stall behavior.

The control variants are hypothetical unsafe T4 designs, not current Sophia
bugs. The safety result assumes the new gate and retained ledger; it does not
prove they exist, are implementable safely, or have acceptable progress.
Deadlock checking is disabled for intentionally stopped finite episodes; the
explicit withdrawal temporal property is checked and fails as reported.

Evidence: `~/.local/state/sophia-t4-model-01/checked/` contains pinned copies,
commands, logs and TLC counterexamples. Earlier local setup and checker-message
matching failures remain in the parent evidence directory. The final checks
recognize the pinned jar's exact temporal-property message. No output or cache
is written into the repository by the reproduction helper.

```sh
SOPHIA_SPECULA_TLA2TOOLS_JAR=/absolute/path/to/pinned/tla2tools.jar \
  timeout 120s bash tools/check_worker_native_submit_model.sh
```

The helper succeeds only when the safety run passes and all eight expected
negative results occur. Its successful exit is **not** T4 implementation
acceptance: it explicitly prints the unresolved progress result.

## Scope decision before Rust implementation

The original narrow experiment has reached a stop condition: submission is
entangled with resource preparation and lifecycle ownership, not just an ioctl.
A viable next design would introduce a bounded per-output submission owner
under existing card/seat authority, without moving all head policy into the
renderer. It must first specify authority linearization and stalled/uncertain
submission handling while preserving current refusal and physical-retention
rules. An irrevocable render-time permission would change the SFT contract;
another per-frame owner approval would retain the handoff being targeted.

Before implementation, niltempus must choose whether to expand T4 to that
submission-owner design or park it. The review may identify a smaller existing
mechanism; no new actor, shared authorization cache or unsafe Send bridge is
authorized by this checkpoint. Retain M1/M2 deferral and every B6 obligation.
No physical run is useful until a concrete implementation survives the
device-free tests listed in the brief.

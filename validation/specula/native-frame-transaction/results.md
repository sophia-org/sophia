# Design-model checkpoint, 2026-11-04

The model-only checkpoints did not change production protocol or runtime source.
The subsequent default-off Rust foundation is described below. The design is not
yet eligible for enablement; see the explicit exclusions and wire gate.

## Reused Rust credit foundation

### Subsequent store adapter

The SFT adapter reuses W3 candidate lifecycle (`f639d21d2`) and W2 retirement
ownership (`892bd1436`) behind the same default-off feature. Current resource
pixel versions and renderer code are preserved. Terminal credit now returns on
successful output transfer, not outcome queueing; a one-credit regression fails
when queue-time return is deliberately restored. The mutation source is restored
byte-for-byte after the test.

The typed Begin seam tests zero reply capacity without consumption, a reserved
Budget refusal, close/reopen identity behavior, continuations at the full
reservation limit, invalid authority and stale stream/legacy permit separation.
These six tests, 22 store lifecycle tests, 12 retirement tests and the 15 ledger
tests pass. Full runtime suites pass with 818 tests enabled and 763 disabled,
both with 41 ignored and zero failures. Evidence is under
`~/.local/state/sophia-frame-store-adapter-01`.

Two earlier regression logs used a fixture that opened two credits when the
assertion required one; those failures are not negative-control evidence. The
corrected fixture explicitly requests one credit, and the separate
`terminal-mutation.log` records the intended early-return failure.

This is store-local evidence, not composed-path conformance. Connection-wide
response/byte admission, complete-input decoding and trailing discard, physical
copy/worker reservation and runtime trace validation remain unwired. There is no
wire opener, codec allocation, producer integration or native performance claim.

### Earlier ledger-only checkpoint

The runtime-only `experimental-native-frame-transaction` feature imports the W3
ledger from signed checkpoint `33ff35990`. Non-comment source is identical; no
new credit algorithm or dependency was added. The original 13 tests plus two
new tests pass. New checks cover every ledger key field, an owner rejecting
reopen until external drain, and old completion/candidate replay after reopen.
They do not test ring/slot identities, which are not fields of this ledger.

Runtime tests with the feature enabled: 778 passed, 41 ignored, zero failures.
Strict Clippy over all runtime targets/features, feature-off library check,
rustdoc and formatting checks pass. Evidence lives at
`~/.local/state/sophia-frame-ledger-reuse-01`.

No Session/CLI enablement, wire opener, codec, producer integration, runtime trace
validation or native launch is included. The ledger is a foundation for the
adapter, not a completed FrameCommit implementation or a CPU improvement claim.

## Intake boundary follow-up

`Intake.cfg` visits 5,285 distinct states at capacity 4;
`IntakeProgress.cfg` visits 8,646 at capacity 5 and checks both fair-progress
properties. All four guard-removal controls fail their intended invariant.
Logs are under `~/.local/state/sophia-frame-intake-model-01`.

This checks proposed reservation and continuation policy, not the current wire
poller. The input byte parser, identity validation, reservation rollback and
composition with the frame/resource stores remain implementation gates. No
runtime FrameCommit opener or codec is enabled.

## Resource-store boundary follow-up

The resource follow-up is signed commit `01b510a8d`.

`Resources.tla` adds a separate finite resource-store model: two resources and
consumers, byte sizes 1/2, total byte budget 3 and two epochs. It checks physical
charge retention during cancellation, leases after retirement/revocation, and
the distinction between queueing ResourceReleased and publishing it to the live
journal. The base safety search visits 5,693 distinct states; fair progress and
four negative controls are included in the repository checker.

The stale-publication negative control initially exposed an unspecified-variable
error in a TLA+ Boolean expression. Parenthesizing the assigned expression fixed
the model; a parser/evaluation failure is not accepted as an invariant failure.
The before-log is retained. Evidence is under
`~/.local/state/sophia-frame-resource-model-01`.

This is a resource-store slice, not proof that the new FrameCommit adapter joins
the frame ledger, byte accounting and wire admission correctly. That integration
and actual callback/identity handling remain required before enablement.

## Implementation-plan follow-up: close/drain/reopen

The design checkpoint is signed commit `903c5b434`; the separately checked
reopen extension is signed commit `2298229e4`.

The additional `Reopen.cfg` enumerated 3,083,753 distinct states without an
invariant violation. It retains physical ownership while requiring delivery of
the close notice, an empty output queue, no remaining source obligations and
terminal settlement before a second stream opens. Credit replenishment is bound
to both connection epoch and stream identity. The new `ReopenSafe` invariant is
enabled in every configuration; the original results below remain the first
checkpoint's recorded evidence. Follow-up logs are under
`~/.local/state/sophia-frame-reopen-model-01`.

Removing the drain precondition produces the expected `ReopenSafe` failure:
`ChangeFacts -> Reopen` opens before even the close notice is delivered. The
negative control is part of the reproducible checker, not a deployed-code bug.

TLC used the repository-pinned jar SHA256
`e22f8ffb4bacdea0a871f444dd94fe5fb0d8013b3388ae39e82e26f852c735d5`.
Full bounded enumeration, no depth/state constraint or simulation cutoff:

| Configuration | Distinct states | Result |
|---|---:|---|
| MC: two frames, one credit/worker/head | 554,527 | All 13 invariants pass |
| TwoCredits: two frames, two credits/workers, one head | 2,159,654 | All 13 invariants pass |
| Mirror: one frame, two heads | 12,440 | All 13 invariants pass |
| Liveness: one frame/head | 4,076 | Invariants and both fair-progress properties pass |

These finite configurations overlap; summing their states does not establish
coverage of a larger configuration. TLC uses probabilistic fingerprints, not a
machine-checked unbounded theorem. Syntax and variable-assignment analysis pass.

All seven mutation controls fail the intended invariant:

| Removed guard | Counterexample |
|---|---|
| Credit debit at admission | Admit immediately violates credit conservation |
| Source read-ticket completion before release | Admit → PublishSource → Deliver while the reader remains active |
| Whole-scene preparation before submission | Admit → CopyDone → Validate → Submit |
| Current authority at submission | Admit → copy/validate/handoff/prepare → Revoke → Submit |
| Remaining predecessor head lease | Primary flips → Presented transfers → free while secondary still displays predecessor |
| Worker handoff before cancelled terminal | Admit → Cancel → Terminal while copying |
| Facts-bound stream closure | ChangeFacts leaves the old stream Open |

Review also corrected the distinction between closing a stream and revoking
authority: Close stops new admission and credit replenishment, but already
accepted work may finish. Facts changes invalidate its tuple; revocation replaces
its epoch. Cancellation is forbidden after renderer handoff. The bounded
read-only re-review accepted these corrections; the reviewer did not rerun TLC.

Raw logs, exact spec/config snapshots, mutation traces, tool checks and the
reproduction log live at `~/.local/state/sophia-native-frame-model-01`.
Earlier exploratory outputs are retained separately and are not the results
above. Specula's background wrapper writes a combined log through two writers,
which produced NUL padding/duplicate lines; normalized mutation-log copies remove
only that padding and make the JSON trace path absolute for the trace parser.
Original logs and counterexample JSON remain intact.

No runtime traces exist for FrameCommit. No claim of native conformance, GPU
fence correctness, cross-monitor simultaneous flipping or CPU improvement is
made. This checkpoint justifies the next narrow protocol/resource proof and
implementation step; it does not justify a performance claim or adoption.

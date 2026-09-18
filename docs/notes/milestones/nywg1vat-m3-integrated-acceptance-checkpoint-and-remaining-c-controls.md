---
id: nywg1vat
date: 2026-09-18
kind: milestone
status: recorded
tags: [milestone, x11, validation]
---
# M3 integrated acceptance: twenty of twenty, and what that does not settle

This records an evidence review under [t093](../../../todo.md) and its
[M3 plan](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md).

The M3 integrated acceptance gate is met: all twenty coordination cases pass
together on one signed source and one binary, in containment, with every actor
collected, and the full contained workspace regression passes on that same
source. Those two results are stated below with their identities.

## Local master integration

On 2026-09-18, local master fast-forwarded from `8276fa04` to signed
`a4626b1bf5810c171b606aa798e5087d843a7754`. This preserves the M3 history and
the current desktop implementation, including P5's Present diagnostics. The
separate Brave admission repair `02548ed9` was deliberately left for a later
merge. Nothing was pushed or installed.

The final code source has tree `982a358d97584a695ed32c21e535743eadfc08c6`.
Its archive SHA-256 is
`fcf789c15b02552bd6fa9d5561cfba02b4c29a13e04ba1c82c2bade331a99982`;
source content SHA-256 is
`a6a29bd9b9068c69b3fffd3218fbff2c59a4674cd83284f9430f16571a7b21cb`.
Under `.artifacts/m3-master-integration-20260918/`, the six reports
`acceptance-a4626b1b-01` through `-06` each pass 20/20 cases and 84 subcases,
with 336 actors started and collected and none pending. Each attests that
source and binary SHA-256
`065688b76d07afd164ccb0bf43070083275031f600e873171ef99a8b8c1a48cb`.
The runner used an optimized xtask executable; the product test binary retained
its ordinary debug test profile, with assertions and overflow checks enabled.

`offline-check-a4626b1b/report.json` records the full contained workspace run:
317 Rust result groups, 4,828 passes, zero failures, 39 ignored and zero compiler
warnings. Strict default and all-feature Clippy, fmt, whitespace and layout
checks pass. The last code change adds `native-session` gating to the catalog
mode helper, matching its only caller. The earlier `0798c4db` results remain
under their own identity; they are not substituted for the final-source runs.
Subsequent tracking edits change documentation only.

The merge exposed a sequence race in ordinary X11 dispatch: a peer could answer
a routed request before the originating connection published its sequence.
`0798c4db` publishes before any peer-visible effect, under the wire lock, and
releases that lock before dispatch. Publication emits no bytes and does not
wait for queued control priority. The first repair, `16f5dee7`, did wait for
that priority and blocked an existing core-focus supersession control; its
failed run is preserved. Both the unchanged supersession control and P5's
three-window Present control pass on the final source.

The negative source `59a05041` adds the paused socket fixture without the
publication repair. `selection-before-rerun` fails only the intended wire test,
at its post-collection assertion: sequence 2 instead of 3. The final fixture
also makes the pause one-shot; this is a fixture refinement, not a production
change hidden in the negative. The earlier `selection-before` run stopped at
an unrelated cleanup watchdog deadline and does not qualify the sequence
discriminator. All these reports remain in the same artifact directory.

M3 is complete on local master. M4–M6, public XTEST discovery, the private
Session host and physical acceptance remain open under t093/t094. Current
Brave presentation and input reports are separate findings; these checks do
not establish a fix for them.

## Earlier finish-branch integration

Review and local finish-branch integration completed first. On 2026-09-18, `codex/m3-finish`
fast-forwarded from `563d5a1a` to the reviewed signed handoff `30ac8940`.
The code is unchanged from tested source `4fcc9f02`; later changes update only
these records. M3 is complete locally, while t093 remains open.
Nothing here claims physical or hardware acceptance; hardware checks are
NOT_RUN throughout. M4 to M6, public XTEST discovery and Session integration
are untouched by this and remain open. This note changes no acceptance
requirement.

## Twenty of twenty on the integration candidate

The combined result is now **20 of 20 PASS, zero FAIL**, on signed source
`4fcc9f02ca3355e2f2f12d3e27cce3e8f1fe99ba`. The contained report is
`.artifacts/m3-finish/acceptance-run-03/report.json` in the common repository.
Source content SHA-256 is
`cc8d5f3dc6b1502c75e8098a53b9b43ad61400d2e7a13547930bf7381f7998ec`;
binary SHA-256 is
`1aa70239b764086283677c2f2cca11e45122ab8776330f4ca5f4459624902bc6`.
Source attestation inside containment and the unchanged-source check passed.
Three hundred and thirty-six actors were started and three hundred and
thirty-six collected, with none pending and no harness error.

Several of these controls make their state out of timing, so the gate was run
five more times on the same source, in containment, one after another. All five
pass twenty of twenty, each attesting the same commit, the same source content
digest and the same binary digest, each starting and collecting the same three
hundred and thirty-six actors with none pending. Their reports are
`.artifacts/m3-finish/acceptance-repeat-01` through `-05`. Repeated running on
unchanged source is what found both of the defects recorded below: a restored
run failed where its predecessor had passed on the same digest, and the first
combined run failed a case that had passed on its own.

`C.indeterminate_send` and `C.control_cleanup` are bound. The former makes each
of its four states rather than describing them: a capsule stopped between its
own two frames, with two frames owed, one committed whole, the writer failing
on the second and the frames walked in that order; a handover the recipient's
queue admitted and whose result was never recorded; work sitting on that queue,
observed rather than resent; and a decided request whose refusal cannot be
published, which keeps its item, its own typed refusal and its single credit
until the admission returns. The latter drives every one of the nine control
kinds on its own invocation, reading back what each actually changed at the
source before its writer was interrupted, then requiring a charged supervised
control visit to refuse for the withheld removal, one charged visit to retire
the record, and a separate later one to return the credit it carried.

**What the nine cleanup subcases do not establish.** Each is interrupted after
its first source effect, before any peer generation has begun, so none of them
is evidence about healthy-peer reuse, a failed peer, or a replacement
recipient. Those remain separate component coverage in the twenty-eight-control
`control-cleanup` suite, whose restored run passes 28 of 28 at
`.artifacts/m3-finish/control-cleanup-restored-N55-N57-1561e3d0/report.json`,
together with its three compiled negatives N55, N56 and N57, each of which
failed its intended control at 27 of 28.

## Compiled negatives for the two new rows

Each mutation was committed, gated in containment, and the tree restored
afterwards; the failed evidence is kept under `.artifacts/m3-finish/c-negatives/`.

Recording an interrupted handover as not-enqueued rather than unknown
(`a8789f84`), bypassing the preserved-frame guard (`4045a5f4`), reporting a
transient visit without reading its cell (`b59ec09f`), and discarding a refused
item with its credit whatever the publication was told (`d47236f8`) each failed
the assertion it was aimed at.

Three results need stating precisely, because two of them are weaker than they
look and one is a survivor.

Suppressing the interruption latch in `ServiceRun::drop` alone **survived**
(`ec9f3146`): the budget is held closed by a second, independent latch on the
abandoned active record, so that single mutation does not qualify the barrier.
Clearing both (`cd321a5e`) fails the row's typed yield requirement, and that
combined bypass is the evidence for it. The two are separate facts and the
combined one must not be read as the single mutation having been killed.

Two attempts at the replay discriminator did not reach a replay at all.
Dropping the retained frame and rewinding (`8b46d8ad`) and rewinding the cursor
alone (`fc23e26d`) both fail earlier assertions, about the home no longer
holding the capsule and about which frame the writer failed on. Neither
attempts a send that afterwards restores state, so neither establishes that the
no-replay evidence would catch one. The discriminator that does is `67e27c2f`:
at the retained output visit, before the guard, the original capsule and socket
are used to attempt its already committed first frame through the real writer,
and the cursor and send state are restored afterwards so the home and the
capsule look untouched. The row still fails, on the observed sequence
`[0, 1, 0]`, and that is what establishes the evidence is not vacuous.

## Two fixture defects repeated running found

A restored run of the two controls failed where the run before it, on the
identical source digest, had passed. The turn loops waited out an exhausted
start budget but treated an exhausted time budget as something waiting could
not fix, and that is not true of it. Four of the six refusals name a delay and
mean the same thing; all four are now waited out for the delay they name, and
the two that name none stop the loop and fail the case. The matches list every
variant, so a new one cannot be swallowed by a wildcard. A separate pass
bounded the traces those loops keep, after a negative run grew a log by
hundreds of megabytes recording turns that said nothing the first few had not.

The first combined run was 19 of 20. `C.capacity` submitted a release on the
grant its press had just used as soon as the press's flushed receipt arrived,
and met its own grant still occupied. A receipt says the bytes went; the
grant's one cell is freed later, when the outcome is observed, and the
accepted-item credit returns after that. The shared press-and-release helper
now requires the store empty before it starts, so its total answers for the
request in hand, and waits for that total to return to empty after each half.
It does not take the outcome itself, which would free the cell on the service's
behalf, and it does not retry a submit until one is accepted. The per-grant
saturation assertions elsewhere are unchanged.

## Earlier evidence, preserved

Everything below records the state of this review at earlier checkpoints. It is
kept because the path to the result above is part of the evidence, and it is
superseded by it: where these paragraphs say a case is unbound, work remains
required, or no full workspace run is claimed, they describe the checkpoint
they were written at and not the candidate.

The previous combined result was **18 of 20 PASS, zero FAIL**, on signed source
`a2bc967c585c6c58e100a3baae7d5140eb125bb9`. The contained report is
`.artifacts/m3-finish/c-acceptance-run-08-merged/report.json` in the common
repository. Source content SHA-256 is
`76b0b83b36e433f038348c8cbeac3f33c007f590269849abf0828ee2ddf4f862`;
binary SHA-256 is
`c8056b98fe2e915eda06199e5fef506b85b5bb1777f985eca8fb1456dd2c1d17`.
The fresh build target, source attestation inside containment and
unchanged-source check are recorded in that report. All A, B and D cases and
four C cases pass together. C.indeterminate_send and C.control_cleanup were
unbound then, so that aggregate was NOT_RUN.

The integration branch repeated that result on signed `39374df4`: 18 PASS,
zero FAIL, the same two cases NOT_RUN, with source attestation and unchanged
source confirmed. That run also includes the final B allowance-wait helper;
its report is `.artifacts/m3-finish/parent-39374df4-acceptance/report.json`.

The earlier signed source `8b4be691e3d71459102245a1dccf6d15ea2ca401` was run
through `cargo xtask check m3-acceptance`. The report is
`.artifacts/m3-finish/parent-all-bound-18/report.json` in the common repository.
Source content SHA-256 is
`d570281892d5ae6e80e874d0e260e806e6eb11b1be337f7d11ab8e9db506ad2e`;
binary SHA-256 is
`02363d3340b39afd51df90ff9a25caa467be166b8c1ebb39f815d52b8745328d`.
Source attestation inside containment and the unchanged-source check passed.

All three A cases, four B cases, seven D cases, C.poison and C.exact_origin
passed: 16 of 20. C.capacity failed because one production turn drained the
six queued items, contradicting its assumed remainder. C.interrupted_ownership
failed because the exact release remained in the turn beside its hold, before
the transition to settlement that its assertion assumed. These failures require
deterministic controls of the relevant stages; they do not permit removing the
identity, remainder or credit assertions. That run's aggregate remains FAIL.
The successor establishes current ownership and a remainder with one production
step before the service turn, then compares credit with every item still owned.
The interruption control checks the retained hold's client/window identity and
exact credit without requiring a particular intermediate list.

At that checkpoint C.indeterminate_send and C.control_cleanup were unbound.
The former needed a demonstrated prefix of the same delivery and a true unknown handover, with
all actors collected. Earlier completed frames were not that prefix. The failed
unknown-handover run also called `finish` on its third, still-running fixture;
the retained audit identifies the missing stop/collection prerequisite, not an
established production collection omission. A corrected diagnostic now performs
the true handover unwind, collects all its actors and visits maintenance before
checking that the handover was not replayed. Its audit report is
`.artifacts/m3-finish/c-indeterminate-unwind-review.md`.

Control cleanup had a separately tested original-source Configure path, and
all-nine-kind integration was still required before binding its C case. An
unresolved publication, missing receipt or peer dependency must stay owned.
Review also requires successful peer writes to discharge their original debt;
indefinite retention after a proven success cannot substitute for cleanup.

Supporting live-recipient controls passed seven of seven on `d9e5e874` and
again after restoring identical source as `4b7c41c6`. Compiled negatives
`e6c829bd` and `819bf570` each failed the intended control: recipient termination
cannot supply native reconciliation, and StateOnly disposal requires its
explicit output disposition. Reports are under `.artifacts/m3-finish/` in
`parent-live-stateonly-01`, `live-proof-false-native`,
`live-proof-untyped-absence` and `live-proof-restored`. These component results
are separate from the twenty-case aggregate.

The harness now keys build directories by archived source content, including
file modes. Reusing a fixed target after restoring older archive timestamps
can reuse a mutant binary; such runs are excluded from acceptance evidence.
Exact diagnostic tests may run under `m3_acceptance::diagnostics::` through the
component gate. That namespace is rejected by acceptance bindings. The gate
change passed 24 contained self-tests; its report is
`.artifacts/m3-finish/diagnostic-gate-selftest/report.json`.

Strict all-target, all-feature X-authority Clippy passed on `8b4be691`.
The formatting check found two line-wrap differences in the harness cache
helper; the accompanying documentation checkpoint corrects them. No full
workspace or physical acceptance was claimed at that point. M4–M6, public
XTEST discovery, Session integration and hardware remain outside this evidence
review throughout.

## What is still open

The full contained workspace gate passed on this exact candidate, exit zero,
at `.artifacts/m3-finish/offline-check-4fcc9f02-final02/report.json`: 289 Rust result
groups, 4426 passed, none failed, 29 ignored, with the X-authority library at
1090 of 1090 and no compiler warning lines. Its source, tree and archive match
`acceptance-run-03` exactly, the archive digest being
`2f28d0ff3613ed7197b4d126940c05884528da7779ea8bb9331d6c0a3c284d91`. Hardware
checks are NOT_RUN there, as they are throughout this review. The predecessor
`cbdb981b` passed the same gate, and the retained-custody regression repairs
merged here passed 11 of 11 with strict Clippy at `c48887b1`.

The strict checks pass on that same immutable source: all-target, all-feature
X-authority Clippy with warnings denied, workspace formatting, layout and the
diff check. Their exact commands, exit codes and logs are in
`.artifacts/m3-finish/final-strict-4fcc9f02/result.json`.

A twenty-of-twenty contained aggregate is still not the whole of t093. Local
finish-branch integration is complete; root master and the Session/dock tree
have not imported it, and nothing was pushed. M4–M6, Session integration,
public XTEST discovery and physical acceptance remain outside this result.

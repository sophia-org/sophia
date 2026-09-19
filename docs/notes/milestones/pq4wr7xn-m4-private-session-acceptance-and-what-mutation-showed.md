---
id: pq4wr7xn
date: 2026-09-19
kind: milestone
status: recorded
tags: [milestone, x11, session, validation]
---
# M4 private Session acceptance: eight of eight, and what mutation showed about it

This records an evidence review under [t093](../../../todo.md) and its
[M4 execution contract](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md#m4-execution-contract).
It does not close t093, enable XTEST discovery, establish physical acceptance,
or complete t094.

## The result

On signed `bf34cbfb`, in containment, source attested inside and unchanged
after, no harness error:

| Gate | Result |
| --- | --- |
| `cargo xtask check m4-acceptance` | **8/8 PASS**, aggregate PASS, 38 subcases |
| `cargo xtask check m3-acceptance` (same source) | **20/20 PASS** |
| `private-session-lifetime` components | **27/27 PASS**, zero leaked processes |
| `M4.lifetime` actor collection | 10 started, 10 collected, 0 pending |

Reports: `.artifacts/m4-c0acc0f2-source/.artifacts/{acceptance,m3,lifetime}-bf34cbfb/`.

`M4.lifetime` was bound only after its five subjects — stop, command loss,
service error, unwind, retained obligations — passed contained. The acceptance
body composes the same control bodies the component suite runs, so the two
cannot drift into disagreeing about what an exit must establish.

## What the mutation negatives changed

The seven compiled negatives the plan requires had never been run. When they
were, on `98dbf398`, **four of seven survived** a suite that was reading 22/22
and 8/8:

- an unmapped surface could be routed;
- an actor could be reported collected without being joined;
- retained bridge work could be discarded;
- a delivery place could be freed without the receipt reaching the caller.

Two of the three kills were incidental — they broke the pipeline until
unrelated controls timed out, which says something broke, not what. So 22/22
meant the tests passed, not that they would catch a regression.

Each gap now has a control that names it, and all seven are killed by their own
targeted assertion with every positive control restored green
(`.artifacts/m4-private-session/mutations-98dbf398/SUMMARY.md`). Three findings
are worth keeping:

- **A detector can be real and still never fire.** `execution != Retained`
  after the join is a correct assertion that loses a race: the report is sent
  one statement before the serving closure ends, so the keeper's abandonment
  beats the read either way. Nothing time-free distinguishes a wait from no
  wait — the absence of a wait is only visible if something is still there to
  be waited for.
- **A vacuity guard must not be anchored on what the defect rewrites.** The
  first unmapped-surface control guarded on the `mapped` field, which the
  mutation itself alters, so the defect surfaced as a timeout instead of as the
  routing it had found.
- **"Unreachable" deserves a second look.** A retryable control refusal was
  judged unreachable and injected, which meant plumbing a test carrier through
  production in release builds. It is reachable: the gate is the ready queue's
  control ceiling at `input_capacity * 2`, not the completion registry, and a
  burst of draws earns a real `Saturated` on the first pump. The injection and
  its plumbing were removed.

## Defects this slice repaired

Found by running the controls rather than by reading them:

- `publication_right_unclaimed` in the custody snapshot was inverted, and both
  assertions over it were wrong in compatible directions, so review agreed with
  itself until a real run did not.
- `stop` read three slots with `.ok()`. For the join slot that reported a
  thread never started while its handle stayed stored; for the command slot it
  was a deadlock, since no stop was sent while the receiver stayed connected.
- An exit owed what nobody took: queued observations and undrained receipts
  were reported as nothing owed.
- The frontend did not defer mapping to the committed decision, so the bridge's
  `AdmitSurface` was refused for a window already mapped. This was the last
  `committed_routing` blocker.

## Limits of this result

- **The contained workspace gate does not pass on this host, on this branch or
  on master.** `cargo xtask check` runs `cargo test --workspace --all-features`,
  and `atomic_scanout_hardware_smoke::native_atomic_scanout_smokes_real_primary_card_when_enabled`
  fails there while passing when run alone: it needs exclusive use of the
  primary card and the gate runs it beside 308 siblings. Verified identical on
  master with the desktop session logged out
  (`.artifacts/m4-private-session/workspace-hardware-fails-on-master-too.log`).
  Pre-existing, and not M4's to fix.
- **Two controls are intermittent under `cargo test` parallelism**, about one
  run in five, and clean 6/6 each in isolation. Both are bounded waits expiring
  under contention rather than wrong answers. The contained runner gives each
  control its own process, so the gate does not see them, and the acceptance
  body includes neither.
- Mutation negatives were run on the host. The contained harness refuses dirty
  source, so each would otherwise need its own commit, and what a mutation
  establishes is that an assertion fires — containment does not bear on that.
- No result here claims physical input, a native display, XTEST discovery, or
  t094.

## What remains under t093

M5 implements the four XTEST 2.1 requests with admission and cancellation; M6
covers affected checks, profiles, provenance and integration. Discovery stays
disabled. t093 stays open.

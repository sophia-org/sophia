# Stage 4a: synchronized validator wake evidence

Status: proposed source checkpoint, review before any build. Stage-4 scoping
(e36325ed) and the stage-4 consult select this runtime-local subdivision, not
complete runtime/backend composition. Stage 3 remains frozen at b239fe5b7.
No crates/*/src or Cargo change; no production feature, bridge or new worker.

## Ordering without inspecting P

The existing cfg(test) take_route call is already at the beginning of each real
validator job. Route now has an optional Checkpoint beside P: bounded(1) entry and
release channels, no callback, inspection bound, Debug or Any. P retains only
Send + 'static. Ordinary submit_combined delegates to the same submission body with
None; the direct generic transport fixture also initializes None. Only wake tests
provide a checkpoint. The gate is consumed before validate and is never part of
forwarded payload authority. These channels/holds are fixture machinery, not a
cost-free production mechanism or a scheduler change.

The checkpoint announces entry and waits for release for at most 30 seconds.
The test waits at most the existing WAIT (10 seconds), and reservations use
60-second deadlines. Thus a normal successful test releases a held validation
before its deadline. Failure to reach/release a gate is a test failure, not a
production timeout policy. Channel bounds and worker max_jobs are sized for every
reserved job (two for A/B and E/F, four for Full plus its successor).

After clearing take_completed to None on a fresh worker, submit A and receive its
forwarded envelope. B is immediately next, held at take_route. Receiving B's
entry orders all of A's iteration, including any forbidden notification after the
forward. Poll the actual owner readiness without consuming it, before any further
clear: it must be quiet. A's credit remains pending. This improves on stage 1's
unordered point observation; it still checks a finite controlled schedule, not
all concurrency histories or a real renderer path.

## Six written tests

1. Ordered quiet success, retained credit and an independent real control Wake
   in the same two-descriptor poll. This is not Session OwnerWake/Receive wiring.
2. Cancellation and malformed validation: checkpoint F follows error E, so the
   real ordinary notify must have executed before the IN assertion. Only then
   take_completed returns E and None; the returned plan precedes completion.
3. Full/Disconnected destination: original successful pixels and plan return via
   the ordinary completion/wake; no forwarded flag. A real prior combined job
   fills the one-place destination in the Full case. F orders the return path.
4. Publication after clear: hold before validation, cancel, call take_completed to
   None, release, then require IN and the exact completion. This is the documented
   clear-before-inspect contract on one schedule, not exhaustive lost-wake proof.
5. Normal exit: replace and drop the actual job sender from the private fixture,
   leaving the worker object/Wake alive. Require IN, then BrokenPipe. NotifyOnExit
   uses a Weak notifier; dropping the owner first would make this check meaningless.
6. Unwind: drop the checkpoint release sender deliberately. The validator thread's
   checkpoint assertion panics; the test thread requires the real exit guard's IN,
   then BrokenPipe. Its pending unreturned row remains held, not releasable via
   release_reserved. This is no synthetic completion or retirement proof. The
   validator-thread panic line, if present in logs, is expected output of this test;
   the actual Rust test verdict must still be ok. The release-channel disconnect,
   not expiration of the longer checkpoint timeout, triggers that unwind.

Once F is released there is no post-cleanup quiet assertion. Exit cases begin with
no queued completion; they do not prove every queued-completion/exit interleaving.
The unwind drops its fixture plan instead of returning it, as the existing validator
death limitation allows; this does not close plan recovery or credit reclamation.
All previous ten combined tests and ordinary/shutdown controls stay required.

## Mutations and boundaries

In separate worktree copies: add a success notify before the loop's continue
(ordered quiet assertion red); omit the ordinary completion notify (ordered error
IN assertion red, before any completion helper); omit NotifyOnExit notify (normal
exit IN assertion red before take_completed). Keep mutation diffs, bounded runs,
exact assertions and restored positives. After frozen-source acceptance use the
existing isolated targeted/control/mutation/full-gate/fmt/seal sequence.

No backend cell placement or poll-ordered flush is added. A manually ordered
sidecar wrapper would not prove the production path. Runtime credit settlement
still is not joined to retained backend Release; cfg(test) is absent in dependency
builds and a non-test bridge remains a scope decision. No GPU execution/teardown,
fallback/scene installation, native launch, push, service-count or CPU benefit
claim follows from this stage.

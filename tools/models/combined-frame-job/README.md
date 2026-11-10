# Combined frame job: preliminary ownership projection

This models a PROPOSED contract, not existing code or a Rust refinement. Parent
contract: n7fj3c2p. Category B: owner and worker state plus asynchronous authority
changes. Two sequential frames retain the displayed predecessor; one job at once.

Source mappings (pinned in source-pins.json): Admit corresponds to the future
replacement of propose_frame_commit / reserve_owned_frame_copy. WorkerReturn
would combine OwnedRingRead::copy, IsbValidationWorker validation and render_frame.
Submit combines the proposed owner result checks and an ABSTRACT submission gate.
Present abstracts exact completion AND last-use proof; it does not establish them.
Replacement/withdrawal stand for events serviced by image_owner::service and the
owner lifecycle, not worker-side authority. EarlyRelease/EarlyRetire/guard bypass
are intentionally incorrect mutations, not findings in existing Rust.

## Brief / scenarios and enabled coverage

- S1: revocation or epoch replacement during private work: FreshSubmission,
  targeted by stale.cfg, removing the Current guard on Submit.
- S2: stalled worker plus cancellation: WorkerOwnership and WithdrawalProgress;
  release.cfg releases before worker completion, none.cfg checks weakly fair owner
  withdrawal without worker fairness. Withdrawal is progress, NOT reclamation.
- S3: frame N+1 with predecessor N retained: PredecessorLifetime; retire.cfg retires
  the displayed predecessor before the completion/last-use action.
- TypeOK and SubmittedOwnership are structural checks enabled in every config.

The finite bound is two submitted attempts and one epoch replacement. It explores
failure and success replies, stalling via stuttering, and withdrawal at every
phase. CHECK_DEADLOCK FALSE permits stopped, withdrawn and exhausted scenarios.
No assertion of eventual render or presentation is made for a stalled worker.

## Deliberate omissions: whole-plan gate NOT satisfied

No bytes, placements, resource registry, source generation, budget amounts,
renderer slot Deferred, job misroute, scene generation, deadlines, journal/ACK,
notification ordering, worker restart, GPU last-use, or actual ioctl state machine.
Submit is an abstract authorization point: it does not prove atomicity of a fresh
poll and syscall. Real uncertain return, pre-ioctl ledger and proven detach MUST
be modeled separately before treating the whole design as qualified. Present's
last-use assumption is not validated here. A passing state count is not coverage.

No implementation exists for trace validation; no Trace.tla or fabricated traces
are provided. Prototype instrumentation must eventually emit job/epoch/output,
owner action, capture finish, validation, render, submit outcome, exact completion
and release-proof events. That later trace mapping remains a gate, not completed.

## Run

From this directory, use the pinned local TLC jar, one worker, 512 MiB heap,
outer timeout 30 minutes, writing logs/states outside the source tree:

    timeout 30m java -Xmx512m -cp /home/niltempus/src/Specula/lib/tla2tools.jar tlc2.TLC -workers 1 -metadir /tmp/<unique-state-dir> -config none.cfg FrameJob.tla

none must exit 0 with complete checking and temporal property satisfied. stale,
release and retire must exit 12 naming respectively FreshSubmission,
WorkerOwnership, PredecessorLifetime. All are design controls. No device access,
Rust build, runtime change, CPU or three-service proof follows from these checks.

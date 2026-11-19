# Proposed test-only real owner seam

Status: niltempus approved writing the test-only seam, with frozen-diff review
before any build. Research branch design/combined-frame-job;
accepted baseline 20070959b, owner-turn review 2350ce57. This is the concrete next
scope requested by the review, not a production change or a physical-run request.
CPU/frame and security remain primary; correctness and ownership come before a
cost comparison. No benefit is inferred from the model's service target.

## Scope and sequence

1. Tagged reservation and proof-consuming settlement, first checkpoint.
   Extend only test-compiled support adjacent to shell_content/isb_worker.rs to
   exercise the real IsbValidationWorker pending array, reserve/cancel/deadline
   checks and validation primitive. Introduce the test-only combined reservation
   tag/token and settle_combined(token, proof) seam. Ordinary/pre-forward failure
   still returns its real IsbValidationCompletion through unchanged take_completed;
   forwarded success cannot also spend that path. Prove shared four-slot capacity,
   byte/deadline retention, wrong/stale/duplicate proof refusal, and one clearance.
   Use existing shell_isb_worker and shutdown fixtures as controls. No copied
   admission ledger or synthetic ordinary completion for forwarded success.

2. Opaque payload transfer, coupled to that tagged path.
   The test-compiled envelope carries generic P: Send + 'static, with no inspection
   bounds, Debug, callbacks, Any, backend lease or device handle. Use real private
   validation and bounded channels, not a fake validated Boolean. Inspect/grep the
   type boundary and test cancellation and Full/Disconnected ownership return.
   A supplied test plan proves generic transport only, not the eventual scene-plan
   implementation. Notification behavior is checked after Release retention below.

3. Dedicated backend Release retention on the real worker facade.
   Test-only additions adjacent to exporter/worker.rs and its existing
   tests/support/renderer_worker.rs fixture reserve a separate combined cell per
   facade/output at admission. Exercise the new retain entry against the real
   bounded WorkerCommand channel and existing slot tokens. Full retains the token;
   success moves it into the channel; neither is physical slot freedom. The ordinary
   discarded_release cell must survive concurrently. Retain precedes quota clearance
   within the consuming owner turn. Disconnected is not successful transfer; any
   supplied retirement proof is labelled and cannot establish real device teardown.
   Include occupied-slot negative control at try_acquire. No new Release ACK.

4. Wake behavior on the composed test route.
   Normal forwarding must leave owner validation readiness quiet; validation error,
   exit and control readiness remain observable. Preserve NotifyOnExit and the
   unchanged ordinary completion path as controls. Do not mistake a quiet supplied
   sender for proof that the real worker dispatch route avoids the success wake.

5. Fallback is then a conventional content-service fixture.
   Reuse candidates::begin_submission_inner, existing Session submit_bundle callers,
   SessionContentFixture and existing target/completion seams where reachable.
   Assert no pre-decision candidate mutation, one ordinary candidate entry, existing
   error rollback and deferral, owner collection before later retry, and persistent
   shell_content/Presented tracking across pending-render supersession. No new
   fallback caller, no second render invented by the fixture and no simulated
   completion reported as physical proof.

## Files and boundary

Expected additions are test-support children of the existing runtime worker,
backend worker and Session content-test modules, plus narrowly cfg(test)-guarded
mounts/fields/methods needed to reach their actual private state. Follow existing
source-layout test mounts. No production visibility widening, feature flag, new
runtime API, dependency, unsafe code, authority shortcut or second owner. No new
thread or polling path in the non-test program. Existing take_completed behavior
is unchanged. Approval permits the ordered research edits, not production changes.

A cfg(test) implementation can establish behavior of that research route, not its
presence in a normal binary. If real-seam access or generic forwarding requires a
non-test refactor, stop with the exact diff/scope needed; do not substitute a copied
worker loop or silently widen authorization. Cross-crate private access and generic
Job wiring are implementation questions to resolve in the first frozen diff, not
claims already discharged by this proposal.

## Review and validation gates

Approval selects writing the ordered test-only seam. Freeze a signed clean diff
for bounded read-only review BEFORE any build, as the lane requires. Each stage
must make the prior proof and its limits clear; it need not wait for all stages to
become an unreviewable change. Only after source review permits it: device-hidden
cargo-slot gates, targeted tests, named negative mutations and relevant existing
controls, followed by required isolated repository checks. Record exact frozen
identity and retain failed attempts; seal evidence without copying binaries.
No native launch, timing, production merge, SDK/wire change, runtime promotion or
three-service/CPU claim is included. Any physical comparison needs a later package
and niltempus's launch decision. The normal owner/private-validation/security
boundaries remain the acceptance conditions throughout.

# Stage 1: test-only validation credit handoff

This describes the frozen stage 1 source at 2269c4d33, accepted with validation
seal a5e5f616. The next source checkpoint extends the fixture as described in
[PAYLOAD-SEAM.md](PAYLOAD-SEAM.md); stage 1 evidence remains unchanged.

Niltempus approved SEAM-PROPOSAL. This first frozen diff requires source review
before any build. Formatting and source-layout checks are not compilation/test
evidence. No production, device, runtime or CPU claim follows.

## Existing owner and added route

`IsbValidationWorker::reserve` still allocates from its real four pending slots,
with its original byte/deadline/cancellation facts and monotonically increasing
id. The test-only `reserve_combined` tags that same row with a fresh Arc identity
and returns a non-Clone reservation. There is no second admission table. Ordinary
ISB/ring/draw constructors leave their test route None.

Test-only submit constructs a real Job from the tagged row; it does not change the
original deadline or validate on the owner thread. The existing validation loop
extracts the optional route, calls the unchanged validate function, then forwards
successful owned ValidatedIsbPixels to a bounded channel. A successful move skips
the ordinary completion send. The receiving fixture is an unrendered downstream
endpoint: it returns those pixels and exact job/reservation identity to
settle_combined. That operation verifies both identities against the live submitted
row, clears just that row, and RETURNS the backing to the owner. No resource budget,
source journal or backend lease is released by that accounting operation.

`take_completed` is textually unchanged. Validation errors take that existing
path. If the downstream try_send is Full or Disconnected, no move occurred and the
actual validation result returns there too, unchanged. This validation-only seam
does NOT select the final renderer-queue failure classification: it invents no
peer refusal reason and does not silently discard successful validation. Shared
output reservation/failed handoff disposition remains a later composed obligation.

The tag distinguishes equal numeric ids in separate allocators. The token and
returned object are not Clone; invalid settlement returns both untouched. Tests
forge one replay deliberately to exercise the stale-row guard. Dropping a token
does not refund submitted credit; completion/settlement or existing worker teardown
owns that obligation. There is no forwarding flag or native retirement proof here.

Cancellation is not a forwarding gate: validate's final check precedes finish,
and forward does not recheck. Cancellation in that interval can forward a row that
the owner must still settle. Even another check would only narrow the race. Render
start and owner publication need their own checks; forwarding grants no authority.

Known stage 1 gap: a receiver dropped after successful send destroys the returned
pixels but leaves the pending credit until worker teardown. Cancellation does not
recover it and there is no ordinary completion. The new destination-drop test
pins that lost-return/capacity state using an ordinary FIFO barrier before dropping
the buffered destination. Later work must compose forwarding evidence and qualified
disposition; a drop alone is never a native-resource release proof. This is a
reclamation gap, not a disproof of Transfer's WithdrawalProgress: that property
requires owner authority withdrawal independently of worker progress, not reclaim.

## Written checks (not run before review)

- Mixed ordinary/combined reservations exhaust the same four slots; replacement
  cannot recycle the released id.
- Real forwarded validation retains credit, has no ordinary completion, survives
  cancellation as an owed result, and settles exactly once while returning pixels.
  A forged replay cannot clear a newer row. A zero-timeout readiness poll before
  take_completed checks no IN for that one job at that observation. It cannot rule
  out a later notification race or establish the composed wake contract.
- Equal-id foreign-allocator proof and wrong-job proof refuse without clearing
  either row or losing either returned payload.
- Stale submit returns the original fd; expired deadline, size mismatch and
  malformed pixels return real validation failures through take_completed.
- Cancellation before submit retains its completion obligation and returns once.
- Dropping the destination after the worker's successful send keeps one credit
  pinned; cancellation and release_reserved cannot recover it in stage 1.
- Full downstream is placed deterministically using an ordinary FIFO completion
  barrier. Full/disconnected returns preserve the old owner result and clear only
  when that ordinary completion is taken. The Full fixture deliberately shares a
  destination; it is not proof of the future per-output inbox reservation.

## Scope and follow-up

Only cfg(test) fields, initializers, dispatch glue and a private external test
module are added to src. All implementation/test bodies for the new seam reside
under tests/support. No production visibility, dependency, feature, or unsafe code
is added. Source-layout exceptions name those two wiring files exactly.

The first-stage source uses sealed ISB descriptors, not the owned-ring copy path.
It proves neither opaque P nor scene authority nor rendered-lease settlement; those
are later stages. The implementation skips the success notify, but no composed
wake/exit/control proof is claimed yet. The real shutdown and ordinary worker tests
are existing controls for post-review validation; integration tests compile the
normal library, while these private assertions compile the test-only route.

Stage 2 access decision: keep runtime composition assertions in runtime's own
test mounts. Do not add a default-off production feature for test visibility.
Backend-live tests compile runtime without cfg(test), so they cannot call these
private methods. Stage 3 tests its real retained-Release facade locally; those
independent crate-local checks are not an end-to-end runtime/backend composition.
An actual cross-crate composition bridge remains a review gate, not permission to
copy the allocator, fake its proof or widen the production API. If it requires
non-test source, return with that exact scope before implementation. Add the
per-job forwarded witness with the opaque-P step; no such fact is inferred today.

After bounded source review: isolated targeted tests and ordinary/shutdown controls,
named identity/early-credit mutations, then required gates on an unchanged signed
tree. Next frozen source step adds the opaque plan boundary; retained backend
Release and full wake behavior precede the conventional content fixture. All remain
device-free and behind separate production/native gates.

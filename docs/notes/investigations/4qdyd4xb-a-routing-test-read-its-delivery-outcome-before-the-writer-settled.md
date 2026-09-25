---
id: 4qdyd4xb
date: 2026-09-25
kind: investigation
status: closed
tags: [investigation, x11, validation]
---
# A routing test read its delivery outcome before the writer settled

## Question

Why did the private producer's key-before-pointer control sometimes observe no
delivery answer after it had already read the key event from the client socket?
Sophia t148 owns this specific test race; t194 retains the other loaded-run
failures.

## Evidence

The retained t148 report observed two failures in forty runs on `76b5b920` and
one in forty three commits later. The implementation record for signed
`4a58c020c7dd63651bb8ea2f3d1a129a3b5882d6` reports seven premature delivery reads
in twelve loaded runs before the repair. That commit also repaired a separate
store-settlement wait; its result does not close all of t194.

Review of the twelve retained `t194-repro-{round}-{copy}.txt` extracts confirms
seven runs containing the delivery-cell failure, five containing the separate
origin-store failure, and eleven failed runs overall. Some runs contain both;
other failures belong to t194. The extracts and original harness output are
retained separately in
`~/.local/state/sophia/development-evidence/todo-closure-t148-original-load`,
with their counts in `summary.json` and file hashes in `SHA256SUMS`. These are
historical artifacts supplied by the original investigator, not a new load run
or independent reconstruction of its source identity.

The accepted source reviewed here is
`9ee301e74ef22a065dcb112dc1873c7e458dfec4`. Its
`a_key_through_the_service_before_any_pointer_observation_carries_the_prepared_position`
control retains the original delivery cell, reads the real socket event, waits
for that cell's answer, then checks the exact client/delivery and `Flushed`
outcome. It also checks that service shutdown cannot replace the answer.

## Finding and resolution

The writer can expose event bytes before publishing its delivery outcome.
Reading the event therefore does not establish that the outcome is already
available. The repair uses the existing bounded `waited_for_value` helper to
observe that outcome instead of making one immediate read or adding a sleep.
No product routing or delivery semantics changed.

## Validation and remaining work

On 2026-09-25, all forty fresh exact-source invocations passed. Every invocation
executed exactly one test, with no failures or ignores. The unchanged clean
source and compiled binary hash are recorded alongside the individual logs in
`~/.local/state/sophia/development-evidence/todo-closure-9ee301e7`;
`SHA256SUMS` covers the retained bundle. The original run directory is
`/home/niltempus/dev/sophia/.artifacts/todo-closure-9ee301e7-57kctrdv`.

These are forty serial confirmations, not a repeat of the original concurrent
stress experiment. Its seven-in-twelve count is supported by the retained
extracts and signed repair, not a freshly reproduced result. Source review
establishes that the immediate-read race was replaced by observation of the
settlement fact. Devices and session sockets were hidden. This closes t148's
specific harness race, not physical keyboard acceptance or the other t194
failures.

## Connections

The [private-input wait investigation](yo5l2jui-private-input-controls-fail-on-a-wall-clock-deadline-under-load.md)
explains the broader progress-wait discipline. The
[private input authority plan](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md)
owns the distinction between accepted work and settled recipient delivery.

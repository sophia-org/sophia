---
id: chc18alj
date: 2026-09-18
kind: investigation
status: awaiting-physical-acceptance
tags: [investigation, native-shell, runtime, dock]
---
# Completed runtime observations must be chunked without dropping work

The attended dock capture `20260918T165507Z` ran Sophia `61f26641` and Hagia
`90590a56`. At 16:57:17.873Z the owner loop failed during the CPU cycle's
KmsSubmit phase: `session runtime observation batch exceeds 64 events`.
Subsequent cleanup recorded native drained and shell components quiescent;
the process exited 1. Original logs and hashes are retained under
`.artifacts/dock-crash-20260918T165507Z`. This is not evidence against the
xterm color settings or a kernel/GPU failure.

The live adapter concatenates X polling, every completed authority transaction,
and scanout lifecycle observations. The driver previously submitted that entire
vector to the runtime's bounded batch decoder. Session's merged-input budget
does not bound one atomic input head, nor all lifecycle observations. Refusing
these observations after work has committed terminates an otherwise valid tick.
The capture does not record the exact oversized vector, so its precise producer
mix is unknown. The real live-adapter/driver regression reproduces the identical
error with supplied completed commits and retirements, without native devices.

The driver now validates consecutive chunks of at most 64 observations before
reducing any of the intake. It reduces the validated events in original order,
then schedules their commands in original order. A malformed record in a later
chunk still refuses the whole intake without partially updating runtime state.
The strict runtime batch API, Session intake/merge limits, and transaction
atomicity are unchanged. This does not increase admitted work per turn or claim
a new latency bound; it accounts for the already-completed work without loss.

The compiled original-source regression fails with TooManyObservations. The
repaired control checks exact counts for 63, 64, 65 and 193 commits plus the same
number of scanout retirements, with one subsequent frame/submission and correct
in-flight state. Separate controls pin cross-chunk command order and invalid
late-record atomic refusal. Device-hidden driver tests (35), runtime supervisor
tests (30), and strict Engine lib/test Clippy pass. Broader gate results and the
signed candidate identity belong in the adjacent evidence summary. The first
shortened exact test selector ran zero tests and is retained as non-evidence.

The [catalog placement repair](n8r3d6qp-catalog-launch-must-capture-the-clicked-output-workspace.md)
separately produced three matching logical-output attribution/commit pairs in
this capture. Those do not make a crashed session pass, prove physical connector
mapping, or close the [three-component acceptance plan](../plans/ptil1ejw-modular-native-shell-components-and-independent-launcher-critical-path.md).
No hardware, display connection, installation or restart was used for this repair.
Attended native acceptance remains open.

---
id: z57dundg
date: 2026-09-18
kind: investigation
status: investigating
tags: [investigation, x11, validation]
---
# Retained private control cleanup

This follows the control ownership contract in
[production entry and ownership](w8vt0ueb-production-entry-and-ownership-for-ordered-synthetic-input-execution.md).
The first source checkpoint is signed `798034d4` plus `5c9bd5c8`, based on
integrated `f6d22edc`. Its ten exact component controls pass in
`.artifacts/m3-finish/control-cleanup-5c9bd5c8/report.json`; compiled endpoint
and publication bypasses N50/N51 fail their intended assertions. Those artifacts
record process containment and source attestation. They are not aggregate C
acceptance. This existing note keeps its filename; its metadata was created
through the notebook's `zk investigate` workflow.

The first supported cleanup is ConfigureSurface interrupted after the runtime
change and before projection or peer event generation. Actual connection setup
retains its original endpoint, resource range and native state. Before the
writer begins Configure, its reserved control record takes execution custody.
Generated local output also stays in that custody through failed or unknown
writes; this is not permission to replay it.

Native resource release records the exact endpoint and removed resources before
the subsequent property and publication steps can fail. The source retains the
resource-release payload and, before invoking the cleanup observer, the actual
derived transaction batch. A native removal receipt does not settle that batch.
The source marks teardown finished only after the actual cleanup observer has
returned successfully. Failure preserves its payload and unknown disposition.

One retained terminal maintenance phase, under the original service budget and
watchdog, visits one control record or one carried control credit. Cleanup needs
an abandoned original record with no dependents, exact native removal and
finished teardown publication, and the original endpoint's independently
established termination. It removes the original connection projection and
retires the exact control record without publishing an acknowledgement. A later
visit returns only its carried control credit. Final custody certification also
requires the original control registry to be readable and empty.

This first slice does not reconcile later Configure progress, peer generation,
or the other eight control kinds. Those stay explicitly outstanding. It does
not reinterpret a cancelled cleanup publication as delivery: direct service
stop can cancel egress before client teardown, and that source remains retained
with its actual batch. The positive control closes the interrupted client while
the service's original egress can publish its teardown, then performs cleanup
after actual service collection. Component controls are not C.control_cleanup
acceptance for all nine kinds.

Source controls are external in tests/support/private_control_cleanup.rs. They
exercise the real writer's runtime effect, retained credit and original
projection, withheld removal and publication evidence, foreign and replaced
receipt identities, and actual stop cancellation retaining its original batch.

## Nine-kind source extension

The next slice retains the journal before routing, including the focus effects
which precede the writer. The original writer resumes that exact journal for
every control kind. The source retains its original surface and metadata maps;
post-collection maintenance removes only that connection's projection after its
exact resource removal, property removal, teardown publication and endpoint
termination have all been established. It does not roll back or replay commands.
External controls interrupt actual metadata insertion, runtime admit/configure,
presentation properties, focus application, clear focus, owned shutdown and
unmap, and withhold the real removal receipt before permitting cleanup.

Unknown peer generation remains a separate retained refusal. Routing records it
before a cross-client FocusOut; a zero dependent count proves quiescence only.
Actual metadata candidates are retained before publication and kept on failure.
Actual local wire bytes are retained before write and can be discharged only by
the original recipient's independently established termination. Actual cancelled
teardown batches remain owned and block final custody. None of these debts are
reclassified by the absence of a later runtime row or by namespace clearance.

The milestone's aggregate acceptance and execution order remain in
[the existing task ledger](../../../todo.md); this investigation is evidence,
not a parallel task list. No hardware acceptance has run for this repair.

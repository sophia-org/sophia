# Retained private control cleanup

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

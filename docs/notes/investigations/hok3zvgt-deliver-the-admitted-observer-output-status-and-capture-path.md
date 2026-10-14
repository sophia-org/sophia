---
id: hok3zvgt
date: 2026-10-14
kind: investigation
status: investigating
tags: [investigation, protocol, capture]
---
# Deliver the admitted observer output status and capture path

## Current resumption

niltempus explicitly promoted the zsx0tk4k/ernn0bkv observer milestone ahead
of CPU optimization on October 14. t319 now includes the narrow t257 output
availability prerequisite; the task ledger owns order. t289 remains open behind
it. Reuse completed t315/t318 admission, composition and recipient bindings.

The first implementation adds `output-status` to the existing admitted
inspection export. It reports the actual native owner/seat separately from
retained policy geometry, with bounded recovery/failure fields and an absolute
session-relative retry deadline. Publication copies one bounded record with a
nonblocking lock attempt; contention withdraws new observations until a later
successful report. Opening a file pins an observation, never capture authority.
Existing authentication, HostDomain admission and revocation still apply.

This is an aggregate status slice, not the complete observer or capture exit.
It deliberately labels the native retirement count as an owner counter, not a
per-output presentation generation. Still required: per-output capture identity,
immutable composed-output readback (CPU and accelerated content), renderer and
lock/revocation joins, an admitted CLI, negative controls, capture-on/off
measurements and attended qualification. No current-session installation,
reload, input injection or screenshot is performed by this implementation.

An installed-session KVM failure interrupted this work at 13:38 UTC. Its
[recovery investigation](nnhpzesp-kvm-output-replacement-retains-shell-pixels-at-the-old-size.md)
owns the immediate repair. The observer changes were not installed when it
occurred.

## Source findings

The WM inspection snapshot retains scene outputs across display loss. Those
records alone cannot establish that a physical owner is available. Session's
output topology owner already tracks quarantine, rebuild, policy publication
and presentation, and the retry owner retains reduced failure identities.
The new view derives from those owners rather than encoding an empty revision-1
output topology, which that protocol forbids.

The existing portal recipient export binds one generic immutable object and
fences reads on revocation. It currently advertises `request=unavailable`.
Capture must supply the renderer-owned object and real grant/execution path;
adding an advertised name or handing it synthetic pixels would not complete
that join. `t318`'s observer dispatch already supplies authenticated inspection
and portal-recipient endpoints, so no ambient screenshot socket is needed.

The next implementation must join three existing boundaries. Session creates
each observer's recipient in `live_session/application_launches.rs` and serves
it through `namespace_launch/dispatch.rs`; that is where the admitted request
must meet its recipient. `sophia-portal/src/recipient_object.rs` currently limits
generic objects to 65,536 bytes, so it cannot carry a full output image unchanged.
The native renderer's `PersistentXrgb8888GlPipeline::read_composition_pixels`
currently returns diagnostic metrics, not retained image bytes. Its diagnostic
capture switch is not an authorized screenshot mechanism. Add bounded immutable readback at the
renderer boundary, then bind it through the capture grant with the output's
actual presentation identity and lock/revocation checks.

## Contract and validation

The new status line is documented in `docs/sophia-wm-inspection.md`. It is
read-only and bounded below 1024 bytes. Absent/fenced reports and lost
publication return EAGAIN on open. An old attachment returns ESTALE after its
service fence. Already opened status bytes are historical observations; future
capture must recheck its own live grant, output and generation at delivery.

Tests exercise output loss while policy geometry remains unchanged, immutable
partial reads, publication contention, write refusal, service fencing, and
refusal to format unbounded backend error text or available-without-owner
records. The runtime inspection owner suite passed 13 tests; the real-wire
inspection service suite passed 10 with one existing ignored test. Native
Session compilation and strict Clippy passed. The final full repository gate
passed on the combined observer/recovery working tree, with its log retained at
`~/.local/state/sophia/development-evidence/t327-kvm-size-20261014/sophia-t327-check.log`.

An earlier gate reached an unrelated bootstrap fixture failure because this
shell's `umask 0002` made the temporary trusted bundle group-writable. The
bootstrap correctly refused it; its three tests pass with `umask 022`, used for
the successful full gate. Security checks were not weakened. A rerun interrupted
by the KVM repair's source edits is not counted as a passing gate.

These checks establish the bounded status implementation, not capture,
performance or attended output-recovery acceptance. The separate recovery
commit `1e2c9e50a` is the release candidate; it excludes this observer code.

## Connections

- [Convergence plan](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#first-deliverable-and-dependency-order)
  owns the promoted observer milestone and unchanged acceptance gates.
- [One-core decision](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md)
  owns authority and composition.
- [Plan 9 integration](../concepts/ernn0bkv-plan-9-integration-points-for-sophia.md)
  explains the observer and driving separation.

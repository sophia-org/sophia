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
records the completed repair: the matched release survived an attended KVM
away/back switch and subsequent normal logout with clean native resource
disposition. The observer changes were not installed when the failure occurred
and remain excluded from that release.

The native renderer now has an explicit composed-image readback operation.
It returns bounded immutable RGBA8 bytes; the CPU/GPU pixel and lifetime test
passes on the isolated render node. The portal recipient now binds a small
frame record and separately retained image under one common lease. Its source
pins a selected presentation and serializes output replacement with reads.
The live renderer-worker/completion identity, request admission, aggregate
retention bounds and CLI remain unconnected. Do not treat supplied presentation
records in the portal tests as proof that KMS presented a frame.

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
must meet its recipient. `sophia-portal/src/recipient_object.rs` limits generic
objects to 65,536 bytes. Capture keeps that bound for its record and retains the
image separately under the same lease.
The native renderer's `PersistentXrgb8888GlPipeline::read_composition_pixels`
returns diagnostic metrics, not retained image bytes. Its diagnostic capture
switch is not an authorized screenshot mechanism. The new explicit readback
supplies bounded immutable bytes at the renderer boundary; the next join must
bind them through the capture grant with the output's actual presentation
identity and lock/revocation checks.

The backend already mints `LiveNativeFrameIdentity` with independent native
owner, output, head, target-generation and frame fields in
`scanout/rendered_scanout/exporter/native_identity.rs`. Reuse the actual
completion identity rather than deriving a receipt from a scene trace or a
render request. `LiveProductionHeadCompositionFrame` retains the per-head
composition inputs. Its route through the renderer worker is the next capture
join; direct scanout also needs an explicit composed-image path when capture is
requested. Do not claim the current primitive handles that join.

Session already creates a `PortalExecutionFence` in `live_session/run.rs` and
blocks/unblocks it in `owner_loop/session_lock_phase.rs`. Capture must share
that barrier. Keep output-incarnation validity separate from the selected
frame's immutable presentation identity: ordinary later frames must not silently
replace an opened image, and an output replacement must not revive its grant.

## Contract and validation

The final delivery barrier still needs an explicit connection-level join.
`PortalClientConnection::poll` drives the shared 9P server, whose connection
queues encoded replies after an export read. A successful guarded read alone
does not prove that queued image bytes cannot be written after lock. The live
capture path must cancel or fence unsent replies before applying that lock,
with a real socket/backpressure control. Holding the existing execution mutex
around the whole server turn would re-enter it during export reads and is not
a valid solution. Keep this separate from the tested source/read barrier.

### Capture record and recipient binding

The new passive 64-byte record names an output incarnation and a completed
presentation, with dimensions, stride and RGBA8 format. `OutputCaptureSource`
selects one immutable image for a handoff; the common coordinator authorizes the
record, and recipient installation obtains that exact image from the trusted
source. The upload protocol's 64 KiB bound is unchanged. The recipient exposes
`frame-info` and `frame`, with one shared revocable lease and fresh binding qids.
The [recipient contract](../../portal-client-files.md#output-capture-bindings)
owns the format and read semantics.

The source-generation guard extends the common lease's read barrier. Its lock
order is execution fence, source, recipient. Publication and output withdrawal
take only the source guard; binding revocation takes only the recipient guard.
Later presentations do not alter an opened image. Output withdrawal waits for
an earlier guarded read; replacement cannot reuse an incarnation. Lock applied
during collection prevents installation, and unlock does not revive old access.

Seven capture-binding tests pass with the real common coordinator, lease and
recipient export and supplied presentation records. They cover a 256 KiB image
through bounded reads, exact record/image agreement, later-frame immutability,
fresh binding identity, output replacement with an unrelated output still
usable, lock/revoke behavior, withdrawal/read serialization and refusal of
invalid presentations. The record codec also passes an independent fixed-byte
fixture and malformed-field checks. Removing the output-generation comparison
from the guarded read made the replacement control expose old pixels rather
than return `ESTALE`; the comparison was restored and all controls pass.

The portal/protocol suites, seven capture-binding controls, record-codec test
and full `umask 022; cargo-slot sophia cargo xtask check` gate passed. The full
gate includes workspace tests, strict Clippy, SDK checks, render-node pixel
checks and evidence verifiers. Logs are retained under
`~/.local/state/sophia/development-evidence/t319-capture-binding/`.
These are supplied-presentation tests, not live renderer or network-delivery
acceptance. The production Session capture path is not installed or connected.

### Composed-image readback

`NativeGbmRenderedScanoutContext::export_composed_with_snapshot` reads the full
composed image before swap. A snapshot owns tightly packed, top-down RGBA8 bytes
with a 64 MiB limit checked before allocation. Ordinary exports do not request
readback. Failed requests return no image and cannot reuse an earlier result.
The image has no native handles and survives later renders and context teardown.

The new `composition_snapshot` tests reject invalid bounds and mismatched byte
counts, retain immutable storage, and compare exact pixels from a CPU upload
and an independent GPU producer's DMA-BUF. The native test also checks a fresh
capture after an invalid request and retains both original images after later
rendering and destruction of both contexts. Removing the GL row reversal makes
the test fail with the red and blue rows exchanged; the reversal was restored.
Strict crate Clippy and the full `umask 022; cargo-slot sophia cargo xtask check`
gate passed, including workspace tests, strict workspace Clippy, SDK checks,
render-node pixel checks and evidence verifiers.

Native evidence lives in
`~/.local/state/sophia/development-evidence/t319-composition-snapshot-final/`;
the deliberate failure is in the adjacent `t319-composition-snapshot-negative/`.
The passing test binary has SHA-256
`f183b7e667a5d7fe7be0d3114ae5625141d01dad879eaba40960375fb8699fe6`;
the full gate log has SHA-256
`ce21b116f476fd1f5881b0896f498c13e8a60d66594280b982ca3292c7975550`.
The runner admitted only PCI `0000:03:00.0`'s render node, with primary DRM,
physical input, VT and live display sockets absent. No live screenshot or
desktop replacement was performed. This validates renderer readback, not the
remaining grant, presentation-identity, lock/revoke, CLI or performance exits.

### Aggregate status

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

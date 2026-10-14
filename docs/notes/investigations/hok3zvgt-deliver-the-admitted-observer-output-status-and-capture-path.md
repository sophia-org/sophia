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
per-output presentation generation. Still required: Session capture request
admission and publication, aggregate retention bounds, an admitted CLI,
capture-on/off measurements and attended qualification. No current-session installation,
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
Queued recipient replies now retain that authority through each nonblocking
socket write. Revocation closes a transport with unsent protected bytes,
including a partly written reply. Socket-backpressure tests cover this boundary.
The renderer worker now carries requested image bytes with its exact native
frame and scanout lease. The native output owner returns them only from matching
displayed custody; mirrors require every enabled head to converge. Capture
forces composition for its exact queued frame, including direct candidates.
Portal frames now require a reservation from a shared retention budget, capped
at 128 MiB and eight frames. Reservations and retained frames share the cap;
source/recipient sharing does not double-charge an image, and replacement
releases its charge once the last frame reference is gone. Session request
admission, publication into the generation-bound portal source, reservation
before native readback, native custody bounds and CLI remain unconnected. The tests use supplied
completion callbacks; they do not establish physical KMS capture acceptance.

The next integration must authorize the target separately from its observer
request capability. Use one Session budget and the existing lock fence.
`run_native_pending_output` stages and submits in one call, so admission after
that call is too late to mark the exact queued composition for readback. Reserve
and mark it before submission, then publish only matching displayed custody.
All enabled mirror heads need accounting even though the recipient receives
only the primary image. Withdraw the portal source when native owner or head
targets change; a renderer frame number alone is not an output incarnation.

## Portal retention budget

The frame constructor now consumes a reservation rather than accepting
unaccounted pixels. Reservations validate the existing 64 MiB per-image record
limit and atomically charge both bytes and frame count. Invalid completion,
cancellation and final frame release return capacity. Pressure leaves the
current recipient object unchanged. An old fid holds only identity and cannot
keep an image charged after recipient replacement. Revocation alone does not
free retained storage, so its charge remains until the owner releases it.

Tests exercise simultaneous reservations, independent byte/count limits,
cancellation, malformed storage, shared references, two output sources using
one budget, and replacement through the real common coordinator. These establish
the portal storage primitive. They do not bound native scanout references or
prove that Session shares one budget; that wiring remains required.

The full `cargo-slot sophia cargo xtask check` passed, including strict Clippy
and the existing isolated graphics checks. All nine capture-binding tests and
three retention tests passed. Changing the capacity refusal from either limit
to both limits made the independent-limit test fail; restoring the check made
both suites pass again. Logs are under
`~/.local/state/sophia/development-evidence/t319-capture-retention/`:
`workspace-check.log`, `negative.log` and `targeted-final.log`. The workspace
log SHA-256 is
`6f798b8d535601c8cf73a1f85e633acd654af8302102709c73a872c451e814ee`.
No live installation or physical capture qualification was performed.

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
composition inputs. Requested snapshots now follow that route through the worker
and displayed custody. The remaining Session join must map the full native
owner/output/head-target identity to a fresh portal source incarnation, publish
only the confirmed frame, and invalidate that source on replacement. Neither a
scene counter nor a hash of native fields is a substitute for this mapping.

Session already creates a `PortalExecutionFence` in `live_session/run.rs` and
blocks/unblocks it in `owner_loop/session_lock_phase.rs`. Capture must share
that barrier. Keep output-incarnation validity separate from the selected
frame's immutable presentation identity: ordinary later frames must not silently
replace an opened image, and an output replacement must not revive its grant.

## Contract and validation

### Native frame and presentation join

`request_pending_output_capture` checks every queued head before scheduling
readback. Each worker reads the same frame slot that supplies its exported
buffer. The image and native identity stay with the lease through submission,
failure and retirement. `presented_output_capture` consults displayed custody,
checks current head generations and mirror convergence, and returns the primary
head's immutable image. These trusted backend methods create no recipient grant.

The worker controls reject a missing image, a foreign native owner and wrong
dimensions. Another control proves that a superseding frame cannot inherit a
capture request. A valid direct candidate takes the composed path only when
capture is requested. Real custody tests with supplied callbacks refuse reads
before presentation, after an old callback and with a mismatched identity;
retiring displayed custody removes access. Letting submitted custody satisfy the
read deliberately fails the pre-presentation assertion.

The isolated GPU-worker test checks exact pixels from a CPU texture and a
DMA-BUF drawn by a separate GPU context. It also checks that ordinary rendering
returns no image and later renders leave a retained image unchanged. Evidence
is in `~/.local/state/sophia/development-evidence/t319-native-capture/`.
The test grants only render-node access; it supplies native identities and
does not perform KMS presentation, live capture, portal admission or a physical
mirror test. Existing mirror lifecycle tests remain separate from attended
capture qualification.

The full repository gate passed; `workspace-check.log` has SHA-256
`27027a345bc45a87b9e5418fedb9775e50e63edfbff4a90387dd74be1807230b`.
A later isolated run passed its pixel assertions but aborted on process exit
with `corrupted double-linked list`. The fixture had omitted explicit worker
shutdown and join. After adding those before producer destruction, five
consecutive isolated runs exited cleanly and retained pixels remained readable
after both contexts were destroyed. Their binary SHA-256 is
`ccd7eb555f7d0b8623808395ee5e231f407359081fbbe046aa5404e9f9d3fe98`.
The `undrained-*` files retain the failed exit; `drained-0-*` through
`drained-4-*` record the corrected runs. Only the ignored test fixture changed
after the full gate; backend tests and strict Clippy were rerun.

### Queued recipient replies

The shared 9P core now retains the node and epoch of each protected read until
its reply leaves the queue. It checks the exact recipient binding under the
common execution and source guards during each nonblocking socket write.
Unguarded replies ahead of it may be batched, but cannot carry protected bytes
past their check. Clunking or reusing a fid does not change queued authority.
Revocation closes the connection and drops the remainder; a partly sent frame
cannot be replaced with `ESTALE`. Bytes already in the kernel before revocation
cannot be recalled. The whole server turn never holds the execution mutex.

The socket tests fill a real Unix send buffer to `WouldBlock`, revoke the
original authority, drain the padding and require EOF without image bytes.
They also cover a partial header, fid reuse, an unguarded prefix and cleanup by
the production Unix driver. Portal tests separately exercise real recipient
leases against lock, binding revocation and output withdrawal after a read.
These tests do not establish the still-pending native presentation join or
live request admission.

As a negative control, bypassing `write_output` in the Unix driver made the
backpressure test receive a byte after revocation instead of EOF. The guarded
driver was restored, and all tests in `sophia-9p`, `sophia-portal` and
`sophia-runtime` passed. Evidence is under
`~/.local/state/sophia/development-evidence/t319-delivery/`; `negative.log`
records the deliberate failure and `final-focused.log` the restored tests.
The initial workspace gate passed tests and lint but refused the connection
file's size. Moving delivery into its own module brought the parent below the
limit; the separate layout gate passed.
The final full `cargo-slot sophia cargo xtask check` passed, including the
isolated native pixel checks. Its `workspace-check-final.log` has SHA-256
`00f67077eb8b5efebde00f44dd9a2de1539f9d35655966554bba6e8e3116c315`.

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

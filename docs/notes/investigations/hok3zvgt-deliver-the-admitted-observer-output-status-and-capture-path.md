---
id: hok3zvgt
date: 2026-10-14
kind: investigation
status: investigating
tags: [investigation, protocol, capture]
---
# Deliver the admitted observer output status and capture path

## Current resumption

The repaired release `niltempus-1ce83e7579cba2b564c9` is installed and running
(Sophia `629cb8a80`, integration `243605d`). Capture now succeeds on both
physical outputs. Launch 2 returned output 1, generation 1, presentation 885:
2560×1440 RGBA, 14,745,600 bytes, FNV-1a `4852afff10148408`. Launch 3 returned
output 2, generation 2, presentation 1352: 1920×1080 RGBA, 8,294,400 bytes,
FNV-1a `7f4a8fe20ae7204f`. Both exited successfully; their recorded process
lifetimes were 497 ms and 472 ms. These are individual observations, not a
performance qualification.

The session evidence is
`~/.local/state/sophia/sessions/00000001792003516767-bbb488ca-c687-4eff-aec7-76fe840a8dbc`.
The capture evidence directory
`~/.local/state/sophia/development-evidence/t319-session-capture/` contains
`repaired-live-output1.log`, `repaired-live-output2.log` and
`repaired-live-launches.log`. The active configuration approves capture only
on DP-1 and HDMI-A-1 of `pci-0000:03:00.0`.

Known CPU, GLX and EGL pixels now match on physical output 1. Each admitted
capture matched 82,800 exact RGBA pixels in the visible three-stripe pattern;
GLX and EGL reported the Radeon RX 7900 GRE renderer. The baseline contained
no matching stripe rows. The top 24 rows were occluded and excluded. The
experiment and its hashes are preserved as `physical-pixel-experiment.json`,
`physical-pixel-experiment.py` and `physical-pixel-hashes.log`; launch records
are in `physical-pixel-launches.log`. No image files or binaries were archived.

The live grant probe also passes. On output 1, replacing the capture made
both retained fids return `ESTALE` before lease expiry, while a new fid read a
newer presentation (23659 → 23661). On output 2, expiry made both retained
fids stale; a subsequent grant was readable (19756 → 19853). See
`physical-grant-replacement-final.log` and `physical-grant-expiry.log`. These are
grant replacement and expiry checks, not output replacement or lock checks.

t319 remains open: mirror convergence, lock cancellation, stale handles after
output loss/replacement, independent replacement output identity, availability
and recovery observations, and bounded capture-on/off costs remain. Next,
extend the admitted probe for attended lock and output changes. The full gate
and packaged checks passed before installation, as recorded below; the new
probe gate passed in `physical-probes-check-final.log`, including workspace
tests, strict Clippy, layout and graphics checks. The first run stopped at the
layout rule because the new module printed directly; the final version returns
its report to the existing dispatcher. `physical-probes-check.log` preserves
that failed run.
The rebuilt final dispatcher also passed live (38278 → 38280), recorded in
`physical-grant-dispatcher-final.log` with `physical-grant-final-hashes.log`.

The grant probe reads the current request serial before allocating its next
one. Its first two launches (13 and 14) incorrectly assumed serial 1 and were
refused; they are probe failures, not capture failures. The final replacement
check bounds the entire interval through both stale replies to 1.5 seconds,
measured from before the first request. The normal lease lasts two seconds.

## Repair checkpoint before installation, 2026-10-14

The matched release `niltempus-1822c8ad7c891b0a9bd3` is installed and running
in the daily session (Sophia `7c95a419b`, integration `46f1e5c`). The active
core configuration approves capture only on DP-1 and HDMI-A-1 of
`pci-0000:03:00.0`. Installation and login are complete.

Live capture is failing. Authenticated requests for output 1 and output 2
expire after about ten seconds with `capture refused: Cancelled`. No physical
image has been delivered. The diagnostic CLI confirms the request reaches
`Capturing` before expiry (launch 8); Session intake works. The next step is
to verify the queued-frame repair below on the physical session. This diagnostic
ran against the installed session without restarting it.

Source inspection found the missed handoff: composition admission stores the
frame in `DeferredNativeCompositions`, but the capture request searched the
exporter's pending slot before activation had populated it. The request could
never mark its newly admitted frame. The repair reserves readback storage on
the queued generation and carries each head's reservation through installation
to its exporter. Session also counts a deferred generation as retained when
deciding whether to retry. Three queue regressions pass: deferred ownership,
supersession without inherited capture, and all-head refusal. The full repository
gate passed, including workspace tests, strict checks and the host's buffer-age
and GLX/EGL pixel checks. Its log is `queued-capture-check.log` in the capture
evidence directory. No repaired build is installed yet.

The repair is signed and pushed as `629cb8a80`. The matched release is
`niltempus-1ce83e7579cba2b564c9`, integration `243605d`, at
`/nix/store/57l1rciwpd1j9badxf8hb8ijf7ms3a6a-niltempus-desktop-niltempus-1ce83e7579cba2b564c9`.
All 87 packaged checksums pass, and the packaged CLI passes the authenticated
33,177,600-byte supplied-pixel transfer in 1.00 second. Hagia, narthex, Lom,
Bemenu, kleis and the preflight executable are unchanged byte for byte.
The logs are `queued-capture-desktop.log`, `queued-capture-checksums.log` and
`queued-capture-packaged-cli.log` in the capture evidence directory.

`~/t319-install` now selects this exact repaired release. Its shell syntax and
the capture configuration validate; the active configuration already matches.
`sudo -n true` still requires a password. Next, run the installer, log out and
back in, then retry ordinary capture before the remaining physical gates.

The session evidence is
`~/.local/state/sophia/sessions/00000001792000065910-2683c23a-6360-423a-bdbb-72ca69d1ed22`.
Its application stderr records capture failures for launches 2, 5 and 6.
Launches 3 and 4 instead failed because a shell wrapper could not see the
`/opt` executable path inside its namespace; they are not capture evidence.
The Nix-store executable is visible to the wrapper.

The full repository gate and authenticated supplied-pixel CLI tests passed
before installation; the packaged 4K transfer took 0.96 seconds. Those results
do not qualify the physical renderer-to-recipient path. t319 remains open:
resolve this timeout, then prove physical CPU/GPU pixels, mirror convergence,
lock cancellation, stale handles after revocation and output replacement,
independent replacement identity, and bounded capture-on/off costs. Do not
advance to attended lock or KVM checks until ordinary capture succeeds.

## Integration checkpoint before installation, 2026-10-14

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

The status view labels the native retirement count as an owner counter, not a
per-output presentation generation. Capture now has the integration described
below. Its release, capture-on/off measurements and attended qualification
remain outstanding; none of this draft is installed in the current session.

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
releases its charge once the last frame reference is gone. The Session join
described below is a draft under qualification. Tests use supplied completion callbacks;
they do not establish physical KMS capture acceptance.

The integration authorizes the target separately from its observer
request capability, using one Session budget and the existing lock fence.
`run_native_pending_output` stages and submits in one call, so admission after
that call is too late to mark the exact queued composition for readback. Reserve
and mark it before submission, then publish only matching displayed custody.
All enabled mirror heads need accounting even though the recipient receives
only the primary image. Withdraw the portal source when native owner or head
targets change; a renderer frame number alone is not an output incarnation.

The integration checkpoint is ready for a matched release. Observer recipes
now parse an explicit bounded `capture-output` connector allowlist, defaulting
to no approval. The optional portal `capture` and `capture-status` files use
fixed-size versioned records and one outstanding request per recipient. Session
now supplies those queues for explicitly approved recipes and services them in
the owner loop. It reserves portal storage, resolves the primary connector,
tracks the full native owner/head-target incarnation, queues a fresh retained
composition, waits for its displayed identity and uses the common coordinator
to bind the image. Planned topology preparation and native retirement revoke
capture bindings first. This candidate has not been installed.

The queue uses the common lock fence, a ten-second deadline and owner-specific
work tokens. Coordinator enqueue now preserves that original fence revision:
delayed rendering cannot obtain a fresh interval after lock/unlock. Regression
tests pass for that race, replay, overlapping requests, cross-owner completion
and the optional file semantics. Native snapshot storage now has a process-wide
128 MiB/eight-image cap, including clones held by workers and scanout leases;
its reservation and lifetime test passes. Portal capacity can be reserved by
dimensions before a native frame identity exists.

Native capacity is now reserved for every head before marking capture. The
worker carries a shared, single-consumption reservation through deferral and
passes it into readback; the renderer checks the reserved length before
allocation. Pressure therefore leaves the queued composition ordinary instead
of failing its export. Superseding a requested frame releases its reservation;
Session retries a fresh frame within the original request deadline. Expired
recipient bindings release portal storage on the next service turn.

`sophia capture OUTPUT_ID [--output NEW_FILE.pam]` now uses the admitted namespace
and launch authentication. The Linux adapter duplicates the inherited socket;
the CLI retains its attested process identity without a shell handoff. The
standalone test exposed a high-descriptor failure in the earlier `/bin/sh`
bootstrap, which has been removed. The CLI reads bounded frame bytes,
rechecks its retained record, reports both generations and a pixel checksum,
and optionally creates a PAM file in an approved writable path. The real
namespace/authentication/portal test ran the CLI against supplied 2×2 pixels
and verified the exact output file. A separate test now launches the built CLI
through the production supervisor with its inherited channel and also passes.
Both use supplied pixels; neither proves physical capture. Descriptor tests
verify independent ownership, close-on-exec and invalid-number rejection.

The isolated GPU worker test passed after the reservation change, with CPU and
GPU pixels and explicit worker shutdown. Its result, stdout and stderr are in
`~/.local/state/sophia/development-evidence/t319-session-capture/`; the test
binary SHA-256 was
`3689fe2d372c182effc18812ddcd39cfaeee7b3686301c7950cead66827dc1d8`.
The capture-filtered backend suite passed (15 tests, one ignored GPU test run
separately). The final `cargo-slot sophia cargo xtask check` passed, including
workspace tests, doctests, strict Clippy, layout and conformance checks, and the
gate's buffer-age and GLX/EGL pixel checks. Its complete log is
`integration-check.log` in the Session capture evidence directory.

Four supplied-backend Session tests pass: presentation waiting and output
replacement, unauthorized targets and memory pressure, lock cancellation, and
retry after a newer frame supersedes the request. Hotplug observation now
revokes capture before the rescan delay. Seat loss and topology quarantine
suspend capture service, and voluntary seat handoff revokes before suspension.
The output-loss audit found a race between native preview failure and the next
Session service turn. A native lifetime guard now surrounds the source checks
and protected delivery. Preview failure, topology preparation and owner drop
withdraw it synchronously. Both lifetime tests pass: withdrawal stales retained
readers without a Session poll, and waits for an existing guarded delivery.
Collection releases the native guard before consulting the execution fence to
preserve lock order; installation and delivery recheck native validity.
The four Session tests also pass with this guard. The first full gate reached
doctests but had compiled its dependencies before the guard edit; a complete
run against settled source subsequently passed as recorded above.

A new standalone 4K transfer check exposed grant expiry during sequential
reads. The CLI now pipelines four bounded reads, and namespace dispatch waits
on socket readiness instead of sleeping between requests. The grant deadline
is unchanged. The 4K retest passes: all 33,177,600 bytes match the supplied
pixels, with the complete test finishing in 1.06 seconds. This establishes
bounded transfer capacity on this host, not physical capture or a quiet-window
performance comparison. Its log is `cli-4k-readiness.log` in the Session capture
evidence directory.

The prepared next-session core config is recorded as `next-session-core.kdl`
beside the test evidence. It validates and approves only the configured DP-1
and HDMI-A-1 connectors on `pci-0000:03:00.0`; the active core config is unchanged.
The source-layout audit passes after placing test-only markers in their external
test files, following the existing repository convention.

The matched candidate is built: `niltempus-1822c8ad7c891b0a9bd3`, Sophia
`7c95a419b`, integration `46f1e5c`. All 87 packaged checksums pass; Hagia,
narthex, Lom, Bemenu, kleis and the preflight executable match the current
release byte for byte. The packaged CLI passes the authenticated 4K transfer
test in 0.96 seconds. Its fixture needed one correction: the protected launcher
already mounts the Nix store, so an explicit executable bind inside it was
redundant and correctly refused. No production code changed for that correction.

Store path:
`/nix/store/66a6irs4ik9h368a4fxnhfbl27084rv1-niltempus-desktop-niltempus-1822c8ad7c891b0a9bd3`.
The short `~/t319-install` command installs that exact release and the validated
core config, after checking its expected hash and backing up the original.
Installation requires an interactive sudo password; `sudo -n true` refused.
The current session and active configuration remain unchanged.

Next, install the candidate and log into it, then collect capture-on/off
performance and attended physical/mirror/lock/revoke evidence. The deterministic
checks pass; this does not establish live capture acceptance. The task remains open.

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

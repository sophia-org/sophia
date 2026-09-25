---
id: 4l5bntke
date: 2026-09-25
kind: investigation
status: investigating
tags: [investigation]
---
# Critical desktop exits require separate device, tab and workload evidence

## Question

Which observations can support the six critical tasks below t100 without
confusing a component test, a reference-shell run, and acceptance of the personal
desktop? niltempus requested this scope on September 25. The review baseline is
signed Sophia `caca7e3770bee2ee87d2b4fda070708ac6df036e`.

Task state remains in [todo.md](../../../todo.md). This note reconciles evidence
for t069, t097, t081, t018, t020 and t021; it does not create new task identities
or relax their exits. No install, reload, physical input, device probe or GPU
execution was performed for this review.

## Device evidence has two different owners

The [universal negotiation plan](../plans/6tewvlbh-universal-device-negotiation-across-sophia-clients.md)
requires normal unmodified clients without a device-selection recipe. A
connection-pinned DRI3 device and successful server transfers do not establish
that an independent client video allocator used it. Earlier override-assisted
browser success remains a control, not this exit. A render-node basename also
cannot substitute for retained device and PCI identity across boots.

The [t097 GPU admission contract](../plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md#t097)
instead requires explicit, exact shell GPU permission. It can reuse a successful
contained GPU render even when a later desktop run failed, provided the original
binary, grant, device and evidence match. Its current proof runner labels its
first completion `presented_synthetic` and reports `native_presentation=false`.
Those labels are substantive: real Vulkan rendering and resource-credit
collection do not prove native frame retirement. Likewise, the t100 backend
fixture exercises real owners with simulated copy/flip completion. Neither is
evidence of a physical KMS completion or a hard VRAM quota.

The agents' current device investigations retain the normal-client allocator
boundary and prepare a bounded exclusion proof separately. Device execution
requires a concrete pinned command and separate authorization; a device-hidden
build or test does not supply that authorization.

## Tabs: reference shell and component desktop

At the review baseline, `config/arguments.rs` selects `shell_process=None` when
independent shell components are configured. It refuses an explicit legacy shell
alongside them. `run.rs` constructs `LiveMetadataShell` only from that optional
process. In `owner_loop/physical_input_loop.rs`, `service_tabs` runs under this
legacy owner; the separate `service_components` path has no tab descriptor join.
The component transport's ability to negotiate a capability does not implement
that service.

Lom `ad349869a8ce3f9eff77c6b91465b80cb54198b6` consumes indicators and content in
`src/service.rs`; it does not implement the tab descriptor workflow. Its t020
still owns combined descriptor launcher/switcher support. The personal desktop
profile inspected for this review selects a Lom bar and a Bemenu launcher.
Neither this profile nor the presence of a packaged Narthex binary establishes
an active tab descriptor peer.

The [t018 contract](../plans/queue-06-4-exercise-real-development-workflows.md#t018)
already specifies a separate Hagia/Narthex reference run. Its implementation and
offline evidence need not be rewritten as a missing Lom feature. That run can
establish physical tab behavior, while t081 remains dependent on the combined
workflow/package exit in t101. Running Narthex concurrently with the personal
components is not the proposed solution.

### Prepared operator sequence, not an executed run

Use one frozen Sophia/Hagia/Narthex pair in a separately authorized test session,
with native scanout and an explicit legacy-shell profile. Retain source and
binary hashes, the exported profile, output topology/scales, control catalog and
the session record. Keep the personal component profile and rollback separate.
Select actions from that session's catalog; an IPC commitment is not proof of
visible pixels or correctly aligned input.

| Layout or transition | Observation required |
| --- | --- |
| `layout-frame-tree` | Create empty and occupied sibling frames; move/focus/resize across them; empty bars have no activation. |
| `layout-notion` | Group multiple windows, activate a hidden member by its visible tab, and confirm focus and client pixels move together. |
| `layout-i3` | Nest split, tabbed and stacked containers using `split-tree-layout-tabbed` and `split-tree-layout-stacking`; verify parent/child navigation. |
| Profile spelling `split-tree` | Verify the documented alias of `i3`; it is not a separate IPC action or implementation. |
| Both outputs | Repeat with different output scales where available; confirm bar geometry and pointer targets use the same presented allocation. |
| Title change and shell recovery | Labels update; old descriptor actions cease immediately; neutral bars during loss acquire no authority; fresh actions follow actual replacement presentation. |
| Fullscreen and floating occlusion | Fullscreen suppresses bars; an overlapping floating client prevents activation of the covered target; restoration requires current presented state. |
| Teardown | Ordinary logout returns to the display manager, input remains recoverable, and submitted native work and source leases drain. |

The existing physical gate launcher is not yet an approved entrypoint for this
matrix. A read-only inspection found a reference to undefined
`recorded_hagia_shell_sha256` where it records `recorded_narthex_sha256`, and
directory-only `.git` checks reject worktrees. No unused launcher was repaired
or run during this review. The final launch command must be prepared and checked
before asking for the physical session.

## A retained session supports a narrower t020 conclusion

Stopped session `00000001790357699798-2dcc9871-919f-4d23-b98d-81a997a4d95b`
records Sophia `9ee301e74ef22a065dcb112dc1873c7e458dfec4`, executable SHA-256
`821feac5cd0c82d235e17c287be8986363f08341a635c4aa40eb8322de2b2d13`, and profile
root hash `acf9c23840b44bcae3e8addf2baf25d2aebc86ed781a206aa13979804f0dfb78`.
Its loaded effective profile digest is
`f140b725dbc4d315acdaee626577f942ddaedec10d187bab2819f3e67632e5da`.
The manifest explicitly says component private configuration was not observed.
WM epoch 1 and 2 have different executable digests; this was not one immutable
WM workload throughout.

The lifecycle records a return to the display manager with `exit_status=0` and
`emergency=false`. The terminal native-resource record has zero snapshot entries,
snapshot bytes, import-cache entries and leased frame slots. These are positive
teardown observations, rather than an inference from the absence of errors.
Both logical outputs have terminal native-head records, without pixel checksums.

The sampler reports 840 samples at five-second intervals. The retained files
contain 673 consecutive samples, sequence 168 through 840, from uptime 840,092
through 4,200,475 ms. The first part of the recording rotated away. Recorder
health is `storage_errors=0`, `discarded=220`, `suppressed=1012988` and
`rotated_bytes=15728538`; this prevents whole-session absence claims.

| Retained interval | RSS, first → last (KiB) | Other observations |
| --- | --- | --- |
| 840–1,795 s | 229,208 → 229,612 | CPU registry bytes stay zero; 4–6 frame slots, 4–5 snapshot entries, 7–9 imports. |
| 1,800–2,995 s | 229,612 → 226,160 | CPU registry bytes stay zero; imports remain 8. |
| 3,000–3,595 s | 226,160 → 247,012 | WM replacement occurs in this interval; imports range 2–12. |
| 3,600–4,200 s | 247,012 → 247,188 | CPU registry bytes stay zero; 2–3 frame slots, 4–5 snapshots, 9–12 imports. |

This is useful bounded-counter and teardown evidence. It does not establish
zero steady-state allocation growth: the workload has no fixed-state markers,
RSS changes, and RSS does not identify retained allocations. The terminal
cursor maximum is 65 ms motion-to-submit against a recorded 16,666 µs scheduler
interval. There is no retained percentile distribution or attributable trigger
for that maximum. It cannot be presented as passing refresh-relative latency.

The sampler, resource-summary owner, completion owner and physical-input-loop
files have identical Git blobs at `9ee301e7` and `caca7e37`; their reported
semantics remain comparable. The newer component reconnect grant allocation
changed, so this earlier session does not accept that new recovery path.
The [retained cursor investigation](0lamaqyi-a-blocking-cursor-only-commit-spends-the-vblank-the-next-frame-needed.md)
supplies separate, measured before/after latency evidence on its stated builds
and workloads, not a fresh percentile result for this session.

Evidence is copied and checksum-verified at
`~/.local/state/sophia/development-evidence/critical-sub100-caca7e37`.
Its 15-file manifest digest is
`a72a65133f76f166337701899ccdc6aac5bec25574231f98f7af78e803885d95`.
It contains sanitized event segments, identities, health, terminal records,
resource samples, the disposable analysis, and exact owner-blob comparisons.
Application stderr was not read or copied. The installed `session list` returned
these records before encountering an unrelated directory-permission refusal;
this audit selected and validated the explicit stopped record directly.

## Evidence review is not milestone completion

The [t020/t021 plan](../plans/queue-07-5-close-milestone-14.md) requires an evidence
review before updating product claims. The accepted operator login/logout and
emergency recovery in [t013's investigation](6q35cl9y-normal-login-arms-recovery-without-a-keyboard-rehearsal.md)
remain accepted; a stale earlier paragraph does not require another rehearsal.
The retained session above adds a specific successful teardown and sampled
resource observation.

Missing tab observations, combined Lom descriptor support, exact workload
resource/latency acceptance and the normal-client device boundary remain
separate limitations. A useful next measured workload must record a stable
topology and workload phases, distinguish maximums from distributions, and show
the applicable content/native inventories drain after replacement and logout.
No arbitrary number of clean days replaces those observations, and no milestone
completion is recorded by this audit.

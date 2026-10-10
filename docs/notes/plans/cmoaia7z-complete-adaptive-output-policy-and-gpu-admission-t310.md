---
id: cmoaia7z
date: 2026-10-09
kind: plan
tags: [plan, milestone]
---
# Complete adaptive output policy and GPU admission (t310)

## Scope and exit

niltempus approved implementation on 2026-10-09, choosing safe fallback for
unsupported output preferences. This completes t310 beyond its accepted startup
port fallback. The exit is an installed desktop that survives ordinary output
loss, return and port movement while keeping workspaces reachable, restoring
their configured affinity when the preferred output returns, and never opening
a GPU excluded from the session. Deterministic checks and attended physical
acceptance remain separate requirements.

Session owns desired output policy and GPU admission. Engine owns hardware
activation, presentation and coherent topology publication. The WM receives
opaque output identities, geometry and policy keys. It owns workspace placement;
no client-specific behavior or new WM protocol is introduced here.

## Task details

### t310 policy

Use one pure resolver at startup, profile reload, hotplug, seat reacquisition and
native recovery. Keep the saved candidate separate from the committed realized
state. Strict profiles preserve settings and startup/reload refusal behavior;
runtime absence waits for the required display without choosing a fallback.
Adaptive profiles use
advertised safe settings when mode, refresh, scale, transform or VRR preferences
are unsupported. Prefer the advertised preferred timing; otherwise choose the
advertised timing nearest 60 Hz, then largest pixel area, with deterministic
ties. Never invent a timing or overwrite the saved preference.

Explicit output exclusions always win. Unnamed outputs remain off under
`inherit-sophia #false` except for the last-output fallback. Retain an eligible
current fallback; otherwise choose deterministically, skipping unusable heads.
An incomplete mirror group is unavailable as a whole. Keep valid positions;
if adaptive changes cause overlap, pack logical groups into a deterministic row.
Startup focus must not steal focus during runtime changes.

No eligible output produces a typed waiting state. Runtime retains application,
WM and workspace state, releases held input safely, and keeps session control
responsive. Startup waits before readiness and application launch. No invented
display is published. Returning hardware restores desired preferences through
the same resolver.

Add stable GPU exclusion to Session configuration, for example
`exclude-gpu "pci-0000:16:00.0"`. Match the exact udev `ID_PATH`; exclusions only
narrow the session's seat and tolerate absent hardware. Filter KMS, render
inventory, topology monitoring, image import, DRI3 export and shell GPU grants
before opening nodes, and revalidate identity and seat at open time. Carry GPU
and connector identity together; permit a GPU qualifier on named outputs and
refuse ambiguous bare connector names. GPU admission and output disablement
remain separate controls.

Configured policy keys, mirror membership, availability and GPU exclusions are
session identity. Decline identity-changing reloads before staging a replacement
WM. Hardware movement may transfer a realized key atomically, with one enabled
logical owner at a time. Reconcile capabilities before activation; never briefly
light every connected head. Publish geometry and realized keys at one topology
generation after the existing test/apply/rollback and presentation barriers.
Stale completions cannot publish. A failed adaptive startup activation gets one
bounded conservative attempt. Runtime refusals use the finite rescan backoff
series, retaining conservative settings through its remaining attempts.

For a replacement with no viable owner, that attempt keeps one admitted logical
group (the focused group, otherwise canonical connector order), with complete
mirrors, unit scale, normal transform and VRR disabled. It chooses an advertised
timing nearest 60 Hz, then largest pixel area, without changing desired policy.
Startup's allowance covers TEST refusal, construction and failed first activation.
Runtime hardware refusals use 250/1,000/4,000 ms retries before waiting for an
external trigger; success is a completed resume, not merely construction.
Unavailable outputs instead retain a five-second admitted probe after the short
series, while the seat is active. Waiting uses its own cadence count and does
not consume the finite refusal allowance. A failed probe or seat open is unknown
availability; it follows the slow cadence without spending or resetting that
allowance by pretending a monitor returned. A changed bounded failure identity
is reported without resetting cadence, so alternating probe errors cannot
create a permanent 250 ms polling loop.
The five-second period favors recovery latency and may keep a GPU with a
five-second autosuspend delay awake. Waiting is logged during the short series
and on entry to slow probing, then only after a notice or changed outcome.
A new topology, seat or profile notice can
resolve desired preferences again. In-place reloads retain their existing
known-working rollback target instead of tearing down a viable desktop.

The first attended same-port cable test on `3ef3d5b77` failed on 2026-10-09:
reconnection produced a burst that retired the just-resumed owner, followed by
two immediate refusals and a black desktop. A subsequent VT return panicked
while closing an adopted replacement whose evidence owner was never opened.
The successor coalesces notifications for a bounded 250 ms before rebuilding,
uses the delayed runtime series, preserves bounded refusal codes, and pairs
with the evidence-owner ordering fix. Exact hardware refusal remains unknown;
another attended check is required after the combined gate and release.

The second physical check, release 219 at `28b76f7ef`, also failed. The GPU
runtime-suspended after the last unavailable probe and did not report the
reconnected DP-2 until the attended VT switch woke it. The replacement then
resumed, but later hotplug notices retired it and four preflights refused with
`stage=validation validation=rejected`. No evidence-owner panic recurred. The
preflight submitted connector/CRTC state without a primary plane, so its answer
depended on an existing framebuffer. The successor needs the slow availability
probe and complete-plane validation with its own resources and retained errno.
The source explanation and captured observations remain distinct; the rejected
kernel errno was not retained by release 219. Physical acceptance is still open.

### Persistent services and replaceable display attachments

This is the monitor slice of the [one-core public-role proposal](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md)
and [Plan 9 integration concept](../concepts/ernn0bkv-plan-9-integration-points-for-sophia.md).

niltempus approved this Plan 9-inspired continuity scope on 2026-10-09. Session,
application admission and window identities, desired layout and workspace
affinity, committed scene state, and retained images outlive a physical output.
The native owner and its presentation bindings do not: replacements have fresh
generations. Old input routes and output-bound grants are invalidated; they
cannot acquire a successor's authority by retaining a handle. An unplug does
not create a dummy monitor or complete a Present that did not occur. Existing
bounded service and custody failures retain their defined handling.

Public role connection epochs and physical output generations are distinct.
Existing pinned inspection snapshots remain immutable historical bytes; keeping
one open does not certify a currently attached or presented head. Output protocol
revision 1 requires at least one head, so runtime Waiting must not be encoded as
an invalid empty topology or as a fabricated lit display. The last presented
snapshot and current availability need separate descriptions.

The later t257 status surface should expose availability, recovery stage,
current owner generation, last presented generation, next retry and bounded
failure identity, updated on transitions and probes rather than per frame.
t318 composes role namespaces without silently rebinding retained fids; t319
binds capture grants to an output/presentation generation and defines stale or
revoked grants, including lock revocation. These interface extensions are
outside the next physical recovery gate. Existing metadata-shell presentation
grants still revoke and reconnect by their current contract; this repair does
not claim to preserve every output-bound role connection or add a 9P frontend.

The immediate implementation is admitted rediscovery, complete-plane validation
and bounded refusal reporting, with the existing publication and input barriers.
Fingerprint-based suppression of unchanged topology notices remains a follow-up.

### Integration and proof

The third physical candidate is prepared as release 222,
`niltempus-0f783c805f8040c77adc`, pinning signed Sophia `ada93fd4b` and signed
niltempus `edb449458`. Gate 221-02 passed 7,498 tests with zero failures and
101 ignored; CLOSURE is `a7c67ca0bb8e3b89104276bbc611ae3bf6f36e3d65d96fca68365e92e9ae21ff`.
Gate 221 remains stopped at SDK contract-copy drift. Its correction restored
the SDK-owned spec; the lifetime extension is recorded here and in the linked
9P notes. Runtime code is identical to `415bc88f0`.

Release checksums, source/profile preflight, unchanged non-Sophia pins and
graphics closure were verified. Install and explicit recovery commands are in
`~/.local/state/sophia/development-evidence/t306-01/222-bare-metal-release/READY.txt`.
The default rollback after installing 222 returns to physically failed 219;
READY also names the still-installed pre-t306 release. niltempus installed it
and relogged before source promotion.

The first two attended checks on release 222 returned the display in the same
Session: same-port unplug/replug, then a live move from DP-2 to DP-1. Session
`00000001791584192075-efb96437-173b-4184-9372-57e199d4edcf` and PID 7778 remained
live on the release executable. The first replacement presented with fallback;
the second committed with zero adjustments and presented at 120 Hz. The
preserved logs show no seat transition or runtime fatal during either test.
These are display-return and session-survival observations, not completed
KVM input, pointer, lock/unlock or workspace-affinity acceptance. Those checks
remain, as does the same-topology realization-publication gap described in the
[investigation](../investigations/ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md#release-222-attended-cable-return-and-port-move-2026-10-09).
Evidence: `t310-runtime-20261009/physical-return-03`, manifest
`32683bb2d2857ac0b89710d12ebce238c73eca1007f02b4d5245b042fabcf852`.
No t306/t310 closure or master promotion follows from these two checks alone.

niltempus then confirmed normal pointer and keyboard shortcuts and reported
survival of the requested locked cable-return test. Owner 4 resumed in the same
Session with lock epoch 1 recorded before loss and after return; application
presentation continued. The retained log omits lock status and coverage fields,
so it is not an independent proof of coverage for the replacement topology.
The snapshot and limits are recorded in the linked investigation. A subsequent
unlocked KVM away/back also survived: the log records removal of eight input
devices, their replacements, a new display owner and post-return routed keys.
The subsequent locked KVM/USB return also survived through unlock in the same
Session. This completes t306's attended recovery sequence on this desktop;
its exact evidence is in the [KVM investigation](../investigations/kdleagg3-kvm-output-and-usb-loss-returns-the-desktop-to-greetd.md#release-222-physical-acceptance-2026-10-09).
t310 remains open for qualification of the same-topology ledger/capability
publication repair and its broader workspace-affinity/policy exits; t297 retains
its wider lock checks. The repair compares the replacement authority payload
with the published snapshot at the current epoch. Equal payloads settle the
new owner's realization after its presentation barrier without republishing;
changed advertised modes, VRR or mappings advance the epoch and publish. The
CPU evidence is in the [investigation](../investigations/ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md#same-topology-publication-repair-2026-10-09).
Same-topology lock coverage also
needs an owner-aware evidence identity; its present dedup key omits the native
owner, so the earlier cover cannot prove the replacement owner's presentation.

The test must recover without a VT workaround. A separate read-only follow-up
found that requesting a VT switch while already Waiting with no native owner
overwrites a held renderer-image handoff with `None` in `lifecycle/seat.rs`.
Its content consequence and repair need their own regression under
[t322](../investigations/kdleagg3-kvm-output-and-usb-loss-returns-the-desktop-to-greetd.md#t322-zero-output-vt-handoff); it is not fixed
by 222. Evidence: `t306-01/followup-seat-vt-handoff-01.txt`, SHA-256
`10d81b19e05c17bc919f1daa17627fb25aa219d997ca86cd65952cd7f332110f`.

Implement configuration and admission independently from master. Integrate runtime
changes on the gated t306 continuity candidate. Claude owns retained images,
suspend/resume, lock coverage and t306/t307 evidence; coordinate before editing
`owner_loop/topology_phase.rs` or `output_topology_owner.rs`.

Device-free tests cover strict controls, safe settings, unavailable and unusable
heads, exclusions, sticky fallback, return restoration, mirrors, deterministic
placement, policy-key uniqueness, focus, GPU renumbering and foreign seats,
identity-changing reloads, no-output waiting and stale completion rejection.
Exercise session transitions and lock barriers on the combined candidate; run
the full isolated repository gate and layout checks. Keep bounded output-policy
diagnostics durable. Client-specific workspace checks belong in the desktop
integration repository.

On 2026-10-09 niltempus removed QEMU qualification from the t306 release gate.
After the renderer-content repair and its failing-without-fix CPU regressions,
run one full isolated gate on the combined recovery candidate, build its matched
desktop release, and perform the attended physical KVM loss/return check. Capture
the AMD driver, Mesa identity and session log. The separate t307 virgl/Mesa work
does not block this path; the stopped 211, 213 and 215 guest records retain their
original dispositions. The physical check supplies the remaining continuity
evidence for this desktop, without asserting that the QEMU environment qualifies.

Release the profile and Sophia pin together in niltempus, preserving the seat1
rule and HDMI exclusion as defense in depth. Record exact installed and rollback
identities. Attended acceptance covers between-boot port moves, runtime loss and
return, every head absent, locked transitions and workspace migration/restoration.
Only then close t310. EDID monitor following, docking presets and display
confirmation UI are outside this task.

### Remaining policy qualification (2026-10-09)

Release 222 supplies the accepted cable, port-move and unlocked/locked KVM
continuity evidence. The subsequent installed `96cb6d6d8` release also survived
zero-output Waiting, a requested VT away/back, and reconnection with working
keyboard and pointer. That sequence committed the replacement under authority
epoch 3 after startup epoch 2; it does not demonstrate the equal-snapshot
commit-only branch. The publication repair's deterministic controls establish
equal-snapshot settlement and changed-capability republication separately.

The remaining workspace-affinity check requires two admitted logical outputs.
The daily niltempus profile assigns all six workspaces to policy key 1, so a
single-display recovery cannot establish migration to a surviving display.
Prepare a temporary qualification profile with two explicit connectors on the
admitted AMD GPU, keys 1 and 2, and workspace sets 1–3 and 4–6. Preserve the GPU
exclusion, disabled HDMI-A-2, component pins and accepted rollback. Identify the
second monitor's actual connector before preparing the profile; no connector
is guessed and no excluded GPU is enabled. Adding a policy identity requires a
separately prepared profile/session, not an identity-changing live reload.

Put recognizable windows in both workspace sets. Removing the key-2 display
must leave its workspaces and windows reachable on key 1; returning it must
restore the configured affinity without duplicate ownership, lost windows,
broken input or startup-focus theft. Also bind a same-port return without a VT
transition to its actual replacement owner, publication epoch and committed
record. If a physical capability-change claim is made, preserve the changed
advertised snapshot; a reconnect alone does not establish that difference.

Existing Sophia controls cover safe settings, sticky fallback, preferred return,
exclusions, mirrors, coherent keys/geometry, focus and publication. The installed
Hagia source has workspace assignment and unplug/replug checkpoint tests; this
inventory is not a new Hagia test run. Client-specific acceptance belongs in
niltempus/Hagia. The old niltempus output-topology shell runner counts excluded
connectors and assumes fixed epochs, so it cannot verify this profile unchanged.
Do not relax admission or reuse its counts as proof.

The read-only boundary and source inventory are frozen as
`t310-policy-acceptance-01`, manifest
`6bef029a11cf0e22b93d4c87edcc5fd176975a4fe074e2a6b04116aa325fbed4`.
No new hardware run or installation is claimed. QEMU/private Mesa work and
repetition of accepted KVM survival are not prerequisites for this policy check;
t297's broader lock matrix remains separate.

#### Two-output profile prepared

niltempus connected the second monitor. Read-only sysfs and udev records show
DP-1 (2560×1440) and HDMI-A-1 (1920×1080) on admitted GPU
`pci-0000:03:00.0`; HDMI-A-1 remains disabled under the current one-output
profile. Signed niltempus candidate `7fec163d047f4556635c0e27f5fc9cb62e39f65d`
changes only the desktop profile: DP-1 keeps key 1 and workspaces 1–3, and
HDMI-A-1 gains key 2 and workspaces 4–6, at scale 1 to the right. Both selectors
name the admitted GPU. The other GPU exclusion and HDMI-A-2 disablement stay.

The temporary release `niltempus-642f4b984163c5317d49` built successfully offline
and passed Sophia/Hagia profile preflight. All ten packaged executable files
are byte-identical to the installed release; every component pin, including
Sophia `96cb6d6d8`, and the Mesa/libdrm closure remain unchanged. The accepted
runtime's gate remains the code evidence; this is a configuration-only build,
not hardware or workspace acceptance. The newer passive diagnostics are not
included. The branch is local and the daily profile is not promoted.

Frozen `t310-two-output-release-01` has manifest
`035b58ab815169401ba4675e36bc2cddea3cecfe8f94a631107633f3bce5b180`.
Its `READY.txt` names the store artifact, first-login checks, workspace loss/return
sequence and explicit rollback to accepted `niltempus-01fa4c74950b5a998db9`.
The release is prepared, not installed; the new output and workspace identities
require a fresh login. First verify both outputs, then remove only HDMI and
check that its workspace set remains reachable on DP before testing restoration.

## Connections

The temporary profile was subsequently installed and reached both outputs,
but the first workspace check ended the session before the requested unplug.
The recorded fatal is stale click-focus admission after an empty layout commit;
see the [diagnosis and CPU repair](../investigations/ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md#two-output-workspace-check-stopped-on-stale-click-focus-2026-10-09).
Preserve that failed run. Gate the narrow Session repair, prepare a matched
successor, then verify ordinary workspace switching before continuing the
two-output loss/return sequence. t310's physical exit remains open.

- [Proposed admitted-discovery boundary](../decisions/nrdadhet-resolve-admitted-output-preferences-before-constructing-native-owners.md)
- [t310 investigation and accepted startup fallback](../investigations/ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md#t310)
- [t306 recovery investigation](../investigations/kdleagg3-kvm-output-and-usb-loss-returns-the-desktop-to-greetd.md#t306)
- [t307 retained image investigation](../investigations/r2m9cx6v-static-retained-dma-buf-images-can-sample-black-before-hotplug.md#t307)
- [Continuity qualification plan](u9rtb0ml-qualify-mesa-lifetime-repair-and-kvm-hotplug-recovery.md)

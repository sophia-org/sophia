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
state. Strict profiles preserve their refusal behavior. Adaptive profiles use
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
Stale completions cannot publish. A failed adaptive activation gets one bounded
conservative attempt, then retains viable committed state or waits.

### Integration and proof

Implement configuration and admission independently from master. Integrate runtime
changes on the gated t306 continuity candidate. Claude owns retained images,
suspend/resume, lock coverage and t306/t307 evidence; coordinate before editing
`owner_loop/topology_phase.rs` or `output_topology_owner.rs`. Guest qualification
gets a quiet host window with fresh preflight and the reviewed exact launcher.

Device-free tests cover strict controls, safe settings, unavailable and unusable
heads, exclusions, sticky fallback, return restoration, mirrors, deterministic
placement, policy-key uniqueness, focus, GPU renumbering and foreign seats,
identity-changing reloads, no-output waiting and stale completion rejection.
Exercise session transitions and lock barriers on the combined candidate; run
the full isolated repository gate and layout checks. Keep bounded output-policy
diagnostics durable. t306/t307 supplies the retained DMA-BUF continuity proof;
client-specific workspace checks belong in the desktop integration repository.

Release the profile and Sophia pin together in niltempus, preserving the seat1
rule and HDMI exclusion as defense in depth. Record exact installed and rollback
identities. Attended acceptance covers between-boot port moves, runtime loss and
return, every head absent, locked transitions and workspace migration/restoration.
Only then close t310. EDID monitor following, docking presets and display
confirmation UI are outside this task.

## Connections

- [Proposed admitted-discovery boundary](../decisions/nrdadhet-resolve-admitted-output-preferences-before-constructing-native-owners.md)
- [t310 investigation and accepted startup fallback](../investigations/ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md#t310)
- [t306 recovery investigation](../investigations/kdleagg3-kvm-output-and-usb-loss-returns-the-desktop-to-greetd.md#t306)
- [t307 retained image investigation](../investigations/r2m9cx6v-static-retained-dma-buf-images-can-sample-black-before-hotplug.md#t307)
- [Continuity qualification plan](u9rtb0ml-qualify-mesa-lifetime-repair-and-kvm-hotplug-recovery.md)

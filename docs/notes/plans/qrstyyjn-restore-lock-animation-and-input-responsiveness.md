---
id: qrstyyjn
date: 2026-10-03
kind: plan
tags: [plan, lock, performance]
---
# Restore lock animation and input responsiveness

## Scope and exit

The operator approved implementation on 2026-10-03 after slow rain, large glyphs
and delayed typing/unlock in the installed kleis provider. Preserve the accepted
keyboard repair at Sophia 316d9695. Build and test isolated candidates before
changing the running session. This work is separate from Nix deployment.

## t302

Implement these five parts together:

1. Restore the existing Matrix GPU shaders through offscreen EGL/GLES on the
   Session-granted render node. Use native output resolution, two bounded frame
   slots per output and a worker so rendering/readback cannot block provider I/O.
   Honor no-gpu and report renderer/fallback once. Software Mesa is not a GPU
   success. The release candidate grants kleis GPU access explicitly.
2. Compute glyph geometry per output. Keep automatic 80-column semantics; the
   operator's candidate uses cell scale 2 (16 px), without reducing framebuffer
   resolution. Rebuild on allocation, scale or topology changes.
3. Add opt-in C SDK upload pipelining, at most eight 64 KiB Twrites with capacity
   reserved for control. Track replies and cursor ownership. End requires all
   bytes acknowledged; cancellation drains borrowed buffers before release.
   Replace the service's unconditional 1 ms sleep with socket/command readiness
   and expiry deadlines. Bound each batch and service outputs fairly.
4. Prioritize semantic UI transitions over animation. Cancel obsolete rain once,
   coalesce colors and let started feedback finish. Process input, authentication
   submissions and verdicts before bounded image work. Accepted unlock must not
   depend on provider progress. Preserve PAM failure delay and cover/epoch gates.
5. Drive animation from monotonic time and honor fall speed, glyph cycle speed,
   trail length and brightness decay in both renderers. Skip stale frames without
   slow motion or catch-up bursts. Correct the CPU-only/stale documentation.

The existing BGRA full-frame contract remains unchanged. This is not a new
solid-fill or DMA-BUF protocol. Secrets, password lengths and real keystroke
timing must never enter diagnostics. Auth latency tests use synthetic input and
a test authenticator. Actual Enter delay remains unproven until measured.

## Verification and handoff

Test bounded pipelining, reply identity/order, short writes, cancellation,
disconnect, borrowed storage, fairness, topology and animation options. Exercise
input and successful authentication with stalled uploads/rendering/provider.
Use mutants for the important ordering and ownership rules. Review CPU/GPU
previews with fixed seed/time and native dimensions.

On the operator's 2560x1440 + 1920x1080 outputs, target 25 fps per output,
feedback p95 below 100 ms and accepted-verdict-to-unlock p95 below 100 ms.
Record actual authentication separately. Measure provider, service and owner
CPU plus memory, frame gaps and idle behavior. Headless protocol tests, isolated
render-node tests and QEMU precede attended acceptance; no physical claim comes
from a virtual GPU. Release changed SDK/consumers and the keyboard hotfix in one
audited candidate with a single `niltempus install` handoff and new login.

## Connections

- [Secure session lock authority](8jcykhdc-secure-session-lock-authority-and-lock-provider-role.md)
- [Work tracking](../../work-tracking.md)

Prior transport evidence: development-evidence/t034-session-lock/kleis-4k-02:
48.8 MiB/s, 1.54 fps at 4K, one write in flight. The Sophia port selects CPU
rendering unconditionally and the service sleeps 1 ms per turn. These are
confirmed causes; GPU restoration alone does not fix upload or input scheduling.

## Component progress, 2026-10-04

The candidate preserves Sophia's keyboard repair. The C SDK candidate is
73110aa (0.9.0, not published), vendored into both consumers. It drains upload
writes before cancellation, distinguishes local short-write cancellation from
remote errors, and exposes a newly queued Cancel through poll interest in the
same service call. Scripted replies and a killed mutant cover that last rule.
The worker's GPU and resource-churn changes are integrated from pX's signed
780eac18 and 8a2dabec commits; per-output native dimensions and cell sizes stay
independent of the transport.

Integration found and fixed two further liveness bugs. A busy Begin previously
freed the only finished solid frame, leaving one output on the old picture;
retry now retains a current frame without ringing a busy loop. End and Cancel
were not retried after EAGAIN because they had no Presenter action; the provider
now retries every staged SDK submission with its own deadline. The real
provider scripted-peer fixture covers both. No real input is traced.

The first release-profile native-size probes use the real provider and
LockFileService, with immediate synthetic presentation outcomes, no Session
owner and no display commit. Both 2560x1440 and 1920x1080 outputs delivered
246 frames in approximately 10 seconds: CPU 24.58 fps, GPU 24.60 fps. The
GPU identified radeonsi on RX 7900 GRE. One synthetic feedback transition
reached both outputs in 17/19 ms (CPU) and 19/20 ms (GPU). These are initial
correctness/throughput samples, not latency percentiles, matched CPU savings
or live acceptance. Full composition, authentication and the owner loop remain
outside this measurement. Raw logs, binary hashes and load are in
`development-evidence/t302-lock-performance-01/native-probe-01*`.

A full service handoff test holds the owner queue full with a readable peer:
passes remain unchanged for 50 ms, then all 48 demands arrive in order after
the owner drains. The idle test likewise shows no periodic polling and proves
command and stop wakeups.

The full Sophia gate passed (workspace tests, layout, formatting and strict
Clippy); Kleis passed 132 tests, with two explicit real-device tests skipped in
the device-hidden run. All C SDK programs also passed ASan/UBSan. The source
review of freeze 02 found no new defects. Raw test summary counts include
subprocesses and are not used as test identity sets.

The final native-size probe repeated the ten-second workload and measured
20 synthetic color transitions per output. Both renderers delivered 246 frames
per output (24.6 fps). Feedback p95 was 11.7/13.5 ms on CPU and 17.5/19.3 ms on
the Radeon GPU. These times end at the real service's candidate receipt, with
synthetic presentation outcomes; they do not measure physical display latency
or PAM. Hashes and logs are in `t302-lock-performance-01/native-probe-02*`.

Fixed-time native-resolution previews also confirm automatic 32/24-pixel cells
and the proposed 16-pixel cells on both outputs. CPU and Radeon paths preserve
opaque full-size frames; their procedural glyph choices need not be identical.
The same GPU module passed a separate conformance client inside Session's
actual LockProvider protection domain and synthesized sysfs projection. The
denied case refused the missing grant; the direct case rendered and read back
on the sole granted Radeon node, with card and input devices absent. Evidence:
`previews/MANIFEST.json` and `GPU-DOMAIN.json` in that directory.

## QEMU startup and qualification

The owner-loop fixture uses pinned Session, factotum and PAM binaries, a generic
C SDK WM and providers that serve, flood or stop reading. It reports observed
log intervals for authentication and unlock separately. It has no host display
or input access and does not replace attended acceptance.

The fixture exposed two startup defects in proof sessions with a public WM:

- An empty WM projection could queue before the CPU scene had a composition
  report. Preserving that native projection then failed on the missing report.
  Startup now seeds the empty CPU scene in both modes, as normal sessions already
  did. Runtime initialization stays mode-dependent. The seed submits no native
  frame, changes no nonzero-pixel counters and consumes no exact-pixel proof.
- That retained projection could also submit an ordinary flip before an output's
  first modeset. The primary virtio display inherited an active CRTC; the second
  had none and rejected the flip. Retained admission now keeps the entire batch
  pending until all outputs are initialized. No queued prefix can hide native
  scanout from the CPU cycle that must initialize the remaining outputs. Topology
  adoption and resume already initialize every output.

The failures are preserved in `t302-qemu-unlock-01/series-03`, `series-03b` and
`series-04`. The first binary was the t302 candidate based on 316d9695, not an
unchanged build of that revision. Pinned candidate 03 (Sophia `87f3c034`) proves
both initial modesets and successful startup. The permanent regression tests
both output orders, repeated admission without a queued prefix or allocated
frame, and admission after initialization. The no-guard and prefix-filter
mutants fail; the restored source passes. Full gate 04 passes on freeze 05.

Fixture repairs keep injected resize out of the public-WM scenario, wait for a
fresh routed-click acknowledgment before Enter, and tolerate exactly one service
ESTALE only after Session completes and the required post-unlock report exists.
Pre-completion or other errors remain fatal. Earlier failed runs are retained.

Final series 10 stopped at its first failure: all three stalled-provider runs,
all three flooding-provider runs and two baseline runs passed. Their observed
verdict-to-unlock intervals were 2.0–10.3 ms, 10.6–10.8 ms and 1.6–10.3 ms,
respectively. The last baseline also unlocked and passed physical input, but its
primary head later exceeded the unchanged 500 ms page-flip watchdog. Its fence
was pending, the DRM reader had no errors or rejected callbacks, and the second
head kept retiring. An ordinary drain poll accepted the primary completion
25 ms after the fatal record, before any detach or disable. No intervening
submit or device teardown explains the signal. This is an unresolved native
retirement failure, not an accepted release or a reason to loosen the watchdog.

The next discriminator runs the same fixture on 316d9695 plus only these two
startup fixes versus candidate 03. Keep per-run identities and failures, and
record host activity. Physical acceptance and the audited one-command release
remain held until the qualification has a supported disposition.

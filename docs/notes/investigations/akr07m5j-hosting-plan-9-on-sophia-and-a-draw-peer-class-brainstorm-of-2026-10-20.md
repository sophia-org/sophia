---
id: akr07m5j
date: 2026-10-20
kind: investigation
status: parked
tags: [investigation, architecture, plan9, 9p, draw, namespaces, performance, t331]
---
# Hosting Plan 9 on Sophia and a draw peer class: brainstorm of 2026-10-20

Record the discussion with niltempus and Claude as something to return to,
not a replacement for the accepted architecture. Three separate questions
emerged: how custody death is observed, whether a software-drawing peer class
belongs beside sealed surfaces, and whether Sophia should host a Plan 9-like
operating environment.

## Decisions and current status

| Topic | Direction recorded on 2026-10-20 | What remains open |
|---|---|---|
| Kernel-reported custody liveness | Continue c28's investigation. Identity, death observation, permission and content integrity should have distinct mechanisms. | Exact freshness contract, all consumers and measured native benefit. |
| Draw peer class | Open decision for niltempus; consider a second class beside sealed surface peers. | Explicit reopening of the no-draw decision, resource/security model and a discriminating experiment. |
| Hosted Plan 9 kernel | Shelved as a product/ecosystem idea. | A concrete client need and a feasible host backend/process model. |

C28 has **not** landed as a permanent kernel-maintained live flag. Its c40
experiment is narrower: a fresh aggregate custody observation at each shell
file turn, bounded to 1 ms/64 checks, with fresh fallback and a final fresh
poll. C41 completed its joins but found no resolved CPU benefit; its frozen
screen also refused one control-run major page fault. The feature remains
disabled by default. See the
[work-scheduling investigation](tt0k1yqv-how-should-one-isb-presentation-drive-bounded-work-across-owners.md).
Continuing the liveness direction does not waive that evidence or accept a
new freshness contract implicitly.

## Why these ideas came up

The t331 profiles motivated asking whether the pixel path is the wrong
interface for some clients. Historical c19 attributed many Session probes to
launch custody and image-connector liveness (about 112 and 22 ppolls/frame),
while the producer paid for fresh mappings, page population and immutable
buffer finalization. The image descriptor uses a side channel because the
accepted 9P stream carries no descriptors. Those historical counts are not
current savings estimates, nor a complete partition of the files/IPC gap.

The proposed design sentence was: identity is proven at admission, death is
reported by the kernel, content is protected by the kernel, and permission is
compared in memory. Its appeal is separation of responsibility. “Nothing is
re-asked” is a design aspiration, not today's contract: admission, revocation,
retries, publication and execution boundaries still need explicit proofs.

## Fit with the accepted architecture

The [one-core ADR](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md)
and [Plan 9 integration concept](../concepts/ernn0bkv-plan-9-integration-points-for-sophia.md)
already specify recipe-built namespaces, portals as binds, role-local
semantic ownership and a common 9P core. The brainstorm re-derived those
choices. A composing root forwarding fids between owners remains deferred
until justified; kernel v9fs/FUSE mounts remain rejected as Sophia's namespace
mechanism.

Calling Sophia the device/namespace half of a Plan 9 kernel and Engine's
renderer/DRM its hardware is a useful analogy, not an equivalence of kernel
contracts. A hosted environment could occupy the existing out-of-process
protocol-authority position, presenting its clients as sub-identities.
The host would prove the environment's identity; per-guest-app attribution
would be that environment's claim. Implementing scheduling, process execution,
input and graphics would still be substantial work.

X11 remains a separate frontend and trust domain per X server. Independent
application grants would require an appropriately isolated X server rather
than treating arbitrary X clients as independently proven peers. Wayland is
not planned. Plain 9P2000 remains a later portability target: a native Plan 9
mount client cannot be assumed to speak Sophia's current 9P2000.L operations.

## Open proposal: a software draw peer class

A peer could own server-side image identifiers and send bounded drawing
commands over 9P, while Sophia owns the rasterized output. For representable
2D workloads this may avoid transferring a newly filled image descriptor
and sealing client memory every frame. GPU and self-rendering clients would
continue to use the surface/ISB path.

This reopens the ADR's rejection of a draw-style content file. It is not
approved by this note. The proposal is a second content class, not adoption
of Plan 9 layers as Sophia's window model. The latter would be a separate
Engine/WM decision; equivalence between draw layers, ISB surfaces and X
windows must not be assumed.

Claude's local 9front research identified `sys/src/9/port/devdraw.c`,
`sys/man/3/draw`, `sys/include/draw.h`, libdraw, libmemdraw and libmemlayer as
starting points. The draw device exposes a small file interface and a compact
command stream; libdraw batches commands. A kernel device in the namespace
can be exported through 9P; the device implementation is not itself proof of
a standalone network server. Pixel load commands still carry bytes whenever
client-generated image content changes. “Load once, then commands” applies
to reusable assets and suitable drawing operations, not arbitrary frames.

Reported compatible families include Plan 9/9front, plan9port's libdraw and
devdraw arrangement, drawterm and Inferno. Exact dialect, connection and
frontend adaptations need checking before claiming unmodified compatibility.
The handback estimated about 7,168 lines for the reference C rasterizer/layer
code; that is a research pointer, not an implementation-size estimate for
Sophia. No exact 9front commit was supplied, so pin the checkout when resumed.

Security moves to command ownership, bounds, memory/work quotas, cancellation
and parser/rasterizer correctness. Owning image IDs avoids trusting mutable
client pixels on this path, but does not make the server's new attack surface
free. A mutable shared pool is a different proposal: a lend/submit protocol
alone cannot revoke an existing hostile writable mapping. The existing seal
guarantee stays on the ISB path. This does not imply every conceivable
kernel-enforced buffer design must allocate a new memfd for every frame.

If niltempus reopens the decision, the smallest test is a CPU-rasterized draw
export for the same timing-fixture content, compared with an ISB peer. Measure
Session and peer CPU, traffic and relevant syscall counts; retain identical
presentation joins, custody checks, ACKs, deadlines and the surface-peer
path. Include a workload that requires changing pixel uploads, not only a
favorable retained-assets case. X11's move toward client rasterization and
GPU rendering cautions against making command drawing universal; Plan 9 and
Inferno motivate investigating the narrower software class.

## Shelved proposal: a hosted environment

A hosted kernel does not by itself remove the pixel path: if it is one
surface peer, it still submits surfaces. Its value would be tools, namespaces
and an ecosystem, rather than an automatic t331 optimization.

The research handback suggests these return points, to pin and verify before
choosing an implementation:

- **9vx:** historical Plan 9 kernel as a host process using vx32 and a host
  display backend. Its 32-bit execution model is a major limitation; this
  discussion did not establish a maintained amd64 native-process solution.
- **Inferno / purgatorio:** a hosted environment with Limbo/Dis, per-process
  namespaces, 9P and drawing. A Sophia backend is a plausible smaller product
  experiment than a new Rust Plan 9 kernel. It does not run Plan 9 native
  binaries merely by mounting their filesystem. No local checkout was
  examined in this discussion.
- **Drawterm:** hosts the device/terminal side for remote Plan 9 processes;
  it is not a local hosted Plan 9 kernel.
- **Virtual machine route:** inspect 9front's Xen port and `sys/src/cmd/vmx`,
  including its 9P support. A guest display/input path and mutable fenced
  scanout class would be separate contracts from sealed ISB. The handback's
  limited driver survey is not proof that no relevant driver exists anywhere.

A full Rust hosted kernel is a large product undertaking with no established
schedule here. Prefer testing whether a hosted environment is useful before
committing to that scope.

## Identity and portability

A key proves possession of a secret, not that its holder is the exact living
local process admitted by Sophia: secrets can survive fork and process death.
Per-message signatures or frame hashes do not replace process custody or
prevent mutation after checking. Keys can serve a machine/sandbox boundary,
which is a different design decision.

None of these sketches removes today's Linux dependencies: pidfds, local
peer credentials, memfd seals, mapping policy, DRM and Unix descriptor
transport remain relevant. Event-based process lifetime observation has
analogues on other hosts, but analogous APIs do not establish identical
identity, event-ordering or revocation semantics.

## Come back when

1. Custody work has measured native coverage and benefit, and the remaining
   files/IPC excess has been freshly attributed. C41 has not met that trigger.
2. A specific 2D client class wants native command drawing: Plan 9 tools,
   Inferno or bounded shell/status content.
3. Plain 9P2000 adapters and the required namespace/frontend contracts exist
   for the proposed hosted client.

This note parks the brainstorm. It changes no ADR, public protocol, trust
boundary, default feature or implementation priority by itself.

---
id: sy7lhvb8
date: 2026-11-08
kind: investigation
status: investigated
tags: [investigation, architecture, performance, sft, ipc]
---
# Which Redox ring and Orbital patterns can reduce Sophia frame overhead safely

## Question

Which ideas from Redox's ring-buffer work, Orbital, and related projects could
reduce Sophia native CPU/frame while preserving admission, custody, immutable
accepted pixels, and atomic tear-free presentation?

This is source research requested by niltempus, not an implementation decision.
No runtime source, build, benchmark, or external agent workspace was changed.
Orbital was inspected read-only over SSH on x13; related upstream sources were
read through Redox's website and GitLab. No remote agents were contacted.

## Evidence

The x13 checkout `~/src/orbital` was clean at
`56620bba7dfd2652bd6b4e97909faecfcde2e27d` before and after inspection.
The source observations below refer to that checkout, not a measured Orbital run.

Pinned related upstream revisions:

| Project | Revision |
| --- | --- |
| redox-rings | d787c28c2dba79b9325d2a5a3e8c96dce4ebb2eb |
| redox-scheme | 3514d5696fb8cbbe7d517d37d3074d7cd5cfdded |
| orbclient | e9573da7e4cce13898094aba9923f69547461c19 |
| event | 9cb80f2b59aa7589b963f0f329c047feb842a3f2 |
| redox-buffer-pool | 259719734304f6d2138b8b910fbdac663ebe6ff0 |
| base (RamFS, graphics-ipc, driver executor) | d63a289a480388f30d3d91d5d8518a6270452fbf |

### Ring transport: the strongest architectural reference

The [Redox ring-buffer project report](https://www.redox-os.org/news/nlnet-rb-summary/)
describes userspace submission/completion rings, shared buffers, futex wakeups,
and optional descriptor notifications. Unlike Linux io_uring, the kernel does
not itself consume these userspace-to-userspace command queues. Ring setup uses
the existing scheme interface and passes mappings/handles to the peers.

The [raw ring implementation](https://gitlab.redox-os.org/redox-os/redox-rings/-/blob/d787c28c2dba79b9325d2a5a3e8c96dce4ebb2eb/src/raw.rs)
uses single-producer/single-consumer indices, acquire/release ordering,
cache-separated indices, and ring-distance checks. Its
[synchronization layer](https://gitlab.redox-os.org/redox-os/redox-rings/-/tree/d787c28c2dba79b9325d2a5a3e8c96dce4ebb2eb/src/sync)
includes waiter notification and spinning before sleep. These are references,
not a safety audit or a recommendation to import the crate into Linux Sophia.
In particular, spinning can increase the very CPU/frame cost we want to reduce.

The report's roughly 14–15x I/O result is a different workload. Its other IPC
figures also include kernel changes such as combined reply/wait and context
switch improvements. None predicts a Sophia compositor speedup. The
[RamFS integration](https://gitlab.redox-os.org/redox-os/base/-/blob/d63a289a480388f30d3d91d5d8518a6270452fbf/ramfs/src/main.rs)
is useful as a concrete consumer of the ring interface; it does not establish
Sophia's pixel authority or presentation guarantees.

### Orbital: readiness and batching, with important limits

[core.rs](https://gitlab.redox-os.org/redox-os/orbital/-/blob/56620bba7dfd2652bd6b4e97909faecfcde2e27d/src/core.rs)
dispatches typed Scheme/Input/DisplayHandle readiness. The scheme path reuses a
request buffer, drains batches, dispatches with caller context, and invokes
`handle_after` after a batch. That is a useful example of handling already-ready
work together before sleeping or redrawing.

It is not an entirely scan-free reactor: `handle_after` walks window handles to
post pending notifications, then redraws. Therefore "Orbital only touches the
ready object" would overstate the source. Also, its Cargo dependencies do not
show that this inspected compositor already uses the new redox-rings transport.

[compositor/mod.rs](https://gitlab.redox-os.org/redox-os/orbital/-/blob/56620bba7dfd2652bd6b4e97909faecfcde2e27d/src/compositor/mod.rs)
queues damage and draws intersecting regions. This is a simple damage strategy,
not proof of optimal merging or opaque occlusion culling: overlapping damage
can cause repeated drawing, and synchronization uses an enclosing rectangle.

[window.rs](https://gitlab.redox-os.org/redox-os/orbital/-/blob/56620bba7dfd2652bd6b4e97909faecfcde2e27d/src/window.rs)
separates opaque blits from transparent blends. Its synchronization comments
explicitly discuss flicker, shadow copies, and future double buffering. Opaque
windows read the public image; transparent windows use a shadow. These are not
Sophia's immutable accepted-pixel semantics.

[display.rs](https://gitlab.redox-os.org/redox-os/orbital/-/blob/56620bba7dfd2652bd6b4e97909faecfcde2e27d/src/compositor/display.rs)
CPU-renders into a framebuffer and calls `dirty_framebuffer`; the inspected path
does not demonstrate atomic page-flip/fence guarantees. Orbital is consequently
not evidence for replacing Sophia's GL/native presentation implementation.

### Other projects worth consulting selectively

| Reference | Useful lesson | Boundary for Sophia |
| --- | --- | --- |
| redox-scheme | Batch request/response buffers; explicit reply-and-receive call | Redox's kernel call is not a Linux syscall available to Sophia; existing 9P buffering should be checked first. |
| event | Typed readiness subscriptions used by Orbital | Keep Sophia's existing Linux poll mechanism; borrowing the dispatch pattern does not require a new dependency. |
| orbclient | Opaque copy vs blend specialization; contiguous full-width blits use bulk copy | Sophia already has pixel and cache optimizations. Demonstrate missing work elimination before another kernel rewrite. |
| redox-buffer-pool | Guards and reclamation around outstanding references | Review teardown principles; do not replace bounded frame slots with a general allocator without evidence. |
| base graphics-ipc | DRM handle and CPU shadow-buffer separation | Useful implementation comparison, not a new custody design or proof of atomic presentation. |

Source anchors:

- [redox-scheme reply-and-receive](https://gitlab.redox-os.org/redox-os/redox-scheme/-/blob/3514d5696fb8cbbe7d517d37d3074d7cd5cfdded/src/lib.rs#L499)
  uses `call_rw` with read/write counts; request-buffer reuse is at line 289.
- [event wrappers](https://gitlab.redox-os.org/redox-os/event/-/blob/9cb80f2b59aa7589b963f0f329c047feb842a3f2/src/wrappers.rs).
- [orbclient image operations](https://gitlab.redox-os.org/redox-os/orbclient/-/blob/e9573da7e4cce13898094aba9923f69547461c19/src/image.rs#L212):
  blend and contiguous-copy paths, not an established SIMD win for Sophia.
- [buffer-pool guards and reclamation](https://gitlab.redox-os.org/redox-os/redox-buffer-pool/-/blob/259719734304f6d2138b8b910fbdac663ebe6ff0/src/lib.rs).
- [graphics-ipc CPU-backed buffer](https://gitlab.redox-os.org/redox-os/base/-/blob/d63a289a480388f30d3d91d5d8518a6270452fbf/drivers/graphics/graphics-ipc/src/lib.rs#L67):
  optional shadow storage and row-wise copies into a DRM dumb buffer.

## Finding and resolution

Ponytail recommendation: borrow the smallest useful pattern, not the stack.
First consult readiness/batching code against actual redundant work in Sophia.
If a ring experiment is warranted, change only the transport of existing SFT
commands and completions. Keep the frame transaction and pixel/presentation
contracts unchanged; do not combine an allocator, async executor, SIMD rewrite,
and protocol-semantic change into the same treatment.

```text
9P: authenticated setup, grants, metadata, configuration, recovery
                          |
                 bounded submission queue
                          |
       validate and privately snapshot untrusted command
                          |
         existing admission, private pixel capture,
             validation and atomic presentation
                          |
                 bounded completion queue
                          |
                    producer observes
```

This is a proposed experiment, not the current transport or an amendment to the
SFT decision. Pixel sharing already exists; the proposed ring targets control
transport, repeated dispatch, and wakeups. It does not authorize direct trusted
rendering from client-mutable pixels.

Security/progress invariants that must survive:

- Validate all shared indices, distances, lengths, offsets, identities and epochs.
  Copy an untrusted descriptor into private state before acting on it; a shared
  mapping is not authority and must not enable time-of-check/time-of-use changes.
- Enforce mapping ownership/permissions and retain fresh custody checks. Model
  hostile mutation as well as cooperative acquire/release publication.
- Distinguish queue consumption, acceptance, presentation, source release and
  resource release. A ring head is not automatically a covering semantic ACK.
- Reserve bounded admission and completion capacity before irreversible work.
  Preserve backpressure, fairness, cancellation, and fail-closed teardown.
- Prove arm/recheck/wake ordering, including notification arriving just before
  sleep, queue exhaustion, disconnect, and stale mappings after revocation.

## Validation and remaining work

Completed: source inspection, upstream revision recording, and comparison with
Sophia's current documented contract. No implementation, build, native launch,
CPU/frame comparison, model-checking run, or full security audit was performed.
There is no demonstrated speedup or claim that this reaches Niri's measured lane.

A separately authorized transport experiment should first model the queue/wake/
revocation rules with existing SFT messages and deterministic negative controls.
Then measure the same workload with only transport changed: combined and owner/
peer CPU per completed frame, calls and wakeups, latency/cadence, and idle CPU.
Keep tracing outside qualification timing. Do not reopen already-inconclusive
batching candidates merely because another project uses batching.

No implementation task is opened by this investigation. Coordinate any future
work with the agent already optimizing Sophia; this note is input to that work,
not permission to interrupt or duplicate it.

## Connections

- [SFT architecture decision](../decisions/dwm37bn6-submit-native-frames-as-bounded-transactions-over-the-existing-shell-transport.md)
  keeps the existing authenticated files transport; the ring sketch above is a
  future experiment and does not silently supersede that decision.
- [SFT design and implementation status](../../native-frame-transactions.md)
  owns frame lifecycle, custody, credits and acceptance criteria.
- [Continuous-frame CPU plan](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md)
  owns the measured optimization queue; this investigation supplies references.
- [Readiness must name an obligation](../concepts/readiness-readiness-must-name-an-obligation.md)
  supplies the local standard for safe source-specific scheduling.

## November 8 follow-up: namespaces, links and portability

Read-only clones on Crunch distinguish three mechanisms: filesystem symlinks
store names, hard links refer to the same filesystem node, and namespace
registration maps a scheme name to an already-open service descriptor. Current
Redox uses scheme-rooted paths such as `/scheme/audiohw`; historical `scheme:`
examples are not current API recipes. URL spelling is naming, not an IPC ABI.

The namespace manager in `base/bootstrap/src/initnsmgr.rs` uses a map of names
to descriptors, can fork a selected subset and can narrow namespace permissions.
Registration receives a descriptor through a registration handle. Opening a
scheme duplicates its registered descriptor; unlink removes the name, not all
previously opened handles. Audiod opens its hardware handle before entering a
namespace containing only memory/rand and retains the hardware handle afterward.
RedoxFS's `flink_internal` links an existing node; relibc implements filesystem
symlinks and inter-scheme resolution separately from namespace registration.

Plan 9's [bind](https://9p.io/magic/man2html/2/bind) changes a process namespace,
with replacement or ordered unions; it is not an on-disk symlink. The inspected
Redox manager is not an implementation of arbitrary directory binds/unions.
Both systems establish handles; neither requires a fresh full name traversal
for every I/O. Redox's scheme request/completion ABI is not 9P. Its shared-ring
extension is distinct from the ordinary scheme ABI's use of SQE/CQE names.

Source pins (shallow clones under `~/src`, clean at inspection; no builds).
Local directories are `redox`, `redox-book`, `redox-kernel`, `redox-base`,
`redox-relibc` and `redoxfs`, respectively; table names below are upstream
repository names, and implementation anchors are relative to those repositories:

| Repository | Revision |
| --- | --- |
| [redox](https://gitlab.redox-os.org/redox-os/redox/-/tree/284f2691855f085608b57b7c6ba4ac87ec5adbb1) | 284f2691855f085608b57b7c6ba4ac87ec5adbb1 |
| [book](https://gitlab.redox-os.org/redox-os/book/-/tree/4bcfb67fb739a6215c9944162ad9db78e85133ad) | 4bcfb67fb739a6215c9944162ad9db78e85133ad |
| [kernel](https://gitlab.redox-os.org/redox-os/kernel/-/tree/909f2e82cf66796fcb1a8ff046d3361cb4ab3342) | 909f2e82cf66796fcb1a8ff046d3361cb4ab3342 |
| [base](https://gitlab.redox-os.org/redox-os/base/-/tree/d63a289a480388f30d3d91d5d8518a6270452fbf) | d63a289a480388f30d3d91d5d8518a6270452fbf |
| [relibc](https://gitlab.redox-os.org/redox-os/relibc/-/tree/c3cc8b5fe1c6fbc40d6b5ea87e39daf2e19ddbc4) | c3cc8b5fe1c6fbc40d6b5ea87e39daf2e19ddbc4 |
| [redoxfs](https://gitlab.redox-os.org/redox-os/redoxfs/-/tree/5ba45a6887374ab425326656a276bb3b62eba7dc) | 5ba45a6887374ab425326656a276bb3b62eba7dc |

Primary implementation anchors: `base/bootstrap/src/initnsmgr.rs` (Namespace,
SchemeRegister, open_scheme_resource, unlinkat/on_sendfd), `base/audiod/src/main.rs`
(open hardware then mkns/setns), `relibc/redox-rt/src/sys.rs` (register_scheme_to_ns),
`relibc/src/platform/redox/path.rs` (inter-scheme symlink resolution), and
`redoxfs/src/mount/redox/scheme.rs` (flink_internal). The book's `communication.md`
and `influences.md` explain the ABI and separate Plan 9 inspiration from
Minix/seL4 microkernel and BSD sandbox influences; documentation has explicit
outdated-information caveats, so the implementation anchors control these facts.

The [portable-interface concept](../concepts/fp9o6pdb-portable-plan-9-interfaces-with-native-frame-mechanisms.md)
records the resulting direction and ring experiment gates. Sophia already has
scoped discovery and authenticated bound connections; do not add a name router
per frame. Keep existing custody/revocation and private pixel capture. No new
runtime interface, shared-ring dependency or supported OS is established by
this research, and no Redox speedup is a Sophia prediction.

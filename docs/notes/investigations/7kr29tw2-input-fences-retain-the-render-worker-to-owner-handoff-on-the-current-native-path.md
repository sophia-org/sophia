---
id: 7kr29tw2
date: 2026-11-09
kind: investigation
status: resolved
tags: [investigation, performance, custody, native, synchronization]
---
# Input fences retain the render-worker-to-owner handoff on the current native path

## Selection and stopping rule

Niltempus parked worker-side T4 at the [reviewed checkpoint](l4ruwq60-worker-side-native-submission-needs-a-new-authority-and-lifetime-contract.md)
and selected this bounded, docs-only feasibility study. Stop if owner-side
explicit-input-fence submission requires an equivalent worker return handoff or
weakens a required boundary. Do not build fence infrastructure merely because an
API exists. Source anchor: `67fd9c7957931ec4f4801945a1e17f40ebc9cc47`.

**Outcome: stop on the equivalent-handoff condition.** This applies to the
proposed change on the current mixed-composition path, not to every possible
renderer or scheduling architecture. No runtime change, build, native capture,
new model, dependency or device access was needed. Worker-side T4 stays parked.

## Fence creation timing is the first discriminator

[Khronos EGL_ANDROID_native_fence_sync](https://registry.khronos.org/EGL/extensions/ANDROID/EGL_ANDROID_native_fence_sync.txt)
(version 3, September 4, 2012; read November 9, 2026) specifies that creating a
native fence without an imported fd inserts a command into the current context.
The subsequent flush creates its native fence, and duplication exports the fd.
Its completion condition covers preceding commands, not future commands.

[Linux KMS explicit fencing](https://docs.kernel.org/gpu/drm-kms.html#explicit-fencing-properties)
requires the plane's IN_FENCE_FD to name a sync file supplied with the commit.
The kernel waits on it before displaying the plane's framebuffer. It is not a
promise to attach a different dependency after that commit has been submitted.

Inference for Sophia: a fence covering frame N must follow N's relevant drawing
commands in the worker's context. It can be exported before GPU completion, but
not before those commands have been issued. The owner must receive that exact
fence and associate it with the exact buffer before submitting. A fence created
before Render cannot stand for the later draw; a fence from N-1 cannot cover N.

```text
CURRENT DEPENDENCY
owner --Render(N)--> worker --validated buffer/lease(N)--> owner --commit--> DRM

DIRECT INPUT-FENCE VARIANT, EVEN ASSUMING DRIVER SUPPORT
owner --Render(N)--> worker: issue draw(N), create/flush/export fence(N)
                           --buffer/lease + fence(N)--> owner --commit--> DRM
                                                                  waits on fence(N)

The worker-to-owner publication before commit remains.
```

This is a dependency finding, not a measured wake count or earlier-reply claim.
The ordinary composition sequence has no unconditional explicit glFinish or
client-wait before replying; GL-error validation is not GPU completion. The
atomic request defaults to nonblocking and emits NONBLOCK when that flag is set
(`sophia-backend-live/src/drm/native_atomic/request.rs:120–143,199–210`). This
does not prove EGL swap, GBM lock or the driver never blocks, nor that the GPU is
still busy when a particular reply arrives. No current GPU-wait cost was priced.
Replacing the reply with shared storage still requires
publication, ordering and an owner observation; polling it or coalescing it with
some other work is a different scheduling proposal, not a demonstrated saving.

## Current source boundaries

Paths below are relative to `crates/`, at the source anchor above.

| Boundary | Evidence and consequence |
|---|---|
| Render dispatch | `sophia-backend-live/src/scanout/rendered_scanout/exporter/worker.rs:567–592` queues Render and records correlation. It does not provide a finished scanout buffer or GPU fence to the owner. |
| Slot and target selection | `.../exporter/worker/service.rs:485–514,632–690` acquires the frame slot inside the worker and executes mixed composition. The owner does not select the eventual GBM front buffer at dispatch. |
| Current context and exact buffer | `sophia-renderer-native-egl/src/gbm_platform/scanout/render.rs:485–503,758–840` makes the target current, validates drawing, swaps the EGL surface, then locks the GBM front buffer and wraps its ownership. The descriptor refers to that returned buffer. |
| Success or refusal | `.../exporter/worker/service.rs:693–726` refuses failed/missing export, wrong required format or exhausted lease identity, then retains the buffer in the worker lease map. `:299–317` publishes WorkerResult on the output's reply route. |
| Owner validates result | `.../exporter/worker.rs:604–716` checks output, request, correlation, capture shape and required format before returning an exported lease; Failed and Deferred remain distinct. `:355–357` binds the reply to the owner wake. |
| Framebuffer preparation and custody | `sophia-backend-live/src/scanout/rendered_scanout/prepare.rs:240–319` waits for an exported descriptor and owner before preparing the native framebuffer; `tracked_submit.rs:30–139` checks in-flight/cleanup state and accepts submission into custody around the synchronous owner-side submit path. |
| Fence interface today | `sophia-backend-live/src/drm/native_atomic/properties/handles.rs:1–18` has the CRTC out-fence property but no plane input-fence field. WorkerOutcome (`worker.rs:975–983`) carries descriptor, lease and slot, not an exported render fence. This is a gap in the inspected path, not a driver-capability finding. |

There are two independent prerequisites before owner submission: receive the
new render fence, and possess the exact buffer with its validated ownership.
Preallocating a framebuffer would not remove the first dependency. Changing GBM
surface selection to owner-selected render targets would be a larger ownership
redesign; this study does not undertake it to evade the stopping rule.

## Refusals and results must keep their meaning

The ordinary composition path calls `validate_composition` before swap/export
(`render.rs:758–764`); its implementation (`sophia-renderer-native-egl/src/gl.rs:613–622`)
checks GL error state, not GPU completion. It does not establish a CPU-wait cost
that a fence would remove. Swap/front-buffer failures and worker result identity
checks likewise precede owner submission today. A fence cannot replace these
checks or turn Failed/Deferred into a usable submission.

Do not confuse those gates with every reported metric. The nonzero-pixel count
is bounded head/startup evidence, explicitly not a measurement of each frame
(`sophia-backend-live/.../singleton_tick.rs:150–160`); the optional readback error
logs a warning (`render.rs:791–811`). This study does not invent a per-frame pixel
readback requirement. It also does not establish that diagnostic identity/stats
fields are all authorization gates.

[SFT's authority contract](../../native-frame-transactions.md#authority-ownership-and-atomic-presentation)
retains fresh checks at handoff/submission, exact identity and physical retirement.
Keeping the ioctl on the owner helps preserve that placement; it does not permit
submitting before the current success/identity checks and treating a later error
as if no submission occurred. Work already accepted by DRM must retain its real
ownership and disposition. The fence does not authenticate pixels or revoke a
queued submission. No early-submit failure/revocation protocol is designed here.

## Disposition and what was deliberately not pursued

The direct input-fence variant hits the stopping rule even if both driver and
Mesa support are granted. Therefore no hardware property probe, Mesa extension
query, pinned-driver implementation audit or build was commissioned. Actual
Crunch IN_FENCE_FD/native-fence availability remains unverified. Capability
absence is not the reason for the stop; capability presence would not remove
the demonstrated worker-to-owner dependency.

A precreated software/timeline dependency, a different render-target owner,
submission pipelining or moving GL work onto the owner would each be a different
contract/design. None is selected or implemented. Fences could still have a
separate synchronization purpose, but this study supplies no CPU, latency,
four-pass reduction or Service-wake saving for them.

The bounded study is complete with a no-change result. Do not commission a
native run for this proposal. Any reopening must name a different mechanism
that eliminates the dependency while preserving the required gates, rather
than adding an input-fence property and assuming the handoff disappears.
M1/M2 remain deferred, B6 retained, M3 parked; no successor optimization selected.
Task selection stays in [4rifczhw](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md)
and `todo.md` t344. The [four-wait note](edc0te2v-which-action-each-of-the-four-lean-owner-wakes-owes-and-which-transition-a-consolidation-would-remove.md)
remains the source for T4's intended removed transition and observation limits.

## Review

Frozen docs proposal `ca4247592` and evidence
`~/.local/state/sophia-input-fence-feasibility-01/` (SHA256SUMS
`020b3213a1238bfa2445869c63d2b3467f1c1d72b3f703eb04da8da5235d6ea8`)
passed bounded read-only review at
`~/.local/state/sophia-input-fence-feasibility-review-claude-01/` (SHA256SUMS
`20ba4c1b5f77ed25929a13c452e67a657c3acbac478170423c0ce2d1ebf8ca17`).
The follow-up clarifies creation timing and the current nonblocking API intent;
it does not adopt a claim about actual Mesa/amdgpu timing or implicit-sync cost.
Frozen evidence and review remain unchanged.

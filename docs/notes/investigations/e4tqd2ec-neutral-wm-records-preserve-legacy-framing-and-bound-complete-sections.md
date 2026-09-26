---
id: e4tqd2ec
date: 2026-09-25
kind: investigation
status: implemented
tags: [investigation]
---
# Neutral WM records preserve legacy framing and bound complete sections

## Question

How can the WM file adapter reuse record-to-domain validation without creating
old IPC Begin/Chunk/End transfers or a second semantic owner?

## Evidence

Worktree branch `protocol/t249-neutral-records` starts at signed t248 tip
`95b39662e1b2547eb5b0593a56cc3e96ca33e421`. Build evidence is retained in
`sophia-borders/.artifacts/t249-neutral-records`. Checks run device-hidden,
nice19/jobs2, serially on the allocated disk cache; no main or device access.
Signed protocol source is `4406008f`, after separate t248 test-mount correction
`7567c745`. The latter only moves the cfg attribute into the external fixture
and corrects the earlier cached-layout claim in its investigation.

An independent detached worktree `sophia-t249-baseline` at unchanged `95b39662`
compiled only the supplied fixture and emitter against the old protocol source,
on a separate disk target. Its actual legacy frames are retained as
`tests/fixtures/policy-records-95b39662.bin`, SHA256
`3cbde2004a05ce970cc69b431fc6833fbfe4f526b4c775cddd63c71d38cea412`.
The fixture exercises absent/all snapshot capabilities, every current extension,
and 1024 presentation instances crossing the legacy chunk split. It compares
complete frame bytes, including transfer counts and ordinals, against the new
wrappers. Generated ABI and existing golden corpus files remain unchanged.

## Finding and resolution

Complete records use passive metadata plus owned or borrowed sections with
`kind`, `count` and `bytes`. Snapshot, projection and configuration conversion
share row validators with the compatibility entrypoints. Extensions cover output
keys, classifications, launch origins/bookmarks, tab/translation groups and
presentation targets/bindings. Configuration retains catalog name/identity and
chrome validation. No queue, phase, envelope parser or role admission lives here.

Legacy wrappers retain transfer identity, counted-prefix and extension placement
checks. Group/presentation chunk splitting stays in compatibility framing; the
neutral encoder never constructs a legacy transfer. The new complete-section
entry checks every kind's total count and checked byte length before allocating
decoded rows or a coalesced copy. Row order within each kind remains significant.
Coalescing requires callers to validate transport ordering first; in particular,
sorting a malformed presentation before checking its original order would erase
an existing rejection. The file envelope separately owns its sorted/unique
section rule, capability checks, object size and section-count limits.

A pinned characterization found that the direct legacy snapshot codec accepts
17 outputs split into chunks of 16 and 1 when the advertised total is 17.
Generated array decoders bound each chunk, not the combined array. The director
explicitly chose to retain that direct-codec behavior: legacy per-chunk decoding
and strict neutral aggregate decoding reach the same row converter, without
silently hardening the compatibility API.

This is not a demonstrated live admission defect. The separate runtime snapshot
admission owner in `policy_ipc.rs` refuses `output_count > SOPHIA_WM_MAX_OUTPUTS`
with `ExcessiveCount` before publishing a transfer or its transaction watermark.
The 17-output control was run before and after snapshot extraction.

## Validation and remaining work

Initial full protocol corpus: 190 passed, no failures or ignored tests. Initial
new focused controls: seven passed, covering the legacy characterization, all
extension roundtrips, independent old frame bytes, per-kind truncation/count
overflow/unknown kinds, aggregate coalescing bounds, cross-array mismatches and
presentation ordering. Strict protocol all-target Clippy passed after mechanical
borrow/Copy cleanup; the initial lint log remains retained. Final protocol suite:
191 passed, no failures or ignored tests, including the additional configuration
catalog/chrome/section-kind control. Final strict all-target protocol Clippy,
format, included-source format, diff and freshly compiled layout gate passed.
Layout first caught the t248 test-mount attribute described above; the red is
retained separately from the subsequent green run. Tests were not rerun for
that test-only attribute placement before releasing the build slot.

Durable evidence is retained at
`~/.local/state/sophia/development-evidence/t249-neutral-4406008f` with a manifest
and SHA256SUMS. The baseline emitter, exact fixture, logs and binary identities
make the compatibility comparison reproducible without claiming a complete
exhaustive corpus for every possible value. Existing malformed protocol tests
remain additional evidence, not replacement of the independent byte baseline.

This checkpoint does not implement the compact file envelope, remaining scalar
control body framing, Session export or a Hagia pair. Domain ownership, old
output-role IPC and physical acceptance are unchanged. The director owns task
tracking; this evidence does not close t249.

## Connections

The [typed driver extraction](uf2wya88-typed-wm-driver-preserves-current-ipc-phase-and-shutdown-ownership.md)
provides the semantic adapter boundary. The
[accepted public interface decision](../decisions/1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
requires transport reuse without moving authority.

---
id: i7pfnyzy
date: 2026-09-27
kind: milestone
status: recorded
tags: [milestone, 9p, tooling]
---
# Desktop SDK extraction checkpoint before application adoption

## Result and scope

An implemented extraction slice of t263, following the
[accepted SDK decision](../decisions/gs6l7tuk-publish-native-c-and-rust-desktop-sdks-with-pinned-contracts.md).
The reviewed t252 file transport was merged into master as `987cfd393`.
The C extraction and offline integration were merged as `67b35fe77`.
This does not complete t263, t252, or the refused t249 qualification.

## Evidence and decisions

The standalone C SDK's signed initial commit is `c4aa402d84ff293d48acbef94c30cda5e9e51e91`.
Its imported `src/` is byte-identical to `bindings/c` at the t252 merge. It
builds without Sophia and provides separate transport, desktop files, and
optional IPC compatibility archives. Commit `2141aa0` adds explicit endpoint
selection and nonblocking authenticated connection; `a895af1` fixes full-backlog
retry and adds a development coverage manifest. Those follow-up helpers are
not yet in Sophia's initial pin.

Sophia's `vendor/c-desktop-sdk` records the exact source revision, per-file
SHA-256 values and raw upstream commit. The gate reconstructs Git tree identity,
including executable modes, and binds it to the commit. It compares all ten
golden frame corpora, both KDLs, three lifecycle/reference documents, and the
generated WM codec with authoritative Sophia files. Source changes, missing or
extra files, symlinks and noncanonical paths fail closed. Git signature review
is a separate release step; object hashing does not authorize a signer.

Observed checks, at nice 19/jobs 2/private target:

- Standalone strict C99 file/pipeline tests and IPC codec/lifecycle corpus pass.
- Production-export C base tests pass 2/2; r7/r8 role integration passes 1/1.
- Source-pin mutation controls, including drift in each of 17 reference pairs,
  pass. The shared Git-tree implementation matches independent `git write-tree`
  results for executable files and directory ordering.
- xtask clippy with warnings denied and layout pass for the extraction. The
  subsequent canonical-path guard and expanded mutation controls pass their
  focused gate.
- Connection controls pass for an actual full Linux Unix-socket backlog and
  retry, wrong-UID/short credential replies, path bounds, descriptor flags, and
  safe zero-state disposal. Test builds undefine NDEBUG.

The Rust extraction is on Claude's isolated branch, with an independently
building SDK and Sophia dependency switch. Its full gate at `e6cd4822b` stopped
at `a_stalled_authenticated_open_expires_without_exec`'s three-second watchdog.
That is the previously observed load-sensitive test family; this run remains
failed. Log: `~/.local/state/sophia/development-evidence/sdk-rust-gate/e6cd4822b.log`.
It is not evidence that the complete workspace gate passed.

Linux is the first qualified SDK platform; FreeBSD is the next native-CI target.
The C helper's FreeBSD credential adapter is unqualified source preparation.
No SDK repository has been published at this checkpoint: GitHub CLI
authentication needs renewal. Local signed repositories and snapshots exist.

## Remaining work

The [t263 plan](../plans/9cd1ie0x-publish-independent-sophia-client-libraries.md)
defines lifecycle completion, release and application-adoption exits. The C
session/native lifecycle layers and Rust B6c are being implemented separately
from this extraction. Bemenu's adoption worktree starts at `7d2d239`, preserving
its five unpushed commits; no application pin or installed default changed in
this checkpoint. Task state remains in [todo.md](../../../todo.md).

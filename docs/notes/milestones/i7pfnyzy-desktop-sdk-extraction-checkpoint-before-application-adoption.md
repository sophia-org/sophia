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

## C session review checkpoint

Subsequent signed C SDK commits `e436f2c`, `0252f68` and `e1741ca` add
submission progress, the bounded session queue and custody tickets, explicit
acknowledgement barriers, paced retries, and upload helpers. Claude's session
agent reports the standalone gate passing all six file test programs and the
IPC programs/corpora, including upload and node-specific ESTALE controls.
Commit `c885e16` adds the build/package integration. A separate consumer built
against a staged files-only install links and runs through `pkg-config
sophia-desktop`; the temporary evidence directory is
`/tmp/sophia-sdk-install.JUAbYk`. These commits are not yet Sophia's vendor pin.

Review found two additional object-fetch cases: trailing bytes beyond the
declared record length, and fetching an object before consuming its announcement.
Signed SDK `7f0f04d` fixes both with regression controls. The public session's
production-export peer is `cd55618`: its new test passes together with the two
existing base tests (3/3), and the existing r7/r8 role test passes (1/1).

Native lifecycle commit `6196078` passes the standalone strict C gate: seven file
test programs, including ten native-session unit scenarios, and the IPC corpus.
Those native tests replace the session with a scripted fake; they establish no
live native lifecycle coverage. A mutation removing the post-Admitted focus
guard fails the relevant unit test. Packaging/reference commit `df94211` adds
the native layer and the clarified contracts. Its 142-file snapshot passes the
pin checker and mutation tests, including all 18 authoritative reference pairs;
xtask clippy and workspace format checks pass.

The [native contract audit](../investigations/c6z62h49-native-launcher-sdk-audit-exposes-timing-and-generation-assumptions.md)
records thirteen questions traced to owners and tests. Clarifications
`436fb1ac` and `e9750572` preserve current behavior, including its fatal expiry
and renderer-revalidation races. The native layer's production-export test is
now covered as described below; no application or installed default has changed.

## Native session production-owner coverage

Signed C SDK `6a59a13` adds the public native-session peer. Sophia integration
`c1eb783d` pins that exact 143-file snapshot and adds `shell_native_sdk` to the
C file gate. The gate passes all five tests: three base/session tests, one
existing r7/r8 record test, and the new native-session lifecycle test. The pin
check, targeted runtime clippy with warnings denied, formatting and layout pass.

The new peer uses the real C session and native layers against Sophia's real
file export, allocation, resource, candidate, focus and input owners. It checks
Submitted custody, upload acceptance, Prepared versus Presented, exact focus
binding, reservation refusal before a UI edit, one edit and acknowledgement,
keyboard activation, focus disarming, close, allocation invalidation and
resource retirement. Cleanup requires quiescent content accounting.

Session admission and presentation observations are scripted. The content clock
is frozen and input timestamps use fixture time; this test does not cover
production deadlines, pointer activation, launch policy or physical rendering.
Rust B6c review found missing event/object epoch checks, submission-kind and
write-count checks, and a custody overwrite path. Claude has reported fixes in
progress; those require their own regression evidence before integration.

## Bemenu adoption and Rust regression checkpoint

Bemenu branch `sdk/desktop-9p` now has signed commit
`a354251a53368b1f99483b5015a20747afab9804`, directly after `7d2d239`; its five
earlier unpushed commits are preserved. It pins C SDK `6a59a13` and selects
the native file session or IPC from exactly one supplied endpoint. Catalog,
menu filtering and Cairo raster ownership remain shared between the paths.

The strict `check-sophia` gate passes at nice 19, jobs 2, in bubblewrap with
devices hidden and two fixed DejaVu font files. It covers the new adapter with
supplied session outcomes, six snapshot mutation controls and existing IPC,
executable, font and raster tests. The adapter test links the actual native
lifecycle and codec, but replaces the lower session; it is not a production
export test. The Bemenu plan records the font hashes and retained failure logs:
the ambient font scan exceeded the existing executable watchdog even with the
unchanged binary, and the first adapter compile rejected an oversized test
stack frame. Neither warning nor watchdog was relaxed.

Claude reports signed Rust SDK `87ab93f` with 33 scripted file-peer tests,
five repeated passes and clippy passing with and without IPC compatibility.
The regressions exposed progress, same-pass retry and reply-order custody
bugs, now fixed on that branch. The production-owner B6c fixtures remain
required; the older nine file-wire tests alone do not establish role completion.

## Actual Bemenu executable over the production export

Sophia C integration commits `8d1680713` and `c23a38453` add artifact preparation
and the opt-in `shell_bemenu_files` test. They are signed and awaiting integration
review. Preparation verifies Bemenu's signed commit, hashes the extracted tree,
requires the exact Sophia C SDK snapshot, and builds the archive with strict
warnings, nice 19 and two jobs. The source checkout is read-only throughout.

The prepared Bemenu revision is
`a354251a53368b1f99483b5015a20747afab9804`, binary SHA-256
`d64a527da40851404825f4bc307aad7286ecaf94517a6a8c1b3bfd12938cb044`, SDK
`6a59a13f026111a7a277943d71c1fdb090613a23`. The first production-export run
passes: two openings, three candidates, one text edit and one keyboard
activation. Real allocation/resource/candidate/focus owners serve the actual
Bemenu process under the production protected supervisor. The test observes
changed uploaded pixels, close-time resource settlement, reopen with reset
query, graceful exit and quiescent accounting.

The domain exposes only the pinned in-tree JetBrains Mono font and one shell
endpoint. Runtime checks verify the executed binary, environment, private PID
namespace, nice value, font visibility and hidden devices. Input timestamps use
the actual monotonic-clock API, enabled only for Sophia's tests. The C SDK and
Bemenu remain C; the Rust harness lives in Sophia.

Evidence is under `~/.local/state/sophia/development-evidence/bemenu-files/`:
`prepare-a354251.log`, `live-a354251-first.log`, `missing-artifact.log` and
`mismatched-artifact.log`. The latter two deliberately fail before process
launch. Four artifact-helper refusal/timeout controls pass, as do targeted
runtime and xtask clippy with warnings denied, formatting and layout. Session
policy and presentation observations remain scripted; content time is frozen.
This establishes no physical rendering, real launch policy, pointer activation,
expiry, reconnect or attended daily-driver result.

## C and Bemenu integration merged

Sophia master `17b1709a4` merges the reviewed artifact helper, live Bemenu
harness and C SDK custody fix. Bemenu master `2e0fd78` merges the adoption
branch without rewriting its five earlier unpushed commits. Both merges are
signed and local; nothing has been installed or pushed by this integration.

The final C SDK pin is `a0ab8c853fe56b68e01ae69b82d06c15fc177484`.
Its production fix (`3ff46a2`) waits for the acknowledgement of the previous
Submitted before opening the next transaction. The old client could reopen
early and receive EBUSY. The retained before run fails; the fixed tests prove
ordering and continued object-fetch progress while an acknowledgement is held.
The final follow-up documents prompt application acknowledgements and object
fetches, and enables the custody rule by default in the scripted peer.

The final tested Bemenu source is
`fc79f64d722b09b6b4f08fd538d74ee2b1dbf35e`; its prepared binary SHA-256 is
`81cf4008d43e59cc944de141737de9404384922415846ee9f628636a9b5cd678`.
The actual executable passes the production-export test with two openings,
three candidates, one text edit and one keyboard activation. C production
tests pass 5/5, the standalone C suite and strict isolated Bemenu gate pass,
and the pin check, formatting and layout pass. Artifact cleanup controls now
assert that descendant processes die. Harness timeout headroom increased;
application protocol deadlines did not change. Font isolation applies to the
configured font directories, not every file reachable under `/usr`.

Evidence remains in `~/.local/state/sophia/development-evidence/bemenu-files/`:
`c-custody-before.log`, `c-custody-after.log`, `c-custody-strict-default.log`,
`c-live-a0ab8c8.log`, `prepare-fc79f64.log`, and `live-fc79f64.log`.
Bemenu's worktree retains `.artifacts/sdk-isolated-a0ab8c8.log`.
These are deterministic integration results with scripted Session policy and
presentation; the physical, timing and policy limits above still apply.

Claude reports Rust SDK `0da1042` and Sophia integration `93ac7747c` passing
35 scripted tests and nine production-export tests, including a live EBUSY
before/after control. Rust's full workspace gate remains required. The compile
slot has returned to that lane to merge the C master base and run the gate.

## Remaining work

The [t263 plan](../plans/9cd1ie0x-publish-independent-sophia-client-libraries.md)
defines lifecycle completion, release and application-adoption exits. The C
session/native lifecycle layers and Bemenu's application pin have production
export coverage and are merged locally. Rust B6c needs its final integration
gate and merge. SDK publication,
the other application adoptions and attended daily-driver acceptance remain.
No installed default has changed. Task state remains in
[todo.md](../../../todo.md).

---
id: pp3pk4dd
date: 2026-09-26
kind: investigation
status: implemented
tags: [investigation, security, validation]
---
# Read-only WM inspection preserves host admission and writer progress

## Question

Can the wmii-style enumerable tree and live text inspection expose useful WM
state without borrowing the admitted writer's authority, retaining its journal,
or disclosing operation and launch tokens?

## Evidence

The approved t249 slice is implemented on `protocol/t249-wm-inspection`, starting
at signed `58762a049571c57e7d667eb396982b7306bb5301`. The independently reviewed
lane checkpoints are:

- Shared TREADDIR: `81e28524214f19465e7c4fc05ec9440e02b5e0ea`, joined as
  `11fb76ee7`. Core tests, the independent Go oracle's 42 checks, six compiled
  mutations, and the actual WM export listing control passed. Enumeration
  preserves writer Qids, snapshot availability and journal metadata.
- Read-only client: `80416141665dbcc8ae2fa5e8aa304a83310d5367`, joined as
  `5471f9a30`. Ten client controls cover real sockets, malformed replies,
  deadlines, interrupted frames and flush custody. Its codec is separate from
  the server codec; these controls do not replace the Go oracle.
- Runtime observer: `f2a9d033fe3f0be0c9dc58e342e3198ab5fb8a62`, joined as
  `4246de02f`. Full protocol tests passed 258/0; runtime lib 27/0 includes seven
  private observer controls; socket service 9/0 plus an explicitly invoked
  namespace-denial child; retained control 13/0 includes namespace proof.
  Strict protocol/runtime all-target/all-feature checks, fresh layout,
  formatting and metadata passed.

The observer's verified 51-file bundle is
`~/.local/state/sophia/development-evidence/t249-wm-observer-f2a9d033`;
its `SHA256SUMS` digest is
`a28ecbc43c604c98c1ef0bf9e56ec14102f8afcf6bfcd4883da98455a5b44266`.
The shared core/client bundle was
`~/dev/sophia-readdir/.artifacts/readdir-client.bundle` (removed with that
worktree on 2026-09-26; its commits are in master, and the branch tip is in
`development-evidence/branch-archive-2026-09-26/sophia-branches.bundle`), SHA256
`9183d934e0aad12ddafafce59d7774a1282e7868b6d93ac5946210f7af86c6f5`.
Combined integration logs are retained under
`sophia-overview/.artifacts/wm-inspection`.

## Finding and resolution

The observer uses a separate export, socket, continuing Qid allocator, ring and
snapshot pins. Admission reuses the existing pinned HostDomain checks, whose
body was compared byte for byte apart from visibility. The new startup setting
is independent of host control and defaults to disabled. Session constructs an
allowlisted view from its existing policy owner on either WM transport; it
copies no raw protocol packet, action catalog or capability token.

Review found that the first mutable-store design let reader-side formatting and
event copies contend with the nonblocking publisher and manufacture observation
loss. The corrected runtime clones one immutable Arc under a short handoff
lock; readers work outside it, Qid allocation is atomic, and producers build
the replacement before the swap. Event bytes are shared across at most 64 ring
entries. Rare contention during the Arc handoff remains explicit loss, not a
claim of scheduler-independent zero contention.

Deterministic controls hold an old reader Arc across publication, delay a
publisher behind a newer fence, force loss, and queue an Rread larger than a
real socket's send buffer. Revocation leaves the already delivered prefix and
discards the unsent suffix. It cannot recall delivered bytes. A lagging watch
is latched stale: jumping to today's tail cannot bypass a fresh snapshot.

Integration review found two avoidable costs. Session now coalesces enum
notifications and publishes once per ordinary owner turn, before early
continue paths, rather than encoding a snapshot for every queued command or
receipt. A deterministic record refusal latches its scene signature until
facts change. Busy/Fenced retains the pending notification for retry; terminal
service errors retire the observer. An epoch or excluded-peer change clears
old pending notifications and publishes a new connection view immediately.
These are change notifications, not a complete transition ledger.

The Session integration also resets selected-capability observation across
replacement, reports receipts only after command admission, and revokes at
quiescence, fatal cleanup and Drop. The CLI validates pinned objects and
reassembles bounded NDJSON records; a gap or revocation ends a watch and requires
an explicit reopen.

## Validation and remaining work

Combined integration validation passed configuration 2/0, Session inspection
8/0, CLI 3/0, and the retained native Session lib 676/0 with 21 ignored.
Application diagnostics/environment and native launcher checks passed 31/0;
the environment parent also explicitly ran its nested child. Workspace
all-target/all-feature Clippy passed with warnings denied. Workspace and direct
new-support formatting passed. The layout checker was freshly compiled in this
worktree. The normal/native CLI dependency graph excludes backend test-support;
this is a dependency check, not a release binary or installed desktop claim.

The first compile failures were confined to the adapter's rectangle field names,
signed extent conversion, and the CLI's Attr import path. The first compiled
Session run was 7/1: its reload assertion inspected the stored requested
candidate instead of the effective startup permission. Correcting that
assertion required no product change. These logs remain beside the corrected
runs. The adapter uses the scene validator's positive-extent invariant instead
of introducing another geometry validator. No WM, Engine or decoder behavior
was changed to accommodate tests.

The full `xtask check` did **not** pass. Its serial all-feature workspace tests
failed first at X-authority's `m3_acceptance::b_ordered_input`, then shared
lifecycle tracking locks were poisoned and cleanup aborted. The original
assertion was lost when the process aborted before libtest's captured failure
summary. The exact test passed 1/0 alone from the same executable. A diagnostic
full-lib run with uncaptured output failed earlier at `a_recipient_disconnect`
with an uncollected service actor, followed by the same poison/abort cascade.
That direct-binary run also had an invalid workspace-root cwd for a relative
font fixture; its font NotFound is a harness invocation error, not a product
finding. It is not a replacement full gate.

The entire X-authority crate is byte-unchanged from `58762a049`; the changed
protocol dependency only adds inspection records/codecs that it does not use.
The first workspace failure remains unresolved, with no fresh baseline
reproduction or claim that it is merely a flake. The
[earlier routing-test investigation](4qdyd4xb-a-routing-test-read-its-delivery-outcome-before-the-writer-settled.md)
records a related class of harness failures, not this instance's diagnosis.
Read-only follow-up found a candidate explanation for the diagnostic run:
services dropped without `finish()` leave test actor records keyed by a freed
registry address, which a later allocation can reuse. The failing cleanup
assertion holds the shared tracking mutex. This explains that observed shape
without proving the missing first assertion in the original run. The source
analysis is retained as `xauthority-lifecycle-inference.txt`; no X-authority
repair was mixed into this slice.
Strict workspace checks and layout were run separately because the full gate
stopped before reaching them. Later full-gate stages remain unclaimed.

The first standalone layout check refused a new 1001-line completion file.
Moving the observer shutdown statement immediately before the existing
completion include preserved its execution order and restored that file
byte-for-byte to its 998-line baseline. The final layout check, native Session
676/0, CLI 3/0, strict workspace checks and formatting passed on that source.
No debt-ledger exception was added.

Builds and tests were device-hidden, serially allocated, nice 19 and jobs 2. A
writable host bind is not read-only filesystem confinement. Generic Session
controls use supplied owner commands/receipts and a capture worker; real socket
admission and independent client/server framing are separate evidence. There
is no new attended live session, installed profile, native presentation,
mounted-filesystem or default-transport acceptance here. Existing live 9P smoke
evidence retains its original candidate identity. The complete t249 exit stays
in its plan and queue.

## September 26 qualification follow-up

A separate test-only repair now addresses the stale actor bookkeeping found
in the diagnostic run. Signed `4c40d177351b8c4904346ecef1d8a9ab073c603a`,
integrated as `a78329c61`, retains a registry allocation while dropping its
actual service, so the regression does not depend on allocator address reuse.
The baseline fails with a joined watchdog but an uncollected service record.
Drop now records its actual join and retires tracking just as finish does;
join assertions run outside the shared bookkeeping locks. A deliberately
withheld actual JoinHandle still fails both finish and normal Drop, without
poisoning those locks. Unwinding retires records without claiming successful
collection evidence.

The focused controls pass 2/0, and the complete sequential all-feature
X-authority library passes 1244/0. Strict package Clippy, formatting and layout
pass. Logs and exact binary/source identities are retained at
`sophia-xauthority-cleanup/.artifacts/xauthority-cleanup/README.md`.
This proves the bookkeeping defect and its repair. It does not recover the
lost assertion from the original workspace run or close t194. A new integrated
workspace gate is separate from that affected-library result.

The new integrated `cargo xtask check` at `7ce5d61eb` exits 0: 5798 tests
pass and 64 are ignored, including the 1244-test X-authority library. Strict
workspace checks, layout and the remaining protocol/profile/tool/archive
controls also pass. The device-hidden, sequential run is retained in
`sophia-overview/.artifacts/9p-qualification/workspace-first.log` with its
exit file. This is fresh integrated evidence, not a reclassification of the
earlier failure. The gate explicitly leaves buffer-age pixel equivalence,
GLX/EGL first-frame and pixmap-export pixels unproved without a device.

## Connections

- [Inspection contract](../../sophia-wm-inspection.md): owns disclosure,
  audience, bounds, loss semantics and CLI behavior.
- [WM migration plan](../plans/80blhke8-migrate-the-hagia-wm-role-to-admitted-9p2000-l-files.md):
  records approval, scope and the remaining milestone exit.
- [WM file contract](../../sophia-wm-files.md): the writer remains a distinct
  admitted role with custody and semantic outcomes.

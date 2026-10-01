---
id: foc74sm7
date: 2026-09-30
kind: investigation
status: awaiting-physical-acceptance
tags: [investigation]
---
# Topology rebind must transfer singleton framebuffer custody before ordinary presentation

## Question and evidence

Why did the first runtime Apply stall while preparing rollback images after
the installed-mode correction?

The attended t253 run used signed Sophia `d4b06de81` and integration `e5dbf36`.
Validate and semantic rejection passed. Commit-restore passed rollback timing
projection and prepared both candidate images, then remained in
`PreparingRollback` with zero of two rollback images for 30 seconds. The peer
timed out and disconnected. Session aborted preparation without any candidate
KMS submission; baseline and peer-exit readbacks matched and console recovery
was clean. Peer-death was not run.

Under `~/.local/state/sophia/development-evidence/ipc-retirement/`:

- `t253-native-run-e5dbf36-01/run/commit-restore/session.log`: preparation at
  lines 125–128, the peer deadline and abort at 141–148, and final counters.
  All three stage recovery records are retained alongside it.
- `t253-commit-restore-slot-audit-01/AUDIT.md`: independent source/log analysis.
- `t253-topology-custody-01/`: isolated regression, controls and check logs.

The log records six renderer slots held during the stall, against four during
validate/reject. There are three slots per head. The final counters show
5,559,525 slot deferrals and 5,598,587 Session turns.

## Cause and correction

Topology installation put the displayed candidate into per-head custody.
Ordinary singleton frames used a separate runtime custody ledger. Their first
accepted flip replaced the topology image physically but never retired its
owner. Each singleton therefore held two buffers while idle. Preparing another
candidate occupied its third slot, leaving no slot for rollback.

Rebind now transfers each singleton's displayed topology owner into the new
runtime ledger. Its next accepted ordinary flip retires that predecessor
through existing custody. Mirror heads keep their head ledger, which already
owns ordinary mirror submissions. Transfer does not retire an on-plane buffer.

The reverse handoff also needed correction: replacing the old runtime set
dropped its off-plane owner without explicit framebuffer cleanup. After the
blocking topology commit, rebind now retires those owners through their head's
DRM device. Native head identity routes cleanup across regrouping or disabling;
output IDs and connector numbers alone cannot identify a card. Failed cleanup
stays in native custody and blocks another preparation. Every handoff is
validated before any owner moves.

Cleanup remaining in a former mirror head stays separate from the transferred
displayed owner. Singleton and disabled-head cleanup drains on ordinary
retirement turns. Current mirror heads retain their existing cleanup consumer,
which must observe each result to settle the mirror presentation cohort.

Preparation also has a monotonic five-second limit shared by candidate and
rollback preparation. Expiry enters the existing abort/drain path without KMS
or dropping an in-flight export. Export retries are spaced by one millisecond;
Session idles to that deadline while ordinary frames are quarantined. A stale
ordinary-frame deadline cannot force this wait to zero. Aborting is paced but
does not discard owners on timeout; worker abandonment and bounded Session
teardown retain their existing responsibilities.

## Validation and limits

Tests use the production handoff with real runtime sets, real three-slot pools
and supplied framebuffer cleanup. They cover singleton transfer, mirror
retention, disabled/regrouped head routing, global preflight refusal, delayed
cleanup, stale callbacks, and room for candidate plus rollback after an
ordinary presentation. Supplied monotonic instants cover the shared deadline,
retry pacing and abort drain. Removing transfer or deadline enforcement in
private copies causes the corresponding regressions to fail.

Final isolated checks passed: backend library 209 tests, `libdrm_events_feature`
305 tests, and Session's output-file group 14 tests (three opt-in tests ignored).
Backend and Session strict Clippy passed with all targets and features; formatting
and diff checks passed. The signed preparation will run the protected peer
fixture. First compile and lint failures are retained in the evidence directory.

These checks do not exercise KMS, a GPU renderer, or the whole native owner
loop. The attended sequence remains required on a fresh signed candidate,
with fresh preparation, performance evidence, integration pins and sealed
inputs. Failed runs remain intact; no acceptance threshold was relaxed.

## Connections

- [t253 and t272](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t253--migrate-the-separate-output-role)
  retain their native acceptance and retirement ordering.
- [Installed rollback timing](l696uhdc-rollback-preparation-must-use-the-installed-mode-after-a-topology-commit.md)
  fixed the earlier projection failure; this run passed that boundary.

---
id: slj1d7hk
date: 2026-10-02
kind: investigation
status: closed
tags: [investigation, session, x11]
---
# Retire obsolete session controls after client withdrawal

## Incident and scope

Release `niltempus-9de41ea905db10201b9e`, Sophia `f650e688`, exited with
`control_rejected` after a client unmapped its main window while WM transaction
148 still named it. The archive does not retain the control kind or rejection
reason. Evidence: `development-evidence/session-exit-9de41ea9-01/`.
The later client SIGSEGV occurred during teardown and is not established as the
cause. No preview recovery failure was recorded.

This independent correctness prerequisite precedes the live qualification of
[t289](../plans/7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md).
Do not suppress generic authority errors. Prove lifecycle races through the real
control writer, keep exact transaction settlement, and preserve diagnostic
outcomes through the archive reducer. No live changes are part of this repair.

## Findings in the incident release

The writer collapses `WindowNotViewable` and missing runtime resources into
`AuthorityRejected`. Session also treats `UnknownSurface` as fatal for admission,
configuration, presentation and withdrawal controls. An old focus completion can
clear standing input authority for a newer request to the same surface.
These are source findings; the lost archive fields prevent identifying which
one ended this particular session.

## Implemented repair

The writer now reports missing surfaces, withdrawn admissions, non-viewable
focus targets and superseded focus claims distinctly. Invalid geometry, foreign
authority and other genuine refusals retain their fatal handling. Supersession
is stale only for focus and clear-focus, and never releases input standing or
clears applied focus. Other stale focus completions may clean up only when their
key is still the latest Session focus request.

Stale control settlement removes only the matching transaction and surface's
pending obligations. It does not invent a surface withdrawal. The existing
post-control layout service resolves a now-ready transaction in the same owner
pass, reporting only surviving applied surfaces. Diagnostics retain bounded
kind, generation and outcome fields through the archive reducer, without
disclosing error text or client metadata.

The existing active-window publication helper was moved unchanged into
`connection/writers/active_windows.rs` to keep the control writer within the
source-layout bound. This is independent of the refusal changes.

## Qualification

Evidence is under `development-evidence/session-exit-9de41ea9-01/repair-01/`.
The source base is `98f22b43ebd0322f8d9a65627870b49fc72336d5`.

- `14-full-gate.log`: `cargo xtask check all` exited 0. The workspace printed
  457 passing test summaries, 6,625 passed, zero failed and 91 ignored. Strict
  Clippy, formatting, layout, generator, both SDK checks and evidence readers
  passed. Re-executed child-helper results contribute to printed totals.
- Real control-writer tests cover unmap before focus, destruction before queued
  controls, withdrawn admission, superseded private focus, and genuine invalid
  commands. Session tests cover the stale-kind matrix, latest-focus ordering,
  exact transaction settlement, immediate progress and the surviving-surface
  result. Archive tests preserve scalar outcomes and reject private text.
- `15-models.log` and `spec/output/`: AdmissionRecovery,
  PolicySettlementRecovery, PolicyOutputSettlement, TargetResolvedInput and
  InputDeliveryRecovery passed TLC. NoDeadline and EarlyBarrier controls failed
  for their expected temporal and BarrierSound violations. These unchanged
  models cross-check existing settlement and input obligations; they are not a
  refinement proof of the new writer outcomes. TLC 1.7.4 used one worker, fp 0,
  a 2 GiB heap and a 30-minute timeout per case; the script pins the jar digest.
- pF reviewed the final outcome boundaries and the unchanged helper extraction
  without finding a remaining source defect.

First failures are preserved: the initial writer and queue probes, the shutdown
negative control's old UnknownSurface expectation, a test-only Clippy
initializer, and the writer's source-layout overflow. The final source gate is
green; later changes to this note only record its results.

## Operator acceptance, 2026-10-03

Task t290 is closed. niltempus confirmed the Ghostty test in the running repaired
session and requested closure. Release qualification and the operator check
complete the remaining acceptance recorded at implementation.

- Installed release: `niltempus-3cd50bdb34d5092f5665`, Sophia
  `b6ad18cff13f84ae05626bb060626cb01ab96699`, niltempus `0b5ce681`.
  The release audit verified all 92 files, SDK identity, sealed profile and
  private install checks. Hagia remains `03be1d1f` with the retained personal
  binary; no WM contract or SDK change was required.
- Running session: `00000001791024317688-8af3b9ce-6c28-4af3-ac60-c8b4e4661271`.
  Its manifest names the repaired revision. The running executable hashes to
  `105babb65ca06d251e6daf7b399e35ee0061eb07f59b08270a5902ab1b7b69de`, matching
  the installed release and session manifest.
- The preserved operator report records opening Helix in Ghostty, entering
  text, pressing Super+Q and affirming Ghostty's confirmation. Ghostty closed
  and the session survived. niltempus confirmed the live test again at closure.
- The preserved control records show `stale_target_retired` for `ClearFocus`
  with `unknown_surface` (sequence 139018), followed by `FocusSurface` with
  `target_not_viewable` (sequence 139027). The excerpt reports zero fatal
  records, and the same session was still running at closure.

Evidence: `development-evidence/t290-release-01/release-audit-3cd50bdb.txt`
and `live-acceptance-01/EXCERPT.txt`, `ACCEPTED.json`, and the copied session
manifest and identity records in that directory. The current event files are
rotated; the preserved excerpt retains the earlier control observations.

This accepts recovery of the tested client-withdrawal race class. The original
archive still cannot identify its rejected control kind, so this is not an
exact replay claim. Genuine authority, renderer and device failures retain
their handling. T289's CPU and Present-timing acceptance remains separate.

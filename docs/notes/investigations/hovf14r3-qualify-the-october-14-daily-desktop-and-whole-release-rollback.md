---
id: hovf14r3
date: 2026-10-14
kind: investigation
status: investigating
tags: [investigation, session, validation]
---
# Qualify the October 14 daily desktop and whole-release rollback

## Current resumption

The operator authorized proceeding with t250 after t249 closed. The current
release is `niltempus-e8818395183f05263ccc`: Sophia `d5938f2ad`, Hagia
`2ba92d9`, integration `38b5b6d`, with explicit WM 9P2000.L selection. The
ordinary session is `00000001791981993103-12d43227-7ebd-4663-be34-b144d1aff63e`.
The operator reports working Lom, application launcher, overview, workspaces,
Alt-Tab and screen lock/unlock. The operator also floated the active Kitty
window, moved it and resized it successfully in this same session. The retained events corroborate lock, coverage
of two heads, authentication checking and unlock. These observations do not
claim every workflow on both heads.

The non-disruptive release audit and rehearsal preparation pass. The next
attended checks concern rejected-profile recovery, shutdown and whole-release
rollback. The operator-triggered WM restart has passed: the session logged
`restart_requested` at boot millisecond 436341656, then `restarted epoch=2
restarts=1 preserved_layout=true` at 436341898. The operator confirmed the
desktop remained normal and the restart was not noticeable. The 242 ms between
those records is an event interval, not a measured visual interruption.
The existing rehearsal must run after logout on tty4; it deliberately restarts
the WM, rejects a profile and logs out. No live restart, profile change,
shutdown or release switch was performed during this audit.

Do not use `tools/desktop rollback` now: `previous` names the faulty
`niltempus-a986fef4f64b032edc11`. The explicitly verified fallback is
`niltempus-7b8051ace95f8581d57e`. Its store root and sealed files remain intact.
Actual fallback login and return to the candidate remain unobserved. Task state
stays in `todo.md`; this is not a t250 completion record.

## Candidate and packaging incident

The first October 14 release paired the new Sophia with old Lom, Bemenu and
kleis pins that lacked authenticated attach. The missing bar and repeated
negotiation failures were real installed failures. The corrected release pins
Lom `dc65f56`, Bemenu `6e2efd9` and kleis `7927c69`; those three binaries match
the earlier working release byte for byte. The integration repository's
`docs/install.md` records the incident and attended recovery. Preserve the
failed release identity rather than treating its checks as desktop acceptance.

The current candidate's desktop profile SHA-256 is
`fa0320daa816a993b48ae18222b13792f7c2eeae106575d8dde6082879322db9`.
Its Sophia binary SHA-256 is
`5c843c0f4081f77f38105a58d12aca060f9c9511b58488f604d3ab18752f83ad`.
Shell components use their separate file endpoints. No standalone output
process is selected by the daily profile; Session owns output configuration.

## Classified evidence

Evidence lives under `~/.local/state/sophia/development-evidence/`:

- `t250-installed-audit-20261014-01`: both releases pass their complete
  `SHA256SUMS`, packaged policy verification and file comparison with their
  rooted Nix store outputs. It records manifests, current session identity,
  selected lifecycle events and logging health. This proves retained artifact
  integrity, not successful fallback login.
- `t250-rehearsal-niltempus-e8818395183f05263ccc-9p2000.L-20261014T125218Z`:
  the existing runner's dry run prepared valid and deliberately rejected
  profiles and exact binary identities. No hardware was opened by the dry run.
- `tools/check_rehearse_wm_9p.sh` and `tools/check_desktop_tool.sh` passed in
  niltempus. These exercise offline refusal and installation mechanics; they
  are not attended recovery evidence.

The corrected release built through the existing Nix package owner and passed
profile preflight. The [t249 final qualification](5rzn1zue-wm-qualification-after-source-retirement-and-attended-output-recovery.md#final-development-qualification-and-coverage-2026-10-14)
retains the complete generic and independent-client gates and approved latency
and resource comparisons for the unchanged qualified implementation. Those
control-path results are not installed input-to-photon measurements. No old
failed campaign is relabeled and no benchmark is rerun merely for this audit.

## Remaining acceptance and prepared commands

Floating, moving and resizing the active Kitty window passed by operator
observation on this candidate. This is an attended functional check, not a
numeric input-to-photon measurement. The subsequent requested WM restart
preserved the layout in the session log and appeared normal to the operator.
Exact-release startup/refusal, rejected-profile rollback and shutdown still
need the attended rehearsal;
September 26's earlier successful rehearsal does not substitute for it.
Physical pointer-drag/presentation evidence remains distinct from t249's
control-path budgets. Whole-release rollback needs an actual compatible relaunch.

After logging out and logging in on tty4, run from `~/dev/niltempus`:

```sh
tools/rehearse_wm_9p.sh --wire=9p2000.L --release=/opt/sophia-niltempus-desktop/releases/niltempus-e8818395183f05263ccc
```

For a separately scheduled whole-release rollback trial, use the existing
installer with the explicit store root, then log in to observe the fallback:

```sh
~/dev/niltempus/tools/desktop install ~/.local/state/sophia-niltempus-desktop/gcroots/niltempus-7b8051ace95f8581d57e
```

Restore the exact candidate through the same owner and log in again:

```sh
~/dev/niltempus/tools/desktop install ~/.local/state/sophia-niltempus-desktop/gcroots/niltempus-e8818395183f05263ccc
```

These commands are prepared instructions, not evidence that they ran. They
switch the next login's complete release; they do not replace running binaries.

## Connections

- [t250 acceptance](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t250--qualify-the-wm-daily-configuration)
  owns the exit and keeps physical acceptance separate from development gates.
- [WM qualification](../plans/80blhke8-migrate-the-hagia-wm-role-to-admitted-9p2000-l-files.md#t249)
  owns the unchanged numeric budgets and their approved comparison rules.

---
id: 6q35cl9y
date: 2026-09-07
kind: investigation
status: closed
tags: [investigation, session, recovery]
---
# Normal login arms recovery without a keyboard rehearsal

The user approved removing the Ctrl+Alt+Backspace arming handshake from ordinary
installed logins. The independent recovery guard remains enabled. This is a
[t013 startup and recovery change](../plans/queue-04-2-establish-the-live-session.md#t013),
not a change to application shortcuts or WM policy.

## Ownership and behavior

The installed launcher already distinguishes ordinary desktop sessions from
explicit proofs and promotion runs. Ordinary logins now request automatic
arming; a user can still select manual arming through
`SOPHIA_INPUT_GUARD_ARMING=manual`. Proof and promotion launches force manual
arming even when their environment requests automatic. Explicit TrueColor and
watchdog environment settings also select the proof path. Development launchers
and the underlying guard command retain their manual default.

The guard itself opens libinput and requires a keyboard before publishing its
armed marker. The automatic state uses the existing emergency-chord reducer:
the first complete chord triggers recovery. Manual mode still requires a full
press and release to arm. The wrapper waits for readiness in both modes and
rechecks guard liveness and pending recovery immediately before graphics
takeover. Guard failure during the session remains a supervised failure.

The startup timeout bounds readiness, not desktop lifetime. Automatic arming
proves that keyboard input opened; only an operator's chord proves the physical
recovery path. Removing the repeated rehearsal does not establish that proof.
The [operations contract](../../operations.md#emergency-recovery-and-fallback)
describes the user-visible behavior.

## Validation and remaining acceptance

Candidate base: `34128d80`, alongside the
[Kitty admission repair](ce2b55uy-blank-thunar-menus-and-frozen-brave-need-separate-pixel-and-delivery-evidence.md#2026-09-07-kitty-starts-but-never-enters-the-composed-scene).
The new public guard-entry tests refuse invalid arming modes and prove that an
input-open failure publishes neither readiness nor recovery in either mode.
Existing reducer tests cover first-chord triggering, full-release manual arming,
and rejection of partial chords and repeats. Installed-launcher fixtures cover
ordinary automatic arming, explicit manual arming, and proof/promotion selection.

`cargo xtask check` passes: workspace tests, Clippy, contract and archive checks,
and host buffer-age pixel equivalence. The installed-launcher check passes
separately after fixture isolation and cleanup were tightened. Formatting,
metadata, and diff checks pass. Private logs and exact source copies are
checksum-verified at
`~/.local/state/sophia/development-evidence/t013-guard-cd6e570277f5`; source identity
`cd6e570277f546eb424bf69413e847ea113079b75f5b749423df903e48399049`.
The existing Kitty before/after pixel evidence remains in its linked investigation.

No install or live-session replacement was performed.
After installing this candidate, accept one ordinary login with
no arming prompt and a normal logout, then separately confirm that one
Ctrl+Alt+Backspace chord returns control to greetd. These physical observations
remain part of t013; deterministic checks do not close that gate.

## Installed startup observation

The user returned in a fresh session on September 7. Session
`00000001788792259184-92b32790-8c54-4552-a0c7-811d5af1c39c` records release
`71b9b0d1960403ccbb8922ae1f1b91f3d34d60b9` and binary SHA-256
`11c76ed1889e891efac957fbe05227bc5287bafd4d61712db16cee012b4d92eb`.
Its guard process has `--arming=automatic`; the session's own guard log records
ready and armed, and its lifecycle reaches the session phase. Reduced events
show continuing presentation and scanout. Recording reports no discarded events
or storage errors at this observation.

Evidence is under `~/.local/state/sophia/sessions/` in that session directory.
The old logs directly under `hagia-session/` include a different release's
failure; use the `current` directory target to attribute this startup correctly.
Normal logout and physical emergency recovery remain unobserved for this release.

## Operator acceptance and retained installation, 2026-09-24

Mason confirmed that they had logged in, logged out, and performed emergency
recovery many times. This supplies the missing operator acceptance; asking for
another rehearsal merely because this September 7 note was stale would repeat
accepted work. The confirmation concerns ordinary use across installed releases,
not a newly timed physical run or a retrospective claim about one session ID.
The automatic-arming login observation above, later accepted normal logout in
t019, and this confirmation satisfy the login/logout/recovery part of t013.

The accompanying read-only installation audit verified all `SHA256SUMS` entries
and the packaged Sophia/Hagia profile parsers for:

- Current release `0.1.0-d8eb04f69c63`, full Sophia commit
  `d8eb04f69c63d3256604be4d3f734dd3b9a7df96`.
- Previous release `0.1.0-4eacfcfbc3be`, retained in the normal rollback slot.
- Known working fallback `0.1.0-d461492da746`, still present under
  `/opt/sophia/releases` and previously accepted for ordinary daily use in
  t007/t009/t011/t012/t019. Its complete package and packaged profile verify.
  It is retained separately from the one-level `previous` rollback pointer;
  the newer previous release is not substituted for that acceptance evidence.

The current Sophia executable hashes to
`9afc8fad471c7034a5270a738d1ab740d82793f3608e1d94b9aad6e35d5a8b0f`.
The packaged Hagia source is `50336ce66a9a14d731b34e34638c2cd9c4a71878`,
its binary hashes to
`d92392c96f182912bf603b4c6ab96a3969adf291be037f5a70af204255337408`,
and packaged Narthex to
`1a22f1212667de6849d82a63acf5967f1a61afc485d60bd93d7d8034302074f1`.
The packaged default-profile hash is
`11cee6b5229e91115c7ae67df138830bab32b7dff48cf8915377316a93e03d40`.

The live user override is recorded separately: Hagia runs from
`~/.local/state/sophia/desktop-releases/20260918-d444eba2/hagia`, hash
`21305861337142418ceb8227ca6ad16bfabf579f4456f2d4e1013bb83ac2d1a8`.
No Narthex process was present in this snapshot; its packaged identity is not
claimed as a running shell. The loaded user-profile record has effective digest
`0695ffa4be9a2a833c05d5cf93052e64f9c94a090d096f9728b1b03903522fb8` and
root hash `73b41fe1f138b56df9f264d74ea275e6bc1fe24d7e2106d601e36764c26026ea`.
The on-disk profile also passes the installed Sophia parser; the audit records
its digest separately rather than asserting that an edit has been reloaded.

Evidence lives at
`~/.local/state/sophia/development-evidence/t013-acceptance-20260924/`:
`identities.json`, `checks.json`, and `status.txt`. The status helper's optional
session-list appendix reported a diagnostic-directory mode refusal; direct
checksum/profile verification succeeded. That appendix is not used as proof of
session completion. The currently inspected XTEST session uses manual guard
arming; it is not presented as a new automatic-arming login observation.

The [operations runbook](../../operations.md) documents normal entry, logout,
emergency escape, the baseline fallback, and rollback. No installation,
activation, rollback, process restart, input injection or live display probe
was performed by this audit. t013 closes on the retained installation evidence
and operator acceptance; the separate development-workflow exits remain separate.

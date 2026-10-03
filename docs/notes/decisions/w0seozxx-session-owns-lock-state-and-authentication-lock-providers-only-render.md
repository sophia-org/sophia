---
id: w0seozxx
date: 2026-10-03
kind: adr
status: proposed
tags: [adr, security, lock, input, session]
---
# Session owns lock state and authentication; lock providers only render

## Context

Sophia has no session lock. [t034](../plans/queue-13-authority-and-lifecycle-hardening.md#t034)
asks for the secure contract: privileged admission, a trusted unlock authority,
complete output coverage across topology changes, revoked input and focus, and
fail-closed provider loss. A full-screen shell image is explicitly not a lock.

The 2026-10-03 source survey found that every mechanism that now draws over
applications fails open. A WM presentation in `ReplaceApplications` mode falls back
to applications when a source is missing. Topology first frames strip the WM tier.
Hotplug, seat release and native unavailability terminate the shell. A new head
scans out before any client learns it exists. Shell content and the descriptor
overlay draw above the WM tier, and nothing draws above them.

Authentication cannot simply move into a shell-like client either. The
protection domain used for native components has an empty `/etc`, no capabilities
and a private user namespace, so PAM, `/etc/pam.d` and the setuid `unix_chkpwd`
helper are unavailable there. Weakening that domain for a locker would trade one
boundary for another.

niltempus wants lockers such as kleis, a port of the Wayland locker lockme, to
build on a native contract. Asked how Plan 9 divides the work, the answer was
factotum: a trusted agent holds secrets and runs authentication, and programs
that need it never verify anything themselves. niltempus chose that shape.

## Decision

Lock is a Session security transition with three owners.

**Engine owns coverage.** While locked, every head of every output draws an
Engine-owned lock layer last, above applications, WM presentation, shell
content and the descriptor overlay. The layer is an opaque fill; a lock
provider's image may replace the fill only where Engine holds an exact presented
candidate for that head and lock epoch. Every path that builds a head frame
includes the layer, so a hotplugged head, a topology first frame, a resumed
scanout and a recovery frame are covered without any client. The cursor is
hidden. Session reports the session locked only after every presented head
frame carries the lock stamp. Application surfaces under the layer count as not
visible to every consumer of visibility, including Present pacing.

**Session owns state, input and the verdict.** Session mints a lock epoch for
each lock. Entering a lock advances the input security epoch and performs the
full revocation set (application leases, grabs, captures, held keys, repeat,
launcher and policy capture). Locked and unlocked are reported only after the X
frontend has applied the new epoch. While locked, the VT and emergency
recognizers still run first; every other physical key goes to the lock capture;
synthetic input is refused. WM shortcuts, applications, shells and launchers
receive nothing. A reserved session action and a control request can lock. No
client can unlock.

**A Session authenticator, `sophia-factotum`, is the only unlock authority.**
Engine's keyboard handling commits text into a Session-owned secret buffer that
is page-aligned, locked in memory, excluded from core dumps and zeroed on every
clear. On submit, Session hands it to a separately executed helper over a
length-prefixed pipe. The helper runs PAM with a configured service name. A
verdict unlocks only if it names the current lock epoch and the current attempt;
any other verdict is discarded. Unlocking advances the input epoch again, so
input typed while locked can never reach an application, and restores focus only
to a still-authorized target. Password text, its length and PAM prompts are
never logged. The name follows Plan 9's agent; its first scope is unlock only.

**Lock providers only render.** A lock provider is a separately admitted role
with its own native file contract, endpoint and pidfd-checked peer. It receives
per-output allocations for the current lock epoch, uploads pixels and presents
candidates within a lock budget derived from the topology. It sees edit and
status events (insert, delete, clear, submit, checking, failed, unlocked) and
opaque IDs for UI chords it registered; it never sees characters. It cannot
enter or leave the locked state, delay the cover, draw outside its allocation
or present into a newer lock epoch from an older connection. Its loss leaves the
fill; unlock still works without it.

## Alternatives

**The ext-session-lock model: the locker authenticates and asks to unlock.**
This trusts the client's claim, puts the password in the client and needs a
protection domain able to run PAM and a setuid helper. niltempus chose the
factotum split instead.

**Factotum split with the locker forwarding text.** Closer to Plan 9's
`screenlock`: the locker reads keys and writes them to an authenticator file.
The locker cannot forge an unlock, but it holds the password and unlock depends
on its liveness. Not selected.

**Lock as a fourth shell component, or as a WM presentation.** Both render
through owners that are revoked, paused or stripped on exactly the transitions a
lock must survive, and both draw beneath content they cannot cover.

**X11 locking (grabs, override-redirect windows, MIT-SCREEN-SAVER).** Grabs are
namespace-scoped and cleared on every security epoch, Session handles keys
before the frontend sees them, and MIT-SCREEN-SAVER is not served. X11 lockers
cannot secure a Sophia session and this decision does not try to make them.

## Consequences

Sophia gains a security authority that holds a secret, a PAM dependency in one
helper binary and a new native role. The helper needs the host's PAM stack and
its setuid helpers, so it does not set `NO_NEW_PRIVS` and does not run in the
component protection domain. Real-PAM controls use `pam_start_confdir` with a
private configuration directory; ordinary checks never read `/etc/pam.d`.

The live X frontend's ungated epoch counter is not enough for a lock. Lock and
unlock move onto the requested/applied split that the private XTEST instance
already uses.

The lock budget is separate from the shell content registry, because whole-output
surfaces do not fit the shell's per-resource and shared ceilings.

While locked, hidden clients are paced as hidden and Present binds to the
fallback clock; any cache keyed on visibility must include the lock state.

Kleis keeps its renderers and configuration and drops its PAM child and password
buffer; their hardening moves into `sophia-factotum`.

Open before acceptance: the lock file contract's byte layouts, the lock budget
numbers, the formal models of lock epochs and stale verdicts, and attended
physical acceptance on an installed release.

## Acceptance and connections

Proposed 2026-10-03. niltempus chose the factotum split and the parallel lane;
the record itself awaits review. Tasks and exits are in the
[lock plan](../plans/8jcykhdc-secure-session-lock-authority-and-lock-provider-role.md).
Related: [synthetic input admission](htm85gg0-admission-ingress-and-provenance-for-synthetic-input.md)
(no synthetic unlock or injection into a locked seat),
[launcher presented input](f64wqfh2-independent-native-launcher-admission-and-presented-input-contract.md)
(Engine-owned XKB delivery precedent),
[output file role](vkkjmufd-use-native-records-for-the-separate-output-file-role.md)
(record identity shape and pidfd admission) and
[GPU execution permission](mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md).

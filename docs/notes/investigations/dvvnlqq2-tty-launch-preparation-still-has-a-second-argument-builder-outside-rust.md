---
id: dvvnlqq2
date: 2026-09-24
kind: investigation
status: closed
tags: [investigation]
---
# TTY launch preparation still has a second argument builder outside Rust

## Question

What still keeps [t027](../plans/queue-11-parallel-production-readiness.md#t027)
from being a TTY/display-manager adapter around `sophia session run`?

## Evidence

Read-only audit at `d5299e39`: `tools/start_sophia_tty3.sh` is 443 lines and
dispatches four ordinary profiles through `tools/run_sophia_session.sh`, which
is 1,180 lines. The latter still assembles the actual argument and environment
vectors, chooses workload defaults, stages proof profiles, checks requested
flags and validates the assembled command before graphics takeover.

`crates/sophia-conformance/src/profile.rs` already has typed profile argument
construction, but that construction feeds profile checks, not this complete
live launcher path. Substituting its current vector would lose normal desktop
application defaults and several explicit proof/workload options.

## Finding and resolution

The remaining boundary is command preparation versus TTY ownership. Rust should
own profile and environment parsing, argument construction, candidate checks,
private proof inputs and the exact command-validation result. The adapter
should retain TTY/display-manager handoff and restoration, with the independent
input guard and watchdog preserved until their ownership is explicitly moved.

The first migration must compare actual vectors for all existing profiles and
proof variants; checking a second approximation of the vector repeats the
current split. Arguments containing spaces or shell syntax must remain single
arguments. Requested flags must be admitted or refused, never dropped.

Installed launchers cannot acquire a dependency on Cargo or an uninstalled
xtask executable. Shared preparation used by installed and development paths
therefore needs a runtime entry point, with xtask remaining the test/gate
driver. Gate-only orchestration can stay in xtask.

## Validation and remaining work

The initial audit changed no launcher behavior. t027 remains open. Required checks
include the existing Hagia preflight fixture, terminal adapters, exact argument
acceptance, refusal before display-manager shutdown, and disposable-PTY recovery
fixtures. Preserve bounded child waits, guard death/recovery behavior, session
process-group shutdown, private evidence modes and bus ownership. A headless
run must not invoke real DRM, input acquisition or display-manager operations.

## First implementation slice

`sophia config check-session-profile` now owns the existing Hagia preflight:
typed desktop loading, selected WM/shell executable checks, file-identity
comparison for Hagia aliases, private policy-only staging, and a ten-second
deadline for the policy checker and its process group. A different selected WM
retains responsibility for its own vocabulary at protocol activation. The
shell helper delegates to the installed binary, with an outer fifteen-second
bound; no Cargo or xtask dependency enters installed startup. It requires the
explicit acceptance record as well as success status, so an older binary that
does not implement preflight cannot silently admit a handoff. The shell fixture
includes that negative control.

Seven new CLI tests cover policy privacy and file modes, cleanup on acceptance
and rejection, missing executables, Hagia aliases, another WM's vocabulary,
invalid envelopes/duplicate options and a stalled checker. Four existing
desktop-config tests and 22 launcher-safety tests pass. The disposable-PTY
fixture still proves that preflight refusal happens before TTY-mode queries or
privileged handoff. Clippy passes for all CLI targets with and without native
features. Logs were kept in the sibling build cache `../sophia-t027/.artifacts/t027-*.log`,
removed with that worktree on 2026-09-26; the retained copies are
`~/.local/state/sophia/development-evidence/t027-preparation-3f446e04/t027-*.log`.
The native-feature normal-session lifecycle test also passes with devices and
installed session sockets hidden (one test, five session-start variants).
Formatting and the layout gate pass.

The live argument/environment builder, remaining profile/gate dispatch and
their exact-vector verification are still outstanding. This first slice does
not close t027 and does not install or launch a live session.

## Argument and environment preparation

The second slice moves the live argument vector for Hagia, native, Kitty and
standalone profiles into `sophia session prepare-arguments`. The installed
binary also owns trace/proof environment entries and bus selection through
`session prepare-environment`. Both commands emit a versioned NUL-delimited
record only after preparation succeeds. The adapter loads arrays without
evaluation and refuses missing acceptance records or failed preparation.
No Cargo dependency enters installed startup. The standalone Kitty override
checker now has a ten-second deadline and a two-second forced-kill backstop.

Retained pre-migration Bash builders are test fixtures, not production paths.
Thirty-five argument comparisons cover all profiles, terminal adapters,
standalone workloads, TrueColor and Firefox slices, explicit requested flags,
and literal metacharacters/newlines. Fifty-six environment comparisons cover
proof priority, explicit empty trace values and all four bus modes. An adapter
test rejects old binaries and verifies literal argument boundaries. Five Rust
application-selection tests replace the Python extraction suite, preserving
its missing-default and invalid-explicit-path cases. Source-text assertions
for moved behavior were replaced by these executable comparisons.

Affected CLI tests, the native-feature normal-session lifecycle, Hagia
preflight including its disposable PTY refusal, terminal checks, lifecycle
diagnostics, and default/all-feature Clippy pass. The live wrapper is reduced
from 1,180 to roughly 800 lines. Remaining preparation includes executable
discovery, private proof staging, control validation and the exact command
validation call; those still prevent calling the wrapper a minimal adapter.
t027 remains open. No live installation or physical session was run.

The full device-hidden native-family gate passed all eight phases on clean
signed `3f446e046a8c28969cb6784ddd64ca8a2a06dcfe`, with Hagia `ad3a738d`
and Narthex `50b9014d`. After the unrelated X input-writer source split landed
as `6155d23a`, the unchanged preparation slice was rebased to signed
`23ec8b3b623cf17c7f73e7ee83cf78245f2062bb`. The affected CLI tests,
all-feature Clippy, formatting and layout gate passed again on that candidate.
The initial layout failure concerned the inherited 1,017-line X input writer;
the owner's split fixed it without adding a debt row.

Checksummed evidence is retained at
`~/.local/state/sophia/development-evidence/t027-preparation-3f446e04/`,
including the native-family identities/report and logs before and after the
rebase. This does not supply the optional Lom content-client acceptance needed
by t099. The next control-validation migration must preserve rejection before
state-directory creation, even though the development launcher currently builds
its binary later; moving those checks past state creation would weaken refusal.

## Connections

The [Session source split](izw9opes-private-session-tests-can-move-without-widening-the-production-api.md)
keeps argument parsing and startup profile evidence behind their existing
private facade. That refactoring supplies a clearer owner for preparation but
does not itself migrate the launcher or satisfy t027.

## Remaining preparation moved to the installed binary

The final preparation slice adds `prepare-controls`, `prepare-inputs`,
`stage-proofs` and `check-launch` under `sophia session`. Controls are checked
before the wrapper creates session state, and before the outer ordinary-profile
TTY adapter queries modes or begins privileged handoff. Development startup
builds the matching validator first; installed startup forbids that bootstrap.
Executable discovery preserves optional normal-Hagia defaults, explicit proof
adapters and standalone workload choices. A retained Bash oracle compares those
results, including absent defaults, invalid paths and literal shell syntax.

Proof staging requires an absolute private owner directory, creates Firefox
profiles with 0700 directories/0600 files, and reclaims only real stale profile
directories. Failed preparation removes the new profile before reporting failure;
only a complete acceptance record transfers cleanup ownership to the adapter.
Direct-scanout fixture copies replace destination links atomically. The Kitty
parser and exact session parser each have a ten-second process-group deadline.
The latter executes the same binary with the actual prepared environment and
argument vector, captures private diagnostics, and requires parser acceptance.
It preserves the existing parser's vocabulary and handling of unknown switches;
it does not introduce a new argument grammar.

The wrapper is now 542 lines, down from 1,180 at the audit. Its remaining work
is TTY ownership, independent guard/watchdog custody, bounded session shutdown,
bus lifetime and recovery logging. The outer adapter retains display-manager
handoff and dispatch to ordinary or explicit physical-proof entry points; its
profile check already delegates to Rust/xtask. Archive verification remains in
the existing Rust tooling. None of these preparation commands opens devices,
starts a bus, installs a release or reloads a running desktop.

The native-feature CLI suite, executable discovery/argument/environment
comparisons, private proof and exact-parser tests pass. Disposable PTY tests
show guard death and early emergency recovery prevent graphics takeover and
invalid controls stop before TTY-mode queries or privileged handoff. The external
watchdog regression, lifecycle diagnostics, Hagia preflight refusal fixture and
terminal checks pass. Clippy, formatting and the layout gate pass. One new PTY
fixture initially allowed stdin EOF to hang up its shell before exec; retaining
the input pipe until exit fixes the harness and tests the intended refusal.
The complete native-family gate and source-bound final acceptance remain pending.

## Accepted completion

Signed candidate `dd2caf20b4aba35180613cd8b197b248c65fb9ed` passes all eight
device-hidden native-family phases. Sophia, Hagia `ad3a738d` and Narthex
`50b9014d` were clean and their identities were unchanged at completion. The
retained evidence directory is
`~/.local/state/sophia/development-evidence/t027-completion-dd2caf20`:
20 files, including the family report and each phase log, have verified SHA-256
checksums. Additional acceptance includes 83 CLI tests (four pre-existing
ignored cases), two disposable-PTY tests, 82 conformance tests, the discovery
and retained-vector comparisons, watchdog recovery, lifecycle diagnostics,
preflight refusal, Clippy, formatting and layout.

This closes t027's preparation/adapter boundary. The independent input guard,
watchdog, TTY/display-manager restoration and explicit physical-proof dispatch
remain deliberate adapter responsibilities. No installed-session or physical
acceptance is claimed. The family gate reports Lom content unavailable when no
Lom client is supplied; that optional branch does not satisfy or close t099.

After t230 landed as `79493df8`, the signed rebase retained identical production
preparation code (`1752d0f8`). Both completed-task rows were preserved. The
full CLI suite passed again: 85 tests, four pre-existing ignored cases; Clippy,
formatting and layout passed. A repeat of the new outer-adapter PTY test exposed
another harness dependency: the refusal can exit before its process-substitution
`tee` drains. Keeping stdin open alone was insufficient. Signed `680f0c09`
records the preparation invocation synchronously and asserts no TTY query or
privileged handoff occurred, without relying on terminal output. Both recovery
tests and three additional refusal repetitions pass. The rebased evidence and
empty production-preparation diff are separately checksummed at
`~/.local/state/sophia/development-evidence/t027-rebased-680f0c09`.

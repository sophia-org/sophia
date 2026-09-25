---
id: dvvnlqq2
date: 2026-09-24
kind: investigation
status: investigating
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

No launcher behavior changed in this audit. t027 remains open. Required checks
include the existing Hagia preflight fixture, terminal adapters, exact argument
acceptance, refusal before display-manager shutdown, and disposable-PTY recovery
fixtures. Preserve bounded child waits, guard death/recovery behavior, session
process-group shutdown, private evidence modes and bus ownership. A headless
run must not invoke real DRM, input acquisition or display-manager operations.

## Connections

The [Session source split](izw9opes-private-session-tests-can-move-without-widening-the-production-api.md)
keeps argument parsing and startup profile evidence behind their existing
private facade. That refactoring supplies a clearer owner for preparation but
does not itself migrate the launcher or satisfy t027.

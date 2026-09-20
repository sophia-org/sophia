# Development Tooling

**Role:** normative repository-tooling and production-boundary contract.

This document defines which layer owns developer convenience, deterministic
checks, conformance logic, production session behavior, and presentation. It
applies the architecture, style-guide, DRY, and data-oriented-design rules to
the repository itself.

## Dependency Direction

```text
human ──► just ──► cargo xtask ──► sophia-conformance / repository checks
CI ─────────────────► cargo xtask ──► sophia-conformance / repository checks

installed launcher ──► sophia CLI ──► sophia-session ──► runtime / Engine / backends
```

The arrows do not reverse:

- production crates, installed launchers, and repository scripts do not depend
  on `just`;
- production crates do not depend on `xtask` or `sophia-conformance`;
- `just` recipes contain aliases, defaults, and short human guidance, not
  workflow logic;
- shell scripts may remain as installed compatibility adapters or hardware
  takeover boundaries, but new deterministic orchestration belongs in Rust.

## Owners

| Layer | Owns | Must not own |
| --- | --- | --- |
| `justfile` | Optional memorable human aliases | Validation, parsing, archive schemas, production behavior |
| `xtask` | Canonical developer/CI command parsing, process orchestration, and presentation | Production session lifecycle or protocol authority |
| `sophia-conformance` | Typed profiles, evidence parsing, archive identity, and passive gate results | Installed runtime behavior or stdout/stderr |
| `sophia-session` | Production session lifecycle, supervision, recovery, and adapters around Engine | CLI presentation or development-only conformance policy |
| `sophia-cli` | Installed command selection and concrete stdout/stderr ownership | Session state machines or duplicate domain helpers |
| shell adapters | Necessary OS/TTY/installed-format compatibility | A second implementation of typed workflow logic |

`sophia-session` reports exact evidence through host-installed line callbacks.
The library never prints directly. The `sophia` binary installs stdout and
stderr callbacks, preserving the existing evidence schema while keeping
presentation at the binary boundary.

## Canonical Commands

Use these from documentation, CI, and new scripts:

```sh
cargo xtask check
cargo xtask check layout
cargo xtask profile check
cargo xtask profile args --profile=standalone
cargo xtask conformance verify direct-scanout-standalone LOG
cargo xtask conformance verify direct-scanout-overlay LOG
cargo xtask conformance verify direct-scanout-cost LOG
cargo xtask conformance verify direct-scanout-cursor LOG
cargo xtask conformance verify direct-scanout-archive [RUN]
cargo xtask conformance run direct-scanout WIDTH HEIGHT HOLD WORKLOAD [PROOF]
cargo xtask conformance gate direct-scanout [PROOF]
cargo xtask conformance desktop-comparison install-reference XLIBRE_SOURCE PREFIX
cargo xtask conformance desktop-comparison prepare RUN
cargo xtask conformance desktop-comparison prepare-soak SOAK_RUN
cargo xtask conformance desktop-comparison gate RUN
cargo xtask conformance desktop-comparison status RUN
cargo xtask conformance desktop-comparison attest RUN SUPERVISOR_PID [CRTC]
cargo xtask conformance desktop-comparison preflight RUN
cargo xtask conformance desktop-comparison qualify RUN
cargo xtask conformance desktop-comparison capture RUN
cargo xtask conformance desktop-comparison finalize RUN
cargo xtask conformance desktop-comparison verify RUN
cargo xtask conformance desktop-comparison report RUN
sophia session run [OPTIONS]
sophia session input-guard [OPTIONS]
```

`session-args`, `check-profiles`, `verify direct-scanout`,
`sophia-live-session`, and `sophia-session-input-guard` remain compatibility
aliases. They are not the spelling for new code.

`PROOF` selects what a probe run exercises beyond ordinary direct scanout:
`--overlay-proof` opens an overlay over a directly scanned frame and proves the
return to composition, `--cost` measures direct against composed frames in one
session, `--cursor` sweeps the hardware cursor, and `--atomic-cursor` asserts
the default atomic path rather than selecting it. Each has a matching
`verify` spelling above.

The active development-session path is CP-14.3 in `todo.md`. Reuse the existing
`sophia session run` entry, installed launcher, and necessary TTY adapter, with
exact binary/profile identity and a known working fallback. The lifecycle fixes have passed deterministic verification; the
[two recovery canaries](native-recovery-canary.md) remain pending. Installed daily sessions now provide `sophia session mark`, `inspect`, `keep`,
and `list`. The [operator guide](operations.md#mark-and-investigate-a-problem)
defines their selection, retention, and disclosure rules. The installed launcher
uses the internal `session _supervise` adapter to bind the TTY wrapper's lifetime
to its record before takeover. This adapter is not a display-control endpoint.
The CLI owns the concrete output callbacks; Session owns bounded recording,
resource cadence, identity, retention, and incident operations. Component hashes
use a separate bounded worker, so reading an executable cannot hold up logs.
Production session events stay in `sophia-session`; developer evidence packaging
and validation stay in `sophia-conformance`/`xtask`. Expensive tracing and pixel
inspection remain opt-in.

Normal usage supplies workflow evidence alongside deterministic tests. A fix
requires the relevant regression and acceptance checks, not another comparison
campaign. The [development-session validation policy](validation.md#development-session-readiness)
defines evidence applicability and promotion. Historical scripts such as
`run_current_critical_path_tty4.sh` retain their existing proof workflows; their
names do not select the current roadmap task or make them prerequisites for use.

The desktop comparison is a deferred, incomplete diagnostic 36-sample matrix.
It resumes only for an explicitly selected stable candidate or named performance
investigation, and gates neither development-session use nor revised Milestone
14 closure. Its typed conformance owner still requires a clean signed candidate,
pins and hashes configuration, stack executables, and hardware/software
identities, rotates stack order across three 60-second repetitions, and owns
workload/process/resource lifetime. It replays kernel-DRM, visibility, and
workload populations and binds every sealed raw attempt by checksum. A separate
`prepare-soak` run contains one optional two-hour Sophia durability row; it does
not block verification or reporting of the interactive matrix.
`desktop-comparison gate` is the typed one-row entry point. Its shell adapter
owns only TTY3 checks, local compositor/X-server launch, bounded teardown, VT
recovery, and tracefs privilege; stack/workload choice, admission, sampling,
replay, and binding remain in Rust. The gate launches no operator application,
never contacts another host, and runs its controller outside the measured
supervisor tree. The first Sophia row runs a four-target physical cursor
qualification before measurement. Capture then stages the row; `finalize`
checks that the exact supervisor has exited before it records clean teardown,
replays, and seals the evidence. Sophia's direct-DRM path may stop and restore
the local display manager. The capture process is the workload's Linux child
subreaper: detached descendants remain measurable and teardown-owned even
after they leave their launch ancestry or process group. The prepared manifest
also binds the canonical
cursor digest and repository-owned Sophia core configuration. The gate
materializes those Engine pixels as an owner-only standard Xcursor theme for
niri, selects XLibre's matching core `left_ptr`, and refuses a Sophia session
that does not attest the same configured asset. Personal cursor configuration
therefore cannot change a comparison row.

`just --list` exposes the small human-facing subset. CI and scripts invoke
`cargo xtask` directly so correctness never depends on a convenience runner.
The TTY development launcher roots its standalone profile-check fallback at the
workspace manifest instead of depending on the caller's directory; a parent
gate passes down its already-running absolute xtask executable. Installed
sessions invoke `sophia` directly.

## Build Directory Isolation

Give each checkout its own Cargo target directory. Do not point a temporary
worktree at another checkout's `target`, through either `CARGO_TARGET_DIR` or a
symlink. Tooling and fixtures embed `CARGO_MANIFEST_DIR`; a reused artifact can
retain the other checkout's path and make `cargo xtask check` inspect that tree
instead of the one from which it was invoked. Keep Cargo's registry cache
shared, but keep workspace build artifacts separate.

If a check reports repository paths from another checkout, correct the target
directory first, then clear the affected workspace package artifacts and rebuild
from the intended checkout. Do not hide the mismatch with sibling-repository
overrides: those overrides can make the wrong checkout's check pass.

## Check Contract

`cargo xtask check` is the canonical offline, non-hardware repository gate. It
runs formatting, diff hygiene, offline metadata, workspace tests, workspace
Clippy, typed profile validation, the exact source-layout debt check, the
evidence-reader schema guard, promoted-archive re-verification, and the active
verifier mutation suites.

`tools/check_live_record_schema_readers.sh` refuses a reader that can match only
schemas older than the one its emitter writes. A record that gains a field and
leaves its readers behind fails nothing on its own: the reader finds no line and
skips the rule it owned, so the run passes with fewer assertions than it appears
to. The guard names its records explicitly, because a record name does not
identify a message -- `sophia_live_wm` writes one schema for `status=ready` and
another for `status=session_action_committed` -- so guarding a record means
having checked that its emitters agree.

One step in the graph needs real hardware and is reported rather than skipped.
`tools/check_buffer_age_equivalence.sh` proves a damage-limited render
byte-identical to a full one on this host's GPU, through a render node only. It
exits 2 where no render node is writable, which the gate reports by name: a
question that was never asked is neither a pass nor a failure, and treating it
as either is how an unreferenced proof rots.

`cargo xtask check layout` compares normalized audit identities with
`docs/source-layout-debt.txt`. That file is not an exception list: every entry
still fails `tools/audit_source_layout.sh`. Exact identities prevent a new
violation from hiding behind an unchanged numeric count and make retirement
visible as a reviewed path change.

A size row also carries the count it was admitted at, and growing past it
fails the gate naming the path, the count and the ceiling together. Shrinking
is free, so the number is a cap and not a measurement. The gate additionally
refuses any `error:` line its normalizer cannot read, because the audit's own
exit status is not decisive here -- it is non-zero whenever any debt stands --
so an unrecognised failure would otherwise be dropped rather than enforced.

Hardware gates remain explicit because they require a real TTY, DRM ownership,
and operator authorization. Their argument parsing, evidence verification, and
archive logic belong in `sophia-conformance`; the minimal TTY takeover adapter
remains transitional shell until production session startup owns that boundary.

## Definition Of Done

A tooling or infrastructure change is complete only when:

- there is one canonical implementation of each parser, schema, verifier, and
  archive operation;
- reusable logic returns typed data or errors and does not print;
- the binary layer owns presentation;
- tests live with the crate that owns the behavior and outside production
  source where visibility permits;
- installed artifacts do not acquire development-only dependencies;
- compatibility aliases delegate to the canonical path;
- the offline check graph and relevant mutation suites pass;
- architecture, the active roadmap, and the dated research log agree.

Current admitted debt is enumerated exactly in `docs/source-layout-debt.txt`.
The next infrastructure retirement slice moves the remaining session test
modules out of `src`, splits the named oversized cohesive units without
changing authority, and replaces the transitional TTY launcher with a minimal
OS adapter around the production session entry point.

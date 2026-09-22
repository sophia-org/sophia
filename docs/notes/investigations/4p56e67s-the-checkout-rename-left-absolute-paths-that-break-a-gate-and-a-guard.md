---
id: 4p56e67s
date: 2026-09-22
kind: investigation
status: investigating
tags: [investigation, tooling, paths]
---
# The checkout rename left absolute paths that break a gate and a guard

## Question

The checkout moved from `~/dev/sophia-stack` to `~/dev/sophia`. Which tools
still name the old path, which of them fail because of it, and which references
are historical evidence that must not be rewritten?

## Evidence

Found while running the X11 conformance gate for
[t057](wzxlxbok-independent-x11-socket-conformance-exposes-missing-client-completions.md),
which died in its regression phase before building its host. That one is
repaired in `564b56c0`; the remainder are recorded here.

Functional, in order of consequence:

| site | consequence |
| --- | --- |
| `tools/run_current_lom_panel_gate_tty4.sh:45` | a fail-closed sibling guard compares `realpath "$PROVLITA_SOURCE/../sophia-stack"` with `$ROOT_DIR`; the sibling is gone, so dock-mode runs exit 2 with "Dock sibling dependencies do not match selected sources" |
| `crates/xtask/tests/dock_launcher.rs:22` | builds its temporary fixture as `directory.join("sophia-stack")`, mirroring that guard; repairing the script without this moves the test out of sync with what it pins |
| `tools/probes/t082_pinentry/capture.sh:16` | exports `T082_BUNDLE` to an absolute path under the old checkout; belongs to t082 |

Documentation a reader would copy and run: `docs/live-session-bootstrap.md:79`,
`docs/quickshell-x11-panel.md:22`, `docs/validation.md:197`, and a comment in
`tools/run_native_output_gate_tty4.sh:7`.

Deliberately excluded. `docs/notes/investigations/kwhei4x4-preflight-setup-disconnect-precedes-an-authority-exit.md:42`
records that a run was "Executed once from `/home/niltempus/dev/sophia-stack`":
that is a true statement about where a measurement happened, and rewriting it
would falsify retained evidence. `docs/research-log.md` and the 2026-08 source
note refer to the `sophia-stack-project` GitHub organization and the
`sophia-stack.org` domain, which the rename did not touch.

## Finding and resolution

An absolute path to one developer's tree is not a location, it is an
assumption about the machine. Each site should derive its path from the
checkout, as `HERE.parents[2]` and `CARGO_MANIFEST_DIR` already do elsewhere,
or take it from configuration. The sibling guard is the interesting case: it
must keep refusing a checkout that is not the selected source, so it wants a
repair that preserves the refusal rather than one that relaxes the comparison
to make the current layout pass.

Not yet implemented; this note records the survey only.

## Validation and remaining work

Open as t151 in [todo.md](../../../todo.md). The dock-mode claim above is read
from the script, not yet reproduced by running the gate, and the exit path
should be confirmed before the repair. A repair wants the dock gate exercised
in both the matching and the deliberately mismatched arrangement, so the guard
is shown still to refuse.

## Connections

- [X11 conformance gate](wzxlxbok-independent-x11-socket-conformance-exposes-missing-client-completions.md)
  owns t057, where the first instance was found and repaired.
- [t082 pinentry](iux6ctsy-pinentry-submission-stalls-before-gui-exit-and-input-recovery-remains-blocked.md)
  owns its own probe bundle path; this survey does not claim its cause.

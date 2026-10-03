# Build and test coordination

Updated with operator approval on 2026-10-03. This policy supersedes the earlier
single build slot and blanket `nice 19` / two-job rules in planning notes.

## Ordinary development

Builds, correctness tests, Clippy and repository gates may run concurrently
across lanes. They do not need a coordinator to allocate a slot.

- Use normal process priority. On the current 16-core / 32-thread machine,
  start with `CARGO_BUILD_JOBS=8` and `RUST_TEST_THREADS=8` per active lane.
  These are adjustable starting points: use available CPU capacity while
  watching aggregate memory use, disk space, I/O and desktop responsiveness.
- Use isolated worktrees and a separate, reusable target directory per lane.
  Reuse existing targets; do not copy a complete Rust target for each baseline
  or mutant. Preserve evidence before removing obsolete task-owned artifacts.
- Keep ordinary tests isolated from live devices and session sockets. Parallel
  execution does not grant access to the installed session or physical devices.
- Share failures and their exact commands. A timing-sensitive failure requires
  investigation; extra host load alone does not establish its cause.

## Work that needs coordination

Arrange a quiet window for CPU, latency, frame-pacing or other performance
measurements that require controlled host load. State the workload, duration,
participating jobs and end of the window. Matched-load investigations may run
concurrently when that load is part of the recorded experiment.

Coordinate exclusive hardware access and other genuinely shared mutable
resources. Reserve the resource and time needed by that test; unrelated work
can continue when it cannot affect the result or resource ownership.

Source ownership, signed integration and merge-order coordination still apply.
They do not require serial compilation or serial correctness gates.

## Evidence

Record the source identity, command, features, job and test-thread settings,
exit status and relevant load conditions. Preserve failed attempts.

Raw Rust summary totals can include subprocess and re-exec summaries. They are
descriptive counts, not a stable identity check. When exact test coverage must
match across runs, compare test-name sets per binary and record skips and
filters; do not infer equality from the summed pass count alone.

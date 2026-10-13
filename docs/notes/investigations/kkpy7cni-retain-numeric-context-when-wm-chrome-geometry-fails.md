---
id: kkpy7cni
date: 2026-10-13
kind: investigation
status: implemented
tags: [investigation, session, diagnostics, wm]
---
# Retain numeric context when WM chrome geometry fails

## Observation and limit

A daily session at `99860d58f911af381233edf336b953c8b81364b4`
ended with `CoordinateOverflow` during window management. Its retained
classification was `unclassified`. The failure string also included accumulated
X request refusals, which were not identified as nonfatal compatibility history.
The private incident snapshot is preserved outside the repository.

The log did not retain the failed conversion or its numeric inputs. It cannot
establish whether the overflow came from constraints, placement, or output bounds.
In particular, an `i32::MAX` maximum content size plus nonzero chrome clearance
reproduces an overflow in the snapshot path, but that is a deterministic example,
not evidence that the application in the incident supplied that value.

## Diagnostic change

Session wraps errors at the content/chrome conversion boundary with a typed
context and retains the original `ChromeLayoutError` as its source. The
`sophia_wm_geometry_failure schema=1` record carries:

- operation and origin: authority snapshot constraints or geometry, output
  allocation bounds, WM proposal content conversion, or reconciled outer geometry;
- opaque surface identity and generation, and output (`0` when unknown);
- chrome clearance, input rectangle, optional minimum/maximum sizes, and optional
  requested size (`none` for absent values).

Failure codes are `chrome_coordinate_overflow`, `chrome_allocation_too_small`,
and `chrome_invalid_clearance`. The daily-capture reducer admits only these
operations, origins, codes and numeric fields. Titles, classes, namespaces,
process identities and application payloads are not included. The fatal summary
keeps the cause first and labels accumulated X errors
`nonfatal_x_request_refusals`.

This is diagnostic work: it preserves conversion failures and cleanup behavior.
It does not clamp geometry, change constraint semantics, or repair the incident.

## Validation and remaining work

On base `930b7e02b`, four headless tests exercise the actual Session snapshot and
proposal conversions, error classification and retained-record filtering. They
pass, as does the existing test for attaching X refusal history to a fatal cause.
Strict native-session library Clippy and workspace formatting pass. The full
device-hidden `cargo xtask check` passed in
`development-evidence/wm-geometry-diagnostics-01/xtask-05.log` (exit 0),
including all four new tests. Default-feature library Clippy also passes with
warnings denied. The final source hashes match `candidate-02.json`; the package
records the test-binary path and hash without copying build output. Its
self-excluding `SHA256SUMS` verifies and has SHA-256
`8720b147844aafe0c42242202226329d4546db31f610224b183def0d9e0a66f7`.

The first broader attempt stopped because the isolation wrapper omitted
`XDG_RUNTIME_DIR`. The second stopped at a conformance test's overlong Unix
socket path under the worktree's build slot. Both failed runs remain recorded;
neither required a production change. The third supplies a private runtime
directory and binds the same leased slot root at a shorter path, with no copied
build output or second target directory. That run passed the workspace tests,
then stopped in the separate SDK build: the host sccache process could not see
the mount alias. The fourth disabled that wrapper but reused a conformance test
binary with the old absolute temporary path embedded. The fifth rebuilt that
package under the alias, passed its targeted tests, and completed the full gate.
The record vocabulary and failure classification are shared with non-native
builds so offline consumers retain the same diagnostic fields.

The new diagnostics have not been installed or physically accepted. The next
incident step is to reproduce the application's geometry inputs in isolation,
then fix the responsible boundary with a discriminating regression. Do not infer
an application-specific cause from the synthetic overflow test.

## Connections

- [Daily diagnostics](e84g9ivq-durable-daily-session-diagnostics-and-incident-markers.md)
  owns bounded, private retained evidence.
- [Missing diagnostic causes](tnf5xqrb-vt-handoff-failure-exposed-missing-diagnostic-causes.md)
  explains why typed failure classification must survive daily reduction.

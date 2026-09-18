# Application stderr needs a private bounded owner

Date: 2026-09-18
Tags: diagnostics, applications, privacy, daily-session

Related: [daily session records](e84g9ivq-durable-daily-session-diagnostics-and-incident-markers.md),
[component launch reload](c9di7qpg-independent-shell-providers-were-omitted-from-desktop-launch-reload-validation.md),
[daily shell acceptance](../plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md#t101).

## Observed gap

The installed daily session on `d444eba2` launched Brave through the configured
application path, but its inherited stderr was `/dev/null`. The structured
recorder intentionally filters arbitrary payloads. Consequently there was no
browser error text to inspect when the operator reported a slow Super+B launch.
The observed high host load was concurrent evidence, not a demonstrated cause.
Old discarded output cannot be reconstructed.

## Implementation boundary

The operator approved private, bounded stderr capture enabled by default for
recorded daily sessions. Session's common application spawn attachment records
the request before execution and keeps one initiating launch identity across
inherited stderr. Startup, WM actions and catalog execution use that attachment.
The original owners still reap children; the recorder never waits for them or
changes admission, launch placement, retry or termination semantics.

One fair pipe collector and one storage worker keep disk operations off the
compositor loop. Binary framing preserves non-UTF-8 bytes and stream offsets.
Raw records and executable identity remain in private application files; the
structured recorder retains only bounded lifecycle/status fields. Normal
inspection/preservation does not include private records. Explicit preservation
copies and hashes the original bytes.

Registration, queue, per-launch and rolling-store bounds are documented in
[Operations](../../operations.md). Recording refusal does not refuse the app.
Metadata shares the rolling store, so rotation or saturation can remove the
ability to identify an old chunk. Queued bytes are not persisted bytes. Final
metadata loss, descendant-held pipes and storage trouble are reported with their
actual scope. No claim is made that an application becoming alive means it
opened a window or finished initialization.

## Verification scope

The new tests use harmless local processes and private temporary directories.
They exercise binary output, exit/signal/spawn failure, independent sources,
flood draining, a progressing peer, disable/re-enable, bounded registration,
blocked storage, descendant-held shutdown, unsafe paths and exact opt-in export.
The catalog test calls the production catalog spawn path with a nonexistent
display endpoint and a shell command that never connects to it. This is not a
browser, native session or physical acceptance run.

Scoped device-hidden checks passed: 488 Session library tests (17 ignored),
11 application-capture controls, 28 diagnostic controls, 21 configuration
controls, and four CLI diagnostic tests. Strict affected library/binary/test
Clippy and the layout gate passed. Two independently compiled mutations failed
their intended behavioral assertions: raising the per-launch capture limit and
including private records in ordinary preservation. The disposable source was
restored and its capture suite rerun. These are process/storage and presentation
tests, not browser startup or graphical-session tests.

Logs are retained under `.artifacts/application-diagnostics-*.log` and
`.artifacts/application-diagnostics-review/`. The signed source identity and
subsequent canonical result belong in the completion record beside those logs;
the scoped checks above do not substitute for that whole-repository result.
No running desktop has been restarted or new release installed for this work.
The requested Kitty/Brave scroller-strip investigation follows this slice.

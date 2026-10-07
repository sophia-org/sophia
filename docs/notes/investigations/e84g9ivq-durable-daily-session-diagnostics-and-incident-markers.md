---
id: e84g9ivq
date: 2026-09-06
kind: investigation
status: closed
tags: [investigation, diagnostics, cp14-3]
---
# Durable daily-session diagnostics and incident markers

## Question

Can a daily-session problem be investigated after a crash and another login,
without reproducing it merely to recover logs? This implements the selected
[t015/t016 scope](../plans/queue-05-3-make-failures-diagnosable.md).

## Evidence

Implementation candidate: working-tree changes based on `9b48672f5d25`.
No installed release or running graphical session was changed for this work.

The installed wrapper reserved immutable attempts before takeover, but copied
session logs only on return. Its active log paths retained one previous
generation. Killing the wrapper could leave a pending attempt without the
session's latest evidence. The resource sampler already emitted observations
every five seconds, but stopped after 1,560 readings. Packaged identity capture
also described packaged Hagia even when a mutable policy executable was chosen.

## Finding and resolution

The persistence gap belonged to Session and the installed launcher. Ordinary
Hagia launches now enter the Rust diagnostic supervisor before the TTY wrapper.
It reserves a private record and binds the live wrapper using boot identity,
process identity, and start ticks. The wrapper writes lifecycle/guard/recovery
records in that directory. The CLI's existing Session output callbacks feed a
bounded asynchronous recorder; mixed application stdout/stderr is not archived.

Daily resource sampling continues with constant schedule state. The rolling
store bounds retained event data and preserves identity/marker journals
separately. Explicit proof sampling and immutable proof archives keep their
existing semantics. Ordinary outcomes describe process lifetime, not proof
acceptance. Legacy archives remain available by explicit path.

WM and native-shell startup/restart hooks pin the executed peer inode and queue
its digest work separately from event recording. Profile load, applied core
configuration, and WM profile activation retain their actual generations and
digests. Component-private configuration is explicitly unobserved. A pending
hash remains pending if the session ends before hashing finishes.

The first stress test exposed executable hashing in the recording worker: a
large binary held up unrelated events. Hashing now has a separate bounded
worker, while identity records have a separate bounded priority queue. Neither
worker performs display policy, protocol dispatch, or rendering.

The records that say why a session ended, `sophia_session_failure`,
`sophia_session_result` and `sophia_session_panic`, share that priority queue
(t309). They are written as the owner returns, often just after a burst of
ordinary records, and a full ordinary queue would otherwise refuse them. The
writer reads the stop request before it looks at either queue, so a record sent
before the stop is always written. A failure that leaves the session before
the owner loop starts, or after the loop has finished cleanly, is recorded with
phase `startup` or `cleanup`; the loop records its own failures. An output
profile that the hardware refuses keeps only its kind, such as
`output_profile_unknown_connector`, and never the connector name.

The installed `mark`, `inspect`, `keep`, and `list` commands access private host
records directly. A stopped desktop need not answer a request. Markers carry
both clocks; post-exit reports use the final retained event window rather than
inventing a crash time. Preserved snapshots have independent copies and
checksums, and automatic pruning does not remove them.

## Validation and remaining work

Deterministic coverage includes live/dead/stopped owners, PID reuse, competing
launches, concurrent markers, wall-clock correction, post-exit report windows,
unsafe links/permissions, automatic pruning, preserved snapshots, event
rotation, bounded queue loss, retained profile generations, and an eight-hour
sampling schedule with the original finite proof limit. CLI subprocess tests
cover abnormal wrapper exits and storage failure without changing exit status.

`cargo xtask check` passed outside the sandbox. Its first sandboxed run stopped
at an existing Unix-socket integration test with `Operation not permitted`;
allowing local socket creation resolved that environmental restriction. The
check transcript is `/tmp/sophia-t015-t016-check.log`. Formatting, native-session
compilation, Bash syntax, and whitespace checks also passed. The final `cargo xtask check` run also exited successfully after the closing
changes, including all thirteen new integration checks. Offline Cargo metadata,
task-ID uniqueness, and local documentation links passed.

At implementation handoff, physical acceptance still required a replacement installed session: mark from another
TTY, log out, log in again, inspect the earlier record, and keep it. The task
rows retain these exits. No comparison matrix or live-session restart was run.
Periodic synchronization can lose the newest tail during abrupt power loss;
recorded loss, stale health, missing hashes, and rotated intervals remain
visible limitations, never passing evidence.

## Connections

- [Operator contract](../../operations.md#mark-and-investigate-a-problem): commands, retention, privacy, and recovery.
- [Tooling ownership](../../development-tooling.md): Session owns persistence; CLI owns presentation; conformance owns proof verification.
- [Selected milestone plan](../plans/queue-05-3-make-failures-diagnosable.md): acceptance criteria and task links.

## Installed-session observation

The operator installed `c01663aaea09be57fd8fba7c5c050da3f818bbcd` and reported
successful login. Read-only inspection found ordinary Hagia session
`00000001788743375093-ef5c2c4c-345b-4afc-a4f0-d816f34de8d6` running with its
recorder active. At sequence 1307, health reported zero discarded records,
zero rotated bytes, and zero storage errors. The identity journal retained
core/desktop profile digests and completed WM/native-shell executable hashes.
These are observations of capture in the installed session; the independent-TTY
marker and retrieval after another login still need operator acceptance.

The subsequent switch to tty3 ended the session. The
[VT incident investigation](tnf5xqrb-vt-handoff-failure-exposed-missing-diagnostic-causes.md)
records the preserved evidence, missing installed CLI command, and diagnostic
fields lost by reduction. Physical acceptance was still open at that point.

## Post-login acceptance

The operator completed logout and a subsequent login on installed `4b4f2841`.
The [acceptance record](../milestones/v4ycp9ba-daily-session-diagnostics-accepted-across-logout-and-login.md)
records successful inspection and checksum verification of the earlier marker,
identity, and preserved evidence. This satisfies t015/t016. The prior session's
status-1 logout remains a separately tracked session-health issue; this result
makes no clean-exit or broader daily-driver claim.

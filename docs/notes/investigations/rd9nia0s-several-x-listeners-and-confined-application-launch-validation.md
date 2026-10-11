---
id: rd9nia0s
date: 2026-10-10
kind: investigation
status: awaiting-physical-acceptance
tags: [investigation, session, x11, confinement]
---
# Several X listeners and confined application launch validation

## Question

Can t142 serve several configured X groups from one synchronous frontend while
preserving admission, bounded progress and the idle-window cleanup contract?
Can a Session launch expose just its selected group's socket directory?

## Candidate and evidence

Implementation worktree `sophia-t142-confined-groups` starts at `8db916f1c`.
The development runs below were bound to signed candidate `7960512ee`; its
acceptance results are recorded below. This is not an installed release.
Logs are retained under
`~/.local/state/sophia/development-evidence/t142-confined-groups-01`.
Cargo ran through the worktree's cargo-slot pool, under Bubblewrap with private
devices, runtime, temporary directory, network and process namespaces. No live
Session, device, installation or physical acceptance was exercised.

The runtime seam reuses the protected-role argument builder. Its exclusive
application role grants the group directory read-only at `/tmp/.X11-unix`
and only the cookie file at `/run/sophia-x/Xauthority`. The version probe and
final Bubblewrap command both clear their host environment and mark every
descriptor above stdio close-on-exec in the child. Existing role callers keep
their prior command policy. The returned command is not supervisor custody
evidence; its caller owns spawning and collection.

`runtime-seam-03.log`: six tests passed; the one ignored test is the child
probe invoked by the parent tests. Bubblewrap 0.13.0 executed. The child
connected to its own socket, could not reach the other host paths or a live
host abstract X socket, and could neither unlink the group socket nor write
the mounted directory or cookie. The inheritable descriptor control observes
it in an ordinary child, absent in the confined child, and unchanged in the
parent. A separate version-probe control observes the environment and
descriptor restrictions before the sandbox starts. `runtime-compatibility-01`
passed all three existing-role/exclusive-role controls. `runtime-SHA256SUMS`
closes these logs and their source/binary identity record.

Earlier runtime attempts are retained: `runtime-seam-01` failed on Unix socket
path length and probe-output parsing; `runtime-seam-02` failed on the parser
and the expected environment (Bubblewrap adds `PWD=/`). These were fixture
failures, not passing confinement evidence.

`listener-groups-01.log`: five wire tests passed. They exercise foreign
window/property/selection refusal through two listeners, complete portal
transfers for CLIPBOARD and PRIMARY, reserve shared capacity while confined
clients leave setup unfinished, and roll back partial binding without
unlinking an already-live listener.

`listener-idle-03.log`: the new two-listener control passed. A real departed
connection is not reclaimed while another listener retains a connection
worker; both idle permits production reclaim and custody retirement. The
private runner prepares one namespace, so both listeners in this control use
that namespace. Distinct namespace admission/isolation belongs to the wire
tests above. The fixture invokes production owner operations directly, not
the whole service loop. The earlier attempts exposed a missing test-struct
field and an unsupported second private-runner namespace.

`x-authority-full-01.log`: the all-feature crate suite passed, 2,252 passed
and two explicitly ignored across 37 libtest summaries, including 554 wire
tests and the M3 acceptance module's cases. This is a crate regression run,
not the separate closed-inventory M3/M5 gate verdict. The ignores require a
Qt probe and private xterm fixture respectively. `x-authority-clippy-01.log`
passed all targets/features with warnings denied.

## Resource and identity choices

`session-confined-03.log`: eight selected all-feature Session tests passed;
two ignored child fixtures were invoked by their parent. The joined cookie
test runs the actual confined command and ordinary x11rb `DISPLAY`/`XAUTHORITY`
lookup against the two-listener frontend and `LiveXAdmissionPolicy`. Keeping
the same display and cookie but removing the sandbox hostname record causes
`SetupFailed`; restoring that record authenticates and answers GetInputFocus.
Worker shutdown leaves no admission. This closes a real hostname mismatch
between the host-only cookie file and Bubblewrap's `sophia-domain` UTS name.
No wildcard record is used.

The first Session invocation omitted `native-session`, selected zero tests
and supplies no coverage. The second found two existing fixture callers missing
the new confinement argument; those were updated before the successful run.
The runtime, compatibility and Session command-selection prerequisites now
require Bubblewrap explicitly rather than reporting a successful skip.

The combined gate uses a private writable Git metadata copy and private device,
runtime and temporary directories. `xtask-01` stopped in the existing
`shell_content_files` fixture: its socket below this worktree's long cargo-slot
path exceeded `sun_path`. `xtask-02` failed before running Cargo because its
proposed mountpoint was beneath the read-only root. `xtask-03` used a short
`/tmp/t` alias for the same leased slot but reused binaries containing the old
`CARGO_TARGET_TMPDIR`, which was read-only in that namespace. The next invocation
cleans workspace artifacts under the lease and rebuilds at the short alias,
without new target storage or a product/fixture source change.

`xtask-04` completed workspace tests (7,558 passing results and 104 ignored
across 540 libtest summaries, including nested child output), then Clippy
refused a complex tuple return type in the new cookie fixture. A named test
type alias fixes that warning. `xtask-05` passed the complete repository gate:
workspace tests, Clippy, layout, shell/SDK checks and archive verification.
Device-dependent pixel checks reported no available device, not a hardware
pass. `candidate-source-05.json` captures the complete modified and untracked
crate/Cargo file set used by that run.

## Acceptance harness storage

The M3/M5 worker formerly copied its test executable into evidence. To obey
the host's cargo-slot storage rule, these gates now execute the Cargo artifact
in place and record its target-relative path and SHA-256. Both worker and host
reject paths outside the target, leaf symlinks and escaping parent symlinks.
The host verifies the hash after contained execution; this adds host binary
revalidation to M3 and preserves M5's identity check. Later slot reuse may
replace the artifact, so its recorded hash is the durable identity.

`harness-target-01` passed 58 selected acceptance-harness tests, including the
new path/identity refusals and unchanged M4 identity controls. Four child
fixtures remain ignored by ordinary libtest. Harness Clippy and layout passed.
`xtask-06` passed the full combined gate after this tooling delta, with its
complete crate/Cargo source set in `candidate-source-06.json`. M4 retains its older
three-binary evidence-copy behavior and is not run here; it needs a storage
repair before any future invocation under the host rule.

The disposable acceptance adapter mounts the leased cargo-slot root at the
gate's required `source-<content hash>` leaf; it creates no target subdirectory
inside the slot. An atomically written source marker precedes every archived
source build, and a different marker refuses reuse. The target parent is
non-writable while its existing lock remains writable: a miscomputed content
hash cannot create another build directory. The archive environment and umask
match the gate's content-identity calculation. Devices and live display/runtime
endpoints remain hidden.

One thread accepts from permanently nonblocking listeners in rotating order.
The server assigns listener provenance to admission; the peer cannot supply
it. The Session policy maps configured listeners to registry namespaces and
refuses unknown listeners. All connection workers remain in one owner table.

The configured global worker limit is divided into fixed listener shares;
the primary gets the remainder. Unfinished setup counts. Full listeners are
excluded from both acceptance and the service's readable-descriptor wait,
so their backlogs do not cause a busy loop. Nonblocking capacity refusal
returns no accepted client rather than ending service. The default global
limit stays 16: with eight confined groups plus primary, each confined group
gets one slot and primary gets eight. Capacity does not grow implicitly.

Additional sockets bind exclusively and their paths are removed with their
owner only while device/inode identity still matches. Partial construction
rolls back newly bound paths; the primary retains its existing caller-owned
cleanup after successful construction.

Registered application mappings are checked before startup and consulted at
catalog, shortcut and secondary startup dispatch. Mapped failures have no host
fallback. The primary startup app and catalog terminal are refused mappings.
Scanned desktop entries remain host launches even for the same executable.
The catalog and its terminal are fixed for the session, so reload cannot change
that startup decision. A replacement registry command retains its ID's mapping.
Session terminates the Bubblewrap monitor; teardown kills the sandbox rather
than delivering a graceful signal to the application's separate session.

## Limits

t142 remains open for physical acceptance. Physical
acceptance requires an installed classic/confined pair and portal transfer;
headless results do not replace that exit. The initial sandbox denies network
and does not provide GPU devices, host home or host runtime services.

## Signed candidate and deterministic result

Signed `c84c1eb6b` contains the acceptance-harness storage repair; signed
`7960512ee0dfc0887a5730b1fa4d3c944574cd86` adds t142. The complete 57-file
crate/Cargo delta from `8db916f1c` equals the dynamically captured
`candidate-source-06.json` set, and each committed file hash equals the tested
bytes. `xtask-06` exited zero: 7,558 passing results and 104 ignored across
540 libtest summaries, including nested child output. This binds the gate
to the signed code without treating a curated file list as a complete tree.

The clean candidate's source content hash is
`ba8c7485f09fb6fa9372fb35a2afbda379ec46e9fd7caf5791d8fc569e26048c`.
Under Sophia's `.artifacts/t142-acceptance-<that hash>/`:

- `m3-self-01`: contained harness self-tests passed; acceptance is correctly
  labelled NOT_RUN in this separate report.
- `m3-01`: PASS, all 20 mandatory cases.
- `m5-01`: PASS, all eight mandatory groups.

Each package has a verified, self-excluding `SHA256SUMS`. The development
package's `result.json` records package paths, report/manifest hashes and the
actual Cargo artifact paths and hashes. Its own manifest SHA-256 is
`36adafd94193741d4f788a524ca5baa02b79ba26138c3e67ba4363419510d24b`.
No test executable was copied into evidence. The first adapter invocation
exited 126 before execution because the scratch script lacked its execute bit;
the retained second invocation used Bash and passed all three runs.

The follow-up documentation and task-context commit changes no crates or Cargo
files. Merge is permitted by the deterministic results; installation and the
attended browser/terminal, portal-paste and departure check remain separate.

## Connections

- [Socket directory and multiplexer plan](../plans/ooy00zjd-socket-directory-and-frontend-multiplexer-architecture.md)
  owns t142's exits, including physical acceptance.
- [Namespaces and portals](../../namespaces-and-portals.md) owns admission and
  path-exclusion contracts.
- [The fifth admission and idle reclaim](w0p6eocj-a-fifth-admission-ends-the-private-service-once-four-connections-have-departed.md)
  explains the single accepting owner's t138 premise.

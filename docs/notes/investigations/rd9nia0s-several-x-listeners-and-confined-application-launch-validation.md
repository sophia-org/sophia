---
id: rd9nia0s
date: 2026-10-10
kind: investigation
status: investigating
tags: [investigation, session, x11, confinement]
---
# Several X listeners and confined application launch validation

## Live portal prerequisite discovered during acceptance preparation

On 2026-10-10, preparing the installed-session browser/terminal check exposed
a missing production join. Session had no consumer of X authority's
`portal_commands` and did not construct its `clipboard_executor`. The
two-listener CLIPBOARD/PRIMARY tests construct that connection themselves,
with a headless Allow broker and supplied participation permissions. Their
passing results prove the frontend/executor path, not live Session wiring.
Configured confined namespaces had `NamespaceCapabilities::NONE`.
No restart or confined-browser trial was performed after this discovery.

niltempus approved bringing the t315 portal-files work forward before this
physical gate. Implement the shared-core 9P export and the live Session join,
derive participation from admitted namespace authority, and keep policy Deny
unless explicitly configured. Do not connect the legacy envelope broker or
silently supply the test's Allow permissions to live groups. Preserve the full
t142 browser/terminal/departure exit; implementation prerequisites remain in
addition to attended acceptance. The existing installed session is unchanged.

## Portal file preparation

The t315 preparation is isolated in `sophia-t315-live-portal` from master
`356d2e366`. It adds the KDL-bound portal codec and a draft shared-core export,
not a live requester. Thirteen codec/schema tests and eleven direct export /
lifecycle tests pass, with focused Clippy clean. The export tests supply
admission and owner facts; they do not prove a protected transport, a real 9P
wire join, an independent client or live X delivery. The full repository gate
and t315's remaining conformance controls are not claimed. Development logs
are under `~/.local/state/sophia/development-evidence/t315-portal-files-dev01`.

Review identified and corrected connection-grant ownership, queued-payload
reclamation, owner-verified source generations and atomic decision custody.
The added controls distinguish End's queued work from work already taken by
the recipient. Exact submission replay ends at receipt acknowledgement;
history and Qids survive reconnect. A failure of reserved terminal credit
permanently fences the export instead of silently retaining active authority.

The [requester amendment](../decisions/88uev9ix-admit-the-in-process-x-portal-requester-by-session-channel-custody.md)
was accepted by niltempus on 2026-10-10: it adds private Session-channel custody
for the in-process X coordinator alongside protected requester admission. It
also permits the explicit one-way acceptance-login rule, confined group 7 to
shared, without a per-paste prompt. Neither has been enabled in production.
No new candidate is merged,
installed or physically accepted by these preparation checks.

## Private requester wire development

The next uncommitted slice distinguishes `ProtectedPeer` from
`SessionXAuthority` custody while requiring the exact admitted connection in
both modes. A real shared-core server and Rust SDK pipeline refuse another
socketpair created by the same process, then negotiate and carry request,
upload and terminal settlement over the admitted connection. This is an
executed same-kernel-peer refusal, not merely a direct export call.

The reusable `PortalFileClient` uses the existing Rust SDK pipeline and one
overall deadline. It validates receipt, request/grant and broker-generation
correlation, retains the upload writer through accepted End, and waits for
the recipient outcome rather than treating a write or Submitted as completion.
Wire tests cover 65,536-byte payloads, recipient success and failure, denied
uploads and single-use grants. The recipient in these tests records bytes and
supplies success/failure; it is not the X executor.

`t315-private-requester-dev01/tests-02.log` records 73 passing portal crate
tests, including 13 file/export tests; `clippy-02.log` is clean with warnings
denied. Earlier logs in that package precede the reusable client. The package
under `~/.local/state/sophia/development-evidence/` records the complete changed
file set and Cargo artifact hashes without copying binaries. These are focused
development results, not the repository gate or independent C conformance.

The configuration slice parses bounded directional rules and computes
participation bits, but namespace creation and the exact-pair live policy
remain unwired. Next are the production X coordinator, final-effect lock and
source checks, and real frontend CLIPBOARD/PRIMARY joins. No installed-session
capability or physical acceptance follows from this wire result.

## Directional configuration and X source authority

Signed `1c03ae883` records the portal foundation above. The next preparation
slice adds the explicit, bounded `--clipboard-rule=SOURCE:TARGET` parser and
directional participation helper. Five `x_group_config` tests pass with the
`native-session` feature. An initial invocation without that feature selected
zero tests and supplies no test evidence. The first feature-enabled offline
attempt stopped at an uncached dependency; `cargo fetch --locked` populated the
cache before the successful offline run, without changing the lockfile.

The X recipient now exposes a read-only lookup of current source generation
through the retained conversion. It checks the pair, target, byte size and
request generation, then resolves that conversion's exact selection owner.
All 66 `sophia-x-authority --test authority` tests pass, including forged
request refusals, independence from PRIMARY changes, absence after settlement,
and a changed CLIPBOARD owner returning a newer generation than the request.
Selection generations use a shared allocator: adding two PRIMARY changes means
the subsequent CLIPBOARD replacement is generation 4, not 2. The initial test
expected 2 and failed; that fixture expectation was corrected, not the owner.

This is source authority and configuration preparation. Namespace capability
wiring, exact-pair live policy, the worker coordinator and the final-effect
lock fence remain pending. Neither a repository gate nor an installed change
is claimed by these focused tests.

## Production files coordinator and execution fence

Signed `588e6723c` records configuration and source lookup. Signed
`51e96c81ae6658590961c7d6d0585ad248a4edca` connects Session's configured namespace participation and exact-pair
policy to the routed X frontend. Its off-owner coordinator uses the Rust SDK
over private socketpairs served by the production 9P core. One persistent
export retains history across per-transfer connections; no public listener,
endpoint environment variable or requester-supplied permission bits are added.

The X recipient performs the property effect. A shared execution fence
serializes that effect with Session's lock transition; lock, unlock and shutdown
invalidate earlier work. The export also pins this revision so a short
lock/unlock interval between polls cannot restore a queued payload. Source
generation is resolved through the retained X conversion and checked again by
the X executor. Notification routing uses the existing contained-delivery owner
to end a stalled recipient, while settlement reports whether the property
effect actually occurred.

Eight real frontend tests pass in `t315-live-join-dev01/focused-02.log` and
`xtask-05.log`: repeated CLIPBOARD and PRIMARY delivery, TARGETS translation
without source-byte retrieval, policy denial, missing participation, lock then
unlock, replaced source, and a late source. The earlier `x-files.log` has seven
tests, before TARGETS was added. They run the production routed
frontend, file service, Rust SDK and X executor. Namespace policy and source
bytes are supplied by the fixture; a test consumer drains Session transaction
batches. The lock case drives the fence directly, not an attended Session lock.
Two fence tests cover permanent closure, deadline/revision refusal and a lock
transition waiting for an already executing effect. A direct export test
covers revision change while a complete payload is queued.

The first isolated repository run stopped at the existing
`normal_session_lifecycle` test with `RuntimeDirUnset`: the wrapper had removed
`XDG_RUNTIME_DIR`. The rerun supplies a private owner-only directory under its
private `/run`. Both runs hide host devices, `/tmp`, display environment and
runtime sockets. This was a gate setup correction, with no production or
existing-test change. A second run exposed a pre-existing shell-conformance
socket path longer than `SUN_LEN` in this worktree's cargo slot. A short mount
attempt first failed to create its mountpoint and then exposed cached binaries'
original target path as read-only; neither attempt is accepted evidence.
Signed `f04b20481` shortens only the shell test's scratch prefix. Its four tests
pass. The final wrapper exposes both names of the same leased slot, with no
additional target storage or binary copies.

The full isolated `cargo xtask check` exits zero in `xtask-05.log`: 544 Rust
test summaries report 7,599 passes, zero failures and 104 ignored tests. Clippy,
SDK, layout and the remaining deterministic checks pass. Device-backed pixel
proofs are explicitly not proved by this device-hidden run. All 26 changed or
untracked candidate files, enumerated dynamically before the gate, match their
recorded hashes afterward. The complete 22-file crates delta from `f04b20481`
to `51e96c81a` matches those pre-gate hashes; this binds the run to the signed
implementation. Subsequent changes are documentation only.

The development package is
`~/.local/state/sophia/development-evidence/t315-live-join-dev01`.
Its self-excluding `SHA256SUMS` has SHA-256
`23319a5250797ca904f409b72018f9c944a26130e02f4ad15667d222f3c70638`.
`result.json` records the source binding, counts, seven binary path/hash
identities and limits; `gate-environment.txt` records the setup failures.
Slot files may change with later builds;
the retained hashes, not continued file availability, are the evidence.

This slice does not close t315 or t142. Independent C portal interoperability,
the device-free 9P recipient bind/revoke seam, remaining migration controls and
attended browser/terminal/departure acceptance remain separate. The existing
socket conformance path is retained. No installed session was changed.

## Independent C portal interoperability

Signed `b6ba4c83f6ec785edbeeb10928fb5f9bafc14f67` adds an independent
C record encoder and reader using only the generic pinned C SDK's 9P pipeline.
The production shared server and portal export serve it. Admission, namespace
rights and current source generation are supplied; actual kernel credentials
are checked, but this fixture does not prove protected launch custody.
The recipient records bytes and reports its actual effect result.

Three tests pass: transfer custody and outcome, refusal with and without the
final ACK, and separately compiled peer controls. Reads fragment events into
13-byte chunks. The transfer case refuses malformed, truncated, stale and
foreign records, denies an old source generation without retaining its identity,
retries an incomplete upload, and distinguishes Submitted from Completed.
Acknowledged replay and cancellation after settlement are refused. The controls
send a stale negotiation epoch or corrupt the payload; neither can report
success. These mutate peer inputs, not production implementation code.
The withheld final ACK proves bounded revocation without attributing it to one
of the overlapping refusal and ACK-progress deadlines.

The full device-hidden `cargo xtask check` exits zero in
`t315-c-portal-dev01/xtask-01.log`: 546 Rust test summaries report 7,603 passes,
zero failures and 104 ignored tests. The complete five-file delta from
`f0396d3f8` matches the dynamically enumerated pre-gate candidate hashes and
the signed commit. The final focused run passes all three tests and records
the C executable paths and hashes. The first focused run failed because the
fixture offered an msize below the C SDK's minimum; the correction changed
only the fixture. No binaries were copied into evidence.

The package is
`~/.local/state/sophia/development-evidence/t315-c-portal-dev01`.
Its verified self-excluding `SHA256SUMS` has SHA-256
`3838264c9ca0c90186c36a7a8065a57683c57a6cf95f66e23d509e66a30fd372`.
The isolated wrapper uses private devices, PID/network namespaces and runtime
directories, with display variables removed and both target aliases bound to
the same leased cargo slot. This adds no recipient bind, physical acceptance
or live-session claim; t315 and t142 remain open.

## Recipient bind seam and serial requester retirement

Signed `f4bd8d106ae00df3f254270d6926a729ad5bddc3` adds four device-free
recipient tests. The recipient owner, admission and object bytes are supplied;
the portal lifecycle, execution fence and 9P core run unchanged. Requests go
through the core's wire decoder and operation dispatch. The fixture proves
retained-fid revocation, metadata and unopened-walk refusal, fresh replacement
Qids, unaffected recipient status, and waiting reads that end with ESTALE before
sampling bytes. Source generation, deadline, source/target namespace departure,
broker restart, lock and lock/unlock each invalidate the retained grant.
This is the generic seam required by t315, not a production application
frontend, namespace compiler or portal-files-to-recipient handoff.

The core control bypasses `Export::check` only at `read_now`. It fails first at
`revoked bound fid must return ESTALE` and, in a separate exact test invocation,
`waiting bound read must end with ESTALE`. Both exit 101 with one failed test.
The control holds a worktree mutation lock, preserves both fixture hashes,
restores the core byte-for-byte and checks the restoration against HEAD.
The subsequent repository gate holds the same lock. Control evidence is in
`~/.local/state/sophia/development-evidence/t315-recipient-bind-control01`;
its verified manifest SHA-256 is
`1072e8fef04a23ec40657fe3f803c24e77e56bba12e27c3d23109a1cb418c8b7`.

The first full gate stopped on the existing PRIMARY files test waiting four
seconds for an X record. A diagnostic run of all 562 X wire tests passed, as
did 40 focused PRIMARY repetitions. The exact ordering of the original timeout
was not captured. Source review exposed a separate reproducible admission race:
a successor connect request can arrive after the old client closes but before
the service observes EOF, producing EBUSY. A direct ordering test failed the
old service at `next private requester must wait for previous transport
retirement`.

Signed `3ecbc41dd` repairs that race. The service leaves the successor in its
existing one-slot queue until the old transport retires, then rechecks its
deadline and authority before admission. An expired queued request allocates
no socket or epoch. Both service tests and all four bind tests pass. This
establishes the repair independently; it does not prove that this race caused
the original X timeout. The initial new service test also required a scoped
closure-capture compilation correction, retained in the diagnostic log.

The final isolated `cargo xtask check` exits zero in `xtask-02.log`: 548 Rust
test summaries report 7,609 passes, zero failures and 104 ignored tests. The
complete four-file delta from `43587b98a` to `f4bd8d106` matches the dynamically
enumerated pre-gate file set and hashes. The core restoration is checked again
before committing. No built binary is copied into evidence; result.json records
the two positive test binary paths/hashes and the control package identity.
The development package is
`~/.local/state/sophia/development-evidence/t315-recipient-bind-dev01`;
its verified self-excluding manifest SHA-256 is
`55ac388fffb17f613caeb5e87e12ccb22c45dfc23a100b9337f6579158cbb627`.

t315 remains open. The current export pressure test lowers record and byte
limits together, so separate discriminating byte/record cases and a distinct
active-limit case are still required by the contract, alongside final migration
coverage reconciliation. This work changes neither master nor the installed
session and does not replace t142's attended acceptance.

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
actual Cargo artifact paths and hashes. The development package's
`SHA256SUMS` has SHA-256
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

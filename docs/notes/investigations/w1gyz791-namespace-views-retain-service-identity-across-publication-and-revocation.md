---
id: w1gyz791
date: 2026-10-12
kind: investigation
status: investigating
tags: [investigation]
---
# Namespace views retain service identity across publication and revocation

## Question

How can Session replace an application's endpoint names without changing the
service or authority of its retained connections?

The t318 implementation starts from Sophia
`8fc62dd48ae253345c1c0fed118f0e81c6637295` in the isolated
`development/t318-namespace-composition` worktree. The live desktop remains
unchanged. niltempus approved an authorized developer launcher for an arbitrary
executable under an explicitly approved recipe. Ordinary application launch
must select policy automatically; users should not configure authentication,
socket paths or namespace commands to launch installed applications.

## Evidence

### Application launch options

The follow-up to `b9a2b7331` preserves working-directory selection, environment
removal and null stdin through the existing process supervisor. Defaults retain
existing role behavior. Host clients apply the options to the child command;
protected clients apply them inside Bubblewrap's already approved filesystem
and environment. A selected working directory never creates a host bind, and
relative or parent-traversing protected working directories refuse. Environment
builder operations retain their order: a later assignment can restore a removed
variable, while a later removal suppresses its assignment.

An independent C child checks these options on both the trusted-host and
protected NamespaceClient paths while exercising the actual launch-channel
handoff. The private argument-builder test separately proves that directory
selection adds no visibility. The device-hidden `t318-application-options-01`
package passes the runtime crate suite, all fifteen namespace-launch parent
tests (with child entry points invoked), runtime all-target and native Session
library Clippy, formatting, layout and diff checks. Its SHA256SUMS SHA-256 is
`d11d8d2ee95885c87cbefa1dd38eb06d50967245b816f9dd574d14050c7b8816`.
The package captures candidate hashes before and after; no binaries are copied.
This paragraph follows that captured candidate. Application callsite wiring and
diagnostic stderr custody remain pending; these options alone do not enable
configured recipes in a live session.

### Application process custody

The slice based on `82b7ac283` gives managed applications one process owner for
ordinary host children and asynchronous namespace preparation, activation,
dispatch and retirement. A namespace application reports Started only after
every selected endpoint has its real service owner. Missing dispatch refuses
the launch and retires its grants and child off the Session owner thread.
Cancellation during preparation or after activation retains the existing
scheduler's bounded slot until retirement. Host children retain their existing
process-group cleanup. A failed managed application settles only its matching
admission; it does not consume another application's transaction.

The restored `t318-application-process-02` package passes the native Session
library suite (1,012 passed, 34 ignored, with child entry points invoked by their
parents), diagnostics, native launcher admission/execution and authentication
integration suites, both library Clippy configurations, formatting, layout and
diff checks. The first package passed tests and Clippy but stopped at the layout
ceiling. Child lifecycle handling was then moved into its own included domain
file and the checks rerun. The final package's SHA256SUMS SHA-256 is
`fb555c8939a9123abe09bfa5d59b4d07854c275916c484a05135cbd07db3f644`.

The separate `t318-application-dispatch-control-01` bypasses the undispatched
endpoint refusal and fails at `an unserved application must never be reported
started`. Source restoration is verified; its SHA256SUMS SHA-256 is
`22a8dbf1e08293bd5b58e5e538f9d9db755ba74719d92205c74be61fa4ef42f8`.
The later lifecycle-file extraction does not change this control's owner.
Both packages retain binary paths and hashes, not binary copies. This paragraph
is added after qualification.

Actual catalog, shortcut and startup recipe construction remains to be wired.
Namespace stderr capture, startup attribution before activation and short-lived
children also need integration before removing the configuration guard. This
slice is process-custody qualification, not completed live recipe launch.

### Application catalog recipe selection

The slice based on `823ee31b5` connects the native and legacy catalog workers
to the trusted core namespace-policy snapshot. Both registered and scanned
desktop identities acquire their assigned recipe without metadata or user
commands. An optional `X-Sophia-NamespaceRecipe` key may only confirm that
assignment; invalid or unapproved requests produce unavailable entries. The
configuration parser and desktop parser share one recipe-name validator.

The resolved recipe is part of command equality and survives `Terminal=true`
wrapping. Source-byte revalidation detects a recipe-only desktop edit. Worker
verification now requires the exact retained catalog identity as well as bytes,
command and label; equal executable or command data cannot substitute another
application identity. The raw-process catalog dispatch still refuses resolved
recipe commands pending asynchronous owner integration, rather than dropping
the recipe and launching on the host. The existing startup configuration guard
also remains. This is catalog selection qualification, not completed app launch
integration or an installed desktop claim.

`t318-catalog-identity-control-01` removes the worker's exact-identity comparison.
The retained-command test fails at `worker must retain exact catalog identity`
with exit 101 and one failed test. Candidate hashes are restored and verified;
its SHA256SUMS SHA-256 is
`d67fd6102d4e80ffbcfb59583a32a7f6e1303923911d48526fbd7af20fb5f0a7`.
The first positive package stopped at Clippy's large-enum warning after tests
passed. Boxing the legacy worker's retained catalog entry fixed that warning;
it does not change the comparison exercised by the earlier control.

The restored final candidate passes `t318-catalog-recipes-02`: four configuration
policy tests; the application catalog, diagnostics, inspection environment,
native catalog publication, native launcher admission/execution and persistent
catalog queue integration suites; the connected component-execution test; and
seven legacy confined-app configuration tests. Config all-target Clippy, Session
native/default library Clippy, formatting, source layout and diff checks pass.
The package dynamically enumerates modified files and verifies their hashes
after execution; binary identities are paths and hashes only. Its SHA256SUMS
SHA-256 is
`dd3079bf55cf40a3b4524eb63ec72e706cd2b90358a80f50205c44dba2c11661`.
This evidence paragraph is added after those checks. The next integration must
retain pending namespace startup and running client custody alongside existing
child diagnostics, catalog admission and launch-origin attribution, across both
catalogs, shortcuts and startup apps. No raw-process path may ignore a selected
recipe when the startup guard is removed.

### Authenticated portal recipient dispatch

The slice based on `8c12c1502` adds the
[portal-client recipient export](../../portal-client-files.md). Recipe clients
receive read-only objects from the existing common coordinator, not the
trusted protocol-authority `portal-requester` upload/execute interface. The
selected catalog now distinguishes these interfaces. No new transfer policy,
grant type or executor was introduced. Request execution remains unavailable
and is reported explicitly; capture remains t319.

The production launch scheduler, Factotum conversation, endpoint-grant factory,
shared 9P core and recipient lease are joined by an independently launched Rust
SDK child. Supplied source bytes and policy produce a common coordinator grant.
The child reads those bytes and retains a transfer fid plus a waiting watch.
Source-generation change, lock/unlock and replacement each make the retained
watch and data fid answer `ESTALE`, while status remains readable. In the
replacement round a fresh fid reaches the replacement. A foreign recipient
namespace is refused before dispatch. The fixture does not qualify live
frontend collection, installed app launch wiring or physical capture.

The negative control removes only the common lease's retained source-generation
comparison. The SDK child fails at `transfer retirement must wake the retained
watch with ESTALE`; its one-second wait is shorter than the supplied two-second
grant lifetime, so natural expiry cannot satisfy the expected refusal. The
parent subsequently fails its bounded child-progress wait. Production source
and every captured candidate file are restored and compared before positive
qualification. `t318-portal-client-control-01` has SHA256SUMS SHA-256
`36a3b1f26046630b9c66cecf8cf82e285d362f38cdfb132443bcf98176baed3f`.

The restored `t318-portal-client-01` package passes the full portal crate suite,
ten namespace-launch parent tests (including four explicitly invoked SDK child
runs), eleven namespace-view integration tests, three startup tests, and 28
authentication parent tests with four invoked child fixtures. The ignored child
entries in parent listings are not skipped qualification. Portal all-target
Clippy, Session native/default library Clippy, formatting, layout and diff checks
pass. Its SHA256SUMS SHA-256 is
`ba246df2c0747c8322c32ecd0e1364bb554a299ce9913ad649cec527948211ee`.
Both packages enumerate candidate files dynamically and record binary paths and
hashes without copying build output. This paragraph follows qualification; it
does not change the tested code. The full isolated repository gate remains to
be rerun for the final candidate. Normal application launch integration and the
authorized developer-launch interface remain open, and the startup refusal for
unintegrated recipe configuration stays in place.

The uncommitted foundation contains passive protocol endpoint references,
discovery encoding, a pure runtime recipe compiler, a pinned configuration
loader, and filesystem publication. These components do not grant access.

```text
trusted config root                 Session policy
        |                           approved references + scopes
        v                                      |
 pinned bytes -- verify unchanged --> compiler <+
                                         |
                                  complete endpoint table
                                         |
                                  prepare listeners
                                         |
                              atomic current-view publication
                                /                     \
                       old view lease             new view lease
                       service A                  service B
                                \                     /
                         existing authenticated role owners
                          (integration remains pending)
```

The compiler captures aliases before modifying its tree. Directory unions
deduplicate only equal references and grant scopes. It never opens a socket,
allocates a grant, or accepts client-selected host paths. The loader walks a
pinned trusted root without following symlinks and checks bytes and file
identities again before publication.

Publication allocates a private generation directory and returns listeners to
their role owners. It atomically changes `current`; launched clients will
receive the immutable generation directory. Dropping a failed candidate removes
its files. Old leases retain old directories even after the publisher exits.
The publisher itself does not enforce authentication or revoke grants.

## Finding and resolution

Discovery version 1 uses the reserved file `discovery`, with
`SOPHIA_NAMESPACE_DIR` identifying its directory. The environment variable is a
location, not a credential. Its bounded UTF-8, tab-separated format is:

```text
sophia-namespace<TAB>1<TAB>revision<TAB>endpoint-count<LF>
endpoint<TAB>path<TAB>protocol<TAB>interface<TAB>version<TAB>service-id<TAB>service-generation<TAB>availability<LF>
```

Protocols are `9p2000.L` or `x11`; availability is `available`, `waiting`, or
`recovering`. Availability never asserts permission. Numeric fields use canonical
decimal; identities and revisions are nonzero. Count may be zero. The count
detects truncation on record boundaries as well as inside a record. Paths are
normalized and relative. Duplicate paths, parent/leaf conflicts and a collision
with `discovery` refuse. The limits are 64 endpoints and 128 KiB.

Two allocation guards supplement the language limits: 65,536 compiler tree
nodes, including empty directories, and 64 retained/prepared view generations
per publisher. Exhaustion refuses a candidate and preserves current naming.
These prevent empty-directory alias amplification and retained-view exhaustion.

Linux socket addresses are shorter than the approved logical name bound.
Publication binds through a pinned directory fd and renames the socket inode
into the completed tree. A deterministic test connects a long name through its
pinned `O_PATH` socket fd. SDK adoption must use that mechanism, without global
`chdir`, and validate traversal from the selected directory.

## Validation and remaining work

The compiler/discovery/publication foundation is signed commit `58b48fd36`.
Its sealed development package is
`~/.local/state/sophia/development-evidence/t318-foundation-01`; the manifest
verified. It contains source hashes, positive logs and the restored scope
control, not build binaries.

Executed development checks:

- `/tmp/t318-recipe-03.log`: four configuration, four discovery and nine
  compiler tests passed.
- `/tmp/t318-recipe-04.log`: expanded compiler suite, eleven tests passed.
- `/tmp/t318-publication-01.log`: five publication tests passed, including
  injected materialization failure, failed publication, retained identities,
  cleanup and long socket names.
- `/tmp/t318-clippy-01.log`: Clippy passed for the three changed libraries with
  warnings denied.
- `/tmp/t318-foundation-01.log`: all 25 focused tests passed, including the
  retained-view capacity boundary and reclamation. Clippy passed again in
  `/tmp/t318-clippy-02.log`.
- The one-site `return Err(Error::Scope)` to `return Ok(())` control failed at
  `foreign admission scope must refuse before publication` with one failed
  test and exit 101. The script restored and compared all dynamically enumerated
  modified/untracked candidate files. Its record and log live under
  `~/.local/state/sophia/development-evidence/t318-foundation-01`.

There is no independent authentication, retained 9P fid, grant-revocation,
application-launch, SDK, physical-containment or live-desktop qualification in
these checks. The scope control proves the compiler refusal only. It does not
qualify authenticated foreign admission. No full repository gate has run for
this slice.

Session recipe policy and authenticated role wiring, the
observer path, portal delivery checks, C/Rust SDK readers, catalog identities,
the authorized developer launch command and complete acceptance remain t318
work. Ordinary launch must cover registered and scanned desktop identities,
preserve existing defaults, and never treat application metadata as authority.

## Approved audience sets

The next implementation slice adds a bounded, immutable set to private
Session-to-Factotum registration. Existing registration still means one exact
audience. New command 9 registers up to 64 approved audiences; its largest
record is 556 bytes. The child afid and relay protocols do not change. This is
authentication scope, not a role-operation grant. Session chooses it from policy
and independently checks it before using a verdict.

```text
Session launch policy: identity I, audiences {A, B}
                 |
         private Factotum registration
                 |
       one generation-scoped launch channel
             /                 \
     afid attempt for A    afid attempt for B
             |                 |
     worker narrowed A     worker narrowed B
             |                 |
       role A checks       role B checks

attempt for C -> refused; A verdict at B worker -> refused
retire launch -> both workers lose local custody before remote cleanup
```

Executed checks: `/tmp/t318-audiences-01.log` has 40 passing Factotum tests;
`/tmp/t318-audiences-02.log` reruns the new set tests and the private queue/client
tests, including abandonment cleanup for command 9. The Session integration
suite passes 25 tests in `/tmp/t318-session-auth-03.log` and again after controls
in `/tmp/t318-session-auth-restored.log`. Two ignored child entrypoints are
invoked by their parent tests. The tests use actual protected subprocesses,
the launch channel and Factotum, with supplied Session policy; they do not
constitute the final namespace-directory/observer join. Probe executable names
are unique per test so concurrent tests cannot overwrite or unlink one another.
Clippy passed for Factotum and native-session libraries with warnings denied.

Controls in `~/.local/state/sophia/development-evidence/t318-audiences-02`
remove, separately, Factotum's audience refusal and the worker's exact-audience
comparison. Each fails its intended named assertion, exit 101 with one failed
test. All dynamically enumerated candidate files compare equal after each
restoration. The earlier `t318-audiences-01` controls predate the unique probe
path repair; the second package repeats both on the repaired fixture.

The complete Session recipe resolver, discovery SDKs, observer export and
authorized developer launcher are still pending. These APIs are not yet
selected by ordinary application launch. No live session was modified.

The audience implementation is signed `c57cc30ee`. The first isolated full gate
(`t318-gate-01`) stopped at the C SDK's exact contract-copy check before running
workspace tests: the SDK still held the older authentication document. The C
SDK signed `860b5d819fa8b635d54c99dd064a44cf3a17050e` and Rust SDK signed
`d9cb67b2393d068b7d869d2ab736fdc4fccab43c` update only that document, its digest
and provenance. Sophia's snapshot verifier accepts the C pin; the existing Rust
vendoring tool verified its new pin before replacement. Client code and public
afid/relay records are unchanged. The full gate must be rerun on these pins.

## Independent discovery clients and gate reconciliation

The C SDK candidate `ba09265d1642e9fc9e7919dfd6a634855057055c` and Rust SDK
candidate `0e5ad3638bac48b6f9633e1bc1c0476fb2a1d481` are signed and published
on their respective `development/t318-namespace-discovery` branches. Each
implements the signed discovery contract independently, pins a Linux generation
directory, refuses symlinks and connects to a captured socket inode even when
the logical path exceeds `sun_path`. Local tests replace the directory name and
prove an already-open view still reaches the old listener. These are naming
and SDK observations, not an authenticated Session launch join.

The full C check suite passes. A namespace-only Clang UBSan trap-mode probe
also passes; ASan was unavailable because the host linker lacks its runtime
libraries. The Rust workspace tests, formatting and all-target Clippy pass.
Removing each parser's declared endpoint-count check makes its truncation
assertion fail; all candidate bytes are restored, followed by positive reruns.
Closed packages are `t318-c-discovery-01`, `t318-rs-discovery-01`, and their
`discovery-count-control-01` counterparts under the development evidence root.
Each SDK's provenance records the package manifest identities.

Both SDKs now support sequential authentication of another endpoint through the
same exclusive launch relay. The previous role root is not reset, and pending
relay work refuses instead of being discarded. The C check suite and ten Rust
launch-client tests pass; Rust Clippy passes too. `t318-c-relay-01` and
`t318-rs-relay-01` retain the candidate hashes and scripted-peer results. Actual
Session multi-endpoint authentication remains a separate join.

The full gate's third attempt stopped on a parallel runtime test-helper race:
two tests compiled and deleted the same PID-only executable. Signed
`970dc9e1b` gives each probe a unique path; all four launch-channel tests pass.
The fourth attempt stopped during WM test setup because its refusal case used
the shared default policy directory. Signed `f4fa77cbd` uses its fixture's
private directory; the native-session selection suite passes five tests, with
one ignored child entry point exercised by its parent. An earlier command
without `native-session` selected zero tests and is not evidence for that fix.

The fifth isolated run at `f4fa77cbd` passes workspace tests, SDK checks and
Clippy, but stops at the final source-layout gate: the publication module's
test annotation belongs in its external test file under this repository's
convention. The annotation is moved without changing test behavior. None of
these five attempts is recorded as an overall passing repository gate. Their
original closed evidence packages remain unchanged.

## Confined client launch scheduling

Session now has a bounded recipe-client launch owner using the existing
Factotum coordinator and protected startup transaction. Trusted callers supply
the selected recipe, approved service references and containment policy. The
owner rejects a different namespace registry even when its numeric namespace
IDs match. Shared-host recipes remain refused here until their distinct launch
path is connected; this is not an implicit confinement change for normal apps.

```text
Session owner                        retained preparation worker
  reserve generation + slot -------> prepare immutable view
  poll prepared view <-------------- publish result
       |
  existing Startup ----------------> protected spawn worker
       |                              spawn + retain thread
  activate custody; bind grants
       |
  running client + endpoint owners
       |
  drop/cancel: fence locally -------> retire pending spawn or child
                                      remove view; release slot
```

Preparation, running and retirement share a 64-client bound. Dropping a pending
or running client immediately fences its authentication workers. Remote
registration cleanup occurs on the next coordinator turn. Background retirement
retains the slot and view until an outstanding spawn returns and the supervisor
reaps its child; it uses the preparation worker instead of creating an unbounded
collection of cleanup threads. Failed endpoint binding after activation already
has retirement custody, so that failure cannot reap on the Session owner thread.

`t318-client-scheduling-01` passes five new tests using real Factotum registration
and protected process startup: abandoned preparation, cancellation during spawn,
failed executable, a running client with immediate quiet-endpoint revocation,
and refusal of a different namespace owner. Cancellation may race Bubblewrap's
setup and print a missing-source diagnostic; no cancelled launch is published.
The three existing startup tests and 28 launch-authentication parent tests also
pass. These tests do not run a namespace-aware application or dispatch its role
requests; the independent SDK joins recorded above remain separate evidence.

Native and default Session library Clippy, formatting, source layout and diff
checks pass. The package dynamically records all ten changed/untracked source
and fixture files, checks they remain unchanged, and records test-binary paths
and hashes without copying them. Its self-excluding SHA256SUMS has SHA-256
`cb724940c4d1520ec6a9dc7abe89dc524462bb84f2dc877d1290aaf9e2a35cfd`.
Application launcher wiring, developer administration, shared-host launch,
portal dispatch and retained-transfer qualification remain pending. The startup
guard against unsupported configured recipes remains in place. No master or
installed-session change is made by this slice.

### Shared-host custody follow-up

The scheduler's temporary shared-host refusal above is now replaced by an
explicit `NamespaceClient` host handoff. It preserves inherited environment
and host reach while the existing Linux fd adapter excludes unrelated
descriptors. The supervisor retains the exact reservation and direct child;
it returns no protection evidence. The custody predicate accepts that path
only for namespace clients. Every desktop-role process is refused by the host
entry point, and a configured protection domain cannot be silently discarded.
Failed handoffs burn their generation. Namespace/recipe profile mismatch is
also refused before reserving or spawning.

The same startup state machine registers and activates both paths. Shared
publication skips the sandbox bind but publishes the same immutable view and
endpoint grants. The Session test uses a supplied portal reference and a real
sleep process; it proves startup/grant custody, not portal protocol execution.
The startup configuration guard remains until actual application paths use
the scheduler.

`t318-host-client-01` passes all six runtime launch-channel tests, including the
independent C descriptor probe for the host path, refusal of all seven other
supervised process kinds, exact-reservation checks, role exclusion and exit
observation without a Session poll. Seven Session scheduler tests, three
existing startup tests and the 28 launch-authentication parents pass. Runtime
all-target Clippy, native/default Session library Clippy, formatting, layout
and diff checks pass. The package binds the dynamically enumerated ten-file
candidate and binary hashes, with no build-output copies. No installed or
master state changes with this follow-up.
Its SHA256SUMS has SHA-256
`2cc434295e7d9d1b740e8210e6efb94a03503a1030d67eb56d8fc58668f8a627`.

### Inspection launch dispatch

The recipe launch owner now dispatches inspection listeners to a captured
acceptor from the existing inspection service. The acceptor preserves the
service generation, connection bound and source fences. Authentication factories
share their existing endpoint budget; cloning the dispatch reference creates
no additional authority. The worker takes at most one connection from each
listener per turn and checks quiet connections at the existing 20 ms idle bound.
Dropping the launch fences its grants before background process/worker cleanup;
the capacity slot is released after dispatch joins.

A review caught an overly broad retirement rule: treating every finished
inspection worker as a failed launch would revoke an unrelated portal grant.
Normal service/grant retirement now removes only the finished dispatcher.
Unexpected worker failure still retires the launch fail-closed. A regression
test fences the real inspection service, waits for dispatch retirement, and
requires the sleep client, registration and unrelated portal grant to survive.
It does not claim portal traffic on that grant.

The new SDK join uses the real recipe scheduler, protected launch, Factotum,
discovery SDK, authenticated inspection export and dispatch worker. A C bootstrap
only preserves the inherited kernel stream across exec; it knows no protocol.
The Rust SDK child reads a published snapshot, starts a waiting event read and
observes ESTALE or transport closure after the grant owner revokes it. The parent
requires the child's progress markers and successful exit; the ignored child
entry point was actually invoked. Scene records, recipe policy and service
references are supplied. No renderer, normal launcher or portal execution is
claimed by this test.

`t318-inspection-dispatch-01` passes ten runtime inspection owner tests, nine
Session scheduler/dispatch parents plus the invoked SDK child, three existing
startup tests and 28 launch-authentication parents. Runtime all-target and
native/default Session library Clippy, formatting, layout and diff checks pass.
The dynamically captured six-file candidate is unchanged after the checks;
binary paths and hashes are recorded without copying binaries. Its SHA256SUMS
has SHA-256
`f01807c909f8ede38ef70bd5e8b417e10fc05703094351f8350041d76df4eed6`.
Actual application/developer launch call sites and portal dispatch remain open.

## Connections

The seventh isolated gate at `49bc4aa1a` reaches the Session suite and stops
with 996 passing tests and one failure: the confined-X layout fixture expects
socket staleness immediately after dropping its local listeners, but the
production probe still connects and correctly refuses to unlink a live socket.
The fixture now waits at most two seconds for that specific live-socket refusal
to disappear; other errors still fail immediately. Its earlier live-socket
refusal assertion remains unchanged. This accommodates transient kernel
references, such as descriptors retained across concurrent fork/exec, without
weakening production cleanup. All seven confined-application configuration
tests pass after the repair. The original `t318-gate-07` package remains failed;
this focused rerun is not an overall repository gate result.

The sixth full-gate attempt at `b6aa4b253` stops in the SDK verifier's own
synthetic snapshot fixture: its list omitted the newly required discovery
contract. The corresponding Rust fixture also expected ten contracts rather
than eleven. Signed `0ae24845d` updates those fixtures; the focused C verifier
test and nine Rust verifier tests pass. This does not turn the sixth attempt
into an overall passing gate.

### Authenticated inspection owner

The existing inspection publication owner now supports a private instance with
no HostDomain listener. Its authenticated route requires a factory, pins the
generation captured for the namespace endpoint, and uses the existing export
and records. Four authenticated connections are bounded independently of the
host listener. Failed adoption releases capacity. Root preparation precedes
authentication commitment; wire reset drops root state without retargeting the
service. Quiet polling checks admission and source liveness before queued
output can progress. Session still has to own endpoint dispatch and scheduling.

The generic Session join launches a protected SDK peer, drives the real
Factotum conversation, authenticates and reauthenticates inspection, reads a
snapshot, then revokes the launch while an event read waits. The connection
closes, the child records refusal, and the admission count returns to zero.
Launch policy and safe snapshot data are supplied. The fixture uses existing
protected generic shell custody with an explicitly assigned inspection
audience; it does not add inspection authority to production shell launches.
Automatic recipe selection and separate per-endpoint grant revocation remain
unimplemented integration work.

The closed `t318-observer-01` package records all modified and untracked
candidate files dynamically before and after validation. Ten runtime owner
tests, nine host-service tests (plus its invoked child), three CLI tests,
four Session inspection tests and the protected SDK join pass. Runtime
all-target Clippy, the Session integration-target Clippy and the source-layout
gate pass. Formatting and `git diff --check` also pass.

The separate `t318-observer-generation-control-01` changes authenticated
adoption to read the current source generation instead of using the endpoint's
captured generation. It fails with exit 101 at `an old namespace endpoint
cannot adopt replacement inspection data`. All enumerated candidate hashes
are restored. Its patch includes the then-uncommitted candidate delta against
HEAD as well as the mutation; the recorded pre-mutation hashes identify the
candidate. The later positive package includes an additional peer-UID assertion
in the Session join. Neither package contains copied build output.
Their self-excluding SHA256SUMS identities are respectively
`8423a3776230781c12efa752badc1fa2e28a67f6eea42c3cbf7fab7025a1f4b8`
and `97b4169f142b096bee70233a957e7bcdf4a5116181ecabeb21a82a82d27cc3ba`.

### Independent endpoint grants

The next slice puts endpoint grants in the existing namespace/admission
registry. A lease retains namespace, principal, launch generation, audience,
service identity/generation and registry ownership. Passive equal numbers from
another registry cannot supply that authority. Grant-backed admission records
and retained leases check the grant's liveness. Namespace revocation collects
the affected admissions before fencing their grants so cleanup notifications
remain complete. Explicit launch retirement can reclaim all of that launch's
grants without revoking a later launch in the same namespace.

Session's recipe factory binding requires the exact captured endpoint and
owner-issued grant. Its existing role-authentication owner enforces that lease
at transport creation, authentication, admission and quiet polling. Existing
fixed role factories retain their explicit legacy binding path; there is no
peer-selectable fallback from a recipe grant. The registry permits 4,096 live
grants, reclaims revoked slots and never reuses IDs. Naming aliases clone a
lease rather than creating authority. Live recipe policy selection and launch
cleanup wiring remain pending; this slice does not claim those startup paths.

The protected inspection join now uses grant-backed factories. A second case
authenticates two audiences over one real launch relay using the Rust SDK,
with two grants for the same inspection owner. Session revokes only the first
grant while its event read waits; that read refuses/closes, and the second
authenticated endpoint still delivers a snapshot. Whole-launch revocation
remains a separate passing case. Policy selection, endpoint sockets and safe
snapshot data are supplied; custody, Factotum, the 9P core and export are real.

The `t318-endpoint-grants-01` package passes four discovery tests, six grant
tests, eleven compiler tests, nine registry tests and the entire Session
launch-authentication suite (28 parent tests; four ignored child entry points
all invoked by parents). Runtime all-target Clippy, Session integration-target
Clippy and the layout gate pass. A stale Clippy SDK metadata artifact initially
reported the already-present `over_relay` method missing; a package-only
`cargo clean -p sophia-launch-client` under the lease followed by rebuilding
resolved it without source changes. The final package records the candidate
files before and after all checks, with no copied build output.

Two separate controls fail at named assertions and restore every enumerated
candidate hash: omitting the grant's active-cell check fails `revoked endpoint
grant must fence retained admission`; omitting the captured endpoint comparison
fails `a changed service generation cannot reuse the captured grant`. The
positive package runs after restoration. The positive, live-control and
scope-control SHA256SUMS identities respectively are
`2275dc2a1b6d3821d2d8d13366d78bcc2a212bd4fa2d7853a6997210497aa89e`,
`7bbe9268a100f8f52268bdfaf4a1aca1ccea5ad5449de4e2f340a0c550d49543`,
and `a87be6816a8c7114dd4b2388feeb0f3f35e35191ec360dcc56eeb248d0f01afc`.

### Repository gate and namespace-client custody

The complete isolated `cargo xtask check` passed at signed
`499373cf3d31d8fcd46bef3dae69c586843921ed` in `t318-gate-08`. HEAD and the clean
working tree were unchanged before and after the run. It used the worktree's
cargo-slot lease, private `/dev`, `/run` and `/tmp`, a fresh private runtime
directory, and no host display, authorization or Session environment. The closed
package's SHA256SUMS has SHA-256
`dd4d365bd5e9e76e4ee161128b1dd6e6c5cb2c828046268e0a3178dd02fc646a`.
This is repository regression evidence for that commit, not completed t318
acceptance or a physical display claim. Earlier failed gate packages remain
under their original identities.

The following slice adds an exclusive `NamespaceClient` protection role and
matching supervised process kind. It uses the existing protected spawn and
launch-channel custody mechanism. The role cannot compose with existing roles,
borrow their domains, or satisfy their launch checks. It carries no implicit
inspection, portal, shell or administrative grant. Existing shared-host and
confined-X command launch paths are unchanged.

The protected inspection tests now reserve this role and launch its matching
process kind, replacing their previously supplied shell custody. Both independent
Rust SDK joins pass with production publication, discovery, Factotum admission,
retained-read revocation and unrelated endpoint progress. The fixture still
supplies approved policy; automatic application selection and live launch
dispatch are not claimed.

The closed `t318-client-custody-01` package records all five changed code/test
files before and after verification and the executed binary paths/hashes. Four
runtime command tests pass, including the bidirectional role-mismatch matrix;
all 28 Session authentication parent tests and their four child entry points
pass. Runtime all-target and Session authentication-target Clippy, workspace
formatting, layout and diff checks pass. Its SHA256SUMS has SHA-256
`3ec534772676db08e9da462010c1f54f4d3539ccea2d6ec2319d40d305da8c78`.
This focused package follows the full gate; it is not included in gate-08's
source identity. The normative and investigation updates follow these checks.

### Trusted launch policy adapter

The core configuration now parses one bounded `session/namespace-policy` block,
with named recipes containing profile, naming source, portal participation and
explicit developer-launch permission. Exact registered and scanned desktop
identities select an assigned recipe. The pure selection function accepts an
application's requested recipe only when it equals that assignment; no assignment
with no request preserves the existing default. Developer selection takes no
executable allowlist and requires an explicitly enabled confined recipe. Its
administrative admission remains a separate required launch-owner check.

Observer policy is restricted to request-only screen capture. Confined-X requires
a nonzero group; shared-X cannot authorize the developer launcher. Source names
are normalized paths relative to trusted configuration, not application paths.
Changes classify as requiring restart. Session currently refuses nonempty policy
on startup/reload rather than ignoring it: automatic dispatch, group/reference
resolution and metadata extraction remain integration work. The following slice
supplies the builtin fragments and their publication path.
This refusal is temporary and is not the intended final application experience.

The standalone configuration build exposed that the earlier pinned loader relied
on workspace feature unification for Rustix's `std` integration. Its manifest now
requests `std` directly, with no lockfile change. The closed
`t318-recipe-policy-01` package passes all 176 configuration tests, including four
new policy tests, and the Session refusal test. Config all-target Clippy, native
Session library Clippy, formatting, layout and diff checks pass. Nine modified
or untracked source files were enumerated dynamically and hashed before and after
the run; binary identities are recorded without copies. SHA256SUMS has SHA-256
`bbf800ce319c6e54e993979d9f4b5b645569de8a2419ac0b82fa4e9d24795130`.
These are parser/selection and refusal results, not live recipe launch acceptance.

### Selected recipe publication and X11 socket capture

`LoadedNamespaceRecipe` snapshots trusted recipe configuration and either the
shipped shared/confined-X or observer fragment, or the existing pinned custom
source loader. Both source forms use one compiler/publication transaction.
Selected publication refuses catalogs outside the profile's service/interface
set. Tests cover immutable selection, observer aliases, extra-role/interface
refusal, missing-service rollback, custom include replacement and distinct X
references under the same shipped naming fragment.

The publisher previously created listeners for every protocol. It now creates
listeners only for 9P role dispatch. X11 requires a Session-supplied
`CapturedX11Socket`, which pins an owned socket using `O_PATH` and links its
inode into the candidate view. Tests rename and replace the original pathname
before materialization: both aliases retain the old inode and connect to the
old listener, while the replacement accepts nothing. Socket mode is unchanged.
Missing captures, wrong generations, fully unlinked sources, symlinks, regular
files and nonprivate group directories refuse. A failure immediately after
linking removes candidate aliases, preserves the old publication and restores
the original socket's link count; that listener still accepts a connection.

A configured confined-X recipe now reaches that existing supplied listener
through the independent Rust SDK's discovery and connection path. Only its
portal endpoint allocates a new listener. These are actual Unix socket and
inode observations, not X11 setup, cookie, namespace-admission or live-launch
acceptance. Cross-filesystem aliases fail closed; Session must choose the
publication filesystem accordingly. No input/render owner or X frontend proxy
was added.

The closed `t318-selected-publication-01` package passes ten runtime publication
tests (including the C/Rust discovery join), eleven Session view tests and all
28 authentication parent tests with four invoked children. Config/runtime
all-target and Session integration-target Clippy, workspace formatting, layout
and diff checks pass. SHA256SUMS has SHA-256
`1e6497a5ba6cdc9aef5808c78ab8c2a9ef4db6aea23b5dc828ca2656b8f1f30e`.
The follow-up `t318-selected-publication-02` adds only the post-link fault test;
all eleven runtime publication tests, runtime all-target Clippy, formatting,
layout and diff checks pass. Its manifest identity is
`4f49ef278ee6d04e6bb549b30b38d1a1e064e31ca2e24f23763feacd7c861b98`.
Both packages enumerate and hash ten candidate code/test files before and after.
Their only source-hash difference is the added runtime test. Binary paths and
hashes are retained without copies; these documentation changes follow the runs.

### Session publication and grant custody

`NamespaceRecipeOwner` now joins the pinned source loader, compiler, endpoint
grant registry and atomic filesystem publisher. It accepts a newer trusted
policy revision, preserves unchanged grant owners, and drops reduced owners
before materializing replacement names. A publication failure retains old
names while revoked authority remains revoked. Newly issued candidate grants
are dropped on failure; aborted IDs are not reused. Repeating the failed or
an older accepted policy revision refuses. A later explicit authorization
requires a fresh revision and grant incarnation.

`EndpointGrantOwner` separates policy custody from the leases retained by
workers and admissions. Its drop fences authority atomically without needing
the registry mutex. This avoids blocking cleanup behind an admission owner
or reviving grants during rollback. Registry tombstones reclaim at the next
allocation. Publication holds the registry only for bookkeeping; pin checks,
socket materialization and filesystem publication run outside that lock.
The approved service catalog is bounded to 64 entries, deduplicates identical
service/audience grants, and refuses inconsistent metadata or availability
for one service incarnation.

Six Session publication tests pass: private equal-name views with shared service
identity and separate grants; A-to-B rebind with retained A permission; failed
publication without authority revival; changed/invalid source refusal; mutex
contention before reduction; and catalog bounds/consistency. Cleanup while the
registry mutex is held completes within the bounded test wait. These tests
supply policy and admission facts, so they do not claim afid verification.

The protected inspection tests now obtain listeners and grant leases from this
owner instead of constructing them separately. Their child uses the independent
Rust namespace SDK to discover and connect before authenticating through the
real Factotum launch channel. Both whole-launch and independent-grant revocation
continue to pass, including progress on the second endpoint after the first is
revoked. Endpoint dispatch is direct fixture wiring; live application recipe
selection, scheduling and the developer launch export remain pending. X11
endpoint materialization is not qualified by this 9P inspection join.

The closed `t318-namespace-owner-01` package passes these six publication tests,
six grant and nine registry tests, and all 28 Session launch-authentication
parent tests (all four child entry points invoked). Runtime all-target Clippy,
Session integration-target Clippy, workspace formatting and source layout pass.
The package records dynamically enumerated candidate files, test binary paths
and hashes without copying binaries, and verifies unchanged source after checks.
Its SHA256SUMS identity is
`6a974c43d98c67bedb1455c67b3df0b19b6e44c58d6ae6bdce24de96245e5bba`.
The separate `t318-view-revocation-control-01` omits the policy grant owner's
revocation on drop. It fails with exit 101 at `failed naming publication must
not revive revoked authority`, then restores all candidate hashes. Its manifest
identity is `5019c94752840a4a99fee5b2be8096bc78304ce92a8e6b8b91515a91a62c3d93`.
The positive package follows restoration and includes a later catalog-metadata
consistency check and its assertion; the control's earlier candidate is recorded
separately. The lockfile change adds only the existing vendored namespace SDK
as a Session test dependency.

Sophia now pins the published C and Rust candidates above. Both snapshot
verifiers also compare `namespace-discovery.md` against the normative source,
so a later contract change cannot silently leave either SDK copy behind.
The runtime's seven publication tests pass, including a new generic join that
runs both independent SDKs against the production publisher. It checks a
361-byte endpoint path, a second alias, service generations, and connections
to retained A versus replacement B listeners. The peers send a marker solely
to identify the connected listener: no role traffic, grant or authenticated
operation is claimed. Production role-owner dispatch remains pending.

The SDK snapshot gates, runtime all-target Clippy and source-layout gate pass.
The closed `t318-sophia-discovery-01` package records the dynamically enumerated
candidate files before and after the seven-test publisher rerun and retains
the gate logs. Its SHA256SUMS has SHA-256
`967097ae6a4bc78777984b6508ce967b61783f9bddcb43aeb102e4fe420b75ae`.
This paragraph follows that captured candidate; code and fixtures are unchanged.

The second full-gate attempt (`t318-gate-02`, signed `a94415e4f`) reached
workspace tests and stopped at `normal_session_lifecycle`: the isolation
wrapper had removed `XDG_RUNTIME_DIR` without supplying a private replacement.
The third wrapper creates `/run/sophia-test` with mode 0700 inside its private
`/run` and sets that variable there. Host display, runtime and devices remain
unavailable. The failed runs remain under their original evidence identities.

- [Discovery contract](../../namespace-discovery.md) specifies the versioned
  representation and safe SDK lookup independently of either implementation.
- [t318 plan](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#t318)
  owns implementation and acceptance.
- [Recipe contract](../../namespaces-and-portals.md#namespace-recipe-contract-target)
  fixes the grammar and authority boundary.
- [t275 source review](kcfh2hdg-adopting-the-plan-9-namespace-model-in-sophia.md)
  distinguishes approved design from earlier implementation.

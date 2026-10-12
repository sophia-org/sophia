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

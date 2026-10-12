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

## Connections

- [t318 plan](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#t318)
  owns implementation and acceptance.
- [Recipe contract](../../namespaces-and-portals.md#namespace-recipe-contract-target)
  fixes the grammar and authority boundary.
- [t275 source review](kcfh2hdg-adopting-the-plan-9-namespace-model-in-sophia.md)
  distinguishes approved design from earlier implementation.

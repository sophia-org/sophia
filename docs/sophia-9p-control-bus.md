# Sophia public interfaces over 9P2000.L

**Role:** accepted architectural direction and open interface design.

**Status:** niltempus accepted this direction on 2026-09-25 and reaffirmed public
WM/shell protocol replacement on 2026-09-26. The broader filesystem design
remains open; the [WM file contract](sophia-wm-files.md) governs the implemented
Hagia development candidate.
The [decision record](notes/decisions/1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
records the acceptance basis and limits. Subsequently niltempus approved
implementation of the [Hagia-first milestone](notes/plans/80blhke8-migrate-the-hagia-wm-role-to-admitted-9p2000-l-files.md):
direct sockets, compact binary WM records, and existing Session owners. This
does not authorize installation or live-session work, and other roles remain
later milestones.

## Agreed direction

9P2000.L is the target common public protocol for Sophia's replaceable desktop
components: window management, shells and administration, with separately
authorized portal interfaces. A 9P application frontend will sit alongside the
X Server Frontend. Applications may also expose their own services through
explicitly granted filesystem interfaces.

The destination is progressive replacement of the custom public desktop IPC
protocols after equivalent behavior, recovery and acceptable performance are
demonstrated. Coexistence supports migration; it is not a permanent division
between compiled clients and scripts. There is no decision to retain the old
WM protocol merely because a client is compiled or its invariants are modeled.

X11 remains supported through X authority. Engine's internal typed records,
queues, rendering and device interfaces retain their existing owners. This
direction does not require sending every internal interaction through 9P.

## Current implementation and target

The [native protocol family](sophia-policy-ipc.md) is the current implementation
contract. It already provides shared binary framing, language independence,
negotiation, bounded records and role-specific lifecycle rules. KDL describes
its schemas; ordinary role messages are binary, not KDL text. Independent
clients need not link Sophia libraries or generated bindings.

The checked-in `sophia-9p-authority` crate is a scaffold. Its message and tree
handlers do not establish Linux 9P2000.L conformance, an integrated application
frontend, or a replacement WM/shell/control service. Existing clients and wire
contracts remain supported until their replacements pass the migration criteria.

Separately, `sophia-9p` supplies the bounded shared codec and connection core,
and Session's WM export is integrated with Hagia's independent Nim client on
the t249/h006 development branches. Opt-in launch, protected admission, real
configuration, layout settlement and restart recovery have focused controls.
These are development checkpoints, not complete role acceptance or a default
switch. The [mounting investigation](notes/investigations/5kqzwmi5-plan-9-belongs-in-the-session-control-plane-not-the-engine.md)
records why direct clients do not depend on unprivileged v9fs support.

The expected gain is a consistent, inspectable interface that reduces custom
client plumbing. Developers could use ordinary file operations through a mount
or a generic 9P client directly. Sophia still needs documented, versioned file
contents and command semantics; filesystem operations do not eliminate domain
contracts or conformance tests.

## Architecture and ownership

```text
 Existing X11 applications       New 9P applications
            |                            |
            v                            v
   +------------------+        +--------------------+
   | X authority      |        | 9P app authority   |
   | X11 resources    |        | fids, app objects  |
   | protocol state   |        | protocol state     |
   +--------+---------+        +---------+----------+
            |                            |
            +------------+---------------+
                         | admitted visual transactions
                         v
             +---------------------------+
             | Sophia Engine             |
             | scene, input, rendering,  |
             | commit and retirement     |
             +-------------+-------------+
                           ^
                           | existing owner interfaces
             +-------------+-------------+
             | Session and role owners   |
             | admission, supervision,   |
             | proposals and outcomes    |
             +-------------+-------------+
                           ^
                           | separate authorized 9P role APIs
                  +--------+----------+
                  | WM | shell | tools|
                  +-------------------+

 Application-owned service exports
   editor / media tool / other app -> explicitly granted consumers
   Application owns its service semantics; Sophia mediates authorized access.
```

Arrows summarize ownership, not a settled process or socket topology. Shared
9P parsing and connection machinery must not become one authority over every
resource. The [9P application frontend](sophia-9p-authority.md) owns application
protocol state; Session and existing role owners retain desktop admission,
WM proposals, shell allocations, administrative operations and portal decisions.
The precise crate and process split remains open.

Hagia remains a metadata-blind WM. It receives opaque spatial facts and proposes
layout and presentation; it gains no pixels, titles, PIDs, raw input or portal
payloads. Lom remains a content shell; Bemenu remains the independently admitted
launcher. Narthex remains the separate descriptor reference and rollback option.
The protocol choice does not add a desktop component or merge those roles.

## Plan 9 ideas adopted deliberately

The useful combination is services exposed as files, a common access protocol,
and private namespaces assembled from selected services. The namespace need not
be one globally visible desktop tree. Different processes can use familiar local
paths while receiving different admitted exports.

An editor could expose its own buffers and commands; a media application could
expose its own playback state. Consumers would need explicit grants. Sophia
would not interpret editor commands, absorb application logic, or grant blind
policy access to application services. Existing restrictions on policy-owned
listening endpoints remain. Discovery, delegation and revocation of application
exports need their own contract before implementation.

Plan 9's [namespace paper](https://9p.io/sys/doc/names.html) and
[system overview](https://9p.io/sys/doc/9.html) motivate this composition model.
[Acme's service files](https://9p.io/magic/man2html/4/acme) illustrate application
composition. These are design references, not compatibility promises.

## Dialect, transport and API identity

The current WM direct-client subset includes bounded `TREADDIR` for its fixed
root. Directory discovery grants no role access. The separately enabled
[host inspection export](sophia-wm-inspection.md) reuses that operation with its
own admission and read-only vocabulary; mounted access remains unaccepted.

- **Chosen wire direction:** 9P2000.L.
- **Access paths to evaluate:** a direct local Unix-socket client and the Linux
  v9fs mount path, both subject to the same role admission and lifecycle rules.
- **Working family label:** `sophia_vfs_v1`; role naming and version negotiation
  remain open. A shared label cannot substitute for individual role contracts.
- **Unsettled:** endpoint placement, number of endpoints, mount paths, export
  selection, supported operation subset, and the identity carried by each attach.

Linux provides a 9P client with Unix-socket transport. The `.L` dialect defines
Linux-oriented operations and numeric `Rlerror` replies. Mount setup, privileges,
caching and caller identity still need a Sophia-specific design. See
[Linux v9fs](https://docs.kernel.org/filesystems/9p.html) and the
[9P2000.L specification](https://github.com/chaos/diod/blob/master/protocol.md).
The [host mounting investigation](notes/investigations/5kqzwmi5-plan-9-belongs-in-the-session-control-plane-not-the-engine.md)
observed v9fs refusing an unprivileged user-namespace mount; this is host evidence,
not a portable kernel guarantee. Direct clients need no mount privilege. A
userspace `9pfuse` path is a later option requiring its own admission, caching
and lifecycle checks, not an already supported access path.

Classic 9P2000 fallback is a separate compatibility question. Supporting `.L`
does not make existing Plan 9 or plan9port applications work unchanged. Graphics,
input, runtime and service conventions need separate compatibility evidence.

## Illustrative filesystem views

These names show the design space. They are not runnable commands, a frozen
schema, or a promise that every role can enumerate this whole tree.

```text
 /sophia/
   wm/                  opaque snapshots, events, complete proposals, outcomes
   shell/               admitted allocations, content, actions, receipts
   session/             authorized status and administrative operations
   portals/             specific brokered transfers
   app/                 this application's windows, content and routed input

 /services/             separately granted application-owned exports
```

A developer could build a WM, a shell, or both components in one desktop
project. Combining source or a language does not combine authority: blind WM
and metadata-bearing shell processes still occupy separate protection domains.
Convenient typed libraries are optional client aids, not hidden sources of
protocol meaning. Small scripts should be possible without a Sophia-specific
framing implementation; complex rendering still needs application code or
libraries.

## Namespaces complement the protocol

Linux namespaces continue to isolate mounts and other operating-system
resources. Sophia resource namespaces continue to govern application resources
and deliberate cross-namespace sharing. 9P exposes authorized operations within
those boundaries; it replaces neither mechanism.

Session must bind each connection or exported view to admitted authority. The
server must enforce object membership, disclosure rules, generations, resource
budgets and revocation on operations through retained handles. Hiding a path or
unmounting a view is not proof that an already-open handle has lost authority.
Client-supplied attach names, UIDs or paths cannot confer a role by themselves.

The mounted client must not be assumed to authenticate every issuing process
through one socket's peer credentials. Mapping mounts, attaches and open handles
to protection domains is an unresolved contract requirement. Filesystem modes
and mount topology supplement authorization; they do not replace it.

During migration, one explicitly admitted WM owns spatial proposals regardless
of its transport. A WM disconnect never automatically grants layout authority
to another client. Administrative action forwarding remains distinct from the
WM role, and read-only views remain subject to disclosure policy.

## Transactions, events and lifetime

The file API must retain coherent snapshots and complete, bounded proposals.
Partially written values cannot alter the committed scene. The detailed
transaction representation remains open: a per-client transaction object or a
complete proposal stream are candidates, not accepted syntax.

The following meanings stay distinct:

```text
 bytes received -> complete proposal -> validated/committed
                                      -> actually presented -> resources retired
```

Each role keeps its own valid outcomes and ordering. A successful `Rwrite`
returns a byte count; it does not prove a visual commit, a page flip or resource
release. Linux may split a large write into several protocol requests, so a
single system call cannot define proposal atomicity. Detailed outcomes need
their own correlated records. Request cancellation cannot be assumed to undo
an already committed operation.

The contract must specify snapshot identity, transaction ownership, event
ordering, reconnect epochs, stale-handle refusal, bounded queues and slow-reader
behavior. Input remains tied to actual presented targets. Disconnect revokes
authority promptly while source leases and native backings remain retained until
their real consumers retire. Protocol neutrality does not weaken these owners.

## Performance is an acceptance question

No Sophia-versus-9P benchmark currently establishes parity or improvement. Both
protocols are binary. A 9P write has 23 bytes before its data and an 11-byte
write reply; Sophia's current envelope is 24 bytes before its payload. Complete
transaction costs depend on payload and operation sequences, not that comparison
alone. The seven-byte common 9P header is not the complete write overhead.

Evaluate direct 9P and mounted v9fs independently. Persistent handles, batched
layouts, coherent snapshots and bounded event waits rather than polling are
design candidates. Text inspection and compact bulk formats can share an
interface; their exact encoding and negotiation remain open.

WM traffic carries spatial records, not application pixels. Content shells and
graphical applications also require bulk-transfer measurements. 9P supplies no
automatic zero-copy, GPU-handle transport, frame-rate or latency guarantee.
Synthetic files need no persistent file backing by design, but that does not
prove zero storage activity or equal copying and CPU cost on the host.

Compare latency distributions under load, idle wakeups, CPU use, allocations,
copied bytes, queue bounds, upload throughput and frame deadlines against the
same existing-owner operations. Formal models and negative controls apply to
both transports. A performance or lifetime gap blocks retirement of the affected
old interface until resolved or explicitly reviewed.

## Migration and evidence

The accepted destination is common public 9P interfaces. The first admitted
implementation is Sophia t247-t249 paired with Hagia h006. Runtime WM records
are compact binary, with text discovery and derived inspection; no KDL runtime
payload is required. The output role remains current IPC in this first milestone.
Task state and order belong in the existing queues, not this document.

The [desktop migration plan](notes/plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
now sequences the next work: qualify the pinned WM daily configuration while
specifying the shell contract; migrate Lom, Bemenu and Provlita through separate
component grants with Narthex as the descriptor reference; then migrate output
and retire old transports per accepted role. Administrative command migration
can proceed independently after the WM development exit.
Portals and application interfaces retain their subsequent contract gates.
This sequence does not claim those later services are implemented. Current IPC
is still the installed default, and the output role still uses current IPC.

Before an old interface is retired, demonstrate:

1. Published role contracts and 9P operation conformance, including malformed
   input, partial I/O, cancellation and bounded resource consumption.
2. Equivalent authorization, disclosure, proposal outcomes, presented input,
   reconnect behavior and independent source/native retirement through the
   production owners, with compiled negative controls.
3. Independently written clients: a WM, a content shell and a small graphical
   application. Evaluate the descriptor reference and paired desktop clients
   within their respective roles; one client's success does not accept another.
4. Measured performance against current IPC for equivalent workloads, plus the
   physical evidence required for any native presentation claim.
5. A documented compatibility, rollback and deprecation path. Coexistence must
   preserve single-writer admission and common owners rather than duplicate state.

Adding an application frontend and migrating desktop roles are distinct
acceptance claims even when they share transport code. Existing X11 acceptance
and open desktop exits are not closed by this architectural decision.

## Questions still open for brainstorming

- What is the smallest useful file vocabulary for each role, and how are its
  version and effective capabilities discovered?
- What are the later shell/application content formats? WM runtime records are
  now binary, with text discovery and derived inspection.
- How do transaction handles, event streams and outcomes work equally well for
  direct clients and mounted clients under fragmentation and cancellation?
- How are exported views bound to admissions, and how are retained handles and
  delegated application services revoked across reconnects?
- What is the first graphical content format, and what measured need would
  justify a Plan 9 draw compatibility layer or another buffer transport?
- Which workload-specific performance bounds must pass before each migration?

These questions do not change the agreed direction; their answers determine the
contract that can be implemented and tested.

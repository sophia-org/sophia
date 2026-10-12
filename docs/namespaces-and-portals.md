# Namespaces And Portals

**Role:** normative architecture and target API contract.

Sophia assigns trust at session admission, before a client can create useful
protocol state. Namespaces isolate client resources. Portals mediate the narrow
operations that intentionally cross that isolation boundary.

The current repository has typed namespace/profile/capability/admission values,
a session-owned in-memory namespace registry, namespace-keyed X resource
tables, an X configuration seam that accepts immutable namespace context, a
live X session that allocates and revokes its classic context through the
registry, per-connection X admission and revocation, and pure portal reducers.
Classic/confined launch, the bounded confinement matrix, broker IPC and
lifecycle, deterministic headless policy, and concrete native-X clipboard
execution are implemented. A graphical prompt provider remains optional
follow-on policy UI; it does not change the completed authority boundary.

## Ownership

The session supervisor owns namespace allocation, profile selection, admission,
and revocation. A protocol authority receives an immutable client admission
context; it does not mint namespace identity or reinterpret the selected
profile. Sophia Engine consumes namespace-checked surface transactions, but it
does not decide whether two clients may share protocol resources.

The target value contract is:

```text
NamespaceProfile = ClassicShared | Confined

NamespaceContext {
    id: NamespaceId,
    profile: NamespaceProfile,
    capabilities: NamespaceCapabilities,
}

ClientAdmissionContext {
    client_id,
    namespace: NamespaceContext,
    auth_provenance,
}
```

`NamespaceId` remains an opaque, nonzero Sophia ID. It is not an XID, Wayland
object ID, PID, UID, or reusable authorization token. `auth_provenance` records
the bounded admission mechanism and session generation; it never contains a raw
cookie or credential secret.

Pure cross-crate values belong in `sophia-protocol`. Namespace allocation,
admission, and revocation belong in `sophia-runtime` and its session supervisor.
Each frontend owns the protocol-specific adapter that consumes an admitted
context.

## Accepted composition target (not yet implemented)

The [2026-10-09 one-core decision](notes/decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md)
extends this contract with admission-derived recipes. Each identity receives
an endpoint directory and discovery file; every role retains its own tree and
authority. There is no shared forwarding root. Mount visibility and host
containment remain separate from server authorization.

The first successful authenticated attach fixes the connection's principal
and namespace. Subsequent attaches must prove the same identity and stay
within its admission; another identity requires a separate connection. Every
retained fid keeps its original identity and generation checks. Authentication
does not establish supervisor launch custody, and a mount sharing a connection
shares that admission rather than authenticating each process using it.

Portal policy continues to decide over bounded facts without owning payloads.
A recipient executor exposes a grant-bound object in its 9P role tree or
translates through X authority. Revocation removes future access, including
through retained fids. The first new transfer is one output screenshot:
immutable renderer-owned bytes, dimensions, format and output/presentation
generation, delivered within byte and time limits. Screenshot bytes cross 9P;
GPU descriptors use a separate channel. Loss or replacement invalidates the
grant. Lock refuses new capture and cancels unfinished delivery; previously
delivered bytes cannot be recalled.

Observer, operator and driver recipes contain independent grants. The shipped
target observer has status, inspection and explicitly authorized capture,
with no injection or administrative authority. Separate identities are the
default for observation and driving; combining grants requires explicit
operator policy. This target does not assert that the current admission or
capture implementation already provides these guarantees.

The [admission boundary design](notes/plans/esnqxpqw-pidfd-and-namespace-admission-optimizations.md#local-authentication-design-and-t317-handoff-2026-10-10)
and [recipe review](notes/investigations/kcfh2hdg-adopting-the-plan-9-namespace-model-in-sophia.md#t275-recipe-design-result-2026-10-10)
map this target to current source and required device-free controls. The recipe
contract below and the authentication handoff are approved implementation
targets, not additional supported interfaces.

The proposed first local authentication target uses a supervisor-provisioned private
conversation channel and a Factotum verdict bound to one 9P authentication
attempt. It proves possession of that launch's delegated capability, not a
current PID or executable image. Session still checks the live role assignment
separately. A forwarded channel never changes its principal or grants and loses
authority when its launch generation is revoked. Wire reset invalidates afids
while preserving the connection's pinned identity; a fresh authentication must
prove that same identity.

The approved initial observer/operator bootstrap requires an authorized Session
launch from trusted configuration or an admitted administrative path. A socket
path or same UID is not an unlaunched-client fallback. Independent credential
provisioning for ad-hoc or remote clients remains a later authentication design;
the first capture CLI must retain this bootstrap requirement. No existing X
admission, role peer check or supported 9P operation changes in this design.
Independent components remain standalone programs using the protocol and SDK.
[t318](notes/plans/jsschoen-converge-public-roles-on-one-9p-core.md#t318) must
provide an authorized developer launch command before this experience is ready;
requesting a role cannot grant it. Local authentication for independently
launched tools and applications is an explicit
[t324 follow-on](notes/plans/jsschoen-converge-public-roles-on-one-9p-core.md#t324),
not a permanent requirement that Sophia launch every application. This staging
was approved by niltempus on 2026-10-10.

## Namespace recipe contract (target)

The [endpoint discovery contract](namespace-discovery.md) specifies the t318
discovery file and SDK lookup. Its implementation status is separate from the
recipe design below.

Approved by niltempus on 2026-10-10 as the t275 design handed to t318. No recipe
parser or composed endpoint directory is implemented by this document. The
existing X group directory contract below remains separate and unchanged.

### Policy and compiler boundary

One trusted policy recipe contains three parts: naming instructions, portal
participation capabilities, and an optional host-containment policy. Shared and
confined profiles select recipes, not separate naming algorithms. Other role
operation grants remain independent Session policy inputs. A recipe may narrow
approved authority but cannot mint a principal, NamespaceId, role or transfer
grant. If its requested scope exceeds policy, reject the candidate rather than
silently publishing a partial view. Portal participation still requires an
individual grant for each protected transfer.

Host containment describes requirements for the platform driver, not shell
commands in the recipe. Omission does not claim host isolation. If a selected
profile requires confinement and the driver cannot meet it, refuse launch;
never fall back to a shared host view. Endpoint visibility alone proves neither
permission nor containment.

The pure naming compiler receives pinned recipe bytes/revision, the selected
namespace context, approved endpoint references and their grant scopes. Session
can prepare a launch view before authentication, but must bind the verified
admission to that policy before exposing protected role operations. Possessing
the directory or its discovery file supplies no authentication. t133/t317 own
that check and launch custody.

Compilation returns a bounded endpoint table and discovery description. Session
materializes the complete candidate privately and publishes it atomically;
compile or materialization failure leaves the old naming view intact and
releases the candidate. Separately applied revocations are never undone by that
failure. No client-selected host path or socket is opened by the compiler.
Each resulting connection still enters one role export on the shared 9P core;
there is no forwarding root or second public service registry.

### Naming language, version 1

The top-level file starts with `namespace-recipe 1`. Included fragments contain
operations only. Spaces and tabs delimit tokens; blank lines and whole-line
comments beginning with `#` after optional whitespace are ignored. There is no
quoting, escape syntax, environment substitution, executable hook or runtime
client command. Unknown verbs, flags or extra operands refuse the candidate.

| Operation | Meaning |
| --- | --- |
| `mount SERVICE PATH` | Install one approved catalog endpoint at PATH, including its protocol and grant scope. SERVICE is not a host path, network address or credential. Flags are refused. |
| `bind SOURCE PATH` | Capture the existing endpoint or finite endpoint-directory snapshot at SOURCE under PATH. Later source changes do not retarget the capture. |
| `bind -a SOURCE PATH` / `bind -b SOURCE PATH` | Append/prepend a directory snapshot's entries to another endpoint directory. Both operands must be directories; never union files within different role servers. |
| `unmount PATH` | Remove the compiled endpoint or directory subtree at PATH. Missing mappings refuse. There is no hidden older mapping to uncover and no effect on open streams or grants. |
| `cd PATH` | Set the compiler's base for relative SOURCE and PATH operands, initially `/`. This is lexical; it neither requires an existing directory nor changes the child's working directory. |
| `include FILE` | Expand a pinned trusted fragment inline. Its `cd` effects persist after return. FILE resolves under the trusted configuration root, independently of `cd`. |

Targets create necessary structural parent directories. Bind sources must
already exist; a snapshot is captured before installing it, so even overlapping
source/target names cannot create recursive aliases. A leaf cannot be a parent.
Paths are case-sensitive UTF-8 names normalized within the recipe root; `.` and
`..` are resolved lexically and escape above root is refused. Reject NUL/control
characters, discovery-name collisions and symlink escape in include loading.
The structural root cannot be removed or replaced by an endpoint.

At an occupied target, an unflagged operation is idempotent only for an identical
endpoint reference and grant scope, or identical directory snapshot. Otherwise
it requires explicit `unmount` first. Directory composition preserves member
order and each member's entry order. Equal relative leaf names deduplicate only
when reference and grant scope match; different references/scopes and leaf vs
directory collisions refuse. Distinct aliases of one service remain visible.
There is no lower-member lookup after an authorization failure.

This deliberately differs from [Plan 9 bind](https://9p.io/magic/man2html/1/bind),
whose unflagged operation replaces directly. Source capture and ordered directory
composition are borrowed; creation flags, cache flags, environment expansion,
`import` and `clear` are unsupported. Sophia's `include` corresponds to the
inclusion operation spelled `.` in [namespace(6)](https://9p.io/magic/man2html/6/namespace).

Bound compilation to 64 KiB aggregate source, 1,024 expanded operations, include
depth 8 (top level depth 0), 32 distinct include files, 64 exposed endpoint paths
(aliases count), 1,024 bytes per normalized path and 255 bytes per component.
Each expansion counts toward source/operation limits, even for a repeated
fragment. Reject cycles on the active include stack and changes to pinned input.
These are design limits, not measured capacity. Failure publishes nothing.

### Worked policy recipes

The following separates policy inputs from naming text. The core configuration
adapter is described below; this is not `desktop.kdl` syntax.

| Recipe | Namespace and portal participation | Host policy and independent role grants |
| --- | --- | --- |
| Shared X | Session-selected ClassicShared context; explicitly approved clipboard request/publish capabilities | Trusted host reach; no administrative or injection grant implied |
| Confined X | A distinct Confined group; zero portal capabilities by default | Required t142 group path exclusion and selected host restrictions; no ambient cross-group grant |
| Observer | Its own admitted context; request-only screen-capture participation for policy-approved targets | Required selected confinement; read-only status/inspection and capture requests; no portal approval, clipboard, administration or injection |

Shared and confined recipes can use the same naming fragment with different
approved `x-group` references, contexts and capabilities:

```text
namespace-recipe 1
cd /roles
mount x-group x11
mount portal portal
```

`x-group` is an X11 listener, not a 9P export. Its conventional X socket lives in
t142's separate one-entry group directory; discovery and portal endpoints must
not be inserted into that directory. Neither recipe joins resource namespaces.
An exposed portal endpoint with zero participation capabilities grants nothing.

The observer's naming fragment is:

```text
namespace-recipe 1
cd /roles
mount inspection inspection
mount portal portal
bind inspection status
```

Here `status` is an endpoint alias, not a new status-file implementation. One
allowed capture creates a grant-bound object in the granting role's tree; it
does not edit the base naming recipe. t315/t319 own execution and capture bytes.
The t318 authorized developer launcher selects an approved recipe for an
independent executable; a requested role cannot grant itself. Directly launched
local client provisioning remains t324, and no remote authentication is implied.

### Trusted launch policy configuration

The core `session` block accepts one `namespace-policy` block. It contains at
most 64 named recipes and 1,024 exact application assignments. Recipe names are
bounded to 64 lowercase ASCII letters, digits, underscores or hyphens. Application
identities are the catalog's `registered:<id>` or `desktop:<desktop-file-id>`;
an executable path never substitutes for an identity.

```kdl
session {
    namespace-policy {
        recipe "browser" profile="confined-x" group=7 {
            request "clipboard"
        }
        recipe "tools" profile="observer" developer-launch=#true
        application "registered:browser" recipe="browser"
        application "desktop:org.example.Browser.desktop" recipe="browser"
    }
}
```

Profiles are `shared-x`, `confined-x` with a nonzero group, or `observer`.
The first two have no added portal participation by default. `request` and
`publish` select distinct participation in clipboard, drag-and-drop, file-handoff,
screen-capture, screen-recording, URI-open or notification transfers. These
capabilities do not approve individual transfers. Observer permits only
screen-capture requests and supplies that default; it cannot publish or acquire
clipboard participation. Developer launch is off by default and requires a
confined profile plus the separately admitted administrative launch request.

An optional `source="recipes/custom.ns"` names a normalized relative path under
the trusted core configuration root for the existing pinned source loader.
Without it, the profile requests its shipped naming fragment. Applications
cannot provide that path. Unknown properties, duplicate names or assignments,
unknown recipes, unsupported capabilities and excess bounds refuse the candidate.

```text
trusted core policy -- exact catalog identity --> assigned recipe
app recipe request ----------------------------> equality check only
                                                   |
                           naming + participation + host profile
                                                   |
                           Session launch/view owner
```

An unmapped application with no recipe request keeps the existing launch policy.
Metadata may confirm its assigned recipe but cannot select a different one;
an unassigned request refuses. Desktop files use the optional
`X-Sophia-NamespaceRecipe=browser` key in `[Desktop Entry]`. Its value uses the
same 1–64 character recipe-name grammar as trusted configuration. Missing
metadata does not opt out of an assigned recipe: both registered and scanned
applications receive their trusted assignment automatically. Invalid or
unapproved requests remain visible as unavailable catalog entries.

The catalog retains the resolved recipe in command equality. Its worker
revalidates the exact catalog identity, source bytes and command before dispatch;
changing only the desktop recipe key requires reopening the launcher. A
`Terminal=true` wrapper preserves the application's recipe selection, rather
than substituting the terminal entry's identity. Session routes the selected
command through asynchronous namespace custody. Raw-process constructors still
refuse recipe commands; they cannot discard the recipe and spawn on the host.

Recipe policy changes require restart until live
launch-policy replacement is implemented. Recipes require a normal Session with
Factotum configured. Observer recipes also require the WM inspection owner.
Startup derives confined X groups from recipes before preparing listeners and
the sandbox-hostname cookie. Explicit CLI groups remain supported within the
same eight-group bound. Selecting both a recipe and the legacy confined-app
mapping for one registered identity refuses configuration.

Portal participation belongs to the actual namespace: recipes deliberately
sharing an X group contribute to that group's request/publish capabilities.
These capabilities do not approve transfers or bypass the exact clipboard-pair
policy. Observer recipes have separate private namespaces. A policy reload
requiring different recipes or groups remains pending restart; it does not
mutate the running Session's listeners or launch authority.

The launch scheduler now distinguishes host custody from containment. A
`shared-x` recipe uses an explicit supervisor-owned host-client handoff: the
child retains normal host filesystem, network and environment access, but
inherits no non-stdio descriptor except its fresh authentication channel.
This records exact launch custody without claiming protection evidence. It
cannot authorize a WM, shell, lock, output or broker role. A confined recipe
requires the exclusive namespace-client protection domain and has no host
fallback. Both paths use the same Factotum registration, endpoint grant and
retirement owners. The recipe profile must match its live namespace profile.
Catalog, shortcut and startup launches now use this scheduler when trusted
policy selects a recipe. Missing prepared resources refuse the selected launch.

`ApplicationRecipeSet` loads immutable policy and naming inputs on the startup
worker. Its exact-identity selection supplies a shared snapshot to each launch;
an unauthorized metadata request is an error, never a legacy-launch fallback.
`RecipeLaunchTarget` binds that selection to Session's namespace context,
principal, captured X group, portal recipient and inspection generation. It
checks participation against the context before creating launch custody.
Shared X retains explicit host policy, confined X reuses t142's group/cookie
validation and sandbox grants, and observer has no X or host-admin binding.

```text
trusted recipe set + verified application identity/command
                          |
                  exact policy selection
                          |
Session X group / portal recipient / inspection generation
                          |
                 resolved launch + role owners
                          |
            namespace scheduler and managed process
```

Before spawning, the preparation worker rechecks the conventional X group's
one-entry layout, cookie and captured socket inode. Replacement refuses rather
than rebinding the published alias. The existing t142 pathname-validation race
limit still applies between that check and Bubblewrap's bind; this is not a new
claim of atomic filesystem custody. Session captures these resources once after
the X listeners are ready. Primary and secondary startup apps, shortcut targets,
and both catalog paths use the same recipe/resource owner.

```text
registered startup / shortcut      verified catalog identity
              \                         /
               +-- Session recipe owner --+
                         |
           selected recipe + captured service owners
                         |
                 asynchronous preparation
                         |
         managed process readiness / failure / exit
                         |
              correlated launch outcome
```

Startup's first-frame wait advances authentication and preparation while
retaining the first lifecycle event for the owner loop. A pending launch is
reported as preparing, not started. A shell launch outcome retains its original
activation until process readiness or failure; it cannot claim successful
launch merely because preparation was queued. Readiness is not proof of a
window, pixels or an authenticated application operation.

Inspection dispatch now retains an acceptor for the exact published service
generation. Its worker serves the client's inspection listeners through the
existing authenticated inspection export and shares that export's connection
limits. It polls quiet connections within the existing idle-check bound.
Revoking a grant closes its retained reads; retiring the inspection service
stops its dispatcher without revoking the client's other grants. The launch
retirement worker joins dispatch before releasing the client slot.

```text
recipe endpoint + endpoint grant + captured inspection generation
                              |
                    bounded dispatch worker
                              |
               existing authenticated inspection export
                    /                         \
          Factotum admission             inspection publication
          and retained-fid checks        snapshots and event journal
```

The same bounded launch dispatcher serves the
[portal recipient view](portal-client-files.md). Its objects carry the common
coordinator's transfer lease; opening the role does not approve transfers.
Normal application call sites are connected. The candidate's
[developer-launch 9P interface](developer-launch-files.md) uses the same launch
owner for an arbitrary executable under an explicitly approved recipe. It
requires host-admin access and a recipe with developer launch enabled; it stays
disabled by default. Its result reports process readiness, not window creation
or successful application authentication.

When an observer recipe is configured without host inspection access, Session
creates a private inspection service without publishing a host socket. Future
launches capture the current inspection generation after WM fencing; old
connections remain bound to their retired generation. This does not grant
observer clients administration or input injection.

Session's managed application record can now retain ordinary host children or
asynchronous namespace application custody. A namespace application's state is:

```text
Preparing (slot + reservation + view worker)
    |
    +-- cancellation/failure --> revoke --> background retirement
    +-- process exits before activation --> actual exit status + retirement
    |
    v
fork held before exec --> capture monitor PID + start time --> release exec
    |
    +-- cancellation / revoked reservation --> prevent exec + retirement
    |
    v
supervised spawn + authentication activation + role dispatch
    |
    +-- missing/mismatched owner --> local failure + background retirement
    +-- X app discards optional protocol --> revoke 9P --> X process retained
    |
    v
Started (monitor PID and peer PID kept distinct)
    |
    +-- X app loses protocol authority --> revoke 9P; keep X process
    +-- namespace loss / exit / cancellation --> revoke --> background retirement
    +-- required protocol loss (observer/role) --> revoke --> retirement
```

Started and terminal observations occur once. Session captures process provenance
before releasing the fork child to exec, so a first X connection cannot precede
the managed launch identity. The monitor identity attributes descendants; it is
separate from the actual peer identity and grants no authenticated authority.
The private handshake is single-use, wakes the owner without waiting there, and
fails closed after ten seconds or cancellation. It is consumed only by the actual
spawn, never the containment driver's version probe. Namespace cancellation never
synchronously reaps on the Session
owner thread; the existing launch slot remains occupied until background cleanup
finishes. Ordinary host children retain their existing process-group cleanup.
The managed record preserves catalog transaction, origin and diagnostic-ticket
fields; connecting selected commands to this record remains part of launch
integration.

Application diagnostics can reserve a bounded record before asynchronous spawn
and move its stderr writer into the existing process supervisor. The descriptor
belongs to one launch, is not cloned with its recipe and is not retained for a
restart. The spawn owner records the actual PID or failure and retains the
resulting diagnostic ticket for exit observation. Dropping an unsettled
preparation closes it as failed; diagnostics exhaustion never refuses an app.
The ordinary command launcher uses the same reservation and settlement path.
The namespace scheduler retains this diagnostic owner through preparation,
spawn, activation and background retirement. The spawn worker records the
actual spawn result; the process owner records reaped exit status, including
cancellation. A failed dispatch still reaps and records the spawned process.
An already reaped process reports its exit before a simultaneous authentication
retirement can turn that exit into an authority-loss failure.

An ordinary shared or confined X application need not retain or implement the
optional 9P launch channel. After the validated X launch passes its exec gate,
loss of authentication authority revokes its 9P grants and dispatch without
terminating the otherwise live X process. This also applies before authentication
activation completes: process custody is retained without creating a factory or
claiming Active protocol authority. Retained protocol handles become stale and
cannot revive. The original X group, containment and process diagnostics remain
unchanged; no new permission or host fallback is granted. Namespace liveness is
checked independently, including before releasing exec. Namespace retirement,
explicit cancellation and process exit still retire the application. Observers
and fixed desktop roles retain their required protocol-lifetime rules. Started
is a process observation, not proof of an authenticated operation or a window.

A short-lived namespace application may exit before activation. Session reports
the observed process status exactly once without inventing Started or Active
custody, and retires the published grants. For protected launches the status is
Bubblewrap's process status, as for an ordinary protected reap; it is not proof
that an application operation succeeded. Fixed desktop roles still require a
live ready peer. Channel EOF while the spawn worker is pending cannot erase a
subsequent exit result; the existing startup deadline bounds that wait.

`LoadedNamespaceRecipe` retains a policy snapshot together with its shipped
naming fragment or pinned custom files. Shared and confined X use the same
fragment; their approved group references differ. Selected publication restricts
the service catalog to `x-group` (X11, interface `x11`) and `portal` (9P, interface
`portal-client`), or `inspection` and `portal` for observer. This recipient view
is distinct from the trusted protocol-authority `portal-requester` export:
clients receive neither its upload interface nor execution custody. The observer's
`status` alias captures the same inspection reference and grant. A custom naming
file may rearrange these services but cannot introduce other roles. Missing
required services refuse the entire publication.

```text
selected policy + pinned source + Session-approved references
                             |
                      existing compiler
                             |
                     private candidate view
                     /                    \
             9P role endpoints          X11 endpoint
             new local listeners        hard link to captured socket inode
                     |                    |
             existing role owners       existing frontend listener
                     \                    /
                      atomic publication
```

X11 materialization captures an owned socket in a private group directory using
an `O_PATH` descriptor. Linux `linkat` through that descriptor creates aliases
without resolving the original pathname again, changing socket permissions, or
creating a proxy. Source and view must support hard links on the same filesystem;
failure refuses publication, with no copying or pathname fallback. Candidate
cleanup removes only its names, leaving the frontend socket intact. The one-entry
X group directory is unchanged. X11 setup, cookie authentication, namespace
admission and revocation remain frontend responsibilities; the alias supplies
none of those permissions.

The inspection implementation shares its existing publication owner between
explicit HostDomain inspection and authenticated observer connections. A private
owner creates no host listener. Its observer route requires a launch-authentication
factory and the service generation captured when the namespace endpoint was
published; it cannot substitute the owner's current generation after a fence.

```text
Session-approved launch + captured namespace endpoint generation
    |                         |
    | launch channel          | accepted observer stream
    v                         v
Factotum verdict ------> authenticated 9P attach
                              |
                              v
                    existing inspection export
                    api / snapshot / events
                              |
                    copied safe publication data

launch/namespace revoke --> admission checks --> close retained connection
source generation fence --------------------> refuse old endpoint and fids
```

This runtime route and its real launch-authentication join are implemented;
automatic recipe selection and launch wiring remain t318 integration work.
The join supplies launch policy explicitly and uses the exclusive
`NamespaceClient` supervised custody role. This identifies a confined recipe
client without borrowing shell, WM, broker or frontend custody; it grants no
endpoint operations by itself. Session still selects each endpoint grant and
audience independently. Shared-host application launches are unchanged.
Connection owners must
service readiness and the 20 ms quiet check bound; revocation must not depend
on the reader sending another request. These are liveness bounds, not latency
claims.

Endpoint grants use Session's existing namespace/admission registry. Each
grant pins the namespace, principal, launch generation, audience and full
service reference. The compiler receives passive scope numbers; authentication
workers retain an owner-issued lease and compare it with the captured endpoint.
Matching numbers from another registry confer no authority. Aliases retain one
grant, while independently approved clients can hold separate grants for the
same service identity. Revoking one grant invalidates its retained admissions
and future attaches without revoking unrelated grants or the entire launch.

```text
one service identity + generation
        ^                   ^
     grant A             grant B
      /   \                 |
   path X alias Y         path Z
      |                     |
 retained admission A   retained admission B
      X                     |
 revoke A              still permitted
```

The registry bounds live endpoint grants to 4,096 (64 launches times 64
endpoints); aliases need no additional grant. Revoked entries reclaim capacity
without reusing grant IDs. Namespace or registry retirement fences all affected
leases. Session must retire a launch's grants with that launch; the existing
supervisor custody check independently refuses a dead launch. A service
replacement requires a fresh grant, and a failed naming publication cannot
revive a revoked one.

Session's `NamespaceRecipeOwner` joins pinned configuration, approved service
references, grants and filesystem publication for one launch. It first accepts
a strictly newer policy revision and applies its authority reductions, then
compiles and materializes the view. It rechecks pinned source bytes immediately
before publishing. Failure drops newly issued grants and candidate resources;
still-approved old grants remain live, reduced grants stay revoked, and the old
naming view remains current. A failed accepted revision cannot be retried as a
stale policy snapshot. Explicit later authorization needs a new revision and
fresh grant incarnation.

```text
trusted pinned source + approved service catalog + launch identity
                          |
                          v
              accept newer policy revision
              retain unchanged grants / revoke reductions
                          |
                          v
              compile -> materialize -> recheck source
                          |
                  atomic current-name replacement
                          |
              listeners + captured service/grant references
                          |
                existing authenticated role owners

failure: old names remain; new grants/resources are dropped;
         revoked grants are never restored
```

Grant custody drops without waiting for the registry mutex; connection leases
cannot prolong it. The publication worker holds that mutex only for authority
bookkeeping, never for filesystem I/O. Automatic application selection,
runtime endpoint dispatch and launch scheduling remain t318 integration work.
The protected SDK join uses this publication owner, while supplying its policy
catalog and dispatching the resulting listeners directly to inspection.

### Revisions, discovery and retained authority

Recipe revision identifies a naming/policy snapshot. Service identity and
generation identify an export instance. Admission epoch bounds a connection's
authority. Output generation identifies a physical target. These are separate:
rebinding a name to existing service B preserves B's identity/generation; a
service restart changes its generation. Aliases do not manufacture new services.

Publish a new sealed view for newly admitted connections when policy changes.
Existing admissions keep their original view; neither a path replacement nor
Tversion changes their pinned principal or namespace. A retained stream/fid
continues to name its old owner while its authority remains valid. Before
publishing an authority-reducing replacement, revoke the affected old authority;
if replacement publication fails, that revocation is not undone. Pending
delivery must recheck revocation. Unrelated grants survive. A new walk on an
old role connection cannot traverse into the replacement endpoint's export.

Sharing a NamespaceId is explicit Session policy. Child launches receive a
selected recipe, and new connections authenticate independently; inherited or
intentionally transferred authenticated streams retain their existing identity,
as specified by t133. Sharing recipe text or a path does not prove custody.

Each discovery snapshot names its recipe revision, relative endpoint paths,
protocol/version, opaque service identities/generations and availability. It
lists only that view's endpoints; credentials and raw policy secrets are absent.
Role version/capability files remain authoritative. Availability is a bounded
observation, not proof of authorization or currently presented output. Refresh
discovery atomically without retargeting an admitted binding. t318 owns the
versioned serialization, reserved filename and SDK reader; t257 owns the
available/waiting/recovering status vocabulary. No single 9P root forwards into
the listed services.

## Session Profiles

### Classic Shared-X

A trusted X session assigns participating clients the same namespace. Those
clients retain ordinary shared-X inspection, coordination, selections, and
existing-resource access. Per-connection XID ranges prevent creation collisions
and provide cleanup attribution; they are not access-control lists.

### Confined

A confined client group receives a distinct namespace and an explicit
capability set. Resource lookup, event subscription, properties, selections,
input delivery, metadata, and transfers fail closed across namespaces unless a
specific portal flow grants the operation.

The live X launcher selects this profile with
`--namespace-profile=confined`. Each run allocates a fresh confined namespace
with explicit zero capabilities; all clients deliberately launched as part of
that group share it. A session can also configure additional confined groups
with `--confined-group=INDEX`, each on its own listener, as described below.
Separate group credentials on one listener remain future supervisor work.
The socket suite proves that two policy-assigned confined
namespaces cannot map each other's windows, mutate their properties, claim
their selections, forge a selection requestor, or emit metadata from a rejected
foreign property write. It also proves that a rejected foreign event-mask
request cannot redirect brokered input: the addressed worker retains its root
target, while the broker's private queues keep delivery client-specific.
Broader XKB, XI2, and grab semantics remain X11 session-correctness work rather
than a namespace exception.

The routed frontend accepts supervisor revocation by opaque
`ClientAdmissionId`. It shuts down only the matching worker, which then removes
its private routes and connection-owned resources, emits surface cleanup, and
revokes the immutable admission lease in the same order as an ordinary
disconnect. A socket regression proves the other client in a classic namespace
remains connected and can continue creating resources.

Capabilities bound which portal operations a namespace may request or publish.
They do not provide ambient cross-namespace access and do not replace a portal
decision for a particular transfer.

A future frontend must assign every admitted client a Sophia namespace context
so portal and metadata policy retain the same trust model across protocols.

## Socket Directories

A client group reaches its listener through a directory, and only through a
directory. The layout is the session's; what mounts it is the sandbox's, and
the contract between them is written here so that neither has to know the
other's name.

The session owns one runtime root per display, under the user's runtime
directory:

```text
$XDG_RUNTIME_DIR/sophia/display-<n>/
  shared/X<n>          the trusted listener, for classic clients
  confined-<k>/X<n>    one listener per confined client group
```

Every group directory is owner-only, contains exactly one entry -- the socket
named `X<n>` -- and contains no symbolic link. The socket name is the same in
every group so that a client whose directory is mounted at the standard X11
socket path sees `:<n>` and nothing about its launch has to change.

**The contract a sandbox must keep.** A confined client is given exactly one
group directory, mounted read-only at `/tmp/.X11-unix` inside the sandbox, and is given
nothing else under `$XDG_RUNTIME_DIR/sophia/`. That is the whole of it. It
names no sandbox: Bubblewrap satisfies it with one bind, an unshare-based
launcher satisfies it with one mount, and the session refuses to know which.
What the contract buys is exit 3 of the socket-directory plan: a confined
client cannot *reach* the trusted socket, as distinct from being denied at it.
Denial is admission's job and stays so; the directory is a second layer under
it, and a client that finds the trusted path simply absent has nothing to
present credentials to.

**What it does not change.** The listener is still transport, not identity.
Which directory a connection arrived through proposes a namespace; admission
still decides, from peer credentials and policy, and still refuses a listener
it has no group for rather than defaulting. The MIT-MAGIC-COOKIE stays one per
session, shared across directories: path exclusion, not the cookie, is what
separates groups, and a group that could read another's directory would have
the cookie already. The classic path `/tmp/.X11-unix/X<n>` on the host remains
valid for clients outside any sandbox.

### Configuring confined application groups

`--confined-group=INDEX` adds a group with a distinct confined namespace, with
no portal capabilities by default. Indices are explicit nonzero `u32` values, stable across configuration
reordering. Duplicate groups are refused; at most eight are accepted. The
existing synchronous frontend serves every listener with one accepting thread.
Worker counts and idle reclamation include every listener. The default sixteen
worker slots are divided between listeners, with any remainder reserved for
the primary listener. With eight confined groups, each gets one slot and the
primary gets eight. A full group defers acceptance without stopping other
groups; incomplete X setup also occupies a slot.

`--clipboard-rule=confined-7:shared` explicitly allows Clipboard and PRIMARY
from group 7 to shared. Selectors are `shared` or a configured `confined-N`.
Rules are directional, distinct and bounded to sixteen; the empty default
denies cross-group transfers. Session adds only the corresponding Clipboard
publish/request capabilities and checks the exact pair, so A-to-shared plus
shared-to-B does not allow A-to-B. There is no per-paste prompt. The
[portal files contract](sophia-portal-files.md) records the private transport,
execution fence and remaining installation/physical-acceptance boundary.

In normal sessions, `--confined-application=APP_ID:GROUP_ID` maps a registered
application to a configured group. The application must also appear in the
selected catalog. Catalog launches, shortcuts and secondary startup launches
consult the same mapping. A mapped launch that cannot be confined fails; it
does not fall back to a host command. The primary startup application and the
catalog terminal cannot be mapped in this implementation. Host working
directories are refused for confined launches.

The mapping names a registry identity, not an executable. Scanned desktop
entries remain unmapped even when they invoke the same executable. This is
not an executable-wide confinement policy, nor a new `client-launch` mode.

The runtime uses the shared Bubblewrap builder with an exclusive application
role. It mounts only the selected socket directory and the session cookie file
read-only, supplies `DISPLAY` and `XAUTHORITY`, and gives the child private
network, `/tmp`, `/run` and home namespaces. No host control or inspection
endpoint is added. Private networking also excludes the host's abstract X
sockets. This bounded launch has no host home, network, D-Bus or GPU access.
Both the Bubblewrap version probe and launch clear the host environment and
mark inherited descriptors above standard I/O close-on-exec; unsupported
`close_range` fails the launch closed.

Session owns the Bubblewrap monitor as the child. On Session termination of
that process group, the sandbox is torn down; this does not deliver a graceful
termination signal to the application in its separate session.

When groups are configured, the cookie file adds a `FAMILY_LOCAL` record for
the sandbox hostname using the same display and secret. It keeps the host
record and uses no wildcard. Ordinary client cookie lookup therefore works
inside the separate hostname namespace.

`--admit-xtest` retains its primary-group meaning. The explicit forms are
`--admit-xtest=shared` and `--admit-xtest=confined-INDEX`; at most one configured
group can be selected. All forms are refused alongside an input proof.
Physical acceptance of shared and confined applications remains the separate
t142 live gate; deterministic socket and sandbox tests do not establish it.

## Admission

### Relationship To Scripting

A resource namespace is distinct from an OS process protection domain and
from permission to control the desktop. A confined X namespace alone does not
prevent a process from reaching a host Unix socket. A new scripting connection
must receive its own verified session admission and command authorization;
X admission, matching UID, or a caller-supplied namespace ID is insufficient
to establish that authority implicitly.

[Scripting Sophia](scripting.md#namespace-and-caller-security) owns the target
distinction between namespace-scoped automation and deliberately authorized
desktop administration. Neither grants application-data disclosure or portal
transfers by implication. Its proposed public endpoint is unimplemented and
does not change the namespace or portal capabilities described here.

### X Client Admission

An X listener is transport, not identity. After setup authentication and before
allocating an X client identity or resource range, accepting a connection
consults a session admission interface using peer credentials, configured
policy, and the bounded `MIT-MAGIC-COOKIE-1` result when enabled. Successful
admission returns one immutable `ClientAdmissionContext`; failure sends normal
X11 setup failure and creates no client resources. The live classic session
currently requires a kernel-authenticated peer UID matching the session user,
allocates a distinct registry admission for every connection, and revokes it
after connection cleanup. Deterministic regressions cover denial, simultaneous
admissions, normal disconnect, and dispatch-failure cleanup.

The production frontend must not infer identity from one hardcoded
`NamespaceId`. Classic policy may deliberately return the same namespace in
distinct admission contexts; confined policy must return the namespace and
capabilities selected for that client group. Cookie creation, Xauthority-file
publication, rotation, removal, and raw-secret handling are supervisor
responsibilities. The live supervisor generates a fresh kernel-random cookie
per session, publishes a standard owner-only record only after it is fully
written, passes its path to launched clients, and removes it on normal or error
teardown. The frontend validates configured setup authorization before calling
policy and never exposes raw cookie material to admission records, diagnostics,
or Engine data.

Disconnect revokes the connection context, releases its creation ledger, clears
its owned selection generations, unregisters input/control routes, and closes
active grants whose validity depended on that client.

The WM remains namespace-blind. It receives opaque `SurfaceId` layout nodes and
never receives namespace IDs, XIDs, credentials, titles, classes, PIDs, paths,
or portal payloads.

## Portal Contract

Portal policy is a reducer over bounded facts. Runtime execution owns payloads,
file descriptors, protocol replies, UI, and external launchers. Sophia Engine
is not a portal broker and portal policy is never part of the input or frame hot
path.

The target portal taxonomy is explicit:

- clipboard;
- drag-and-drop;
- file handoff;
- screen capture;
- screen recording;
- URI open;
- notification.

`PortalTransferKind` now represents all seven directly. Screenshot capture and
screen recording are distinct kinds, URI-open no longer aliases notification,
and every kind maps to exactly one directional `NamespacePortalCapability`.
MIME and type hints may refine a request but never determine its authority.

A request contains a transfer ID, source and target namespaces, kind, source
generation, bounded metadata, and a deadline. It carries no raw application
object ID, pixel buffer, clipboard payload, file descriptor, URI launcher, or
unbounded string.

The lifecycle has two layers:

```text
request: Pending -> Allowed | Denied | Revoked
grant:   Active  -> Completed | Revoked | Expired
```

An allowed decision creates a scope- and generation-bound grant. Grants are
single-use unless their kind explicitly defines a bounded stream. A stale
source generation, source or target disconnect, deadline expiry, policy
failure, executor failure, or broker restart revokes or denies the request.
Unknown states fail closed.

The policy provider receives only sanitized request facts. A deterministic
headless provider supplies allow/deny decisions for tests. A later prompt UI is
an interchangeable policy provider, not authority or Engine logic.

## X11 Clipboard And PRIMARY Reference Flow

Same-namespace selections use ordinary X11 semantics. A selection request whose
owner and requestor are in different namespaces becomes a portal request.

The native frontend implements that same-namespace path as client-addressed,
bounded protocol-event queues. `ConvertSelection` delivers `SelectionRequest`
to the owning connection; the owner may return only a core `SelectionNotify`
whose destination and requestor agree. The frontend routes that event to the
requestor connection and rewrites its sequence for that connection. This does
not grant either client cross-namespace resource access.

The X frontend retains requestor XID, selection atom, target atom, property,
timestamp, and source-owner generation. The broker sees only namespace IDs,
transfer kind, normalized target/MIME, bounded size facts, and generation.

After an allowed grant, the frontend allocates an authority-private proxy XID
below every client allocation range and delivers a normal `SelectionRequest`
to the source owner. Only clients in that source namespace may write the proxy
property or return its `SelectionNotify`. The frontend captures and deletes
that property, emits only the correlated bounded bytes to runtime, and keeps
the broker connection open until the target executor succeeds or fails. The
target executor rechecks grant state, namespaces, and owner generation before
installing the requestor property and routing `SelectionNotify`.

The first complete adapter supports `CLIPBOARD` and `PRIMARY` with `TARGETS`,
`UTF8_STRING`, and bounded UTF-8 `text/plain` data. Approval is valid only for
the observed owner generation. Denial, expiry, stale ownership, unsupported
targets, or executor failure becomes normal X11 selection failure through
`SelectionNotify(property = None)`. The session is never frozen while policy is
pending.

Large `INCR` transfers and broader target conversion remain compatibility work
after the bounded reference flow is proven.

## Invariants

- Namespace identity is assigned once at admission and never inferred from
  client-controlled metadata.
- Classic sharing is explicit session policy; confinement is not simulated by
  treating XID ranges as ownership ACLs.
- Cross-namespace lookup and delivery fail closed without a live grant.
- Portal grants authorize one bounded operation, not general resource
  visibility.
- Engine and WM never receive credentials, portal payloads, or raw
  protocol-request context.
- Protocol denial maps to native protocol failure, never synthetic input.
- Policy reducers remain deterministic and I/O-free; executors own effects.

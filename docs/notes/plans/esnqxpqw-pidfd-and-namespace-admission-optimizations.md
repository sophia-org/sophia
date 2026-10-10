---
id: esnqxpqw
date: 2026-09-20
kind: plan
tags: [plan, security, session]
---
# Admission identity and launch custody boundary (t133)

## Scope and correction (2026-10-10)

This is the proposed implementation boundary for the accepted
[one-core decision](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md).
niltempus authorized the t249 evidence audit and t133/t275 design together.
This design does not implement authenticated attach, change X admission, or
remove today's peer checks. t317 owns implementation after this boundary and
its authentication exchange are reviewed; t275 owns the recipe consuming it.

The earlier plan asserted that comparing pidfds proves ancestry across
reparenting. That assertion is withdrawn. A pidfd refers to a task; it is not
an ancestry certificate. See [pidfd_open(2)](https://man7.org/linux/man-pages/man2/pidfd_open.2.html).
No sub-millisecond admission result was measured. Keep those claims out of the
exit; measure admission cost and bounded overload behavior on the implemented
candidate. The stable note ID preserves links to the corrected plan.

## Current boundaries, source b0ca8280a

| Owner | Observed behavior | Consequence for the design |
| --- | --- | --- |
| `live_session/x_frontend.rs::LiveXAdmissionPolicy::admit` | Checks peer UID, admits the configured namespace, then records a best-effort launch-origin hint. | Missing ancestry does not deny admission or select a different namespace. Keep X behavior unchanged. |
| `launch_origin.rs::process_ancestors` | Bounds depth at 64, checks PID/start time, rereads the chain; missing or changed observations yield an empty vector. | Workspace provenance is not proof of namespace or launch custody. |
| `sophia-runtime/src/session/namespace.rs` | Owns monotonic namespace/admission IDs and revokes immutable contexts. | Reuse this owner rather than introducing a second identity registry. |
| `sophia-9p/src/wire.rs` and `connection.rs` | Tauth is unsupported; attach refuses an afid other than NOFID. | Factotum export availability is not authenticated attach support. |
| `sophia-9p/src/export.rs` | Attach receives connection, peer and claimed names; every operation checks the attached epoch. | Add verified authentication through the core's neutral seam, keeping domain policy outside the core. |
| `sophia-runtime/src/lock_files/transport.rs` | Checks the supervised assignee and connector pidfds before adopting the stream. | Preserve this control until its authenticated custody replacement passes equivalent negative tests. |

`ClientAdmissionContext` currently contains client ID, namespace and sanitized
authentication provenance, not an authenticated principal. A future change
must name that missing binding explicitly in the admission owner; a supplied
uname or aname must never be treated as it. Secrets and challenges stay outside
the passive context and outside WM snapshots and durable logs.

## Proposed state and ownership

1. A new connection has no admitted principal. The 9P core bounds framing,
   tags, fids and pending authentication work. Session policy chooses the
   allowed authentication mechanism and role; the client cannot downgrade it.
2. A factotum-backed exchange yields a verified principal and an authentication
   result bound to this connection, role audience and session generation.
   The requested namespace is resolved by Session policy, not trusted from a
   name in the request. An authentication fid can be reused only within its
   authenticated scope; it cannot transfer identity to another connection.
3. The first successful attach fixes principal and namespace for the lifetime
   of the transport connection. Later attaches prove the same identity and
   receive only subsets of its admitted grants. A failed attach changes
   neither existing fids nor identity. Tversion may release wire state but
   must not permit an identity switch on the same connection.
4. Session produces the immutable admission and a sealed recipe selection.
   Role owners receive only the facts they need. Walk, open, pending-read
   completion and every retained-handle operation check current admission,
   role epoch and any object grant. Authentication never bypasses revocation.
5. Disconnect releases authentication conversations and role resources;
   revocation cancels unfinished work. Reconnect obtains fresh admission and
   epochs. Reused numeric fids and paths do not revive old authority.

[Plan 9 auth/attach](https://9p.io/magic/man2html/5/attach) provides the afid
conversation shape, not Sophia's authentication protocol. Before t317 code,
specify the actual factotum mechanism, challenge/replay binding, result lifetime
and SDK exchange. Today's PAM unlock conversation is not silently reused as
a reusable socket credential. The bootstrap must give a new client access only
to its authentication conversation, without first admitting it to protected
role files or exposing factotum control/key administration.

## Custody, transfer and threats

Authentication answers who; supervisor custody answers which authorized launch
created the child; grants answer what that connection may do. Keep all three
facts distinct. A custody-required role must join a live supervisor launch
record, its generation and assigned role to the authenticated connection.
The launch record is created by the supervisor's actual spawn, never reconstructed
from a claimed PID, matching UID, environment variable or ancestor hint.

The t317 exchange must prove possession tied to that launch record, with replay,
wrong-role and wrong-session refusal. Merely sending its public record ID is
insufficient. Until the bootstrap/channel binding is implemented and tested,
the existing Linux peer checks remain required. No new cryptographic protocol
or optional-peer-check claim follows from this design.

| Threat | Required behavior |
| --- | --- |
| Double fork, orphan or PID reuse | Lose optional workspace attribution if necessary; never gain another namespace or manufacture custody. An unregistered descendant has no automatic custody-required role. |
| Nested PID namespaces or unreadable proc | Platform evidence may be unavailable; it is never a reason to widen admission. A role still requiring that evidence refuses. |
| Forwarded authenticated socket or inherited fid | It remains the same connection and admission, not a new authenticated person. Restrict transport/credential delegation with host containment; cannot promise to stop a trusted client intentionally proxying its authority. |
| Stolen or replayed launch proof | Refuse wrong connection/audience/generation and spent or revoked proof without consuming an unrelated valid launch. |
| Same UID but unlaunched peer | Authentication alone cannot acquire a supervised WM, lock or output assignment. |
| Auth/attach flood or stalled factotum | Bound concurrent connections, conversations, bytes, pending requests and time per principal/endpoint and globally; no blocking work on Engine. Per-UID rate limiting alone cannot enforce namespace fairness. |
| Revocation races with read completion | Recheck before delivery; no new protected bytes after revocation takes effect. Bytes already delivered cannot be recalled. |

The socket-transfer limit follows the connection model:
[unix(7)](https://man7.org/linux/man-pages/man7/unix.7.html) describes descriptor
passing as sharing an open file description. Re-reading original peer credentials
does not authenticate the process currently holding a transferred stream.

## Tests and implementation handoff

The first t317 change needs a state model and device-free controls before
production use. Its required cases are: unauthenticated attach; forged names;
second-principal/second-namespace attach; same-principal restricted reattach;
Tversion after admission; cross-connection afid reuse; wrong launch, audience
or generation; reparenting/PID reuse; auth timeout/flush/disconnect; resource
exhaustion; and revocation with an already-open or blocked fid. Failed operations
must preserve unrelated admitted work and reclaim their own resources exactly
once. Retain compiled controls that remove identity pinning, custody binding
and the final revocation check. Then test the independent C/Rust SDK peers.

t142 separately proves two group listeners and actual host-path exclusion,
including inherited descriptors and alternate socket paths, while preserving
CLIPBOARD and PRIMARY portal delivery. The
[recipe boundary](../investigations/kcfh2hdg-adopting-the-plan-9-namespace-model-in-sophia.md#proposed-recipe-boundary-2026-10-10)
consumes these facts; composing a path view cannot substitute for either proof.

This slice supplies a reviewed-source design draft, not those tests or t133
completion. The unresolved implementation choices are the factotum exchange
and launch channel binding, plus numeric authentication resource/time limits.
Resolve them before admitting t317 implementation; no live-session experiment
is needed to resolve them.

## Authentication implementation review (2026-10-10)

Read-only review at `f77244abe`; no authentication code, experiment or change
to current admission. The missing exchange is a concrete implementation gap:

- `sophia-factotum/src/proto/mod.rs` lists only `Pass` and `Pam`.
  `proto/pass.rs` retrieves a password and has no server authentication side.
  `proto/pam.rs` restricts login to `ChannelClass::Session`; its successful
  `AuthInfo` has the owner's name and an empty secret. Neither implements a
  connection-, audience- and launch-bound attach proof. Do not turn the existing
  PAM verdict or password-retrieval reply into such a credential.
- `sophia-factotum/src/export.rs::attach` requires a connection already present
  in `classes`, and its tree includes `ctl`, `rpc`, `proto` and `log`. Merely
  exposing that export to a not-yet-admitted client does not provide the proposed
  conversation-only bootstrap. The restricted bootstrap needs its own explicit
  access boundary and controls excluding key administration.
- `sophia-9p/src/connection.rs::version` resets fids and pending wire state.
  The proposed lifetime identity must survive that reset and end only with the
  transport, while unfinished authentication work is cancelled. Add separate
  assertions for those two lifetimes; retaining fids is not the solution.
- `sophia-protocol/src/packets/namespace.rs::ClientAdmissionContext` and
  `sophia-runtime/src/session/namespace.rs::admit` carry provenance, namespace
  and admission identity, but no verified principal or supervisor launch proof.
  Extending the passive record alone cannot establish either fact.

The next design artifact must specify the supported authentication mechanism,
the private bootstrap/channel binding to an actual supervisor launch, and
numeric conversation limits. Then a device-free state transition fixture can
exercise successful attach, refused identity changes, replay, cancellation and
revocation using explicitly supplied verification outcomes. Such a fixture
would qualify the admission state machine only; actual authentication still
needs the independent peer exchange and its negative controls. t133 remains
open and t317 implementation is not promoted by this review.

## Local authentication design and t317 handoff (2026-10-10)

This section resolves the design choices left open by the earlier review at
source `b089c15d5`. On 2026-10-10 niltempus approved supervised authentication
first for desktop components, an authorized developer launch command, and an
explicit follow-on for independently launched local tools and applications.
This is an implementation target,
not implemented support or authentication evidence. X admission and all current
role checks remain intact.
The first mechanism is **Session-attested, Factotum-verified launch-channel
authentication**. Session and its supervisor are the root of trust: Session
attests channel possession, while Factotum verifies the attempt against that
registration. This mechanism does not defend against a compromised Session,
which already owns admission and role authority.
It proves possession of a capability provisioned for a particular authorized
launch. It does not prove the current holder's PID, executable image or human
identity afresh.

### Mechanism and bootstrap

Use ordinary 9P `Tauth`, reads/writes on its authentication fid, and `Tattach`
with that verified afid. [Plan 9's attach specification](https://9p.io/magic/man2html/5/attach)
deliberately leaves the authentication protocol to the implementation. The
mechanism takes the external-credential approach described by
[RFC 4422 Appendix A](https://www.rfc-editor.org/rfc/rfc4422.html#appendix-A):
authority comes from a protected channel established outside the exchange.
This is an analogy of security semantics, not a claim of SASL wire compatibility.
The channel's custody and isolation must be established; the exchange itself
adds neither encryption nor protection from deliberate delegation.

Session's trusted launch policy chooses an opaque principal, namespace and role
audience before spawning. A principal is a policy identity, not a claimed uname,
UID or role name. Factotum validates the corresponding delegation through its
private Session control connection; it does not accept a caller-selected
principal. Namespace selection and grants remain Session decisions.

1. Reserve a fresh launch generation and create a private connected channel
   `P` for the prospective child. Session retains the trusted end and serves a
   bounded conversation-only relay, bound to that reservation. Session registers
   the delegated principal with Factotum over their existing private control
   connection. No descriptor crosses the 9P stream, and Factotum does not need
   to adopt the child endpoint. No path exposes `P` to another same-UID client.
   A client on `P` cannot walk, open or call `ctl`,
   keys, `log`, `confirm`, `needkey`, PAM or password retrieval.
2. Pass only the designated child end through the protected spawn. An environment
   variable may name its descriptor number; that number is not a credential.
   The parent closes its child-end copy after spawn. The launch becomes Active
   only when this reservation's spawn and protection confirmation succeed.
   A child can run before confirmation: requests during Reserved wait only within
   their original deadline and cannot receive a successful verdict. Failure
   retires the reservation, closes its channels and burns its generation.
3. On the ordinary role transport `X`, `Tauth` allocates afid `A`. Its owner asks
   Factotum for an attempt bound to `(server instance, X, wire generation, A,
   attempt generation, role audience, session generation)`. Factotum creates a
   fresh 256-bit nonce using the platform random source; failure refuses the
   attempt. The nonce is an opaque correlation challenge, not a bearer grant.
   The afid serves it to the client; no protected role file is accessible yet.
4. The client relays the challenge over `P`. Session's relay attaches the
   trusted channel-registration identity; the child cannot supply that field.
   Factotum resolves the original attempt and checks the registered delegation,
   session and allowed audience, with Session checking Active launch custody
   again before using the verdict. Factotum trusts Session's channel attestation;
   it does not inspect `P` itself. Client-supplied connection IDs, principals,
   generations or namespace names cannot replace those facts. A successful
   response consumes the nonce once and sends a typed verdict to Session over
   the trusted owner channel. The client cannot manufacture that verdict by
   writing success to an afid. Wrong-channel, expired or spent challenges fail
   without consuming a different valid attempt.
5. The afid reports success only for its own still-live attempt. `Tattach`
   checks that result, the live supervisor launch record and current Session
   policy together. It reserves root-fid/reply/admission capacity before the
   first successful attach pins the immutable principal and namespace. Failures
   leave previous admissions and fids unchanged. Uname/aname must match the
   authenticated request and policy-selected view; neither selects authority.
   The `.L` numeric uname is also a claim, never a substitute proof.

Version the local mechanism independently of the role records. t317 must publish
its bounded afid transcript and independent SDK exchange before enabling it;
do not pretend that the existing `pass` or `pam` protocol implements it. The
minimum transcript carries a versioned challenge, its relay and a terminal
status. It carries no password, private key, bearer ticket or client-asserted
verification result. Server-to-Factotum messages are private owner commands,
not new WM/shell records. A later standard remote mechanism can use the same
neutral verified-result seam, but requires its own review and provisioning.

This preserves two independent decisions even though they share launch facts:
Factotum confirms the Session-registered delegation for the relayed attempt;
Session separately checks whether launch `L` still owns the requested role. Revoking one must
prevent attach even if the other remains valid. A valid principal alone never
creates a role assignment. The core sees a verified result, not Factotum keys
or Session-specific policy.

### Lifetimes and atomic transitions

| Event | Required transition |
| --- | --- |
| Spawn reserved / confirmed | Reserved becomes Active only for its own successful spawn/protection result. A different spawn result cannot activate it. Rollback retires the generation permanently. |
| `Tauth` / completed exchange | One bounded Pending attempt becomes Verified only through the trusted verdict. A late result must match transport, wire and attempt generations as well as the afid number. |
| First successful attach | Pin principal, namespace and launch binding for the transport lifetime; publish the admission and root fid together. No partial identity pin or consumed unrelated grant on refusal. |
| Further attach | Reuse a valid afid only for its authenticated uname/aname and scope. Another view needs a fresh authentication scope and must still be a subset of the pinned identity's grants. Refuse another principal, namespace or launch. |
| `Tflush` | Cancel the pending wire request. It cannot undo an already committed attach. Reading or rereading the fixed challenge does not consume its verification right; the attempt remains bounded by its original deadline and can be explicitly clunked. |
| Clunk of afid / attempt deadline | Cancel and release that attempt; late verification is ignored. Clunk of a role fid does not unpin the transport identity. |
| `Tversion` | Release fids, afids and their verdict handles and cancel pending verification. Preserve transport identity, launch binding and the lifetime attempt budget. Fresh authentication must prove the same pinned identity. |
| Revocation / launch exit / transport loss | Prevent new protected operations, cancel pending attempts and invalidate results before closing or replacing the transport. Closing `P`, Session's per-launch relay, alone is insufficient: consult the owner's revoked generation before attach/delivery. The shared Session–Factotum control connection remains open for other launches. |
| Reconnect in the same live launch | Allowed via the ordinary role endpoint and a fresh attempt using the still-live `P`. A single-assignee role must retire its previous transport before accepting a replacement; a second connection cannot steal the live assignment. New connection epochs never inherit old fids/verdicts. |
| Launch replacement | New generation and new `P`; the old generation cannot authenticate the replacement even if PID, path, afid or numeric connection values repeat. |

`P` therefore lasts for the authorized launch generation, while each challenge
is single-use. The design does not leave old afids valid across `Tversion`.
The admission owner retains the pinned identity outside the core's wire-reset
state and ends it only with the transport. Secret and challenge material stays
out of the immutable `ClientAdmissionContext`, WM snapshots and durable logs.

### Initial bounds, to be enforced and tested by t317

These are conservative policy limits for the first local mechanism, not measured
latencies or capacity claims. They bound unauthenticated work independently of
the existing role's post-attach limits; a smaller existing role limit still wins.

| Resource | Limit and refusal behavior |
| --- | --- |
| Unattached transports | 32 per Session, at most 8 per endpoint; refuse before allocating another conversation. Existing admitted clients retain their slots and service. |
| Provisioned channels | One per launch generation, at most 64 active or Reserved delegations per Session. Reserve before spawn; refusal starts no child, and retirement reclaims its channel and registration. |
| Pending authentication | One afid attempt per transport, 2 per authorized launch, 8 per verified principal and 32 globally. Before identity is known charge endpoint/global quotas, never a claimed uname. |
| Attempt and first-attach time | 5 seconds from `Tauth`, within a 10-second absolute first-attach deadline from transport adoption. Activity, Reserved state and `Tversion` do not renew either deadline. |
| Attempt count | 8 starts per transport lifetime, including cancelled/failed attempts; `Tversion` does not reset it. Exhaustion requires a new admitted connection and remains subject to endpoint limits. |
| Authentication bytes | At most 4 KiB aggregate input/output transcript per attempt, including relay framing; the initial local exchange should be much smaller. Oversize fails before forwarding or allocation. |
| Unattached wire state | Maximum 4 fids, 4 outstanding tags, 64 KiB negotiated message size and 128 KiB unsent replies, or the existing endpoint's smaller limits. Afids count toward fid limits; no role-root fid before successful attach. The smaller authentication-transcript bound applies before forwarding. This does not force a smaller post-attach role message size. |
| Owner work and completion | At most 32 pending verification requests and 32 bounded results globally. Reservation follows a request until its result is consumed or cancellation acknowledged; never release a slot early while an unbounded late-result queue can still grow. No blocking send, per-attempt thread or Engine wait. |

Quota rejection does not evict another principal's admitted work. Dispatch must
give admitted connections progress while unauthenticated endpoints are full;
bound work per turn and test a flood alongside an already-admitted operation.
Saturated unauthenticated service may refuse newcomers: these finite bounds do
not claim fair access against an unlimited external connection flood. No PAM
worker or user prompt is involved in the local mechanism.

Custody-required path endpoints retain today's cheap expected-peer credential
filter before allocating these slots on platforms that provide it. This is an
availability filter, not the authenticated principal or the launch proof.
Do not remove it merely because the afid proof works: an unlaunched same-UID
process must not squat on the lock provider's admission capacity. A platform
without this filter needs a separately controlled reserved transport handoff
before enabling a custody-required path listener; anonymous quota sharing is
not an acceptable fallback. Removing the hard `SO_PEERPIDFD` requirement and
removing this accept filter are different changes.

Failure to reserve a launch slot is an explicit unavailable launch result and
uses the supervisor's bounded restart/backoff policy, not an immediate retry
loop. A lock-provider launch or attach failure must preserve Session's locked
input/output protection; it cannot release the lock or fall back to an
unauthenticated provider. Test both a pre-auth flood and quota refusal while
locked, without weakening the existing fail-closed lock contract.

### Implementation boundaries and controls

`ProtectionDomainSpec::inherited_fds` currently accepts only descriptors 0–2;
the spawn code nulls unlisted standard descriptors. This is not a universal
close-all-fds guarantee from Rust `Command`. t317 needs a narrow typed channel
handoff with child-only CLOEXEC handling, exact bubblewrap preservation at the
supported minimum version, and closure of every unintended inherited descriptor.
The SDK sets CLOEXEC again when adopting `P`; this reduces accidental exec
inheritance but cannot prevent fork or intentional descriptor passing.

The holder can proxy or transfer its existing capability. That remains the
same launch's delegated authority, not a newly supervised child. Controls must
prove that it cannot change principal/namespace/audience and that launch
revocation cuts it off; they must not claim that passing the descriptor is
impossible. t142 owns host reach and transfer containment. Current pidfd and
credential checks remain required until the replacement controls pass for each
role; removing one role's check does not waive another role's gate.
Specifically, the holder can relay a challenge from another process's transport
and thereby authorize that transport within the same delegation. The SDK must
relay only challenges read from its own afid; this does not constrain a malicious
holder. Current platform accept filters can further limit such transport use,
but the capability mechanism itself does not prevent intentional delegation.

| Control family | Required first discriminating observation |
| --- | --- |
| Bootstrap exposure | The child can use only its provisioned conversation; key administration and another launch's channel are unreachable. A deliberately inheritable unrelated sentinel fd is absent inside the sandbox. Observe that an ordinary exec after SDK adoption does not inherit `P`; do not claim this stops fork or intentional transfer. |
| Activation and custody | Early child request waits without admission, failed spawn refuses it, wrong reservation cannot activate it, unlaunched same-UID peer gets no verdict. Removing the Active/custody guard fails before protected bytes are delivered. |
| Attempt correlation | Wrong audience/session/launch, nonce replay, another afid/transport, reused IDs and late completion after reset each refuse at the exact attempt. Removing a correlation field must fail its corresponding case. |
| Immutable admission | Wrong principal or namespace on second attach and after `Tversion` refuse without rebasing old authority. Stale afid is invalid; fresh same-identity authentication succeeds. Removing the pin check is the control. |
| Revocation | A verified but not yet attached result, an open fid, a blocked read and a forwarded `P` lose authority when their source generation ends. Remove the final live check to discriminate the race. Delivered bytes cannot be recalled. |
| Cancellation and overload | Test both reply/flush orders, clunk, timeout, reset and disconnect; counts return exactly once. Fill each named quota and make independent admitted work progress. Unverified names cannot charge another principal's quota; foreign path connectors cannot consume the lock assignee's slots. Replace the nonblocking handoff with a bounded blocking mutant to distinguish owner progress. |
| Independent peers | C and Rust SDK peers execute the real afid exchange and use only the returned admitted view. Tests with supplied verifier outcomes qualify only the state machine, not authentication. |

t317 starts with this state model and device-free controls, then implements the
real Factotum exchange and protected handoff, then admits each role after its
controls pass. No current peer check is removed by this planning task. Existing
9P core/export and affected external client gates are rerun on the candidate;
admission CPU, memory, wakeups and bounded failure are measured without inventing
a sub-millisecond target.

### Scope for ordinary tools and alternatives

The first observer/operator client is also an explicitly authorized Session
launch. A trusted profile/configuration, or an already-admitted administrative
path under t254, requests that launch and chooses its recipe. An ad-hoc CLI
started from a shell gets no UID-only fallback. t318/t319 must preserve this
bootstrap dependency; unlaunched or remote principal provisioning is not
silently supplied by a socket path. This is a bootstrap constraint on the first
delivery, not removal of the later ordinary-client target.

Third-party WMs, shells and desktop components remain independent executables
using the public protocol and SDK, in any supported language. Supervised launch
supplies their authorized connections; it does not require their code to live
in Sophia. Before calling this developer experience ready, t318 must provide
an authorized command to launch a developer's executable under a selected,
policy-approved recipe. The caller cannot grant itself a requested role.

[t324](jsschoen-converge-public-roles-on-one-9p-core.md#t324) owns local credential
provisioning and authentication for tools started directly from a terminal and
independently launched 9P applications. Supervised launch is not a permanent
requirement for all applications. That follow-on does not itself deliver the
future 9P application frontend, and remote authentication remains separate.

Using the role socketpair itself as the only credential would simplify custody,
but would not supply the reviewed Factotum afid exchange and would force every
reconnect to relaunch. Keeping path sockets plus mandatory peer checks is the
current safe baseline, not the portability exit. Reusing PAM success or a
retrieved password as a role credential, and placing reusable bearer tokens in
environment variables or files, are rejected. A standard network authentication
mechanism would still need separate launch custody and credential provisioning;
it is unnecessary for this local supervised path. No new cryptographic
challenge-response algorithm or remote-authentication claim is introduced.

The t133 planning result is the reviewed boundary and this measurable handoff.
The state-machine, real-exchange, containment and role-migration results remain
t317/t142 work; t133 completion must not be described as those controls passing.

## Connections

- [Admission investigation and source corrections](../investigations/1pv291te-namespace-and-client-admission-security-gaps.md).
- [Observer delivery dependencies](jsschoen-converge-public-roles-on-one-9p-core.md#first-deliverable-and-dependency-order).
- [Namespaces and portals](../../namespaces-and-portals.md).

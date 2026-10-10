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

## Connections

- [Admission investigation and source corrections](../investigations/1pv291te-namespace-and-client-admission-security-gaps.md).
- [Observer delivery dependencies](jsschoen-converge-public-roles-on-one-9p-core.md#first-deliverable-and-dependency-order).
- [Namespaces and portals](../../namespaces-and-portals.md).

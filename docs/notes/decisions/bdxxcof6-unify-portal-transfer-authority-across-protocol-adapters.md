---
id: bdxxcof6
date: 2026-10-11
kind: adr
status: accepted
tags: [adr, portal, architecture]
---
# Unify portal transfer authority across protocol adapters

## Context

Investigation of the attended t142 paste failure found an incomplete X selection
requestor: property handling existed, but a generic toolkit-style inspection of
the temporary endpoint failed. No browser wire trace establishes that this is
the only cause of the attended failure. Fixing that adapter must not create
another transfer authority. niltempus
required one integrated portal authority for all portal activities and all
protocol authorities, with X11 and 9P joins first.

## Decision

Keep admission, exact-pair policy, participation, grant lifecycle, bounded
custody, replay protection, cancellation and terminal outcomes in
`sophia-portal`. Family handlers validate the meaning of a request and payload;
source and recipient adapters translate their own protocols. XIDs, fids, client
paths and drawing resources remain in those adapters. No Engine or WM policy
receives portal payloads or protocol identifiers.

The common lifecycle covers all seven portal kinds. The bounded upload
coordinator is one execution path, not a requirement that notifications, capture
streams or future descriptor handoffs pretend to be clipboard uploads. Only
registered family handlers may execute; the default remains bounded Clipboard
text/TARGETS. An unsupported kind cannot succeed through a generic grant alone.

Generalize the narrow in-process admission in
[88uev9ix](88uev9ix-admit-the-in-process-x-portal-requester-by-session-channel-custody.md)
from an X clipboard worker to a Session-registered protocol authority. The
trusted owner selects the adapters and mints a registration identity. A private
socketpair, adopted connection, epoch and exact namespace pair identify each
requester. No peer can choose its registration through attach strings, paths,
UID equality or request fields. Protected out-of-process admission remains a
separate obligation; this does not implement t317 or a public 9P app frontend.

Separate installation from continued access. `Completed` records one successful
recipient effect. A bound object remains readable only through an owner-minted
lease checking the original deadline, source generation, namespace rights,
pair policy and security interval. Replacement gives the binding a fresh Qid;
old fids stay stale. Completion cannot extend the grant, replay the effect, or
make a revoked object readable. Copied X property bytes are an executed effect;
revocation does not promise to erase bytes already delivered to an application.

See the [architecture overview](../../portal-authority.md) for the ownership,
transport and lifetime diagrams.

## Alternatives

Separate X and 9P brokers would duplicate policy and lifecycle decisions and
make cross-protocol transfers depend on two grants. A generic broker containing
X selection logic would invert the protocol boundary. A new 9P application
frontend is unnecessary for validating its adapter contract and remains outside
this repair. Declaring all kinds implemented merely because the lifecycle can
name them would overstate executor support.

## Consequences

The four protocol pairings require real shared-core file custody, not direct
calls that manufacture a grant. Device-free fixtures label their supplied
admission and source bytes. X temporary requestors must support coherent
inspection and bounded property/event handling without becoming scene windows.
All paths must retain the same refusal and lifetime controls. Compatibility
family reducers share validation/decision machinery but do not mint live grants;
the live X path must not keep their parallel pending ledger.

The installed browser/terminal check remains attended acceptance. Generic joins
do not prove a particular client's UI, physical input or scanout. Other kinds'
executors, out-of-process authentication and the production 9P app frontend are
not silently added to the scope.

## Acceptance and connections

Accepted on 2026-10-11 through niltempus's request for a unified DRY system,
approval of the revised plan, and instruction to implement it. This succeeds
88uev9ix's X-only admission scope and preserves its exact-pair, off-owner and
default-deny requirements. The [file contract](../../sophia-portal-files.md)
remains normative. The [one-core decision](zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md)
continues to govern transport and recipient-owned binding.

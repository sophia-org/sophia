---
id: 88uev9ix
date: 2026-10-10
kind: adr
status: superseded
tags: [adr, portal, session, protocol]
---
# Admit the in-process X portal requester by Session channel custody

The [protocol-neutral successor](bdxxcof6-unify-portal-transfer-authority-across-protocol-adapters.md)
generalizes the X-only registration on 2026-10-11. The original decision below
records its narrower scope; exact-pair custody and default-deny policy remain.

## Context

The t142 acceptance preparation found no live Session consumer of cross-group
clipboard requests. The existing two-listener tests supply their own coordinator
and Allow broker. The [investigation](../investigations/rd9nia0s-several-x-listeners-and-confined-application-launch-validation.md#live-portal-prerequisite-discovered-during-acceptance-preparation)
records that distinction. niltempus approved implementing the t315 file path
before trying the installed-session paste check.

The [portal file contract](../../sophia-portal-files.md) and
[earlier decision](xa78u03g-keep-broker-and-portal-file-authority-and-custody-separate.md)
require a protected-process PortalRequester admission. The X authority already
runs inside Session and owns selection routing, namespace attribution and the
recipient executor. Its private socketpair would report Session's own kernel
credentials; those credentials are not evidence of a separately protected role.
niltempus separately approved this narrower admission change and the standing,
one-way acceptance-login clipboard policy on 2026-10-10.

## Decision

Use one additional, explicitly named admission mode for the in-process X
authority only. Session creates a private socketpair and gives the requester
end only to its X clipboard coordinator. The portal owner adopts the other end
on `sophia_9p::unix::Server`, with a Session-created admission record bound to
that connection, epoch and exact source/target namespace pair. No listener path
or endpoint environment variable exists. This proves Session channel custody,
not protected-process identity, afid authentication or a new remote principal.
It is not an admission mode for arbitrary same-UID clients, host tools or apps.

Keep one pair per connection and one active requester at a time. Each transfer
gets a new connection/epoch; the decision owner retains transfer history and
monotonic Qids across them. Reconnection does not reset the 4,096-identity
ceiling or discard issued executor work. This avoids broadening the admitted
pair into an arbitrary namespace set. Out-of-process requesters retain the
existing protected admission obligation; this exception does not implement or
bypass t317 for those clients.

After PayloadEnd passes custody checks, a bounded typed handoff carries the
grant and payload to the X recipient executor. The portal policy receives only
request metadata. The executor validates its retained selection identity and
source generation, and the live grant, deadline and lock fence, immediately
before installing the property. Only its successful effect produces Completed.
Neither a 9P write reply nor Submitted means the paste happened. No payload or
descriptor enters a request record, and no legacy-envelope coordinator is added
to Session.

Use explicit startup configuration for directional clipboard rules between
named X groups, defaulting to none. A rule both enables the source's publish and
target's request participation and lets the decision owner allow each matching
clipboard transfer while both namespaces and generations remain live. The pair
rule must still match: capability bits alone cannot authorize a different pair.
This is persistent operator policy for matching paste requests, not a user
prompt or consent for just one transfer. It covers CLIPBOARD and PRIMARY within
the existing bounded text/TARGETS executor; it grants no files, capture, XTEST,
network access, reverse-direction transfer or general namespace visibility.

For the attended t142 configuration, allow only confined group 7 to shared.
Ordinary logins keep no such rule. No live configuration changes or restart occur
until the implementation and device-free joined controls pass and the attended
configuration is ready for review.

## Alternatives

Keeping the protected-peer-only contract would require extracting an X
requester process and transporting its frontend custody across another boundary.
That is not required to separate policy from execution and is substantially
larger than the missing X join. Reusing a legacy Allow broker would contradict
the accepted one-core direction. Treating Session's socket credentials as a
protected requester would misstate the admission proof. An admitted namespace
set would reduce connection churn, but broadens the pair contract unnecessarily.

## Consequences

The connection churn is bounded and off the compositor's input/render loop.
The implementation needs a real shared-core wire join, not only calls to export
methods. Controls must refuse an unregistered socket even with identical UID/PID,
a foreign pair, missing participation, an absent rule and the reverse direction.
They must show no execution after source replacement, lock, deadline, disconnect
or broker-generation change, and no duplicate execution after replay. A blocked
portal must not block the Session owner turn or leave a selection waiter hanging
without a bounded failure. Both CLIPBOARD and PRIMARY require real frontend joins.

The installed browser/terminal/departure check remains the t142 physical exit.
Neither this decision nor the headless joins substitute for it. Prompt UI and
other transfer kinds remain outside this amendment.

## Acceptance and connections

Accepted by niltempus on 2026-10-10. This amends the portal contract's requester
paragraph and the protected-requester-only portion of xa78u03g, preserving its
unrelated broker, history and custody decisions. Acceptance authorizes the
implementation and explicit acceptance-login rule, not an installed capability
claim or completion of the required gates.
The [one-core decision](zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md)
continues to govern policy/recipient separation and 9P transport.

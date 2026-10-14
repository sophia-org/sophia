---
id: 0photrwd
date: 2026-10-14
kind: investigation
status: closed
tags: [investigation, protocol, portals, notifications]
---
# OSC 7501 reports program status through admitted notification clients

## Question

Can OSC 7501 help Sophia's notification portals or independent-client
authentication? niltempus approved incorporating the findings into t046 on
October 14. This records an interface use case, not an implemented integration.

## Current resumption

The research is complete. Use OSC 7501 as a terminal-originated notification
use case under t046. Parsing and task state stay in the terminal or multiplexer;
Sophia receives generic admitted notification requests. No new Engine parser,
OSC-specific service, public wire format or authentication mechanism is needed
for this documentation change. Task order remains in todo.md. The provider
implementation and acceptance are still t046 work.

## Evidence

Sources read on 2026-10-14:

- [Program Status Protocol](https://www.superlogical.com/rex/docs/build/program-status),
  revision 0.4, dated October 10. It reports idle, working, blocked, done and
  error, with optional progress and nested task IDs. The terminal maintains
  records and their lifetimes; reports are untrusted. The application name is
  not authenticated identity. Authentication and permission are reasons for
  being blocked, not requests that authorize an operation. Notifications are
  an optional presentation of state changes.
- [Ghostty PR 14560](https://github.com/ghostty-org/ghostty/pull/14560), merged
  October 6. The initial implementation supplies libghostty parsing and effects;
  the embedder owns records. The PR says the Ghostty application initially
  ignored reports. This establishes library support at that revision, not
  current application UI support or an installed Sophia integration.

Sophia was inspected at `68ffb8e0d`. `NotificationPortal` in
`crates/sophia-portal/src/notification.rs` validates requests and reduces transfer
decisions. `crates/sophia-engine/src/chrome/notification.rs` has notification
presentation structures. Neither establishes a complete native provider route;
[t046](../plans/queue-16-portals-and-confined-applications.md#t046) explicitly
retains that missing integration. The current summary/body limits in
`crates/sophia-portal/src/types.rs` are 128/1,024 bytes.

The vendored shell protocol's `AttentionState` has only None, Notice and
Critical. It cannot preserve progress, blocked reasons or a task tree. t257's
role-status files describe Sophia services, not arbitrary programs within a
terminal. No OSC 7501 implementation was found in Sophia.

## Finding and resolution

Use a notification-first integration. The terminal selects meaningful state
changes and retains detailed progress and task trees locally. An admitted
client sends generic notification requests through the t046 route; Sophia
checks grants, recipient authority and lifetime. The terminal's OSC feature
detection must not imply that desktop notification permission was granted.

Attribute requests to the admitted sender. A program writing to its PTY,
including through SSH, cannot acquire a trusted principal by supplying a name
or task ID. Those fields are presentation data. Keep messages out of blind WM
policy and apply the recipient's disclosure rules. A claimed need for
authentication must never trigger trusted credential entry, automatic input
or a permission grant. t324 retains its credential and immutable-attach rules.

The adapter must respect Sophia's text limits; arbitrary OSC messages cannot
be copied unchanged. Choose and test UTF-8-safe shortening or refusal when the
notification route is implemented. Bound notification rate and queued work,
and deduplicate repeated state so progress updates cannot flood the desktop.
Revocation and owner replacement must invalidate the old sender's authority.
Terminal task clearing and desktop notification dismissal are separate
lifetimes; document their mapping rather than assuming either implies the other.

This follows the existing authority boundaries. It introduces no new protocol
decision or requirement for a particular terminal, shell or agent product.

## Validation and remaining work

This was source and specification inspection, not a runtime test. t046's future
generic-client acceptance must cover denied/foreign grants, spoofed labels,
hostile text, size limits, flooding, bounded backpressure, sender replacement
and revocation, and stale notification handling. Explicitly prove that blocked
authentication/permission labels confer no authority. Terminal repositories own
OSC parser, task-clear, prompt/reset and process-lifetime tests. Named desktop
assembly and UI acceptance belong in the integration/client repositories.

Detailed task-state transport to shells would need a separate versioned
contract and disclosure review. It is not introduced by this notification use
case. t324 does not depend on OSC 7501 support.

## Connections

- [t046 portal scope](../plans/queue-16-portals-and-confined-applications.md#t046)
  owns notification delivery and per-recipient acceptance.
- [t324 credentials](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#t324)
  owns authentication independently of status presentation.
- [One-core decision](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md)
  keeps admission and recipient execution with their existing owners.

---
id: u399qgtv
date: 2026-10-14
kind: investigation
status: closed
tags: [investigation, protocol, portals, interoperability]
---
# Kitty protocols as interoperability evidence for Sophia portals

## Question

Which Kitty protocols and Ghostty implementation lessons help Sophia's core
architecture? niltempus approved the clipboard, prompt, notification and
drag-and-drop recommendations on October 14, then requested a broader review.

## Current resumption

The research is complete. The approved portal cases are part of t046's existing scope and task row.
Protocol translation remains in terminal clients; Sophia owns the admitted
underlying transfers. Generic clients prove Sophia contracts. Named terminal
interoperability belongs in external integration repositories. The broader
review below now feeds the critical path by niltempus's subsequent approval:
t067 cursor acceptance, t111 input/text contracts, t320 input-design review,
t324 authority controls and t046 portal acceptance precede t289. Graphics
workloads belong to t289. The queue owns execution order; terminal-local
features remain outside Sophia's core scope.

## Evidence

Sources read on 2026-10-14; Sophia baseline `f36d9ecb5`:

- [Kitty clipboard protocol, OSC 5522](https://sw.kovidgoyal.net/kitty/clipboard/):
  typed clipboard transfer is a useful case for t046's non-text payloads.
- [Mitchell Hashimoto's oversized-write report](https://github.com/kovidgoyal/kitty/issues/10399):
  he found a clipboard implementation could truncate a binary write while
  returning success. This is historical bug evidence, not a claim about the
  current Kitty release.
- [Ghostty prompt fix reviewed and merged by Mitchell](https://github.com/ghostty-org/ghostty/pull/14470):
  a contributor removed caller-supplied program names from trusted clipboard
  prompts because those names were unverifiable and could mislead users.
- [Kitty notifications, OSC 99](https://sw.kovidgoyal.net/kitty/desktop-notifications/):
  replacement, dismissal and activation reports provide lifecycle cases for
  Sophia's notification route.
- [Kitty drag-and-drop, OSC 72](https://sw.kovidgoyal.net/kitty/dnd-protocol/):
  format offers, data requests and machine identity expose useful local/remote
  handoff cases. Kitty originated these protocols; Ghostty implementation work
  does not make them Ghostty-authored protocols.

## Finding and resolution

The approved t046 acceptance additions are:

- Preserve exact binary clipboard payloads, including MIME types. Reject an
  over-budget transfer explicitly; never report successful delivery of a
  silently truncated image or file. Record what happens to any prior clipboard
  value on failure and prove cancellation, source replacement and backpressure.
- Derive trusted prompt attribution from admission. Caller-supplied friendly
  names may be untrusted content, never the trusted identity requesting access.
  Reuse t324's credential boundary rather than treating a prompt label as proof.
- Bind notification replacement, dismissal and activation to the admitted
  sender and live generation. A notification ID is not permission to modify
  another sender's notification. Reject stale activation after replacement or
  revocation; do not turn an action string into arbitrary command execution.
- Preserve MIME offers and requested types across drag-and-drop and file/URI
  handoff. A remote machine identifier is routing data, not authentication.
  A remote path must not be opened as a local file. Unavailable remote content
  must be refused or separately transferred under an explicit grant.

These requirements strengthen existing portal semantics without adding Kitty
escape parsing to Engine, changing Sophia's transport, or granting terminal
children the terminal's authority. The existing per-recipient and lock rules
continue to apply; this note grants no extra input or remote-file authority.

## Broader review

The initial recommendations below were promoted by niltempus on October 14,
including the Ghostty findings. Their owning plans now carry measurable exits;
the queue promotes t067 and t111 and makes them prerequisites of t289.

| Protocol | Useful lesson for Sophia | Boundary and owner |
| --- | --- | --- |
| [Pointer shapes, OSC 22](https://sw.kovidgoyal.net/kitty/pointer-shapes/) | Test application-selected shapes, restoration and movement between windows. Kitty's shape stack and alternate-screen behavior provide external interoperability cases. | t067 already owns cursor presentation. Audit the current path before assuming its historical failure persists. The terminal owns its stack; the frontend and Engine deliver the resulting cursor. |
| [Keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/) | Review press, repeat and release; shortcut identity versus text; and focus changes while keys are held. | Useful evidence for input contracts and t111's text/IME design. CSI encoding and negotiation stay in the terminal. This does not establish a defect in Sophia's current input path or authorize broader t320 input injection. |
| [File transfer](https://sw.kovidgoyal.net/kitty/file-transfer-protocol/) | Require explicit destination grants, bounded transfers and cleanup after cancellation. Review directories and links so an offered path cannot escape its grant. | t046 providers own file custody. Compression, delta transfer and remote transport are provider choices, not new Engine responsibilities. |
| [Graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/) | Image placement, reuse, deletion, resizing and animation offer varied external rendering workloads. | Potential later t289 measurement input. Keep terminal image storage and escape parsing outside Engine; do not reopen accepted t319 capture work. |
| [Remote control](https://sw.kovidgoyal.net/kitty/rc_protocol/) | Async request correlation and cancellation are useful lifecycle comparisons. | Sophia's 9P admission and t324 credentials remain authoritative. A request ID must not permit another caller to cancel work. |

Three details deserve attention during portal implementation:

1. **An offer is more than its first MIME type.** At the inspected revision,
   `crates/sophia-portal/src/drag_and_drop.rs` accepts `offered_types`, then
   places the first entry in `PortalTransfer.mime_type`. This is policy metadata,
   not evidence of live payload loss. The executor must retain the full offer
   and honor the selected type; it cannot reconstruct negotiation from that
   single hint.
2. **A paste gesture can authorize one transfer.** OSC 5522 has a one-time
   password for a paste event as well as reusable permission shortcuts.
   Sophia can borrow the narrow lifetime: bind a grant to the recipient,
   selection generation and transfer. Terminal passwords and friendly names
   do not become Sophia credentials. The current specification also defines
   oversized-write failure; the historical truncation report is a regression
   case, not an outstanding Kitty bug claim.
3. **Cancellation needs authority too.** Kitty's remote-control specification
   allows unencrypted cancellation on its stated denial-of-service rationale.
   Sophia should preserve its own threat model: cancellation must remain bound
   to the admitted caller and live request. Copying a request ID or a timestamp
   is not proof of authority.

The strongest additional compatibility probe is cursor restoration under t067.
Keyboard semantics deserve a contract review; graphics workloads can wait until
architecture gates are complete. Terminal text sizing, colors and cell layout
do not call for new Sophia core protocols.

## Validation and remaining work

This is specification and code-informed research, not a live interoperability
result. t046 owns the executable acceptance and any contract amendments needed
to implement these cases. Do not infer a working provider from policy reducer
types. Reuse accepted capture evidence; no new capture campaign is required.
The research is complete; promotion does not imply implementation or acceptance.

## Connections

- [t046 portal scope](../plans/queue-16-portals-and-confined-applications.md#t046)
  owns the approved transfer and prompt cases.
- [OSC 7501 findings](0photrwd-osc-7501-reports-program-status-through-admitted-notification-clients.md)
  separate persistent program state from notification delivery and credentials.

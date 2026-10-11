# Portal authority and protocol adapters

A portal authorizes one bounded action across namespaces. It does not merge
the namespaces or let the recipient browse the source. One common owner decides
whether the action may happen; protocol adapters collect the source and install
the result in the recipient's own protocol.

This overview explains the architecture. The
[portal file contract](sophia-portal-files.md) defines wire formats, admission,
credit and custody. The [decision](notes/decisions/bdxxcof6-unify-portal-transfer-authority-across-protocol-adapters.md)
records why the owner is shared.

## Who owns what

```text
                  Trusted Session owner
               registers and selects adapters
                  |                 |
          namespace facts       exact-pair policy
                  |                 |
                  v                 v
  +-----------------------------------------------------------+
  | sophia-portal: common transfer authority                   |
  |                                                           |
  | admission -> policy -> grant -> custody -> terminal outcome|
  |                                                           |
  | bounds, replay, deadline, cancellation, security fence     |
  | family handler: supported operation + payload validation   |
  +-----------------------------------------------------------+
             |                                    |
      collect only after grant             execute only after
             |                             final grant checks
             v                                    v
  +------------------------+          +------------------------+
  | Source protocol owner  |          | Recipient protocol owner|
  | X selection / 9P object| --bytes-> | X property / 9P binding |
  | owns source identity   |          | owns target identity    |
  +------------------------+          +------------------------+

  Engine: no portal policy, XIDs, fids or payload custody
  WM:     no portal decisions, payloads or protocol identities
```

The common owner sees namespace IDs, transfer kind, MIME/size metadata,
generation and deadline. X window IDs, selection atoms, properties and event
routing remain with X authority. A 9P recipient owns its names, Qids and fids.
An adapter cannot turn a decoded grant into a retained read lease: only the
common handoff creates that authority.

Participation capabilities are necessary but insufficient. If policy allows
A to B and B to C, it does **not** allow A to C. Reverse transfers also need an
explicit rule. The ordinary rule set is empty.

## One transport and custody path

The in-process coordinator runs off the Session input/render thread. It uses
the real portal/files client and the shared 9P2000.L server, even though both
ends currently run in Session.

```text
  Session registers an authority
       |
       +-- private socketpair, no public pathname
                  |
     requester    |   9P2000.L       portal/files owner
        |         |                       |
        +---------+-- Negotiate ---------->
        |            TransferRequest -----> policy + live facts
        |       <--- Decision(grant/deny) --+
        |
        +-- granted source collection
        |
        +------------ PayloadBegin -------> bounded upload custody
        +------------ payload writes -----> complete bytes, not execution
        +------------ PayloadEnd ---------> validate; queue handoff
        |                                      |
        |                             recheck under security fence
        |                                      |
        |                              recipient's actual effect
        |                                      |
        +<----------- TransferOutcome ---------+

  A write reply or Submitted is not a successful recipient effect.
```

One private connection admits one exact namespace pair and epoch. Successor
connections do not reset transfer history, Qids or issued execution custody.
The registration is selected by Session, never by an attach name or a peer's
claimed UID. Out-of-process requesters still need their separate protected
admission; this private path is not an authentication shortcut for applications.

## Four protocol pairings

```text
  Source adapter          SAME portal authority       Recipient adapter

  X11 selection ----------> grant/custody -----------> X11 property + notify
  X11 selection ----------> grant/custody -----------> 9P bound object
  9P object --------------> grant/custody -----------> X11 property + notify
  9P object --------------> grant/custody -----------> 9P bound object
```

For X sources, a temporary requestor receives the selection bytes. It answers
GetWindowAttributes, GetGeometry and QueryTree consistently, but is not a client
scene window or drawable. Only the captured owner connection may use its
assigned property and exact SelectionNotify tuple. Capture, refusal and timeout
retire its property and subscriptions. CLIPBOARD and PRIMARY share this path.

For an external source feeding X, the trusted owner registers a source offer.
No fake X owner window is created. A real X ConvertSelection creates the
recipient's retained request; the common owner checks the external source's
generation and policy before the X adapter installs its property.

The production application frontend is still X11. The 9P joins use supplied
admission and generic object exports through the production shared core. They
exercise the adapter contract; they do not claim a finished 9P application
frontend, namespace compiler, discovery UI or an installed 9P desktop.

## Completion and continued access are different lifetimes

```text
  Transfer: Pending -> Allowed -> upload -> executor -> Completed
                       |          |           |
                       +----------+-----------+----> Revoked / Expired

  Successful bound-object installation:

      fresh Qid + original grant lease
                    |
           readable through retained fids
                    |
       each access checks original deadline,
       source generation, namespace rights,
       exact pair and security interval
                    |
        revocation / expiry / replacement
                    |
              old fids: ESTALE
              waiting reads: ESTALE
              replacement: fresh Qid
```

Completed means that installation succeeded once. It neither revokes access
immediately nor grants permanent access. The lease cannot renew the original
deadline. Lock/unlock advances the security interval, so unlocking cannot revive
old work. A replacement under the same pathname cannot retarget an old fid.
The recipient's unrelated objects remain usable.

Revocation prevents future authorized reads or effects; it cannot retract bytes
an application has already read. X property delivery is a copied effect, not a
promise to erase the application's memory later.

## All portal kinds share authority, not identical payloads

Clipboard, drag-and-drop, file handoff, screen capture, screen recording, URI
open and notification share the request/grant lifecycle and custody rules.
Family handlers own semantic validation and supported execution forms.

The current live upload handler supports bounded clipboard text and TARGETS.
Unsupported handlers fail closed. A stream, notification or descriptor handoff
does not become a byte-upload operation merely to reuse the coordinator, and
descriptors never travel as bytes on the 9P stream. Adding a kind's executor
requires its own meaningful end-to-end witness, while reusing common authority.

## Evidence and limits

The [t142 investigation](notes/investigations/rd9nia0s-several-x-listeners-and-confined-application-launch-validation.md)
records the candidate, executed checks, controls and remaining acceptance.
Device-free joins distinguish supplied admission/source bytes from production
codec, shared-core transport, custody and adapter execution. They do not prove
physical input, scanout, or a particular browser's UI. The attended confined
browser to shared terminal paste check remains required.

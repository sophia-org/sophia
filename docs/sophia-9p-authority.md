# Sophia 9P application frontend (`sophia-9p-authority`)

**Role:** application frontend design under the accepted public 9P direction.

**Status:** target architecture; application API and integration remain
unimplemented. The checked-in crate is a scaffold, not conformance evidence.
See the [public interface design](sophia-9p-control-bus.md) and its
[accepted direction](notes/decisions/1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md).
The admitted [Hagia-first implementation](notes/plans/80blhke8-migrate-the-hagia-wm-role-to-admitted-9p2000-l-files.md)
builds shared transport and the WM role; this application frontend remains a
later milestone and is not activated by that work.

## Purpose and scope

The target frontend lets an application create and update its graphical content
and receive routed input through synthetic files served over 9P2000.L. It sits
alongside X authority and reduces its own protocol into the existing Engine
transaction and input boundaries. X11 applications keep their existing frontend.

This application interface and the proposed 9P WM/shell/administrative services
share a protocol direction, not one grant or semantic owner. The application
frontend must not become the owner of session control, WM policy or shell
allocations merely because those interfaces also speak 9P.

```text
 X11 application                     9P application
        |                                   |
        v                                   v
 +------------------+             +----------------------+
 | X authority      |             | 9P app authority     |
 | X11 object state |             | file/object state    |
 +---------+--------+             +----------+-----------+
           |                                 |
           +---------------+-----------------+
                           | admitted visual transactions
                           v
                +------------------------+
                | Sophia Engine          |
                | scene and presentation |
                | target-resolved input  |
                +-----------+------------+
                            |
                            | opaque spatial facts / proposals
                            v
                +------------------------+
                | admitted WM policy     |
                | current: sophia_wm_v1  |
                | target: 9P WM role API |
                +------------------------+
```

## Ownership

| Owner | Responsibility |
| --- | --- |
| 9P application frontend | Protocol decoding, per-connection handles and object state, admitted application content and metadata, translation to existing visual transactions, protocol replies and routed-input encoding |
| Session | Caller admission, protection domains, supervision, grants, revocation and service routing |
| Engine and rendering owners | Scene truth, hit-testing, atomic visual commits, source retention, native backing retirement, rendering and scanout |
| WM | Metadata-blind spatial and focus policy over opaque nodes |
| Shell and metadata broker | Admitted UI and metadata disclosure under their separate contracts |
| Portal owners | Explicit cross-namespace transfer decisions and execution |

Application requests for size, fullscreen or focus are proposals interpreted by
the existing owners; file writes do not give applications global placement or
input authority. Metadata stays at the frontend/broker boundary and is not
included in the WM's filesystem view. A common codec or executable must not
merge blind policy with metadata-bearing protection domains.

## File API design space

The following is illustrative. Allocation syntax, formats, IDs, open/close
semantics and version negotiation have not been frozen.

```text
 /sophia/app/
   windows/
     <opaque-object>/
       state         admitted geometry and allocation facts
       content       bounded content submission
       events        routed input and lifecycle events
       outcomes      correlated proposal/presentation results
```

A complete design must define how a client creates an object, stages content,
submits damage and receives explicit outcomes. File byte offsets and protocol
request tags cannot silently replace transaction, allocation or connection
identities. Reads and writes can fragment; the parser must retain bounded
assembly state and must not present an incomplete buffer.

Possible content interfaces include bounded pixel uploads and an explicitly
specified drawing command stream. Neither is selected by adopting 9P. GPU
buffer sharing and synchronization require their own measured design and
ownership proof; 9P does not transport Linux file descriptors automatically.
Text widgets, terminal emulation, font layout and other application behavior
remain in clients or deliberately separate services.

## Admission, input and lifetime

Session establishes the application's resource namespace and effective rights.
The server restricts every attach, walk, open and operation to that admission,
including operations on previously opened handles. Linux mount namespaces can
expose only the intended view but do not replace server checks, FD custody or
revocation. The mounted client's relationship to process identity needs an
explicit contract; a mount's transport peer is not assumed to identify every
process using it.

Engine resolves input against presented state. The frontend receives only the
events routed to the admitted application and translates them into its own
protocol. Keyboard transitions, text input, repeat, capture, cancellation,
backpressure and release obligations require specified behavior. A UTF-8 file
or a mouse record alone does not establish a complete input contract.

Client disconnect or explicit object removal revokes future use promptly.
Already queued, rendered, copied or submitted work retains its resources until
the existing consumers retire. Closing a 9P handle does not prove those consumers
have stopped. Reconnect creates fresh authority and cannot revive an old object,
input target, transaction or completion.

Frontend failure must be contained without claiming recovery behavior before
the joined owner transitions are demonstrated. Physical presentation evidence
remains distinct from headless tests with simulated completion.

## Plan 9 applications and application services

The goal includes applications built around ordinary file operations and
composable services. It does not establish compatibility with unmodified Acme,
Sam, Rio or plan9port. Their dialects, runtime assumptions, graphics and input
protocols need separate implementation and independent-client evidence.

Plan 9's [draw interface](https://9p.io/magic/man2html/3/draw) carries a specific
graphics protocol inside files. [Rio](https://9p.io/magic/man2html/4/rio) exports
window services. They are references for API design, not interchangeable APIs
obtained by implementing 9P2000.L. Classic 9P2000 fallback remains an open
compatibility decision.

An application could separately export its own domain services: for example,
an editor's buffers and commands, following the
[Acme model](https://9p.io/magic/man2html/4/acme). Those semantics belong to that
application. Sophia's role is to mediate explicitly granted access, with service
discovery, delegation and revocation still to be designed.

A nested desktop would be an ordinary application whose internal children do
not gain control of other Sophia applications. A system-wide WM must instead be
explicitly admitted to the WM role. No permanent policy-serving bridge or
automatic authority handover is part of this design; any migration adapter
would require its own reviewed ownership and retirement criteria.

## Performance and storage

Synthetic content files do not require persistent file backing. That does not
prove zero host disk activity, zero copying or equal overhead to current IPC.
Direct 9P and mounted v9fs paths need separate measurements, with caching chosen
to preserve snapshot and event semantics.

Measure bounded uploads, input-to-presentation latency, slow readers, idle
wakeups, memory usage and retirement under load against equivalent existing
owner paths. Keep application transport costs separate from rendering and
scanout costs. Neither a generic filesystem client nor a small protocol parser
establishes a production graphics performance claim.

## Contract and acceptance work

The shared [migration criteria](sophia-9p-control-bus.md#migration-and-evidence)
apply. Application acceptance additionally needs a real independent client
creating content, responding to routed input, resizing, disconnecting and
reconnecting through the production owners. Negative controls must establish
namespace exclusion, stale-identity refusal and resource retirement.

The first content format, required 9P operation subset, object allocation API,
input encoding and compatibility scope remain design questions. They are not
an implementation schedule or a second task queue.

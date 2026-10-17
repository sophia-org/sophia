# Metadata broker file contract

**Design only.** The export and SDK broker clients are not implemented. The
current metadata broker still uses `sophia_broker_v1`. This contract specifies
its replacement; it does not change production selection or accept retirement.
The [decision](notes/decisions/xa78u03g-keep-broker-and-portal-file-authority-and-custody-separate.md)
records admission, compatibility and resource choices. Exact native layouts are
in [sophia-broker-files-v1.kdl](../protocol/sophia-broker-files-v1.kdl).

## Endpoint and authority

Session serves `SOPHIA_BROKER_9P_SOCKET` to its protected metadata-broker child.
Admission requires `MetadataBroker` protection evidence and the exact kernel
peer UID and PID. The supervised-PID-only admission route is insufficient.
Only one peer and one attach may hold the role in a connection epoch. Attach
strings grant nothing. Revocation invalidates old fids, offsets and candidates.
Neither host-user access nor a WM/shell attachment grants broker authority.

The child owns disclosure policy. Session routes reduced candidates and applies
the broker's validated rules/descriptors. Raw application metadata does not
gain a route to the WM or shell through this migration. The existing launch
parameter `SOPHIA_BROKER_DEFAULT_DISCLOSURE` remains a launch parameter.

Use standard 9P2000.L without protocol sniffing or socket fallback. This role
has no descriptor transfer. There is no new restart policy: a broker failure
takes Session's existing failure path. Any future connection uses a newer
nonzero epoch and starts with no pending request; outstanding work is not
automatically replayed. Session request IDs remain nonzero and strictly
increasing for the Session owner's lifetime, including across connections.

## Files and record identity

| File | Access | Meaning |
| --- | --- | --- |
| `api` | read | `sophia-broker-files version=1 role=metadata-broker epoch=<epoch> fd_transfer=none` followed by newline |
| `limits` | read | Immutable Limits object, available before negotiation |
| `events` | read | Retained journal of handshake, request and custody events |
| `transaction` | read/write | One bounded candidate assembly |
| `submit` | write | One 24-byte submit control at offset zero |
| `ack` | write | One 16-byte cumulative acknowledgement at offset zero |

The 32-byte header uses little-endian total length, API version, kind, connection
epoch, submission ID and sequence. Objects have both final fields zero;
candidates have a nonzero submission ID and zero sequence; events have zero
submission ID and a nonzero sequence. Typed body validation follows envelope
validation. A wrong outer epoch is ESTALE before custody.

Submission checks the exact declared length and matching epoch/submission ID.
At most one assembly and one accepted candidate exist. Another transaction open
is EBUSY while either exists. Append writes and identical already-written ranges
are allowed; gaps, changed ranges and bytes past the declared length are refused.
The first byte starts the advertised assembly deadline; progress does not extend
it. Clunk or expiry discards only an unaccepted assembly. Accepted bytes remain
immutable and readable through their original handle until receipt ack.

An exact submit retry before receipt ack succeeds without a second event or
effect. A different submit cannot replace retained custody. IDs at or below the
accepted submission watermark return EALREADY after release. Resource pressure
returns EAGAIN before mutation and permits an explicit same-ID retry. No replay
crosses a connection epoch. A journal read may be partial; it waits at the tail
and refuses released offsets. Acknowledgement releases retention, not semantic
completion. Fixed and dynamic Qid paths are never reused for a different node.

## Negotiation and refusal

Submit one Negotiate with the desired broker interface revision range. Revision
2 is the supported semantic revision; the file API version is independently 1.
Malformed reserved bytes are EINVAL. An invalid or unsupported revision range
produces Submitted followed by Refused; otherwise Submitted and Negotiated.
Each pair is reserved and committed atomically. Limits and Negotiated must agree
on surface, label and response-row bounds. ResponseSet before negotiation is
EACCES; another Negotiate is EALREADY.

Refused enters a drain state. No new candidate is accepted, but its journal stays
readable until the terminal ack or the advertised ack deadline. Immediate
revocation after appending Refused is forbidden because it would hide the event.
An actual authority revocation remains immediate and distinct from this drain.
The drain deadline starts when Refused is appended and cannot be extended by
acknowledging only its preceding Submitted. Normal ack-progress resets do not
extend a refusal drain.

## Requests and atomic responses

Preserve all five request kinds, including the currently unused production
senders for attention and disclosure changes. Exactly one request may be pending.
One ResponseSet answers it with one or two ordered rows. The set carries the
exact request ID and kind. `s` below is the pending request's surface.

| Request | Allowed non-rejection response |
| --- | --- |
| SurfaceAdmitted | PublishRule(s); or PublishRule(s), EmitDescriptor(s) when default ClassOnly lowers an existing disclosure after a candidate |
| CandidateReduced | EmitDescriptor(s) at the candidate generation; or NoChange (accepted by the existing Session API) |
| AttentionChanged | NoChange; or EmitDescriptor(s) with the requested attention and current generation |
| SurfaceRemoved | RetireSurface(s) |
| SetDisclosure | PublishRule(s); or PublishRule(s), EmitDescriptor(s) when lowering after a candidate |

The two-row lowering response has absent label and false label-redacted in its
descriptor. It retains the current generation and grant. Rejected and NoChange
are always single rows. Request-specific reducer rejections remain:
SurfaceAdmitted 1/3, CandidateReduced 1/2/4, and the other requests 1. Rejection
5 (InvalidConnectionEpoch) is also representable for every pending request,
as in the old codec, despite having no current reducer producer. It is not
equivalent to wrong-outer-epoch ESTALE. Unknown rejection values are malformed.

Validate layout, unused zero fields/rows, request correlation, allowed ordering,
surface correlation and generation/grant relationships before custody. Then
prepare all Session effects and their enqueue capacity against current routes,
retirement and descriptor state before applying any row. An owner-state failure
applies no row and follows broker failure handling. This explicitly repairs the
old wire server's refusal when one reducer operation emits two commands; it does
not split those commands into independently accepted submissions.

Preserve absent versus present-empty UTF-8 labels, including control characters
accepted by the old codec. Reduction/disclosure stays with the existing
authorities. Boolean presence/redaction fields remain strict; redacted requires
a present label, and unused fixed text bytes are zero. Present icon zero remains
representable. Raw request surface IDs and candidate generation zero reach the
reducer's UnknownSurface/StaleGeneration paths. Returned authority-bearing
identities still undergo Session validation. A descriptor action has nonzero
token, revocation epoch and target generation; its target generation equals the
descriptor generation. Dynamic revocation is not claimed: the reducer currently
uses revocation epoch 1 and relies on generation and retirement checks.

## Bounds and deadlines

Defaults/ceilings: 1,024 surfaces, 128 label bytes, two response rows, 16 journal
records, 4,096 retained journal bytes, 512 staging bytes, 5-second response,
2-second ack-progress and 12-second assembly deadlines. Limits may lower these
within the KDL ranges. A surface bound sizes the broker's state and is enforced
by the reducer, not merely reported to its caller.

| Retained item | Records | Bytes |
| --- | ---: | ---: |
| Submitted + Negotiated | 2 | 104 |
| Submitted + Refused | 2 | 88 |
| BrokerRequest + its reserved Submitted | 2 | 248 |
| Maximum paced retained batch | 4 | 352 |

Reserve the handshake pair before negotiation custody, and the request plus its
receipt before publishing a request. The next request waits until the prior
response is applied and its Submitted acknowledged. Capacity refusal consumes
no identity and leaves the unappended request in Session's single pending slot.
The current request has a 5-second deadline from publication. Any nonempty
journal starts the ack-progress deadline; it need not become full first.
Only an advancing ack resets that deadline. Withholding the final ack cannot
stall the next request forever. Sequence, byte-offset or request-ID exhaustion
refuses before mutation; identities never wrap.

## Required replacement evidence

Require literal and malformed vectors, rejection 5 versus stale outer epoch,
empty/control labels, zero optional icon, all five requests, both two-row paths,
and second-row failure with no first-row effect. Exercise exact retries,
pre-custody pressure, terminal refusal draining and the final-ack deadline.
Preserve protected admission negatives and the existing metadata-chain,
issuer-generation and broker-transport coverage. Independent C and Rust desktop
SDK clients must drive the real export. These are implementation requirements;
the checked layouts alone do not establish any of that behavior.

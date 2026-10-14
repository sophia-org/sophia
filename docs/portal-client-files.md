# Portal recipient files

The t318 namespace recipient endpoint is a read-only `portal-client` version 1
export on the shared 9P2000.L core. Session selects its namespace, service
generation and endpoint grant. The ordinary launch authentication conversation
must succeed before attach. A pathname, UID or client-supplied namespace cannot
select another recipient.

This is separate from the [portal coordinator files](sophia-portal-files.md).
The `portal-requester` admission contract still belongs to trusted protocol
authorities. A namespace client cannot upload payloads, approve transfers or
claim executor custody through this endpoint. Client-originated requests are
not implemented here: `api` and `status` explicitly report
`request=unavailable`. The renderer readback and recipient binding primitives
exist; the live presentation, request and CLI connections remain t319 work.

```text
Session-selected recipe / endpoint grant
                 |
       authenticated portal-client root
                 |
          api  status  transfer  watch
                         |        |
                         +----+---+
                              |
                  exact retained object identity
                              |
                  common PortalAccessLease
                              |
              coordinator policy / source generation /
              deadline / namespace / execution fence

trusted protocol authority -> common coordinator -> recipient object
                               approval + handoff     immutable bytes
```

| File | Behavior |
| --- | --- |
| `api` | Immutable open snapshot: `sophia-portal-client version=1 epoch=<generation> request=unavailable fd_transfer=none` followed by a newline. |
| `status` | Immutable open snapshot: `request=unavailable transfer=absent`, `ready` or `revoked`, followed by a newline. It is historical status, not a grant. |
| `transfer` | Current immutable recipient payload, bounded to 65,536 bytes by the common owner. Reads support offsets. |
| `watch` | A nonempty read at offset zero waits while its captured object remains live. Revocation, expiry or replacement answers `ESTALE`; no payload is returned. Nonzero offsets are invalid; zero-count reads return empty. |
| `frame-info` | Present only for an output-capture binding. A 64-byte immutable record describing the selected image; it uses the same lease as `frame`. |
| `frame` | Present only for an output-capture binding. Immutable, tightly packed, top-down RGBA8 pixels, at most 64 MiB per image. Reads return at most 65,536 bytes and support offsets. |

Before the first transfer, `transfer` and `watch` are absent (`ENOENT`). Walk
captures the current object; an old fid never follows a later object at the
same name. Replacement allocates fresh qids. All retained object operations,
including metadata and retried waiting reads, recheck the common recipient
lease. A stale object answers `ESTALE` while unrelated status access on the
authenticated role remains available. Endpoint-grant or launch revocation
instead retires the role itself.

Queued `transfer`, `frame-info` and `frame` replies retain their original
object identity. Each nonblocking socket write holds the recipient lease,
source-generation guard and lock barrier. Revocation therefore prevents any
further bytes from entering the socket. If a queued reply loses authority,
the server closes the connection and drops its unsent bytes; it cannot replace
a partly sent reply with an error. Fresh stale reads still answer `ESTALE`.
Bytes already accepted by the kernel before revocation cannot be recalled.

Only read-only opens are accepted. Unknown control names are absent. No
descriptors cross the 9P stream. `Tversion` clears wire fids and requires fresh
authentication under the same pinned launch identity; it does not reset object
identity allocation. Open `api` and `status` snapshots retain their original
bytes and metadata until their fids are released.

Each transport uses the core bounds: maximum 64 KiB msize, minimum 512-byte
msize, eight pending requests, sixteen fids, 128 KiB queued replies, and one
connection per server. Dispatch shares Session's existing authentication quotas (eight
transports per endpoint and 32 globally). It accepts at most one connection
per listener per turn and retries quiet connections within the existing
20 ms idle-check bound. This bound is not a latency measurement.

The recipient retains only the current payload. Old fids contain identities,
not copies of earlier payloads. Transfer approval, source collection and
recipient installation use the existing common coordinator; this export adds
no second transfer policy or grant type. Tests may supply policy and source
bytes without making an application-frontend or renderer claim.

## Output-capture bindings

An output-capture transfer uses MIME type
`application/x-sophia-output-frame-v1`. Its portal payload is the 64-byte frame
record, so the common upload limit remains 65,536 bytes. The trusted source
separately retains the immutable image. Recipient installation requires the
source image's exact record and output generation to match the approved
transfer. An approved record alone cannot bind pixels. `transfer` contains the
same record as `frame-info` for this binding.

The record contains eight little-endian unsigned 64-bit fields:

| Field | Version 1 value |
| --- | --- |
| Version | 1 |
| Output | Nonzero opaque output identity |
| Output generation | Nonzero incarnation; never reused after replacement |
| Presentation generation | Nonzero identity of the selected completed presentation |
| Width | Positive pixel width, representable as a u32 |
| Height | Positive pixel height, representable as a u32 |
| Row stride | Width multiplied by four |
| Format | 1, meaning RGBA8 |

The dimensions must fit the image limit; storage must contain exactly
`width × height × 4` bytes. Unknown versions, formats, trailing bytes or
noncanonical strides are refused. Decoding the record establishes no authority
or proof of presentation.

`OutputCaptureSource` holds one candidate presentation and at most one image
selected for handoff per trusted recipient/coordinator binding. A later
presentation replaces the candidate without changing an installed image.
Accepting a new transfer replaces the recipient's one current object; old fids
then answer `ESTALE`. Output withdrawal or replacement independently invalidates
both files without revoking another output's grant. A new incarnation requires
a strictly greater output generation.

Retained reads take the common execution fence, then the source-generation
guard, then the recipient binding guard. Each read copies at most 64 KiB while
holding these guards; it does not wait for transport progress. Output withdrawal
waits for earlier guarded reads, and subsequent reads refuse. Session lock,
grant expiry and coordinator teardown still fence access through the common
lease. Unlock cannot revive an earlier grant. Bytes already delivered remain
with the recipient.

These are storage and handoff contracts, not a live screenshot feature. Session
must still connect actual renderer/completion identity, request admission,
aggregate image retention limits and the CLI. Supplied-presentation tests prove
binding and stale-handle behavior; they do not prove native presentation or
final network-delivery timing across a live lock.

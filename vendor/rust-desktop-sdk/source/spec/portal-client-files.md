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
available unless the trusted owner supplies a capture request queue: otherwise
`api` and `status` report `request=unavailable`. The optional request files below
exist; Session supplies them for explicitly capture-approved observer recipes.
The integration and admitted CLI are under t319 qualification and have not
been installed. Physical/performance evidence remains required.

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

Only read-only opens are accepted except for the optional `capture` request file.
Unknown control names are absent. No
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

## Optional output-capture requests

A trusted recipient owner may supply a `CaptureRequestQueue`. Such an endpoint
advertises `request=output-capture-v1` and adds `capture` and `capture-status`.
The same authenticated root owns both. A queue admits one outstanding request
and holds no image bytes. It does not authorize a transfer: Session must apply
its explicit output policy and the common coordinator before publishing pixels.

`capture` accepts only write-only opens and one complete 24-byte write at offset
zero. Its three little-endian u64 fields are version 1, a nonzero request serial,
and a nonzero output identity. Serials must increase within the recipient owner;
they correlate requests and never select a grant, namespace or native frame.
Malformed writes return `EINVAL`, overlapping requests `EAGAIN`, old serials
`ESTALE`, and admission while locked or closed `EACCES`.

`capture-status` is a read-only immutable open snapshot with four little-endian
u64 fields: version 1, request serial, output identity and state. States are
0 idle, 1 queued, 2 capturing, 3 ready, 4 denied, 5 unavailable, 6 pressure and
7 cancelled. Idle has zero serial and output; all other states identify the
request. A status record is historical information, not authority to read a
frame. Requests expire after ten seconds and are tied to the lock-fence revision.
Unlock cannot revive a queued or active request from an earlier revision.
Only the queue's exact trusted work token can settle it, and a late completion
cannot settle a replacement request or another recipient's queue.

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

Every `PortalCaptureFrame` requires a reservation from a shared
`CaptureRetentionBudget`. One budget permits at most 128 MiB and eight frames;
owners may choose smaller limits. Pending reservations count toward both
limits. Completing a reservation requires exactly the reserved byte length;
cancellation or an invalid completion releases the charge. Sharing a frame
between source and recipient does not charge it twice. Its charge remains
until the last frame reference is dropped, including after revocation if an
owner still retains it. Old wire fids hold identities, so they do not retain
the old frame after recipient replacement. Capacity refusal preserves existing
bindings.

Session shares one portal budget across recipients and reserves before
scheduling readback. Native images have a separate process-wide cap of 128 MiB
and eight images, including mirror heads and scanout leases. Every head's native
reservation must succeed before capture is marked on any head. Reservations
follow deferred work and are consumed once by readback; snapshot clones retain
the same charge until their last reference is dropped. Constructing separate
portal budgets per recipient would not establish a session-wide bound.

Retained reads take the common execution fence, then the native output lifetime
guard, the source-generation guard and the recipient binding guard. Native
preview failure, topology preparation and native owner destruction withdraw the
lifetime guard synchronously; delivery does not wait for another Session poll.
Each read copies at most 64 KiB while holding these guards; it does not wait
for transport progress. Output withdrawal
waits for earlier guarded reads, and subsequent reads refuse. Session lock,
grant expiry and coordinator teardown still fence access through the common
lease. Unlock cannot revive an earlier grant. Bytes already delivered remain
with the recipient.

The Session integration is still under qualification. Its state-machine checks
and authenticated CLI tests pass, including an exact 4K transfer within the
unchanged grant deadline. Physical and performance evidence remain incomplete.
Supplied-presentation tests prove
binding and stale-handle behavior; they do not prove native presentation or
final network-delivery timing across a live lock.

### Image extent, occlusion and cursor

Version 1 captures the entire native composition of the selected logical
output's primary head. Dimensions and coordinates are native image pixels,
after that head's projection and clipping. There is no window selector or crop
rectangle in the request, and the image does not combine separate outputs.
For a mirror group, all enabled heads must converge on the selected frame
before disclosure; the returned image is the configured primary head's image.

The image contains the visible application and shell layers in their composed
order. Covered content stays covered, and content outside the output is clipped.
This interface does not recover the unobscured pixels of a covered window.
Capture forces composition even when direct scanout would otherwise be eligible.

Session's pointer sprite is omitted. Native capture builds its scene without a
software cursor and does not sample the separate hardware cursor plane. A cursor
painted by an application into its own content remains part of those pixels.
The returned rows run from top to bottom; no cursor metadata or separate cursor
image is attached to the grant.

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
`request=unavailable`. Renderer capture remains t319.

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

Before the first transfer, `transfer` and `watch` are absent (`ENOENT`). Walk
captures the current object; an old fid never follows a later object at the
same name. Replacement allocates fresh qids. All retained object operations,
including metadata and retried waiting reads, recheck the common recipient
lease. A stale object answers `ESTALE` while unrelated status access on the
authenticated role remains available. Endpoint-grant or launch revocation
instead retires the role itself.

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

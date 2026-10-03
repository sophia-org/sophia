# Lock provider file records — revision 1 (draft)

**Draft.** The native codec, `sophia_protocol::lock_files`, is bound to
`protocol/sophia-lock-files-v1.kdl` by `tests/lock_file_schema.rs`.
`sophia_runtime::lock_files` serves it (custody, the 9P export, the endpoint
and a worker thread), and Session launches and supervises the provider and
draws its images. It is the t294 design under the
[proposed lock ADR](notes/decisions/w0seozxx-session-owns-lock-state-and-authentication-lock-providers-only-render.md);
a layout change must change the KDL and the codec together. An independent C
peer (`tests/support/lock_files_peer.c`, the C SDK's generic 9P client and
hand-encoded records) negotiates, uploads, demands, offers and is presented
against the production export. Not yet implemented: following a
render-device change after a direct GPU grant (restarts reuse the device
granted at start).

This document specifies the lock provider role over 9P2000.L. A lock provider
draws what a locked session shows. It does nothing else: Session owns the lock
state, Engine owns the cover drawn beneath the provider's images on every head,
and the Session authenticator, `sophia-factotum`, alone decides an unlock. A
locked session needs no provider at all; without one, every head shows Engine's
opaque fill, and unlocking works the same.

## Authority

The provider:

- receives no character, keycode, keysym or length of the secret, only the
  [entry events](#entry-and-chord-events) below;
- cannot enter or leave the locked state, delay the cover, or report a lock or
  an unlock; no record here carries such a request;
- receives no pixels other than its own and no application, window or output
  metadata beyond its own allocations;
- draws only within its allocations, and only for the lock epoch that granted
  them;
- receives no pointer input.

Its images draw above Engine's fill, and nothing draws above them. A missing,
stale, rejected or slow image leaves the fill. Provider failure, stall,
replacement or departure never uncovers a head.

## Endpoint and admission

The operator selects at most one provider in the profile:

```kdl
session {
    lock-provider {
        executable "/usr/libexec/kleis"
        config "/home/user/.config/kleis/config.kdl"
        // GPU execution defaults to denied, as for shell components.
        gpu "denied"
    }
}
```

Session launches the provider once the first output topology is published,
so its limits follow the real screens, and only when an authenticator is
configured. It runs in its own protection domain with no network, its socket
directory and configuration file read-only, and receives
`SOPHIA_LOCK_9P_SOCKET` and, when the profile names one, `SOPHIA_LOCK_CONFIG`.
A provider that exits is restarted after a delay doubling from 1 s to 60 s,
reset once a replacement negotiates; it is never given up. Peer
admission follows the output role: the endpoint retains the launched process's
pidfd and compares it with `SO_PEERPIDFD` before and after credentials, and
admits one attach per connection epoch. A replacement process gets a fresh
connection epoch. Paths, attach names, UIDs, fids and qids grant nothing.

The provider is resident so that its first image is ready soon after a lock
begins. It holds no allocation while the session is unlocked.

## Records

The header is the common 32-byte identity shape of the output and shell
contracts: total bytes u32, API version u16 (1), kind u16, nonzero connection
epoch u64, submission ID u64, journal sequence u64. Objects have submission ID
and sequence zero, candidates a nonzero submission ID and zero sequence, events
zero submission ID and a nonzero sequence. Unknown kinds and versions refuse;
reserved bytes are zero.

| Kind | Record | Direction |
| --- | --- | --- |
| 1 | Limits | object |
| 2 | Lock | object |
| 16–19 | Negotiated, Refused, Submitted, ObjectPublished | event |
| 33, 34 | ResourceStatus, ResourceReleased | event |
| 35 | CandidateOutcome | event |
| 36 | FramePermit | event |
| 40 | Entry | event |
| 41 | Chord | event |
| 256 | Negotiate | candidate |
| 258–261 | ResourceBegin, ResourceEnd, ResourceCancel, ResourceRetire | candidate |
| 262 | Candidate | candidate |
| 263 | FrameDemand | candidate |

The root vocabulary is fixed: `api`, `limits`, `lock`, `events`,
`transaction`, `submit`, `ack` and `upload/0` .. `upload/N-1`. Submission,
acknowledgement, journal and upload-slot custody follow the
[shell file rules](sophia-shell-files.md#records-and-submission) unchanged; a
candidate is at most 128 bytes.

## Negotiation

`Negotiate` names a revision range, the requested capabilities and up to eight
chord requests. `present` (bit 0) is required; `chords` (bit 1) is optional. A
chord request is an XKB keysym and a modifier mask holding at least one
modifier other than Shift. Session refuses the whole negotiation
(`invalid_chord`) if a chord is malformed or collides with a reserved session
chord: VT switching, the emergency chord, or any other chord Session keeps for
itself. `Negotiated` grants the chords in request order; a chord's ID is its
index.

## Limits and allocations

`Limits` is fixed for the connection epoch. A resource holds at most the
largest allocation the topology at admission grants (an ordinary 1920x1080
screen at least), under the contract's ceiling of 16,384 pixels a side and
1 GiB, and two live resources per output are allowed, so a provider never
holds more than twice the screens it covers. A later topology that needs more
answers larger uploads with `budget`, and those outputs show the fill. The lock budget is
separate from the shell content registry, whose per-resource and shared
ceilings cannot hold whole-output surfaces.

The `lock` object is republished whenever the lock phase, lock epoch or
topology changes. It carries the phase (unlocked, locking, locked, unlocking),
the lock epoch (zero when unlocked), the topology generation and one allocation
per output while the phase is locking or locked. An allocation covers its
output: its pixel size is the output's logical size times its scale. Mirror
heads of one output share its allocation; Engine scales the image to each head.

A provider draws for locking and locked, and stops at unlocking or unlocked.
An allocation of an earlier lock epoch or topology generation is stale.

## Resources and candidates

Resources are premultiplied BGRA8. `ResourceBegin` binds a free slot below
`Limits.upload_slots` to a new resource; a busy or absent slot, or a resource
already live or uploading, fails the write. A size over the limits, or more
live resources (uploads included) than `max_live_resources`, is answered
`rejected` with reason `budget`; otherwise `admitted`. Bytes are written to
`upload/N` at exactly the upload's cursor and never past its declared size.
`ResourceEnd` naming the declared total, after every byte arrived, makes the
resource whole (`accepted`); anything short is `rejected` (`size_mismatch`).
Either ends the binding, as `ResourceCancel` does (`cancelled`). Resources
belong to the connection epoch, so a provider may keep one across lock epochs;
`ResourceRetire` of a whole resource frees it and is answered
`ResourceReleased`.

A provider asks for a frame with one standing `FrameDemand` per allocation; a
newer demand replaces the standing one, and a demand for an allocation the
current lock object does not grant fails the write. Session answers with a
`FramePermit` only while that allocation shows nothing unretired: a
candidate is in flight until every head of its output has retired it, so
presentation paces the provider at its slowest head. Session's permits expire
after 100 ms (the contract allows up to 250 ms) and grant one candidate. Candidate generations only
increase per allocation. Every forwarded candidate and standing demand holds
room in the journal for its answer, so a full journal refuses new work rather
than an answer. A
`Candidate` names the lock epoch, output, allocation, permit and one resource
whose size equals the allocation exactly. It has no placements, targets or
actions. `CandidateOutcome` reports prepared, presented, rejected, superseded or
revoked. A candidate for a stale lock epoch, allocation, resource size or
permit is rejected and changes nothing on screen. A new lock object revokes
forwarded candidates, and lapses demands and permits, for allocations it no
longer grants.

## Entry and chord events

`Entry` reports what the secret did, never what it holds:

| Value | Entry | Meaning |
| --- | --- | --- |
| 1 | insert | committed text was added |
| 2 | delete | the last character was removed |
| 3 | clear | the secret was emptied without a submit |
| 4 | submit | the secret was submitted |
| 5 | checking | an attempt is in flight |
| 6 | failed | the attempt was rejected |
| 7 | unavailable | the authenticator could not decide |

`empty_after` says whether the secret is now empty. Insert and delete events
let a provider count characters; that disclosure to the provider is accepted,
and the provider's protection domain receives no network. An accepted unlock is
not an entry: the provider sees the `lock` object move to unlocking.

`Chord` reports a granted chord by ID. Its keys are delivered only as the chord
and never reach the secret.

## Controls

The t294 exit requires: the codec bound to the KDL by test; an independent C
peer exchanging records with the export; and controls proving that a provider
cannot claim or end the locked state, see characters, draw outside its
allocation, present into a newer lock epoch from an older connection or delay
the cover, and that crash, stall and replacement leave Engine's fill.

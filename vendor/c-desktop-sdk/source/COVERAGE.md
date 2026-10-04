# Desktop SDK coverage

## Required boundary

The C SDK and Rust SDK remain independent repositories, one per language.
Each must cover the complete public WM, shell, output and admin/control surface
previously served by IPC. Modules and library archives inside a repository do
not create additional SDKs. Hagia's Nim bindings call the C SDK.

All roles use ordinary 9P2000.L operations and error numbers. Sophia defines
the admitted files and their typed contents, not a private 9P dialect. Role
authorization, semantic acceptance and presentation remain with the server's
owners. Sharing transport does not merge roles or grant authority across them.

No product implements its own 9P pipeline, role envelopes or submission custody.
Clients retain their own policy and UI logic. No missing file operation falls
back to IPC. Former descriptor-based operations require a reviewed file or
protected-grant contract, rather than a hidden ancillary-data channel.

## Release 0.8.0

| Role | File contract | Client status |
| --- | --- | --- |
| Shell: bar, native launcher, catalog/dock, indicators, content | Pinned API 1, revisions 6–8 | Implemented; scope and evidence in README and tests |
| WM: negotiation, profile handoff, configuration, snapshots, cycles, projections, session operations, presentation receipts | Sophia WM file API 1 (`b0721d0de`); `api` names the 9P2000.L output transport | Implemented: literal/scripted tests including exact `api` refusal of `current_ipc`, Sophia production WM export gate (`c4e17899e`) and Hagia thin bindings (`b3d8496`) |
| WM chord lifecycle: `action_lifecycle` Configuration rows and ActionLifecycle cause; `chord_actions` ChordAction cause | Additive contracts (`86bf6504`, generated rows `3a1dce11`; ChordAction `b0a2b030`) | Codec and negotiation, with literal and scripted-peer tests only; Session's implementation, production export gate and Hagia use are pending |
| WM held capture: `held_capture` (bit 22) keyboard output and bindings on an Overlay presentation | Additive contract (`ac04e1a7`) | Negotiation dependency with scripted-peer tests only; the codec is unchanged; Session's implementation and Hagia use are pending |
| Output authority: negotiation, topology publication, validate/apply proposals, outcomes | Sophia output file API 1, revision 1 (`b0721d0de`) | Codec/session, literal and scripted-peer tests, independent C exchange against the production export, and Sophia's attended native acceptance (`ddd27bd6d`; one card, two heads, refresh-only change) |
| Lock provider: negotiation and chords, the lock object, uploads, frame demands and permits, candidates and outcomes, entry and chord events | Sophia lock file API 1, revision 1, experimental: the contract still marks itself draft (imported from `fd6ea7538`; byte-identical at the signed master merge `61d545c90`) | Record codec: literal round trips of Sophia's golden records and the contract's refusal rules. Client (`sophia_lock_client.h`): tested against a scripted 9P peer for bootstrap, negotiation and refusal, lock object fetches including a superseded announcement, uploads (admitted, rejected, refused, cancelled), demands, candidates, EAGAIN retry, custody held until acknowledged, and ESTALE. One scratch run against Sophia's production export (fd6ea7538): negotiation with a chord, lock fetches, a two-chunk upload, demand and permit, a presented candidate, entry and chord events, and the republish to unlocking. `lock_files` stays false until the contract is accepted and Sophia's permanent production export harness passes with this SDK vendored |
| Admin/control: the public control operations and their results | Existing control schema does not establish a 9P file contract | File contract and SDK client gap |

This table does not claim that a transport codec provides a session client, or
that a unit test proves operation against a live Session. `compatibility.json`
continues to describe implemented support until the relevant gates pass.

## Parity gate

Descriptor support covers all seventeen native envelopes:
three whole objects, eight events, two activation acknowledgements and four
presentation candidates in `spec/sophia-shell-files-v1.kdl`. It validates envelopes, identities,
bounded text, row counts and uniqueness, action/connection bindings, style and
outcome relationships using literal vectors. Snapshots and larger candidate
rows borrow caller-owned encoded storage; fixed candidate arrays remain bounded.
The descriptor profile validates its explicit api role and exact
selected capabilities, supports metadata-only and combined content readiness,
and tracks fetch/ack holds for every disclosed feed. Scripted tests exercise
partial reads, EOF, supersession, qid/generation matching and capability
refusals. Existing content roles refuse descriptor disclosure before consumption.
The low-level file client accepts caller-owned transaction scratch through
`sophia_sf_client_init_buffers`; scripted tests send complete maximum tab and
reference candidates, preserving custody and explicit same-id EAGAIN retry.
The queued session accepts separate caller-owned transaction storage through
`sophia_ss_open_fd_staging`. Scripted tests cover maximum tab/reference groups,
fragmented writes, immutable hand-off, atomic refusal, reservations, paced
same-id retry and disconnect custody. Existing initializers retain 8 KiB inline
staging; per-kind codec limits and the total queue bound remain unchanged.
Sophia's independent C peer exercises all seventeen families against the
production export, all three descriptor host modes and the launcher host.
Its protected CPU presentation test checks reservation changes only after
matching presentation. These are deterministic gates, not physical GPU or
installed-desktop acceptance.
These descriptor records are part of the published API-1 contract.

For each role, inventory every former IPC request, response, event, capability,
resource grant and terminal outcome against a named file-contract operation.
The inventory must include negative behavior: refusal, backpressure, stale
epochs, revocation, disconnect, unknown custody and resource retirement.

A role is complete only after its authoritative contract, server export, C and
Rust client modules, literal codec tests and real-export conformance tests agree.
Migration must not silently drop an operation or report socket tests as file
coverage. Removed product IPC tests are retired with that wire; their required
behavior is accounted for by the replacement file tests.

## Product pins

Products vendor or otherwise bind an exact SDK source revision and tree. Pins
are per product: adding WM support for Hagia does not change Bemenu's existing
SDK pin. Builds verify the vendor manifest and record its digest and revision.
There is no ambient sibling SDK checkout in the build inputs.

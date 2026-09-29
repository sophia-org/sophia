# C shell file client

## Native 9P file backend (t252 B7)

`sophia_9p_client.h` with `nine_p/*.c` implements version, attach, walk,
lopen, read, write, clunk and flush over an already admitted stream fd.
`sophia_shell_files.h`, `sophia_shell_files_content.h` and `shell_files/*.c`
provide the independent base file records and `sophia_shell_files_client.h`
orchestrates a file session. Compile these C99 sources together.

### Role record codecs

`sophia_shell_files_roles.h` adds Catalog and Indicators objects, native launcher
events/requests, and persistent catalog candidates/activations. These use the
same `sophia_sf_encode` / `sophia_sf_decode` entry points. The file session also
supports r7/r8 through `sophia_sf_client_init_profile`; Bemenu adoption remains
separate work.

Candidate bodies also expose `sophia_sf_role_candidate_{encode,decode}_bytes`
and `sophia_sf_role_candidate_validate_value`. Byte operations enforce field
bounds; normal record operations additionally enforce family value rules.
Native one-surface/nonempty-placement, slot uniqueness, selection membership
and target/row-count rules are value checks (submit EINVAL, no journal entry).
Catalog one-surface/nonempty-placement checks and state-dependent eligibility
belong to the runtime owner. Byte decoding alone grants no authority.

Catalog and Indicators rows borrow immutable encoded storage. A decoded
`sophia_sf_catalog` points into the original record; `sophia_sf_catalog_entry_at`
returns a typed row with borrowed text. Row encoders let the caller construct
the storage without allocating a 4096-entry C struct array. Native input text
also borrows its record buffer. Keep that storage alive until finished with
the decoded value. Persistent catalog validation uses about 12 KiB of bounded
stack scratch for slot and identity uniqueness; it does not allocate a catalog
or change the borrowed rows. Encode into separate storage: destinations must not overlap
the input value or its borrowed data. Failed encoding leaves destinations
unchanged; failed decoding leaves output values unchanged.

The largest record cap is 4 MiB (`SOPHIA_SF_MAX_RECORD`); submission staging
retains its 64 KiB cap (`SOPHIA_SF_MAX_TRANSACTION`), and native/catalog
candidates remain bounded to 8192 bytes. The record union contains bounded
typed candidates and snapshot views, never a full catalog allocation.

`sophia_sf_client_init_profile` takes BAR, LAUNCHER or DOCK plus caller-owned
object scratch. It checks the API role and, for launcher/dock, the exact
revision/capability profile. Supply 4 MiB for maximum catalogs, or 32 KiB for
indicators. The base initializer retains its inline 1 KiB buffer and BAR profile.
Large reads respect msize/iounit. An oversized object fails the fetch and closes
its pin without overflowing the buffer or terminating the session. Storage must
remain alive and separate from client/wire state until disposal; returned views
remain borrowed until the next fetch.

The `SOPHIA_SF_DESCRIPTOR` profile uses `spec/sophia-shell-descriptors.md`. Its api
must name `descriptor`, and its offer must obey that proposal's revision and
capability dependencies. `sophia_ss_welcome` exposes the validated selection.
Metadata-only sessions become ready after bootstrap Submitted and Negotiated
are consumed, with no Limits request; combined content also needs valid Limits.
Unselected object and record families are refused locally. Descriptor, tab and
shortcut announcements participate in the same fetch/ack holds as other feeds:
full decode, matching qid/generation and an EOF probe precede release.
`sophia_sf_client_init_buffers` and `sophia_ss_open_fd_staging` support caller-owned
large transaction scratch. A 52,488-byte transaction buffer accommodates all
native candidates (including 8,260-byte maximum tab candidates); provide
separate queue capacity for the desired admission group. The queue retains its
512 KiB total bound and 64-slot bound. The transaction buffer must be separate
from queue storage, object scratch and the session; the initializer checks all
regions before starting I/O. Records are copied on admission, then copied into
transaction storage before queue compaction. The inline initializer still uses
8 KiB and refuses records that can never fit without issuing a ticket or
submission. `sophia_ss_record_bytes` validates and measures candidates without
allocating encoded scratch; it does not promise admission capacity. This
development path has scripted-peer coverage, not production-export
qualification yet.

Reference inputs are the shell file KDL at `bae4ec4a9` (including the Limits
rules from `f64d670e0` and conditional-bound correction `6bb0c8f2e`), extended
with role layouts and normative value rules from `ee5e7f809` and the validation
layer clarification in `ae60576f8`,
profile blob `de101e3d` and diod reference blob
`48d63c80` (upstream `de51d1ee1bd5`). The KDL alone supplies record layouts
and validation. The API lifecycle contract is pinned at `43e4530b3`.
After attach, the client reads the complete `api` file and learns its epoch
before submitting any records. It rejects malformed, oversized or unterminated
API lines. Session's launch owner supplies the revision range, capability mask
and fd. Bemenu can select this backend explicitly at startup
and own a `sophia_sf_client`; it must continue consuming events and servicing
the fd. There is no endpoint discovery or automatic backend selection.

Transport memory is caller-owned: `(2 * requests + 1) * msize`, with 1–32
requests and msize 4096–16777216. Each request reserves its completion storage
and a paired flush slot. Completed replies, including empty ones, retain their
tag until consumption and any Rflush. Handles include a monotonically increasing
serial so stale handles cannot consume a reused slot. Flushing a clunk is refused.
An unanswered flushed attach/walk releases its reserved fid; a reply preceding
Rflush preserves its effects. Each service call performs at most 32 send and
32 receive attempts with separate byte budgets. A clean EOF allows validated
completions to drain; malformed replies poison the connection. The caller closes
the fd and disposes of all storage. No reconnect or replay is implicit.

The session requires at least eight request slots and enough fids for the root,
four service files and temporary object/upload pins (32 is a suitable setting).
It holds at most one staged submission, one decoded event, one object fetch and
one upload. An events read can remain outstanding while other operations run.
Call `service`, inspect and consume events, and explicitly call `ack` after
every service and event-processing pass. Fetch announced objects promptly:
their acknowledgement holds also block the next queued submission. Do not wait
for a full journal or an acknowledgement deadline. Submitted proves custody,
not semantic acceptance. Positive
short writes advance the acknowledged cursor; submit EAGAIN retries the same
submission. Every completed submission closes and reopens its transaction fid.
The client validates the selected revision and required capabilities.

Negotiation starts a Limits fetch when announced. Drain `object_result` after
it completes. Subsequent Limits/Outputs fetches use fresh pins and compare the
announced generation and qid; a superseded pin reports AGAIN. The returned
object pointer remains borrowed until the next fetch. Unknown record kinds
fail closed. Catalog/Indicators use the same pin and announced-identity checks.

Use `upload_begin`, then wait for status 1 and the upload writer to open before
calling `upload_chunk`. The chunk pointer remains borrowed until acknowledged
or terminal status cleanup; the client advances it across short writes. It
supports one transfer at a time within the granted slot count. Consume pending
events before continuing uploads. End requires every declared byte to have been
acknowledged. Cancel waits for the writer to open and returns BUSY while writes
remain outstanding; retrying follows them in order. Status 2 accepts, 3 rejects
and 4 cancels; terminal
status closes the writer before the slot is reused. The application still owns
allocation, permit, action and presentation lifecycle decisions.

Run `sh tools/check_shell_c_wire_files.sh`. The Rust test harness compiles the
independent C tests and exercises the production export with supplied protection
evidence. Coverage includes R4-1 through R4-7, tag wrap, reply poisoning, clean
EOF, 22 base and 17 role KDL-derived literal vectors, negotiation, rejected allocation, a split
64 KiB upload, cancellation and explicit acknowledgements. It does not start a
desktop or prove protected launch. Base records include native whole Candidate
rows. `shell_files_c_roles` additionally drives the C launcher and dock sessions
through real allocation/resource/candidate/focus owners, including a 4096-row
catalog, query disarm, semantic input, activation and close. Catalog admission
decisions are scripted from the contract; this is not a Session launch-policy
test. A small-buffer dock fixture also proves oversized-object refusal releases
the pin and leaves the session usable. Neither peer starts applications or
changes the Bemenu repository. These live flows pass against runtime
`3856d1014`; the file gate runs two base tests and one role integration test.

# Sophia desktop SDK for Rust

The opt-in [experimental shell read scheduler](EXPERIMENTAL-READ-SCHEDULING.md)
is a t331 performance candidate, disabled by default and not yet qualified.
The independent [experimental ACK handoff](EXPERIMENTAL-ACK-HANDOFF.md) also
remains disabled by default; it changes when a drive queues eligible ACK work.

CPU images use **Immutable Shared Buffers (ISB)**, currently backed by sealed
memfds. The buffer builders are `create_isb` / `finish_isb` in Rust and
`sophia_ip_isb_create` / `sophia_ip_isb_finish` in C. Names changed together;
source-kind numbers, discovery strings and seal enforcement are unchanged.
Image adapters require ISB and never select byte fallback. Shell clients may
enable the adapter after negotiated Limits, including dock and launcher roles.
Shell and lock byte-upload APIs have been removed. CPU images require the ISB adapters; passive legacy protocol values remain available for conformance checks.

Mapped ISB builders fill pixels in place, with no staging copy: Rust's
`create_mapped_isb` (the `sophia-isb` crate's `MappedIsb`) and C's
`sophia_ip_isb_create_bounded`. Both check dimensions and the caller's resource
bound before allocating, seal the size at creation, exclude the mapping from
`fork` and verify every seal on finish; see
[the mapped builder contract](API-PARITY.md#mapped-isb-builder-2026-10-18).

Native launchers can adopt a settled ISB into their resource lifecycle with
Rust's `adopt_native_image` or C's `sophia_ns_adopt_image`; see
[the adoption contract and coverage](API-PARITY.md#native-isb-resource-adoption-2026-10-18).

The [2026-10-17 release audit](RELEASE-QUALIFICATION.md) records verified
C/Rust capability parity, exact tested revisions and remaining limits.

Allocation-free [shell candidate sizing](SHELL-SIZING.md) is available in both SDKs.

Shell startup supports caller-driven progress from a connected socket or an
admitted root. See [startup controls and ownership](SHELL-STARTUP.md).

Launch authentication now supports caller-driven version negotiation, timers
and bounded I/O from startup through root handoff. The existing blocking
constructors remain available; see [launch parity](LAUNCH-PARITY.md).

Clients for the Sophia desktop's role contracts over 9P2000.L. A shell, window
manager, output or admin client links these crates; none of them needs a
Sophia checkout to build.

| Crate | What it is |
| --- | --- |
| `sophia-broker-protocol` | Native metadata broker request, response and custody records. |
| `sophia-broker-client` | Caller-driven broker session with retained requests, atomic two-row responses and explicit retry; see [broker parity](BROKER-PARITY.md). |
| `sophia-portal-protocol` | Portal recipient and requester file records. |
| `sophia-portal-client` | Recipient capture/frame and transfer sessions, plus bounded requester uploads and outcome tracking. |
| `sophia-admin-protocol` | Passive administrative info, catalog, invocation and status codecs. |
| `sophia-admin-client` | Administrative session with retained catalogs, command custody and caller-driven status snapshots; paired production admission and recovery gates pass. |
| `sophia-9p-records` | 9P2000.L value records (tags, fids, qids, wire errno, requests, replies), shared with Sophia's server core. |
| `sophia-9p-client` | Bounded 9P2000.L clients: a blocking read-only `client` and the nonblocking, pipelined `pipeline` the role clients drive. |
| `sophia-launch-client` | Sophia's local launch-channel authentication: correlated afid exchange, bounded progress and an admitted role root. |
| `sophia-namespace-client` | Bounded discovery parsing and Linux lookup against a pinned namespace directory; endpoint visibility confers no authority. |
| `sophia-desktop-ids` | Identifiers every role contract shares. |
| `sophia-image-producer` | Clockless ISB descriptor producer for shell r9 and lock r2; shell r9 and lock r2 adapters. |
| `sophia-isb` | Mapped ISB builder: one private writable mapping, sealed and verified on finish. The only crate that permits `unsafe`, in its private mapping module. |
| `sophia-shell-protocol` | The shell's typed records and the `sophia_shell_fs_v1` file contract codec. |
| `sophia-shell-client` | The shell client over the file contract. |
| `sophia-wm-protocol` | The WM file codecs and passive policy records. |
| `sophia-wm-client` | Developing WM session with negotiation, events, custody and Cycle-bound snapshots. |
| `sophia-output-protocol` | Output file codecs and passive topology values. |
| `sophia-output-client` | Output session with startup, submissions, journal and topology fetching; paired export and protected idle-revocation gates pass. |
| `sophia-lock-protocol` | The lock provider's `sophia_lock_fs_v1` file contract codec (experimental; see Contracts). |
| `sophia-lock-client` | Experimental lock session with startup, custody, events, object fetching and sealed-image submission; paired export and protected revocation gates pass. |

Shell, WM and output have native clients; their release qualification differs.
The lock provider now has an experimental session against its draft contract.
Administration has a developing session on a versioned pipeline. [API parity](API-PARITY.md) records
the comparison, evidence and remaining gaps. [Portal parity](PORTAL-PARITY.md)
records the recipient and requester scenarios and their qualification limits.

For reusable portal image buffers, `Session::frame_buffer(&mut Vec, deadline)`
keeps storage with the caller on local rejection. Admission takes the original
buffer without a copy; recover it with `take_frame`, or `take_frame_scratch`
after failure. The consuming `frame` convenience method remains available.

The broker client starts from an exclusively owned, quiescent 9P pipeline and
admitted root: drive `startup::Opening`, transfer its `Opened` result, then drive
`session::Session`. It retains one request independently of journal consumption.
Respond once, acknowledge events explicitly and retry only when requested by the
application; disconnect never reconnects or replays. The supplied-root constructor
does not authenticate the caller. The `broker_peer` example demonstrates the
lifecycle against Sophia's generic export and protected-launch gates. Sophia's
live broker now uses this SDK. Performance acceptance and the full desktop SDK
release audit remain open; [broker parity](BROKER-PARITY.md) records their limits.

The WM protocol crate includes negotiation, profile handoff, snapshots,
configuration, projection, Cycle events and outcome records. Its schema and
golden corpus match the C SDK's pinned contract. Encoding a valid record does
not establish custody or a committed scene. See [WM parity](WM-PARITY.md) for
the session's production evidence and remaining qualification limits.

The developing output client also exposes `Connecting::direct` and `attached`.
Drive startup with caller-monotonic milliseconds and a byte budget, then transfer
the ready session once with `take_session`. Consume retained events explicitly;
topology publications are exposed only after their exact object reaches EOF and
clunk. `topology` borrows the verified snapshot without a deep-copy handoff.
Proposal admission returns custody tickets, not proof of committed topology.
See [output parity](OUTPUT-PARITY.md) for remaining qualification gates.

Start the developing WM client with `Connecting::direct` for a supplied-admission
socket or `Connecting::attached` for an authenticated pipeline/root. Drive
`dispatch` until `ready`, then call `take_session`. Version, file opening and
role negotiation share the original absolute deadline; constructors perform
no socket I/O. Direct startup is never an authentication fallback.
WM requires at least eight request slots, sixteen fids, msize at most 64 KiB,
and input capacity of twice the negotiated msize plus 64 bytes for concurrent
event/snapshot reads and scalar replies. Handoff requires no unfinished traffic
and the exact sole root. Default pipeline bounds satisfy these requirements.
Drive it with monotonic milliseconds and a byte budget; inspect socket interests
and the wake deadline before sleeping. `event` retains one application head
until `consume`. `submit` returns a local custody ticket; `custody` distinguishes
Submitted from refusal and uncertain disconnect. Submitted does not establish
scene commit. Call `request_snapshot` while its Cycle is the retained head;
the request freezes the expected epoch, transaction and scene generation before
the event is consumed. `snapshot_result` borrows the verified object only after
EOF and clunk; `release_snapshot` frees the result for another fetch. One fetch
or retained result is allowed at a time. The generic `wm_conformance_peer`
example passes Sophia's supplied-admission production adapter/worker exchange
alongside the independent C peer. Protected launch, terminal-error coverage and
full WM qualification remain separate gates; see [WM parity](WM-PARITY.md).

For an external level-triggered event loop, `ShellConnection` implements
`AsFd` and exposes `io_interest()` alongside `wake_deadline()`. Drive `poll_io`
and drain typed events before sleeping. Recompute interests after state changes;
service `immediate` work without waiting, monitor only the requested socket
interests (plus hangup/errors), and wake by the retry deadline even on an idle
socket. The borrowed descriptor is for monitoring only; the SDK owns I/O.
`wait_for_io` remains available for applications that need a simple wait.

Use `dispatch(now, byte_budget)` to supply time and bound one complete service
call to that many bytes and 32 syscall attempts per direction. A zero budget
does no socket I/O. Drain with `take_*` methods when controlling the clock;
`poll_*` receive methods and `wait_for_io` use the default driver. Retry backoff
matches C: 4 ms, doubling to 256 ms, with earlier retry after event progress.

Before a local UI change that needs a content record, `reserve_content` (or
`reserve_content_group` for a complete candidate) validates and holds the record
and its queue capacity. Perform the local change, then call the returned
handle's `commit`; dropping it cancels. Commit performs no I/O or allocation and
returns Queued tickets. The handle exclusively borrows the connection, so keep
it within one synchronous operation.

For records chosen later, `reserve_capacity(slots, bytes)` returns a connection-
bound token that can stay live during dispatch and other admissions. Use
`commit_reserved_content` or `commit_reserved_content_group`, or explicitly
`cancel_reservation`; dropping this token alone does not release credits.
Invalid records or sizing leave it live. This form guarantees queue credits,
but encoding/allocation can still fail at commit, unlike the prepared handle.

For several content operations, build `ContentBatchItem::Record` and
`ContentBatchItem::Candidate` items, each with its own transaction. Use
`enqueue_content_batch`, `reserve_content_batch`, or
`commit_reserved_content_batch`. Admission is all-or-none locally, with one
ticket per item. The server still accepts or refuses each operation separately;
one accepted batch is not an atomic server transaction.

`ShellBatchItem` extends those forms to native, descriptor, indicator and catalog
operations through `enqueue_shell_batch`, `reserve_shell_batch` and
`commit_reserved_shell_batch`. It validates records and role support but does not
register lifecycle state or prove action eligibility. Use role helpers for
those checks. A one-item batch provides delayed single-record admission.

`set_queue_limits(QueueLimits { .. })` configures an empty live queue before
handoff to a role facade. Defaults preserve 64 records/2 MiB with 32 records/
8 KiB of control headroom; zero headroom uses a shared pool. In-flight records
remain charged until settlement. Queue resizing is refused while records or
delayed credits are held. Limits do not shrink retained allocation capacity.

`close` ends locally without I/O while keeping custody and decoded observations
readable. It releases delayed credits and blocks new work; the owned descriptor
closes on Drop. The native facade also clears input and lifecycle authority.

Use `custody_result(ticket)` to distinguish retained custody from
`CustodyLookupError::Unavailable` (evicted history) and `Unissued`. The existing
`custody` method still returns `None` for both lookup failures. Tickets must
be queried on their issuing connection.

For the native launcher, negotiate revision 7 with exactly the application
catalog, content surface, discrete input and native launcher capabilities.
`enqueue_native_launcher_tracked` sends allocation requests, input ACKs and
activations; `enqueue_native_candidate_tracked` sends a whole candidate.
`reserve_native_input_response` prepares an ACK and optional keyboard activation
before a local edit; commit admits both or neither, with separate custody tickets.
Drain `take_native_launcher_observation` for native events, content outcomes and
catalogs in order. Do not mix other receive methods into that ordered stream.
`NativeLauncherLifecycle` adds opening, presentation, focus and keyboard-input
tracking. Feed observations in order, use `enqueue_native_candidate_with_lifecycle`
to admit and track a candidate together, and `prepare_native_input` before any
edit. The returned handle holds the exact response; commit consumes the retained
input, while Drop leaves it available. Its status reports edit/activation
eligibility and the input ACK scheduling deadline. Call `service` after driving
the connection to reconcile custody and disconnects. Publication notices disarm
stale scenes before snapshot fetching completes.

Use `allocate_native`/`release_native_allocation` for allocation ownership.
Submit pixels through the image adapter, then call `adopt_native_image` after
Accepted and settled Retained disposition. `NativeLauncherSession` also exposes
`enable_image`, `image_begin`, `image_cancel`, `image_forget` and `adopt_image`.
Use `retire_native_resource` for retirement. Closing an allocation invalidates
local use but resources remain owned until the server releases them.

`demand_native_frame`, `cancel_native_demand` and `present_native_scene` track
permits and build candidates from current allocations and accepted resources.
Drive `service_at` with host monotonic milliseconds. Permit deadlines and the
caller-supplied margin are advisory; local expiry cannot authorize another
demand before the server ends the old one. `action` exposes retained pointer
events; `prepare_native_action` reserves an exact ACK and optional activation
of any eligible displayed row. Drop preserves the event; commit consumes it.
Cancellation events owe no ACK. `NativeLauncherSession` owns a fresh negotiated
connection and combines these operations with ordered `next_event`/`done`,
automatic transaction IDs, prepared responses, bounded `dispatch` and scheduling
`obligations`. Its two supplied clocks are transport `Instant` and host monotonic
milliseconds. Response Drop keeps the event; commit consumes it. Fetched
catalog/facts observations must be installed by the caller before presenting.
`presented` returns borrowed metadata for the last Presented candidate.
`apply_input` reserves before an editable input callback and commits its result
as Consumed or Stale. A callback panic fails the lifecycle closed and resumes
unwinding; it cannot safely replay an edit whose external effects are unknown.
Never use local admission as proof of presentation or application startup.

The protocol crate also carries the descriptor, tabs, shortcut/reference and
revision-4 launcher records, native file codecs and pure validation. This
descriptor client role follows `spec/sophia-shell-files-v1.kdl` and
`spec/sophia-shell-descriptors.md`. Scripted-peer tests cover custody and
disclosure; Sophia separately tests these clients against its production export.

## Contracts

`spec/namespace-discovery.md` defines namespace discovery. On Linux,
`sophia_namespace_client::Namespace::from_env()` pins the exact directory in
`SOPHIA_NAMESPACE_DIR`; `discovery()` exposes passive endpoint metadata and
`connect(path)` returns an owned, nonblocking, close-on-exec Unix stream.
Check protocol, interface and version before connecting, then use the existing
protocol authentication client. Keep the namespace object for subsequent
lookups in the same generation. No method follows a mutable `current` symlink
or changes the process working directory. Local tests are not yet a production
Session launch qualification.

For several 9P endpoints in one approved recipe, keep the admitted role pipeline
and pass its returned relay pipeline to `sophia_launch_client::Client::over_relay`
with the next endpoint stream. This transfers exclusive relay ownership and
refuses outstanding work. Only the relay is reset; existing role roots remain
untouched. Do not duplicate the relay between drivers. Authentication failure
closes the relay and can revoke the launch; there is no unauthenticated retry.

`spec/sophia-shell-files-v1.kdl` is a pinned copy of the file contract Sophia
owns (`protocol/sophia-shell-files-v1.kdl` in the Sophia repository).
`spec/sophia-shell-files.md` is the normative text of the file
contract (custody, retry, snapshot pins, role outcomes) that the client
implements, `spec/sophia-wm-files.md` the envelope, custody and retry rules
it adopts, `spec/sophia-9p-profile.md` Sophia's 9P2000.L subset, and
`spec/references/diod-9p2000L-protocol.md` the pinned dialect text, which is
GPL-licensed and kept for reference only under its own notice.
`spec/sophia-lock-files-v1.kdl`, `spec/sophia-lock-files.md` and
`spec/golden/sophia-lock-files-v1.records` are the lock provider contract, its
text and Sophia's golden records, which `sophia-lock-protocol` must render
byte for byte. They are pinned from Sophia's signed master merge `61d545c90`,
but the contract still marks itself revision 1 (draft): the lock codec is
experimental and can change incompatibly until Sophia accepts the contract
explicitly. A lock provider only renders: the contract carries no secret and no
unlock.
`spec/SHA256SUMS` holds every digest; Sophia's gate refuses a vendored SDK
whose copies differ from its own.

Generic callers can use `pipeline::VersionHandshake` for nonblocking version
negotiation with caller time and a shared `IoBudget`. `take_pipeline` transfers
ownership once without repeating Tversion. Existing blocking constructors remain
available. `limits`, `is_quiescent` and `owns_fid` support role handoff checks.

The dialect is plain 9P2000.L; wire error numbers are the Linux values
9P2000.L defines, whatever the host (see `PLATFORMS.md`).

## Release 0.3.0

The t317 development branch adds `Pipeline::auth` and `attach_with_afid`,
plus authentication qids and records. `sophia-launch-client` builds Sophia's
local launch conversation on those generic wire primitives. This is not a
claim that deployed roles require authentication.
New public enum variants require downstream exhaustive matches to be updated.
The profile copy describes Sophia's candidate opt-in export seam.

## Local launch authentication

An authorized supervisor supplies the private generation channel; the
application supplies its already-connected role stream. Adopt those descriptors
in the platform bootstrap and transfer exclusive ownership of both Unix streams
to `sophia_launch_client::Client::over`. Descriptor adoption from an environment
variable is outside this API; a number is not an authority claim.

```text
role stream                     private generation channel
    |                                      |
    +-- Tauth -> afid                       +-- attach/open rpc
    +-- read own challenge ----------------+-- relay exact challenge
    |                                      +-- read relay result
    +-- read afid status
    +-- attach WITH afid -> role root
    +-- clunk afid                          +-- clunk rpc/root
    |
    +-- return role pipeline + root, retain generation pipeline
```

`over` negotiates both 9P versions with bounded blocking I/O. That time and the
subsequent nonblocking exchange share one five-second deadline; `with_timeout`
may shorten it. Drive `poll`, then `wait_for_io` when idle, until `ready`.
`into_authenticated` returns both pipelines and the admitted root. Use that
root directly; do not issue a plain attach. Claims are bounded to 255 bytes and
must match Session policy; they do not choose a principal or namespace.

Shell clients pass that role pipeline and root to
`ShellConnection::from_attached`. It takes exclusive ownership and starts with
the shell `api` read, without another version or attach. The pipeline must be
healthy and quiescent, with the root as its only live fid. Keep the separate
generation-channel pipeline alive for the launch. Failure closes the role
connection; it does not trigger an anonymous fallback. The shell negotiation
has its own configured timeout after authentication. Scripted tests cover bar,
launcher, dock and descriptor handoffs; a production authentication join and
application migration remain separate qualifications.

Only `Tauth`'s `EAGAIN` is retried, without renewing the deadline. Malformed
records, refusals and cancellation never fall back to anonymous admission.
Dropping an unfinished conversation shuts both streams. Keep the returned
generation pipeline for the launch lifetime. `Authenticated::reauthenticate`
requires no outstanding requests, resets both wire conversations and verifies
the same claims again. Old role fids are invalidated; Session retains its
immutable transport identity. The generic pipeline does not interpret any of
this policy.

The crate's tests use independently scripted peers. Real protected-process
interoperability belongs in Sophia and must be recorded separately; scripted
success is not a Factotum verification claim. Linux x86_64 is the qualified
platform; no remote authentication or prevention of intentional delegation is
claimed.

Adds `sophia-lock-protocol`, the lock provider's experimental file contract
codec (no client), and `Pipeline::write_secret` in `sophia-9p-client`, which zeroes
every copy of a secret request it leaves behind. Both are additive.

## Release 0.2.0

The SDK uses the file wire exclusively. The socket compatibility crate,
feature, connection API and frame corpora are removed. `connect_from_env`
requires `SOPHIA_SHELL_9P_SOCKET` and refuses any `SOPHIA_SHELL_SOCKET` value.
Recovery uses a complete compatible older desktop release.

Shell event loops should drain typed observations and call `wait_for_io` with
their next application deadline when idle. It wakes on socket readiness or an
SDK retry deadline. A fixed sleep between `poll_io` calls adds that sleep to
each dependent file operation; an update can require many such operations.
Both I/O servicing and the wait are bounded. Do not wait while application
work is ready to run.

## Provenance

`PROVENANCE.md` records the Sophia commit each crate was extracted from.

# Sophia desktop SDK for C

CPU images use **Immutable Shared Buffers (ISB)**, currently backed by sealed
memfds. The buffer builders are `create_isb` / `finish_isb` in Rust and
`sophia_ip_isb_create` / `sophia_ip_isb_finish` in C. Names changed together;
source-kind numbers, discovery strings and seal enforcement are unchanged.
Image adapters require ISB and never select byte fallback. Shell clients may
enable the adapter after negotiated Limits, including dock and launcher roles.
Legacy byte APIs remain temporarily while production callers migrate.

Native launchers can adopt a settled ISB into their resource lifecycle with
Rust's `adopt_native_image` or C's `sophia_ns_adopt_image`; see
[the adoption contract and coverage](API-PARITY.md#native-isb-resource-adoption-2026-10-18).

The [2026-10-17 release audit](RELEASE-QUALIFICATION.md) records verified
C/Rust capability parity, exact tested revisions and remaining limits.

Allocation-free [shell candidate sizing](SHELL-SIZING.md) is available in both SDKs.

Shell startup supports caller-driven progress from a connected socket or an
admitted root. See [startup controls and ownership](SHELL-STARTUP.md).

Native C99 libraries for Sophia desktop components. This repository builds
without a Sophia checkout and uses no Rust code. Nim clients can use its C API.

## SDK scope

Sensitive generic writes can use `sophia_9p_write_secret`. The SDK wipes its
copies after sending and on failure or `sophia_9p_close`; callers still clear
their input and close the borrowed descriptor. Call close before abandoning
secret-bearing storage. Ordinary writes keep their existing behavior.

Generic 9P now supports attributes and directory pages, matching Rust's
read-only client. Request-slot storage has grown to retain directory cookies;
rebuild C consumers and foreign-language bindings with matching headers and
libraries. See [transport parity](TRANSPORT-PARITY.md). Broker dependency files
are now included in incremental builds so header changes rebuild those objects.

The current development candidate is **0.10.0-dev, unreleased**. Reservation
tokens now identify the issuing session address and epoch as well as their
serial. Previously two live sessions with matching serials could accept each
other's tokens. The public `sophia_ss_reservation` layout has grown: recompile
C consumers and foreign-language bindings before adoption. Do not mix old
headers with this library or move/reinitialize a session with a live reservation.
The wire contract is unchanged. [API parity](API-PARITY.md) maps the Rust controls.

One desktop SDK per language covers Sophia's WM, shell, output and admin/control
roles. Role modules share standard 9P2000.L transport, bounded I/O and custody
rules; they are not separate SDK repositories. Nim uses this SDK through thin C
bindings. The target is the complete functionality of the former IPC APIs over
Sophia's admitted file contracts, without IPC fallback or private 9P opcodes.
WM and shell clients use file contracts exclusively.

[COVERAGE.md](COVERAGE.md) distinguishes that requirement from implemented and
tested support. A missing server file contract is a migration gap, not permission
to tunnel the old socket protocol through a file.

## Current coverage

Broker sessions now retain their submit handle across responses. The private
fields in public transaction storage have changed; rebuild consumers and
foreign-language bindings with matching headers. Receipt acknowledgement,
assembly cleanup and retry semantics are unchanged; see [API parity](API-PARITY.md).

The developing broker API (`sophia_broker_session.h`) retains one request and
submits its one- or two-row response atomically. Startup borrows a quiescent 9P
client and an admitted root; it does not authenticate the caller. The caller
drives progress and acknowledgements, chooses explicit retries and closes the
borrowed descriptor on failure. The session never reconnects or replays work.
Independent C/Rust peers pass Sophia's generic export and protected-launch gates.
Sophia's live broker now uses the Rust counterpart. Performance acceptance and
the full desktop SDK release audit remain open. See [broker parity](BROKER-PARITY.md).

Portal recipient sessions cover capture, frames and transfer reads. The requester
session (`sophia_portal_requester_session.h`) adds bounded uploads, cancellation
and terminal outcomes. [Portal parity](PORTAL-PARITY.md) records the paired SDK
and export checks and distinguishes them from physical desktop acceptance.

`sophia_namespace.h` implements the pinned
[`namespace-discovery` contract](spec/namespace-discovery.md). Parsing uses
caller-owned bounded storage. On Linux, `sophia_ns_open()` pins an absolute
generation directory and reads its discovery file; the caller closes the
returned directory descriptor. `sophia_ns_connect()` returns an owned,
nonblocking, close-on-exec socket for a declared endpoint. Check the endpoint's
protocol, interface and version, then perform its normal authentication.
`SOPHIA_NAMESPACE_DIR` is a location, never a credential. Lookup rejects
symlinks, supports long logical names and never changes working directory.
Local socket tests cover these properties; Session launch integration remains
a separate qualification.

To authenticate another endpoint from the same approved recipe, use
`sophia_la_next_endpoint()` with a completed conversation and a fresh role
client. It transfers exclusive use of the quiescent launch relay, refusing
pending requests, replies or fids. The earlier role client and root remain
unchanged. Never drive the previous authentication conversation again or share
the relay between concurrent drivers. Failure handling retains the existing
launch-client contract; no endpoint falls back to unauthenticated attach.

Release 0.9.0 provides a generic nonblocking 9P2000.L client, shell file
records and sessions for bar (r6), native launcher (r7) and persistent
catalog/dock (r8), WM and output file codecs and sessions, and an experimental
lock provider codec and client. WM and shell socket
compatibility is removed; recovery uses a complete compatible older desktop release.
WM file record codecs and a bounded WM session pass scripted-peer tests and
Sophia's production WM export gate, and Hagia uses them through thin Nim
bindings. The WM session requires the exact `api` file naming the output role's
9P2000.L transport and refuses the retired `current_ipc` value. The WM codec
also carries the additive chord lifecycle (`action_lifecycle`): Configuration
rows declaring lifecycle actions and the ActionLifecycle cause, plus, with
`chord_actions`, the ChordAction cause that marks a chord's own activations, and
`held_capture`, which lets an Overlay presentation carry a keyboard output and
bindings while a chord is held. The codec carries presentations unchanged; the
session negotiates `held_capture` only with `surface_instances` and
`presentation_actions`, and it is tested against scripted peers only.
The output file
codec and session pass scripted-peer tests, Sophia's production output export
exchange and Sophia's attended native output acceptance on one card with two
heads (see provenance for its scope). The output default rollout and other
physical configurations are not qualified here. The developing admin file client
now provides startup, retained catalogs, command custody and status snapshots;
its current qualification is recorded in [API parity](API-PARITY.md).

Descriptor codecs and profile selection use the native layouts in
`spec/sophia-shell-files-v1.kdl` and the rules in `spec/sophia-shell-descriptors.md`.
Scripted tests cover readiness and snapshot custody. Sophia's independent C
production-export, descriptor host and protected presentation gates cover this
SDK's descriptor role; see provenance for the candidates and limits.
See [coverage](COVERAGE.md#parity-gate) for the remaining migration work.

The WM file codec (`sophia_wm_files.h`) covers API-1 envelopes, typed scalar
bodies, cycle causes, complete section bounds and negotiated section disclosure.
`sophia_wm_records.h` supplies all 23 neutral fixed row codecs without socket
framing. These enforce structural wire rules; Session still validates scene,
geometry, policy phase and authority. The focused codec test uses literal
bodies and the pinned golden row corpus, not a live export. The fixed rows are
generated from the `row-layouts` block of the WM file KDL with
`python3 tools/generate_wm_rows.py`. `make check-generator` checks the
generated files and the generator's parser controls; Python is not a library
build dependency. `compatibility.json` sets `wm_files=true` because the
production WM export gate has passed (Sophia `c4e17899e`).

The WM session (`sophia_wm_session.h`) owns file bootstrap, immutable candidate
submission, custody tickets, cumulative acknowledgements and snapshot pins.
For authenticated endpoints, `sophia_ws_open_attached` continues on the exact
quiescent wire and root returned by `sophia_launch_auth`. It borrows both
without Tversion or another attach; callers retain their storage until close.
The constructor requires eight request slots, sixteen fids and no other live
fid or pending request. It schedules bootstrap immediately for poll-first
callers. See the header for storage and overlap requirements.
It accepts a borrowed fd and caller-owned storage; policy and profile decisions
remain with the caller and server. See [the WM API notes](src/README-wm.md) for
lifetimes, deadlines and evidence limits.

The bounded shell session (`sophia_shell_session.h`) provides atomic local queue
admission, per-record custody tickets, paced retries, object acknowledgement
barriers, uploads, and poll integration. Applications still consume events,
fetch announced objects, acknowledge progress, and enforce role deadlines.
The native launcher layer (`sophia_shell_native_session.h`) adds opening,
allocation, candidate, focus and input acknowledgement state. Its unit tests
use a scripted session; Sophia maintains separate production-export harnesses.
Permit deadlines are advisory and cannot guarantee server validity at ingest.

The record-level API and explicit nonblocking connection helper
(`sophia_desktop_connection.h`) are also available. The helper
requires `SOPHIA_SHELL_9P_SOCKET`, refuses the retired `SOPHIA_SHELL_SOCKET`,
authenticates the same-user peer and transfers its fd to the file client.
The caller polls and enforces a connection deadline. See
[the shell API notes](src/README-shell.md) for buffer lifetimes and wire behavior.

## Build and test

The development authentication API, `sophia_launch_auth.h`, borrows fresh role
and launch-channel 9P clients and caller-supplied monotonic time. Initialize it
with `sophia_la_init`, advance it with `sophia_la_service`, and poll both fds plus
`sophia_la_next_wake` until ready or terminal. It relays only the challenge from
its own afid and never falls back to ordinary attach. The total deadline is
five seconds, including cleanup; the caller owns descriptor closure on failure.
The helper performs no environment lookup or principal selection.

On success, `sophia_la_root` names the admitted role root. For lock providers,
pass it to `sophia_lc_init_attached`; that constructor starts at `api`, preserving
the authentication and transport identity. It refuses a busy wire or unknown
root. Output clients pass that same wire and root to `sophia_os_open_attached`,
which starts at `api` without resetting or reattaching the transport. It borrows
the quiescent wire with its root as the only fid; output state and storage must
be disjoint from that wire and its storage. Its lifecycle tests run through both
the fresh-wire and admitted-root constructors. WM and shell clients likewise use `sophia_ws_open_attached` and
`sophia_ss_open_attached`, respectively.
This is a candidate API, not an automatic migration of existing applications.

Requires a POSIX system, a C99 compiler, GNU make, ar, and sha256sum. Tests use
Unix socket pairs; they do not discover or connect to a desktop.

```sh
make -j"$(nproc)"
make -j"$(nproc)" check
make install PREFIX=/usr/local DESTDIR=/path/to/staging
```

The static libraries are:

- `libsophia-9p.a`: generic transport, link with `-lsophia-9p`.
- `libsophia-desktop.a`: WM, shell and output file codecs/sessions; link with
  `-lsophia-desktop -lsophia-9p`.

Headers install under `include/sophia-desktop`; pkg-config packages are
`sophia-9p` and `sophia-desktop`.
The package name uses SDK terminology; `-dev` is reserved for a distribution's
development package. Releases are consumed as pinned source revisions; no
stable ABI is promised across 0.x releases.
The machine-readable coverage declaration is [compatibility.json](compatibility.json).

## Contract and integration

Sophia owns the normative contracts. `spec/` holds immutable copies pinned in
[PROVENANCE.md](PROVENANCE.md) and checked by `make check-spec`. Changes require
an explicit source revision and digest update. Local tests cover literal file
vectors, pipeline failure sequences, and neutral WM row corpora. Sophia owns
the real-export integration tests and the independent Go oracle.

Local queue admission, server Submitted custody, and semantic outcomes are
separate stages. A sent record without observed Submitted has unknown custody
after disconnect. Clients must not replay it into a new attach epoch.

## Platforms

Linux is the initial tested platform. FreeBSD is the next qualification target;
OpenBSD and NetBSD require separate native test results. Keep wire codecs and
lifecycle logic portable, with OS-specific socket and credential code in a small
adapter. 9P2000.L error numbers are wire values, independent of host errno.
The connection helper includes a FreeBSD credential adapter, still untested on
a native FreeBSD runner. This is preparation for qualification, not a support claim.
BSD support requires native CI for socket behavior, peer authentication, retry,
revocation and protocol tests. Cross-compilation alone is insufficient. The
first Linux release does not wait for BSD qualification.

License: BSD-3-Clause; see [LICENSE](LICENSE).

## Output role

`sophia_output_files.h` encodes and decodes the output file records in
`spec/sophia-output-files-v1.kdl`. A published Topology is authoritative, so
decoding also applies the snapshot invariants in `spec/sophia-output-files.md`;
proposals decode structurally and reach the server's topology owner for
semantic validation.

`sophia_output_session.h` owns one output attach: bootstrap, negotiation,
submission custody, cumulative acknowledgements and topology fetches. Each
ObjectPublished event is presented only after the SDK has read that exact
object (matching Qid path and epochs) and released its handle, so the ack
that follows consumption never precedes the full read. The next proposal opens
its transaction only after the previous Submitted is acknowledged. EAGAIN
retries identical bytes; other refusals settle the ticket. A negotiation
refusal ends the session after its acknowledgement.
`sophia_desktop_output_environment` requires `SOPHIA_OUTPUT_9P_SOCKET` and
refuses the retired `SOPHIA_OUTPUT_SOCKET`.

Revision 1 does not publish a head's current transform or VRR policy, and a
proposal restates them for every enabled head. Callers must obtain explicit
values rather than defaulting them.

## Lock provider role (experimental)

The lock API is experimental. The pinned contract (Sophia's signed master
merge `61d545c90`, see PROVENANCE.md) still marks itself as revision 1
(draft), and Sophia has not yet accepted it explicitly. It can change
incompatibly before that acceptance. `compatibility.json` sets
`lock_files=false` until the contract is accepted. The permanent production
export and protected revocation gates have passed; see LOCK-PARITY.md for their
scope. This does not accept the draft contract or qualify physical locking.
`sophia_lock_files.h` encodes and decodes the records in
`spec/sophia-lock-files-v1.kdl`, checked against Sophia's golden records.

`sophia_lock_client.h` owns one lock provider attach: it reads `api` and
`limits`, negotiates (with any UI chords), and handles submission custody,
cumulative acknowledgements, lock object fetches and uploads to
`upload/<slot>`. Negotiated, Refused and Submitted are consumed internally;
every other event is acknowledged only after the caller consumes it. An
ObjectPublished is presented only after the SDK has read that exact lock
object. One whose object was already replaced is consumed unread, because
the newer object's announcement follows it. A provider renders only: it never
sees the secret's characters and cannot enter or leave the locked state. The
client does not discover the socket (`SOPHIA_LOCK_9P_SOCKET`) or reconnect;
a replacement process gets a new connection and a new client.

### Bounded lock uploads

`sophia_lc_upload_window(client, writes)` opts into up to eight ordered Twrites
on the same upload fid. The default remains one. Reserve at least five wire
request slots beyond the upload window for event reads, acknowledgements,
submission custody and object reads. The caller still supplies one borrowed
chunk; ready means every byte was acknowledged, never merely queued.

Cancellation can interrupt a chunk: no further writes are issued, outstanding
replies drain, and Cancel follows on the same transaction. Keep the chunk alive
until pending ends (or dispose a terminal connection). A positive short write
is retried in single-write mode. In pipelined mode it cancels that resource,
because later requests may already name offsets beyond the peer's cursor. It
never replays an ambiguous write or publishes incomplete pixels. No wire
contract changed. Callers must recompile for the enlarged 0.x client struct.

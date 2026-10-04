# Sophia desktop SDK for C

Native C99 libraries for Sophia desktop components. This repository builds
without a Sophia checkout and uses no Rust code. Nim clients can use its C API.

## SDK scope

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
physical configurations are not qualified here. The admin file client is not
implemented yet.

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
`lock_files=false` until the contract is accepted and Sophia's production
lock export gate passes with this SDK vendored.
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

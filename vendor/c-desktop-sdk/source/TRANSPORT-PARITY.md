# Transport and namespace parity

Operation map revision 1, 2026-10-17. The shared pins are
`spec/sophia-9p-profile.md` (SHA-256
`0f30cb51d6b9e3327948997b6c4cf046277d874324d454047623a0d4dafcefc1`)
and `spec/namespace-discovery.md` (SHA-256
`e6ef96ddd4a1dea3bf271ebcfb3cf359d18ecc8c5887f30bd7ae519db1f8e82e`).
The profile identifies the pinned diod protocol source. This map covers generic
access, not a replacement for typed role sessions or launch authentication.

## Generic 9P

C names below have the `sophia_9p_` prefix. Rust `Pipeline` is the
nonblocking client; `Client` is its separate read-only convenience client.

| Capability | C | Rust | Contract and ownership |
| --- | --- | --- | --- |
| Adopt transport and choose bounds | `storage_bytes`, `init` | `Pipeline::over`, `VersionHandshake::new`, `PipelineLimits` | C borrows fd and caller storage; Rust owns stream and bounded buffers. |
| Negotiate 9P2000.L | `version`, then service | `VersionHandshake::dispatch`, `take_pipeline`; blocking constructors | Negotiated msize bounds frames. Only a completed version admits ordinary requests. |
| Authentication fid | `auth` | `Pipeline::auth` | QTAUTH only; no authority until the export verifies it. |
| Attach with or without afid | `attach_with_afid`, `attach` | Same Pipeline method names | Claims never confer permission. New fid reservations settle on reply or flush. |
| Walk, including clone | `walk` | `Pipeline::walk`, `Client::walk` | At most 16 names; partial walk cannot leak a new fid. |
| Open/read/write | `lopen`, `read`, `write` | Pipeline methods; read-only Client helpers | Offset/count and negotiated framing limits apply. The export controls rights and write meaning. |
| Attributes | `getattr`, `attributes` | `Client::getattr` | Request mask 0x3fff; expose valid, qid, mode, nlink, size. Copy passive fields before C reply consumption. |
| Directory pages | `readdir`, `dirent_next` | `Client::readdir`, `list_all` | Strictly increasing cookies, matched qid/type, bounded names without slash/NUL/dot entries; empty page ends listing. |
| File read to completion | Repeated `read` with caller byte bound | `Client::read_to_end` | Reject excess rather than silently truncate. C chooses storage and loop policy. |
| Clunk | `clunk` | Pipeline/Client `clunk` | Releases local fid custody; no implicit retry after uncertain transport loss. |
| Flush | `flush` | `Pipeline::flush`; `Client::read_until` | Reserved flush admission, original-tag correlation and reply/flush races. Pipeline callers retain raced replies; the read-only timed helper intentionally discards them. |
| Bounded progress | `service`, `wants_write`, borrowed fd | `poll_with_budget`, `io_interest`, `AsFd` | Partial I/O stays owned. Each budget limits bytes and syscall attempts per direction. |
| Completion | `peek`, `consume` | `Pipeline::take_reply`, `wait` | C replies/names borrow a stable slot until explicit consume; Rust transfers owned replies. Tags remain reserved until settlement. |
| Error and shutdown | Result code, reply errno, caller fd close | Typed wire/remote errors, owned stream close/drop | Protocol corruption is terminal; ordinary server errno is a request outcome. No reconnect/replay. |

The read-only Rust Client's directory and attribute operations are not Pipeline
methods. C provides equivalent capability through its asynchronous client; it
does not require a blocking convenience layer. Creating, renaming and removing
export nodes is outside the accepted profile. Neither SDK adds such authority.

C exposes raw numeric fids and expects its caller to retain their connection
ownership. Rust's read-only File handles carry client identity; its Pipeline
also uses numeric fid values. C can hold up to 32 ordinary requests and 128 fids;
Rust bounds are configurable. Memory layouts, capacity ceilings and allocation
strategies are not promised identical.

The C metadata addition grows private request-slot storage to retain directory
cookies. Recompile all C consumers and foreign-language bindings against the
matching unreleased 0.10.0-dev headers/library. The reply structure does not
grow: attribute and directory bytes use its existing borrowed payload, avoiding
extra per-reply clearing in unrelated role traffic. No metadata request allocates.

## Namespace discovery

| Capability | C | Rust | Ownership and limits |
| --- | --- | --- | --- |
| Parse complete discovery v1 | `sophia_ns_parse` | `Discovery::parse` | At most 131072 bytes and 64 endpoints; canonical numbers, bounded UTF-8 names, exact record count. |
| Find visible endpoint | `sophia_ns_find` | `Discovery::endpoint` | Missing entries remain absent; availability is descriptive, never permission. |
| Open immutable directory view | `sophia_ns_open` | `Namespace::open` | Pin directory, refuse symlinks and nonregular/oversize discovery, leave cwd unchanged. |
| Session location convenience | Caller reads `SOPHIA_NAMESPACE_DIR` | `Namespace::from_env` | Environment identifies location only; no credential or fallback. |
| Connect declared endpoint | `sophia_ns_connect` | `Namespace::connect` | Pin socket inode; return owned CLOEXEC/nonblocking stream. Authenticate separately. |
| Lifetime | Caller closes returned directory/socket fds | Namespace/File/UnixStream ownership | Replacing a directory name cannot retarget an existing view. |
| Platform | ENOTSUP outside Linux for lookup/connect | Namespace type available on Linux | Passive discovery parsing is portable in both. |

C parsing leaves output unchanged on failure and allocates no heap storage.
C open uses a bounded temporary read buffer; Rust owns parsed strings/vectors.
Neither implementation implicitly changes working directory or follows a
replacement generation name. Their paired directory fixtures check long logical
paths, symlink refusal, descriptor flags and old-view identity after replacement.

## Evidence

One transport capability is still missing in C: Rust `Pipeline::write_secret`
scrubs SDK-owned outgoing secret bytes after sending and on drop. C's generic
`write` has no equivalent scrubbing contract. Audit the C lock caller and add
explicit secret-write/cleanup support with independent tests before closing
t333's transport parity gate. Caller cleanup alone does not establish matching
SDK handling of its own copies. The metadata addition does not close this gap.

C `src/tests/sophia_9p_client_test.c` and Rust
`crates/sophia-9p-client/tests/client.rs` now independently cover literal
attributes, fragmented directory pages, empty-page termination and ten malformed
metadata/directory cases. Both reject bad lengths, QTAUTH metadata, nonadvancing
cookies, mismatched directory types, unsafe names, truncated entries and replies
larger than the requested count. Existing generic suites cover transport
backpressure, retained replies, tag wrap, flush races and terminal failures.

C `src/tests/namespace_test.c` and Rust namespace `discovery.rs`/`directory.rs`
cover their parser and Linux lookup boundaries. These tests do not grant role
authority or claim physical acceptance.

Sophia's generic real-core test `reads_stats_and_lists_through_the_real_driver`
covers Rust. Its opt-in `external_c_metadata_matches_the_real_driver` invokes
the independently built C `metadata_peer` over a supplied stream and checks
the same attributes and directory listing. Exact revisions, executable hashes
and run results belong in Sophia's t333 note.


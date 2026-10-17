# Shell record sizing

Both SDKs can size complete client candidates without allocating a scratch
buffer or changing session state. A size is not admission, custody, queue space
or permission to act. Values and their referenced rows must remain unchanged
between sizing and encoding.

C exposes `sophia_ss_record_bytes` for application candidates. It ignores the
header and assigns a provisional candidate identity, retaining descriptor body
epochs for validation. Zero means invalid or not an application candidate.
`sophia_sf_record_bytes` instead checks the supplied complete file record,
including its header. Both borrow the record and its rows.

Rust exposes the following functions in `sophia_shell_protocol::shell_files`.
Each borrows its value, validates the supplied `ShellFileHeader`, and returns
`Result<usize, ShellFilePayloadError>`. The size includes the 32-byte envelope
and transaction prefix. Use a valid nonzero provisional submission ID when
sizing a candidate before session admission; descriptor header and body epochs
must agree. The session assigns its own submission identity at admission.

| Candidate family | Rust query |
| --- | --- |
| Allocation request, resource end/cancel/retire | `shell_file_content_bytes` |
| Resource begin, including upload slot | `shell_file_resource_begin_bytes` |
| Frame demand/cancel, action ACK | `shell_file_transaction_bytes` |
| Whole base content candidate | `shell_file_candidate_bytes` |
| Native allocation request, input ACK, activation | `shell_file_native_launcher_transaction_bytes` |
| Whole native-launcher candidate | `shell_file_native_candidate_bytes` |
| Catalog activation | `shell_file_catalog_action_bytes` |
| Whole catalog candidate | `shell_file_catalog_candidate_bytes` |
| Indicator activation | `shell_file_indicator_activate_bytes` |
| Descriptor, Tabs, reference and launcher candidates and activation ACKs | `shell_file_descriptor_bytes` |

Whole candidates borrow their vectors; callers need not clone or split them
into Begin/Chunk/End records. Resource chunks travel as upload writes, not file
candidates. Negotiation belongs to startup. Some family queries also support
server records; this table maps application candidates, not every server codec.
Indicator activation retains the encoder's structural validation; it does not
invent semantic admission rules.

Rust counting and encoding share field writers. Descriptor uniqueness checks
use bounded stack storage, so valid rows do not allocate during validation.
The independent C literal fixtures check size against actual encoded lengths;
Rust checks the corresponding families, limits and malformed identities. A
standalone Rust allocation probe covers a transaction, descriptor, maximum Tabs,
nonempty base/native/catalog candidates and indicator activation. Encoding is
its positive allocation control. This proves those query allocations, not a
throughput improvement or an allocation-free session.

Run the probe from the Rust SDK checkout after building the current library:

```sh
cargo-slot sdk-rs-t331-reply-storage cargo build --offline -p sophia-shell-protocol
cargo-slot sdk-rs-t331-reply-storage bash -c 'rustc --edition=2024 --test tests/encoded_size_allocations.rs -L dependency="$CARGO_TARGET_DIR/debug/deps" --extern sophia_shell_protocol="$CARGO_TARGET_DIR/debug/libsophia_shell_protocol.rlib" -o "$CARGO_TARGET_DIR/debug/encoded-size-allocations" && "$CARGO_TARGET_DIR/debug/encoded-size-allocations"'
```

Use the local checkout's cargo-slot pool name if different. The probe's unsafe
allocator delegates to `System`; it lives outside production crates, whose
unsafe-code prohibition remains unchanged. Ordinary Cargo tests do not run it.

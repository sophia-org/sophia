# D4 owned-resource mechanism probe

This standalone Linux probe compares fresh sealed ISBs with ring copy-out by
mapped raw copy, sealed `pread`, and unsealed `pread`. It is not a production
protocol, Session integration, or native acceptance test. It extends the lifetime
model missing from Claude's original single-destination probe without altering
that retained evidence.

Run from Sophia, through its fixed build pool:

```sh
cargo-slot sophia cargo test --offline --manifest-path tools/probes/d4_owned_ring/Cargo.toml
cargo-slot sophia cargo run --offline --release --manifest-path tools/probes/d4_owned_ring/Cargo.toml
cargo-slot sophia cargo run --offline --release --manifest-path tools/probes/d4_owned_ring/Cargo.toml -- --reverse
```

It reports JSONL setup, raw sample and final-drain records. Each arm has 32 warmups
and 128 measured iterations for 1920x24 and 1024x1024 BGRA pixels, with one or four
accepted resources retained. A resource is created once per acceptance and stays
readable until release. Three source slots rotate independently; four retained
resources therefore outlive source-slot reuse. No destination pool or permanently
reused output vector hides allocation costs. Resource samples are checked at
release; tests compare all retained bytes after repeated source reuse.

At the selected retention limit, the oldest resource is released before the next
is allocated. This models finite admission capacity, not arbitrary consumer
release delays or journal backpressure. Peak accepted payload is `retain * bytes`;
ring extent (three fixed, page-aligned slots with generation headers) is reported
separately. Allocator overhead, temporary bookkeeping and kernel accounting are
not included in that payload count. Last-close cost for fresh files is assigned
to Session in this single-process model; native ownership may place it elsewhere.

`session_ns` includes acceptance (geometry/seal checks and mapping for fresh;
generation checks, a new destination allocation, copy and owned-pixel validation
for rings). `release_ns` includes checking sample pixels and dropping one old
resource. Sum those two fields for the consumer stages. `producer_ns` includes
fresh creation/fallocate/map/fill/unmap/seal or ring fill/publication. Ring setup
and final drains are separate records and must not be represented as free.
Per-iteration minor/major fault deltas cover both roles. All times are wall time,
not CPU time or upper bounds. This is one process, sequential producer/consumer,
no fd passing, 9P, renderer, upload, display cadence, scheduling or native gates.
The generated fill loop and retained lifetimes differ from the old probe, so
compare arms within these captures rather than subtracting historical medians.

Every adapter checks immutable geometry. Mapped access requires GROW/SHRINK seals
before mapping and only copies through raw pointers; it lends no immutable Rust
slice over mutable source pixels. This probe serializes producer turns; it does
not qualify arbitrary adversarial concurrent raw-memory access for production.
The unsealed `pread` consumer maps neither headers nor pixels and rejects short
reads and errors instead of stitching partial reads. Linux memfd is the producer
backing in all runs; this does not establish `shm_open` positional-read portability.

Generation words use page-aligned native-endian atomics in this local Linux model.
The `pread` arm uses two additional positional reads for generation observations.
A changed/odd/unexpected generation refuses the capture, without retry. Generation
checks do not authenticate a hostile writer which changes pixels without updating
metadata. Validation and all future reads use the owned destination. Tests inject
an intervening publication, invalid pixels, truncation, missing seals and invalid
geometry. No production wire encoding, publication-memory-model claim, accounting
ledger, cancellation owner or release journal is supplied by this probe.

The first three retained runs are in
`~/.local/state/sophia/development-evidence/t331-d4-owned-copy-20261021/probe-01`.
They alternate forward/reverse/forward arm order. Native fresh-ISB versus ring
measurement, using the t331 gates, is still required before any production path.

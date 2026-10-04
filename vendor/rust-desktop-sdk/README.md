# Pinned Rust desktop SDK

`source/` is an immutable archive of the signed revision of
sophia-org/sophia-desktop-sdk-rs named in `manifest.json`; `upstream.commit` is
that revision's raw commit object. Sophia builds the SDK's crates from
`source/` through path dependencies, offline. `cargo xtask check
rust-desktop-sdk` rejects modified, missing, extra or symlinked files, checks
the SDK's contract copies against Sophia's authoritative files, and runs
the SDK's own tests (including the contract's conformance tests). The 0.3.0
release contains only file clients and codecs; its socket crate and feature
have retired. It adds the experimental lock provider codec
(`sophia-lock-protocol`, no client), whose contract copies and golden records
the check also compares; the lock contract still marks itself revision 1
(draft).

Change code in the SDK repository, test it, and sign the commit. Then run
`cargo xtask vendor-rust-desktop-sdk <sdk checkout> <revision>`, which stages
and verifies the new snapshot before replacing this one, and the check.
Never edit `source/` or pin a moving branch.

The SDK is published at sophia-org/sophia-desktop-sdk-rs; pin only revisions
reachable there, so consumers and this archive name fetchable commits.

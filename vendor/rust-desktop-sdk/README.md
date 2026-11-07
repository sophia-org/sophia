# Pinned Rust desktop SDK

`source/` is an immutable archive of the signed revision of
sophia-org/sophia-desktop-sdk-rs named in `manifest.json`; `upstream.commit` is
that revision's raw commit object. Sophia builds the SDK's crates from
`source/` through path dependencies, offline. `cargo xtask check
rust-desktop-sdk` rejects modified, missing, extra or symlinked files, checks
the SDK's contract copies against Sophia's authoritative files, and runs
the SDK's own tests (including the contract's conformance tests). The 0.3.0
release contains only file clients and codecs; its socket crate and feature
have retired. The current pin also includes an experimental lock provider
client and codec, whose contract copies and golden records the check compares;
the lock contract still marks itself revision 1 (draft). Role operation maps
in the archive distinguish implemented capabilities from release acceptance.

Change code in the SDK repository, test it, and sign the commit. Then run
`cargo xtask vendor-rust-desktop-sdk <sdk checkout> <revision>`, which stages
and verifies the new snapshot before replacing this one, and the check.
Never edit `source/` or pin a moving branch.

The SDK is published at sophia-org/sophia-desktop-sdk-rs; pin only revisions
reachable there, so consumers and this archive name fetchable commits.

The experimental SFT client includes explicit r13 transport, `FrameStream`
ownership and joined owned-ring submission. This import does not select r13 in
Sophia or change the default client revision; production integration remains a
separate gate in `docs/native-frame-transactions.md`.

This pin also includes worker-fill ownership and explicit same-binding legacy
image fallback after independent frame/source drain. The host's real copied
Rust SDK test supplies authentication verdicts and renderer completion; it is
not a native performance result. The host test now uses real explicit owner
negotiation; Session/CLI enablement remains separate.

The initial-image API additionally permits ordinary uploads before the first
Open. The host test establishes retained images through it before copied-frame
admission and same-connection fallback; production selection remains disabled.

Revision `564be8c7a6c20b3100fd6345b2df7aa0b542737a` discovers the optional
`frame-submit` file after opening the ordinary reusable writer. Eligible complete
FrameCommits use one write; absent nodes retain the paired path. Retry, ACK and
custody rules are unchanged. This import also includes the journal-progress
wait fix from that SDK's ancestry. CPU benefit is not yet measured.

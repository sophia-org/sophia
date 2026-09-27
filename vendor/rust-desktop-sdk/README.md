# Pinned Rust desktop SDK

`source/` is an immutable archive of the signed revision of
sophia-org/sophia-desktop-sdk-rs named in `manifest.json`; `upstream.commit` is
that revision's raw commit object. Sophia builds the SDK's crates from
`source/` through path dependencies, offline. `cargo xtask check
rust-desktop-sdk` rejects modified, missing, extra or symlinked files, checks
the SDK's contract copy against `protocol/sophia-shell-files-v1.kdl`, and runs
the SDK's own tests (including the contract's conformance tests) with and
without `ipc-compat`.

Change code in the SDK repository, test it, and sign the commit. Then run
`tools/vendor_rust_desktop_sdk.sh <sdk checkout> <revision>` and the check.
Never edit `source/` or pin a moving branch.

Publication of the SDK repository awaits GitHub authentication; the pinned
revision is a local signed commit until then.

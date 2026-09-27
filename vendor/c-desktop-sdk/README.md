# Pinned C desktop SDK

`source/` is an immutable archive of the signed SDK revision in `manifest.json`.
It builds offline and contains its own contract and import provenance. The
manifest covers every source file; `cargo xtask check c-desktop-sdk` rejects
modified, missing, extra, or symlinked files and verifies the copied contracts
against Sophia's authoritative versions.

Change code in sophia-org/sophia-desktop-sdk-c, test it, and sign the commit.
Then replace `source/` from `git archive <exact revision>`, regenerate the sorted
SHA-256 file manifest with that revision, and run the snapshot check and C gates.
Do not edit the snapshot or use a moving branch as its identity.

The initial pin is a local signed extraction commit; publication awaits GitHub
authentication. This snapshot does not require network access. Generated WM
socket binding checks remain under bindings/c until WM SDK integration.

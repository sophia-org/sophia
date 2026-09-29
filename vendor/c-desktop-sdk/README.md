# Pinned C desktop SDK

`source/` is an immutable archive of the signed SDK revision in `manifest.json`.
It builds offline and contains its own contract and import provenance. The
manifest covers every source file; `cargo xtask check c-desktop-sdk` rejects
modified, missing, extra, or symlinked files and verifies the copied contracts,
golden vectors and generated WM codec against Sophia's authoritative versions.
It hashes Git blobs/trees offline (including executable modes), then checks that
the raw `upstream.commit` has both the declared revision and that source tree.
Signature authorization remains a release/review step; hashing is not signature
verification. No object database or network fetch is needed for this check.

Change code in sophia-org/sophia-desktop-sdk-c, test it, and sign the commit.
Then replace `source/` from `git archive <exact revision>`, save
`git cat-file commit <exact revision>` as `upstream.commit`, regenerate the sorted
SHA-256 file manifest with that revision, and run the snapshot check and C gates.
Do not edit the snapshot or use a moving branch as its identity.

The 0.2.0 pin contains only file clients. Its checks compare the retained file
contracts and neutral WM rows; socket schemas, bindings and corpora have retired.
The snapshot and its checks require no network access.

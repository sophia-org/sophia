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

The current pin keeps 0.4.0's strict WM API naming the independent 9P output
transport, refusing the retired `current_ipc` API, the chord lifecycle
(`action_lifecycle`), the ChordAction cause (`chord_actions`) and the held
capture (`held_capture`). It adds the experimental lock provider codec and
client (`lock_files=false`: the lock contract still marks itself revision 1
(draft)). Its checks compare the WM, shell, output and lock file contracts,
the lock golden records and neutral WM rows; socket schemas, bindings and
corpora have retired. `crates/sophia-runtime/tests/lock_client_c_sdk.rs` runs
the pinned lock client against the production lock export.
The snapshot and its checks require no network access.

The experimental SFT client includes explicit r13 transport, one-credit stream
ownership and joined owned-ring submission. Rebuild consumers against the pinned
headers: its passive record union and image-slot bookkeeping have grown. This
import does not select r13 in Sophia or change the default client revision.

This pin also includes worker-fill ownership and explicit same-binding legacy
image fallback after independent frame/source drain. The host's real copied
Rust SDK test supplies authentication verdicts and renderer completion; it is
not a production negotiation or native performance result.

The paired initial-image API permits ordinary uploads before the first Open.
Its independent C tests are included; copied-source C-to-host interoperability
remains a separate gate.

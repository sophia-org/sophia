# Extraction and contract provenance

Initial source: `https://github.com/sophia-org/sophia`, commit
`987cfd393a11edaa99e8b3c58be80d21167fb287` (signed t252 merge).
The source tree was copied from `bindings/c/` into `src/`, preserving paths
below that directory, license, copyright, and existing test vectors.
`provenance/import.sha256` records the unmodified imported bytes. Later SDK
changes are recorded by signed commits; that import manifest remains historical.

The authoritative shell file contract is API version 1. This snapshot includes
native record families for shell revisions 6, 7, and 8. A revision's presence
does not imply every capability is granted: negotiation and runtime owners
remain authoritative. The api file supplies the attach epoch.

`spec/` contains copies of the following files at the same source commit:

- `protocol/sophia-shell-files-v1.kdl`
- `protocol/sophia-shell-v1.kdl` (IPC compatibility)
- `protocol/golden/sophia-shell*.frames` (IPC test corpus)
- `docs/sophia-shell-files.md` (file lifecycle contract)
- `docs/sophia-9p-profile.md`
- `docs/references/diod-9p2000L-protocol.md`

`spec/SHA256SUMS` pins all copied reference bytes. The original B7 codec work
also recorded profile blob `de101e3d` and diod reference blob `48d63c80`, upstream
`de51d1ee1bd5`. The current complete copy digests are the SDK reference inputs.

The shell file KDL comments and lifecycle document were subsequently updated
from Sophia commit `436fb1ac`, with audit qualifications in `e9750572`, to
clarify native launcher clocks, generations, input acknowledgement and close
settlement. These updates change no field layout. `spec/sophia-wm-files.md`
is also copied from `e9750572`: the shell contract references its custody rules,
including paced EAGAIN retries and unknown custody across disconnect. The other
reference copies retain the initial source revision above. Reference documents
retain their own notices; they are not compiled into the libraries.

To update a contract, choose a reviewed Sophia commit, copy the changed
authoritative files, update the digest manifest and provenance revision,
implement and test compatibility, then sign the SDK commit. Sophia integration
must pin that exact SDK commit and verify the copied contract against its own
authoritative KDL before running offline gates. No build downloads contracts.

The copied generated `sophia_wm_v1.{c,h}` is legacy socket compatibility from
the same source commit. Its generator and authoritative schema remain in Sophia;
this initial extraction does not advertise a WM file client.

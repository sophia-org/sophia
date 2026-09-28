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

## WM file implementation inputs

The WM file layer uses these additional unmodified contract inputs from Sophia
`de776c68afdf9a133818f86917893c3362dc9fb7` (signed):

- `protocol/sophia-wm-files-v1.kdl` → `spec/sophia-wm-files-v1.kdl`
- `protocol/sophia-wm-v1.kdl` → `spec/sophia-wm-v1.kdl`
- `protocol/golden/sophia-wm-v1.records` → `spec/golden/sophia-wm-v1.records`
- `docs/sophia-wm-api.md` → `spec/sophia-wm-api.md`

The WM file KDL, rows, corpus and WM file lifecycle document are byte-identical
between Sophia `9fcaec782` and this revision. These pinned documents still
describe transitional IPC support in Sophia; they do not weaken this SDK's
complete 9P migration target.

## WM file row layouts

`spec/sophia-wm-files-v1.kdl` is copied unmodified from Sophia
`264080faeeabb1df69765a1c3cb26eb1bd30d265` (signed), which moves the 8 ordinary
and 14 extension fixed row layouts, the extension capability gates and scalar
constants into the file contract's `row-layouts` block and spells its booleans
`#true`. `spec/sophia-wm-files.md` is copied unmodified from the later signed
Sophia `4a03927421d13a9084295c5ace62c6d9de81d381`, which only narrows that
document's claim: the schema owns extension gates, while ordinary row
disclosure and additional capability dependencies stay in the typed file
validators. The KDL is identical at both commits. The copies at `264080fae`'s
parent were byte-identical to the previous pins, so these two files replace the
`de776c68a` and `e9750572` copies above; no other contract input changes.
Widths, kinds, maxima and values are unchanged.

`tools/generate_wm_rows.py` now takes rows only from that block. The generated
`src/sophia_wm_records.h` and `src/wm_files/rows.c` are byte-identical to the
previous output except for the first comment line naming the source. The
socket schema stays pinned while the compatibility library remains; the
generator refuses any drift between its rows and the file rows, but it no
longer defines the file codec. The golden row corpus still tests the row codecs.

## Native shell queue and welcome bounds

`spec/sophia-shell-files-v1.kdl` and `spec/sophia-shell-files.md` are copied
unmodified from signed Sophia
`d040013bbdd9e84716faa1e06aeb7b7e020ba44a`. They specify native record queue
charges and exact journal reserves, and name `max_chunk_bytes` as the file
upload budget. The existing Limits layout and its mandatory relationships
remain unchanged. On every valid Limits value this budget equals the previous
`min(max_frame_payload - 48, max_chunk_bytes)` calculation.

The same contract makes the existing role welcome bounds explicit on the file
wire: 1–16 descriptors, 1–128 label bytes and 1–16 pending activations. Invalid
welcomes are refused during encoding and decoding. This replaces the two shell
file references above; every other contract copy retains its earlier pin.

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

## Persistent catalog identity rule

The shell file KDL and lifecycle document above are refreshed from signed
Sophia `1ae31f132105b5e178c62d3689f1f3d73bf95412`. They explicitly require
byte-exact distinct identity names when a catalog discloses identities. This
documents the existing persistent identity bijection; labels may still repeat
and plain launcher catalogs have no identities. No layout or code changes
accompany this reference update. Other contract copies keep their earlier pins.

## Proposed descriptor records (development only)

`spec/proposed/descriptor-layout.kdl` and `descriptor-records.md` are unmodified
copies of Sophia's proposed ADR `4oapm903` and its layout fragment from signed
commit `0cbb7ea5b3aade7fee6ee271fe03e57e3cf2e78f`. Their original paths are
`docs/notes/decisions/4oapm903-descriptor-layout-proposal.kdl` and
`docs/notes/decisions/4oapm903-carry-descriptor-families-as-native-shell-file-records.md`.
`spec/proposed/SHA256SUMS` binds these development inputs separately from the
published contracts; `make check-spec` checks both sets. Relative links and the
proposal's historical implementation status are preserved in the copied ADR.

The ADR copy is subsequently refreshed from signed Sophia
`348dee082260706158447bc2e65745992a071423`. It clarifies that hidden descriptor
candidates have no entries and that generations may repeat across distinct
slots. The KDL is byte-identical; no field layout changes.

The ADR is refreshed again from signed Sophia
`731c5295bb2bf5bc875a1704a27f76a9e44e5086` to state shortcut slot uniqueness and
mandatory chord/action text explicitly. This preserves the prior shortcut
validator's rules; the proposed byte layout is still unchanged.

The descriptor codec implements all seventeen proposed envelopes: three whole
objects, eight event bodies, two activation acknowledgements and four
presentation candidates. Literal native file vectors test them independently
of the Rust codec and the old socket frames. Development profile selection,
metadata/combined readiness and snapshot fetch/ack holds have scripted-peer
coverage. Large queued candidates use explicit caller-owned staging with
scripted capacity, reservation and custody controls. Real-export conformance
remains pending in the C SDK path. This is not acceptance of the contract or a
release claim.

## Accepted descriptor file contract

`spec/sophia-shell-files-v1.kdl`, `spec/sophia-shell-files.md` and
`spec/sophia-shell-descriptors.md` are copied unmodified from signed Sophia
`3330ecf7701356ffc42eb986c294ab6ffe422229`. The seventeen descriptor kinds and
twenty-four body/prefix/row layouts are now part of the normative file KDL;
their bytes and validation rules match the earlier proposal. The separate
proposal copies are removed. This supersedes the development-only contract
status above, without changing the library's wire behavior.

Sophia's independent C production-export test covers all seventeen kinds
(`8fa095da3`); its protected C CPU work-area test (`6e7ddf8ea`) checks matching
presentation before reservation changes. Descriptor proof/serve/bar-proof and
launcher hosts (`85158df80`) use the C SDK peer. Session startup is fixed to
9P at `6fdee6049`, with 704 passing tests and the protected C presentation
assertions retained. Narthex's thin C bindings at `c49dd92` pass local and
protected host tests. These are deterministic and isolated development gates;
no installed-desktop or physical GPU claim is added by this contract update.

## Descriptor contract documentation correction

`spec/sophia-shell-files.md` is refreshed from signed Sophia `086cd6e75ba4d12e30735de5b9aad7df9c687e18`.
The role table now uses the accepted native descriptor sizes and feed bounds,
and removes superseded client-migration observations. Wire layouts, library
sources and tests are unchanged from the accepted-contract revision above.

## WM Session source retirement

`spec/sophia-wm-files.md` is copied unmodified from signed Sophia
`8c4c58d6e99ac98b59d5f0d4d07e9e11bee2c2fe`. Session now defaults to WM files and refuses
current-ipc selection. Rollback selects a previously verified compatible release;
latency qualification stays open. Wire layouts, custody rules and SDK library
sources are unchanged. SDK compatibility removal is a separate follow-up.

## Shell Session default retirement

`spec/sophia-shell-files.md` is copied unmodified from signed Sophia
`49d63533d235d94574f9b32b9cd476cfbdb319b1`. Every Session shell role now defaults to 9P
and refuses explicit current-ipc selection. Protected launch supplies the
owner's 9P endpoint; recovery uses a compatible older whole release. The
default remains experimental while latency and physical qualification are
open. This reference update changes no library source, wire layout or custody
rule. SDK compatibility source removal remains a separate follow-up.

## Release 0.2.0: socket compatibility retirement

Sophia's signed source retirement at `5b1d9ac4e` follows the accepted
whole-release rollback decision. This SDK removes the shell/WM socket library,
its headers, frame tests, socket schemas and frame corpora. Surviving spec
files and the WM row corpus retain their original digests. The row generator
reads the file contract alone; its generated codecs are unchanged.

`sophia_desktop_select_shell` now takes the file path and output argument.
Environment selection refuses the retired socket variable, including empty
values. File connection authentication, bounded retry and ownership stay with
the existing connection helper. The C file suites and generator checks pass
in device-hidden isolation; release integration is recorded by Sophia's pin.
The initial checksum-pruning attempt incorrectly treated already prefixed
paths as relative to spec; the checksum gate refused it. The corrected list
retains every existing file and passes without changing retained digests.

The release also imports `spec/sophia-shell-files.md` from signed Sophia
`2ea546bac9836aa1aed61ddeb52cc552710a9b8a`. This documentation correction points
to the file schema and records that the existing Limits fields and validation
relations survive adapter retirement. No wire layout changes. Its new digest
is recorded in `spec/SHA256SUMS`; all other retained digests are unchanged.

## Release 0.3.0: output file role

`spec/sophia-output-files-v1.kdl` and `spec/sophia-output-files.md` are copied
unmodified from signed Sophia `2f3264c432cc50af93e4fd911a71fc83117ca78e`.
The KDL is the layout authority for `sophia_output_files.h`; the document
supplies the file lifecycle, snapshot invariants and the revision-1 limitation
that current transform and VRR are not published. The codec and session were
written from these files, not translated from Sophia's Rust implementation.
Their digests are recorded in `spec/SHA256SUMS`; all other digests are
unchanged. Scripted-peer tests enforce the documented export rules. Sophia's
independent C fixture at that commit passes negotiation, exact topology fetch,
proposal delivery, terminal outcome and cumulative acknowledgement against the
production OutputFileService. Evidence is
`t253-c-peer-lifecycle-exchange.log` in Sophia's development evidence directory.
The final 0.3.0 candidate also passes `make -j1 all check` (contract digests and
17 test programs) in device-hidden isolation, recorded in
`t253-sdk-030-check.log`. The peer linked against those built libraries passes
the same production export fixture in `t253-sdk-030-export.log`.
The SDK sources were the release candidate; version and provenance edits do not
change those sources. This qualifies the C file client, not physical topology
effects, native presentation, Rust client parity or output default selection.

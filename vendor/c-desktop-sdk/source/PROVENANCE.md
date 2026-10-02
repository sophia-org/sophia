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

## Release 0.4.0: 9P2000.L output transport in the WM api

`spec/sophia-wm-files.md` and `spec/sophia-output-files.md` are copied
unmodified from signed Sophia contract commit
`b0721d0de6a03cb44e57b0a923c6c385cf40b676`. The WM document names the WM
`api` file's exact bytes and removes the WM's output grant. The output document
becomes revision 1 and takes over physical ownership and recovery from the
retired output IPC contract. Their new digests replace both entries in
`spec/SHA256SUMS`. No other imported contract changes; every other digest is
unchanged. That commit is a contract source, not a qualifying Session build.

The WM session (`src/wm_session/bootstrap.c`) requires the exact `api` bytes
`sophia-wm-files version=1 output_transport=9p2000.L` followed by a newline.
The retired `current_ipc` value, a missing newline, a different letter case and
an api without the transport field are refused before negotiation. Scripted
tests assert each refusal with no submission. A private mutant accepting the
retired string alongside the current one fails that test. The output session's
wrong-api negative (a WM api offered to the output role) is unchanged. The
output codec already keeps the outcome reason as an open u16 value, as revision 1
requires. No output codec or session source changes in this release.

`output_files` stays true. It is backed by Sophia's output file role at signed
Sophia `ddd27bd6d9ac6d8e73394d9326705a7a62916f35`: native proof preparation
(13 export tests and the protected Session fixture), performance qualification,
and an attended four-stage native run (validate, reject, commit-restore and
peer-death rollback) assembled by niltempus `bec6db137d7e`. The evidence is
`t253-native-run-bec6db1-01` and `t253-perf-ddd27bd6d-01` in Sophia's development
evidence directory. The run covered one DRM card with two heads and a
refresh-only change on one head, verified by KMS and owner readback records.
Resolution, position, enable, transform, mirror and multi-card changes are not
qualified by it.

`make all check` passes in device-hidden isolation at normal priority with the
available CPUs (17 test programs). Build and test guidance no longer prescribes
a fixed nice value or job count.

## Release 0.5.0: additive chord lifecycle

`spec/sophia-wm-files-v1.kdl`, `spec/sophia-wm-files.md` and
`spec/sophia-wm-api.md` are copied unmodified from signed Sophia commit
`86bf65046627ef9fea2a041ae10b08472b3271d1` on `feature/generic-chording`. That
commit is the additive contract commit. Its parent, `dd62b50c8`, held copies
byte-identical to the previous pins, so the import brings only this change.
The commit is published for source reachability. It is not an implementation
or acceptance claim: Session does not implement the lifecycle at that commit.
The three new digests replace their entries in `spec/SHA256SUMS`, and every
other digest is unchanged.

The contract adds capability bit 20, `action_lifecycle`. It also adds the
Configuration-only extension row `ConfigurationActionLifecycle` (kind 65294,
16 bytes, at most 256, gated on `action_lifecycle`, `actions` and
`configuration`) and the ActionLifecycle cause (kind 7, 24 bytes).

`tools/generate_wm_rows.py` now expects 15 extension rows. It routes this row
to the Configuration family: the contract reads it only in a Configuration,
although it is written with snapshot transfer. The regenerated rows add the
capability constant, the row codec, its layout and its reserved check; earlier
rows are unchanged.

`src/sophia_wm_files.h` and `src/wm_files/bodies.c` hand-code the cause. Its
body is serial, action, phase, reason and count. It requires `actions`,
`configuration` and `action_lifecycle`. Decoding, and encoding through the
same check, accept only Held with reason 0, or Ended with reasons 1 to 5, a
nonzero serial and action, and a count of at least 1. Negotiation
(`src/wm_session/events.c`) fails when the selected capabilities include
`action_lifecycle` without both `actions` and `configuration`, matching
Session's selection rule.

The codec checks the row's layout, its capability gate and its reserved bytes.
Session's semantic validation owns two further kinds of rule, and the codec
repeats neither:

- Catalog and cross-row rules: each action is in the catalog, is not a session
  operation, and appears in at most one row.
- The per-row value rule: `held_ms` is 0 or 50 to 5000.

Literal vectors cover every phase and reason pairing, count 0 and the
saturated maximum, a zero serial or action, an unknown cause 8, each missing
capability, and the Configuration rows with their capability gate and reserved
bytes. Scripted-peer tests cover the negotiated dependency in both directions
and the cause being refused before an application sees it when the capability
was not negotiated. Three bounded mutants are each killed by these tests:
removing the cause's capability gate, removing the negotiated dependency, and
accepting Held with any reason.

## Release 0.5.1: golden row for the chord lifecycle

Sophia's contract commit `86bf6504` changed the WM file KDL but not the outputs
generated from it, so 0.5.0 copied a golden row corpus without the new row.
Signed Sophia commit `3a1dce11febb64df2fc98a83f5ac1ff342287947`, the follow-up
to `86bf6504` on `feature/generic-chording`, adds those generated outputs and
nothing else. `spec/golden/sophia-wm-v1.records` is copied unmodified from it.
Its one new line is the `configuration_action_lifecycle` sample: action 5,
`held_ms` 150, reserved 0. The new digest replaces that entry in
`spec/SHA256SUMS`; every other contract copy and digest is unchanged.

`wm_files_test` decodes that row with the existing generated codec and now
expects 23 golden rows. An unrecognised row still fails the test. Against the
0.5.0 sources the new corpus fails with "unrecognized golden row". No library
source changes; v0.5.0 is unchanged and remains tagged.

## Release 0.6.0: ChordAction

`spec/sophia-wm-files-v1.kdl`, `spec/sophia-wm-files.md` and
`spec/sophia-wm-api.md` are copied unmodified from signed Sophia contract
commit `b0a2b0303b02af27598def07f455e8e0240b9366` on
`feature/generic-chording`. Its parent held copies byte-identical to the 0.5.1
pins, so the import brings only this change. The golden corpus is unchanged,
because the new cause adds no fixed row. The three new digests replace their
entries in `spec/SHA256SUMS`.

The contract adds capability bit 21, `chord_actions`, which requires
`action_lifecycle`, `actions` and `configuration`. With it, a keyboard
activation of a followed chord arrives as ChordAction: cause 8, 24 bytes,
holding the activation serial, the chord serial and the action, all nonzero.
The opener carries equal serials. The regenerated rows add only the capability
constant.

`src/sophia_wm_files.h` and `src/wm_files/bodies.c` hand-code the cause.
Decoding, and encoding through the same check, require all four capabilities
and refuse a zero field, so a client that selected `action_lifecycle` alone
(0.5.1 behaviour) refuses cause 8. Negotiation (`src/wm_session/events.c`)
fails when `chord_actions` is selected without the lifecycle, actions and
configuration. Cause 9 is now the unknown code in the literal tests.

New tests:
- literal ChordAction vectors: the round trip, the opener's equal serials,
  each missing capability, and each zero field;
- scripted peers: all sixteen combinations of the four related capabilities,
  and cause 8 refused before an application sees it without `chord_actions`
  but delivered with it.

The ChordAction cause is qualified only by literal and scripted-peer tests:
Session does not send it yet.

## Release 0.7.0: held capture

`spec/sophia-wm-files-v1.kdl`, `spec/sophia-wm-files.md` and
`spec/sophia-wm-api.md` are copied unmodified from signed Sophia contract
commit `ac04e1a7487e9be3a238075e3c624aff93705f2d` on `feature/held-capture`. Its parent held copies
byte-identical to the 0.6.0 pins, so the import brings only this change. The
golden corpus is unchanged, because the contract adds no fixed row. The three new digests replace their entries in
`spec/SHA256SUMS`.

The contract adds capability bit 22, `held_capture`, which requires
`surface_instances` and `presentation_actions`. With it, a presentation whose
outputs are all Overlay may carry a keyboard output and bindings. Sophia passes
modifier keys through such a capture and takes only other keys. The
regenerated rows add only the capability constant.

The presentation codec already carries `keyboard_output`, output modes and
bindings without judging their combination, so no codec change is needed.
Negotiation (`src/wm_session/events.c`) fails when `held_capture` is selected
without both dependencies. The scripted peer's full capability set (`WP_CAPS`)
now includes bit 22.

New test: scripted peers check all eight combinations of the three related
capabilities.

Held capture is qualified only by scripted-peer tests: Session does not
implement it yet.

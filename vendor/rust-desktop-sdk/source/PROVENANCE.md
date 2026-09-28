# Provenance

Extracted from the Sophia repository (`sophia-org/sophia`), branch
`sdk/rust-desktop-sdk` at `e47c37472` (base: master `987cfd393`, the t252
merge). Later changes are made here first; Sophia vendors a pinned snapshot.

| Crate | Sophia source |
| --- | --- |
| `sophia-9p-records` | `crates/sophia-9p/src/records.rs` |
| `sophia-9p-client` | `crates/sophia-9p/src/{client,client_codec,pipeline}.rs` |
| `sophia-desktop-ids` | `crates/sophia-protocol/src/ids.rs` (`OutputId`, `TransactionId`) |
| `sophia-shell-protocol` | `crates/sophia-protocol/src/{shell,shell_files,byte_cursor}` and tests `shell_file*.rs`, `support/shell_files_kdl` |
| `sophia-shell-ipc` | `crates/sophia-protocol/src/ipc/{types,frame,cursor,neutral_errors,shell_content,shell_catalog_actions,shell_indicators,shell_catalog_transaction}.rs`, the handshake half of `ipc/shell_v1.rs`, and `byte_cursor.rs` |
| `sophia-shell-client` | `crates/sophia-shell-client` |
| `spec/sophia-shell-files-v1.kdl` | `protocol/sophia-shell-files-v1.kdl` at `d040013bbdd9e84716faa1e06aeb7b7e020ba44a` |
| `spec/sophia-shell-v1.kdl` | `protocol/sophia-shell-v1.kdl` |
| `spec/sophia-shell-files.md` | `docs/sophia-shell-files.md` at `d040013bbdd9e84716faa1e06aeb7b7e020ba44a` (the normative shell file contract) |
| `spec/sophia-wm-files.md` | `docs/sophia-wm-files.md` at `4a03927421d13a9084295c5ace62c6d9de81d381` (the custody and retry rules the shell contract adopts) |
| `spec/sophia-9p-profile.md` | `docs/sophia-9p-profile.md` |
| `spec/references/diod-9p2000L-protocol.md` | `docs/references/diod-9p2000L-protocol.md` (GPL; reference only, see its notice) |
| `spec/golden/sophia-shell-*.frames` | `protocol/golden/` (content, content-malformed, catalog-actions, indicators, launcher, v1, v1-malformed) |

Edits made on extraction: crate paths, crate roots and manifests, doc links
that named server modules, the socket wire behind `ipc-compat`, and the KDL
path in the conformance tests.

`spec/sophia-wm-files.md` was refreshed unmodified from Sophia
`4a03927421d13a9084295c5ace62c6d9de81d381` (signed). Since the earlier pin, the
document points the WM row layouts at the file schema's `row-layouts` block
(`264080fae`), says the socket schema no longer defines them, and says the
schema owns only extension capability gates, with ordinary row disclosure left
to the typed file validators. The custody and retry rules this SDK adopts are
unchanged. This SDK has no WM client and copies no WM schema.

`spec/sophia-shell-files-v1.kdl` and `spec/sophia-shell-files.md` were
refreshed unmodified from Sophia `d040013bbdd9e84716faa1e06aeb7b7e020ba44a`
(signed), which includes `fe5bd3f6b`. The `Negotiated` body keeps its layout
and now states the shell role's welcome limits for every profile: descriptors
1-16, label bytes 1-128 and pending activations 1-16. File upload chunks use
`max_chunk_bytes` directly; the `max_frame_payload` and
`max_input_queue_bytes` fields and their +24/+48 relations stay mandatory in
the Limits layout. The document also states Session's native queue charges
and journal reserves, which this client does not implement.

## Persistent catalog identity rule

The shell file KDL and lifecycle document above are refreshed from signed
Sophia `1ae31f132105b5e178c62d3689f1f3d73bf95412`. They explicitly require
byte-exact distinct identity names when a catalog discloses identities. This
documents the existing persistent identity bijection; labels may still repeat
and plain launcher catalogs have no identities. No layout or code changes
accompany this reference update. Other contract copies keep their earlier pins.

## Descriptor record model

The `shell::{descriptor,tabs,reference,launcher}` passive records and the
sanitized label/trust/attention values in `shell::metadata` are extracted from
Sophia `262eb82bcf2ce87401ba82dc17cc832952046e43`, under
`crates/sophia-protocol/src/packets/{shell_v1,shell_tabs,shell_reference,shell_launcher,chrome}.rs`.
The field shapes and enum values are unchanged. Their pure validators preserve
the corresponding `ipc/shell_*` structural checks, with tab counts checked by
subtraction and each tab entry validated directly instead of constructing a
temporary standalone snapshot. Owner checks and transport are not included.

This model prepares the descriptor file role; it does not add that role to
the served contract or enable it in the client. The spec copies and wire kinds
are unchanged. The new model tests cover cardinalities, exact action identity,
family-specific text and selection rules, style bounds and outcome epochs.

The native `encoding::descriptor` whole-object values implement the layout
proposal in Sophia ADR `4oapm903` at `262eb82bc`: Descriptors, Tabs and
Shortcuts after the domain transaction. They are newly written against those
offsets, not copies of socket frames. Literal-byte tests cover every prefix
and row, maximum object sizes, padding, optional-value flags and truncation.
The published shell file kind list and client role remain unchanged until
the complete descriptor amendment is implemented and accepted.

The same proposal now has native event and candidate values for descriptor,
tab, reference and revision-4 launcher records. Literal-byte controls cover
all fourteen values, enum tags, the shared launcher activation identity,
reserved words, complete tables, text padding and individual size caps.
Every truncation and trailing data are refused. Two compiled mutations
(accepting a non-boolean acknowledgement and changing a reservation edge tag)
fail these controls. The default and all-feature workspace gates pass 624
tests in total, with strict clippy in both configurations. That slice supplies
value codecs only.

The following file-envelope slice assigns the proposal's seventeen kinds,
checks header class and domain identity, and preserves per-kind size bounds.
The byte-identical layout proposal from Sophia `262eb82bc` is copied to
`spec/descriptor-files-proposal.kdl`; it stays separate from the unchanged
published contract. KDL completeness checks include that proposal, with no
old record or assertion removed. The default and all-feature suites pass
632 tests in total, with strict clippy in both configurations. An epoch-check
removal compiles and fails the cross-epoch refusal control.

The descriptor client role, production export and independent C peer are not
implemented by these codec commits. This branch remains unpublished until
those pieces and the contract amendment agree.

## Descriptor file client

The client now reads the pre-admitted API role separately from capability bit
0, validates the descriptor capability families and selects the plain catalog
when that role requests it. Metadata-only admission does not fetch Limits;
content admission requires Limits bound to the attach in both the envelope
and grant. Bootstrap events require ordered, current-epoch custody before
Negotiated. No socket compatibility encoding was added.

The three new snapshot feeds use the existing immutable-object fetch and ack
holds, with individual encoded caps and decoded allocation budgets. Descriptor
candidates use the existing typed outbox and Submitted custody lane;
activation acknowledgements use its reserved control capacity. The client
does not present geometry, commit work areas or decide action admission.

Scripted 9P tests cover all seventeen kinds, maximum cardinalities and text
sizes, fragmented object reads, supersession, role and family refusals,
metadata-only and combined admission, plain catalogs, indicators and queued
custody. Bootstrap controls cover stale epochs, repeated sequences, missing
or duplicate custody, invalid record lengths and both Limits identities.
The default and all-feature workspace gates pass 664 tests in total, with
strict clippy in both configurations and formatting clean. Compiled mutations
removing the role gate, snapshot ack hold and Limits grant-epoch check each
fail their specific assertions. Production export and independent C peer
gates remain outstanding; the branch stays unpublished.

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

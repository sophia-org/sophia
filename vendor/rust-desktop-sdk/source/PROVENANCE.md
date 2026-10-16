# Provenance

## t331 conditional retry clock

Submission retry handling reads the clock only when a pending retry has no
new event progress and is eligible for a later-pass deadline check. Idle and
ordinary upload rounds no longer query time for an absent retry. The refusal
pass rule, event-driven retry and deadline comparison remain unchanged.
Workspace tests, all-target Clippy and formatting pass. Native CPU savings
remain unmeasured; this change does not alter socket scheduling or custody.

## t331 batched writer setup

The shell-file contract snapshot comes from signed Sophia `d817e6d8a`.
Its request-order guarantee permits a fresh writer's walk and open to be
queued together. The SDK still waits for both successful replies before
payload, uses sequential setup at the existing capacity limit, and discards
both pending replies when an upload slot retires. Submission, acknowledgement,
upload and clunk barriers are unchanged. The generic 9P API is unchanged.

The SDK workspace tests and all-target Clippy pass. Independent scripted peers
check either reply order, refusal, malformed walk, pending-slot retirement
and the one-credit fallback. These checks establish protocol behavior, not
native CPU or allocation improvement; Sophia's paired qualification remains
separate.

## t317 final contract reconciliation

The launch-authentication and shell-descriptor contract snapshots are copied
from signed Sophia `a41011f27dec1e82b58957099691381c03471231`.
They document the implemented supervised role boundaries and generic supplied
descriptor-client conformance seam. This changes documentation and its hashes
only: no SDK source, tests, API or wire layout changes. Earlier execution
evidence retains its original candidate identity. Final Sophia integration
and release qualification remain separate gates.

## t317 reusable local launch client

`sophia-launch-client` is implemented here against the pinned
`spec/factotum-launch-authentication.md` contract. It adds no Sophia-specific
behavior to the generic 9P pipeline. It owns the role afid and private relay
conversation, a total deadline, exact challenge/status validation, temporary
fid cleanup and reauthentication after Tversion. Descriptor-number adoption
belongs to the application's platform bootstrap; this crate accepts owned Unix
streams, marks the generation channel CLOEXEC and introduces no unsafe code.

On Linux x86_64, the device-hidden SDK workspace run passes 351 tests, zero
failed or ignored, all-target Clippy with warnings denied and formatting.
Eight new independent scripted-peer tests cover the launch client. Controls
replace authenticated attach with plain attach, alter the relayed nonce, and
omit CLOEXEC; each fails its named wire/descriptor assertion, and all source
bytes are restored before an eight-test positive rerun. These are SDK controls,
not a production Session/Factotum interoperability claim.

The closed development-package SHA256SUMS identities are:

| Package | Manifest SHA-256 |
| --- | --- |
| `t317-rust-launch-sdk-01` | `cbee6ce3adab7d99257ea3b2ac72699ba76efac3ed4437cb85e5cd7bca418160` |
| `t317-rust-launch-afid-control-01` | `bff08b1aa30f06acd3cc3dd87e398521b6cec1e7feffc64f723a1582c7b3d641` |
| `t317-rust-launch-nonce-control-01` | `97109739994b370bdf9ddc96a824db8e6faf0524bc4c1288679cffbb10f7e313` |
| `t317-rust-launch-cloexec-control-01` | `406c164232d0f15a78a3cc2ea7d28ce7f884bdf23a6fdb24cd6951377a997be2` |
| `t317-rust-launch-restored-01` | `7c144f76e96467225168e0a1a1b65043a77681ff697fb570829014e8e13417cc` |

Existing contract copies are unchanged. Sophia must pin this signed SDK source
and run its independent protected-client join before claiming that integration.

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

| `sophia-lock-protocol` | `crates/sophia-protocol/src/{lock_files,byte_cursor.rs,codec_error.rs}` and tests `lock_files.rs`, `lock_file_schema.rs`, `lock_file_golden.rs`, `support/file_schema.rs`, at `fd6ea7538` (branch `lock/t034-next`, unmerged) |
| `spec/sophia-lock-files-v1.kdl`, `spec/sophia-lock-files.md`, `spec/golden/sophia-lock-files-v1.records` | `protocol/sophia-lock-files-v1.kdl`, `docs/sophia-lock-files.md`, `protocol/golden/sophia-lock-files-v1.records` at `fd6ea7538` |

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

Following signed Sophia `5b1d9ac4e`, the SDK removes `sophia-shell-ipc`,
`ipc-compat`, the socket connection API and socket schemas/corpora. Retained
file contracts keep their digests. The fixed Limits value in
`crates/sophia-shell-protocol/tests/fixtures/limits.value` preserves the
original corpus payload without its retired frame envelope.

Native file round trips, offsets, malformed records and bounds remain in the
protocol suite; socket/file parity assertions retire with the socket encoder.
Candidate refusal/retry and catalog response atomicity move to the scripted
file peer. File object tests retain complete catalog publication, fragmented
reads, epoch/generation fencing and mixed event handling. The old socket
handshake and counted catalog assembly tests retire with those encodings.

Migration exposed missing file-client response checks: catalog and indicator
activation pairs could use an acknowledgement for a different action. Both
helpers now reject mismatched echoes before acquiring queue or submission
custody. The file-peer controls vary each identity and disposition field,
verify refusal spends no ticket, then observe the correct ordered pair.
Recovery remains a complete compatible older release; no live-session or
performance claim follows from these source changes.

The release imports `spec/sophia-shell-files.md` from signed Sophia
`2ea546bac9836aa1aed61ddeb52cc552710a9b8a`. This documentation correction points
to the file schema and preserves existing Limits fields and validation
relations; it changes no layout. All other retained spec digests are unchanged.
The restored indicator check also exposed an inconsistent existing ordering
fixture, which now acknowledges the same action as its activation. Isolated
workspace tests and strict all-target, all-feature Clippy pass.

## WM output transport naming

`spec/sophia-wm-files.md` is copied unmodified from signed Sophia contract
`b0721d0de6a03cb44e57b0a923c6c385cf40b676`. The WM `api` file now names
`output_transport=9p2000.L`, and the WM receives no output endpoint or grant.
This SDK has no WM client, and its connection reads only the shell role's
`api` line. The document remains the reference for the envelope, custody and
retry rules the shell contract adopts, which this revision does not change.
Library sources, wire layouts and the other spec digests are unchanged.

## WM chord lifecycle contract

`spec/sophia-wm-files.md` is copied unmodified from signed Sophia contract
`86bf65046627ef9fea2a041ae10b08472b3271d1`, the additive chord lifecycle
commit on `feature/generic-chording`. It adds the `action_lifecycle`
capability, the Configuration rows declaring followed actions and the
ActionLifecycle cause. This SDK has no WM client, so none of that is
implemented here. The document remains the reference for the envelope,
custody and retry rules the shell contract adopts, which this revision does
not change. Library sources, wire layouts and the other spec digests are
unchanged.

## WM ChordAction contract

`spec/sophia-wm-files.md` is copied unmodified from signed Sophia contract
`b0a2b0303b02af27598def07f455e8e0240b9366` on `feature/generic-chording`. It
adds the `chord_actions` capability and the ChordAction cause that marks a
followed chord's own keyboard activations. This SDK has no WM client, so
nothing of it is implemented here; the document remains the reference for the
envelope, custody and retry rules the shell contract adopts, which this
revision does not change. Library sources, wire layouts and the other spec
digests are unchanged.

## WM held capture contract

`spec/sophia-wm-files.md` is copied unmodified from signed Sophia contract
`ac04e1a7487e9be3a238075e3c624aff93705f2d` on `feature/held-capture`. It adds the `held_capture` capability: an Overlay presentation may
carry a keyboard output and bindings while modifier keys pass through. This
SDK has no WM client, so none of that is implemented here. Library sources,
wire layouts and the other spec digests are unchanged.

## Lock provider codec (draft)

`sophia-lock-protocol` is extracted from Sophia `fd6ea7538` on
`lock/t034-next` (t294), which is not merged: the lock contract is a draft and
can change until Sophia merges it. The lock codec files are unmodified. Edits
on extraction: `codec_error.rs` drops the conversions from Sophia's shell
`ValueError` and `InvalidRecord`, which the lock codec never uses;
`byte_cursor.rs` drops the `u8`/`i32` reads and writes, which it never uses;
the tests take the KDL and golden records from `spec/`, and the golden test
only checks Sophia's file, never regenerates it. The C SDK's lock files pin
the same three contract copies.

## Release 0.3.0

This release brings two additive changes and the WM contract documentation
imported since 0.2.0 (see the four WM sections above). Every `spec/` copy is
byte-identical at Sophia's signed master merge
`61d545c9087a685c5161f0d5f3132436aa82d31e` ("Merge the lock provider role
(t294)").

- **`sophia-lock-protocol`** (new crate): the lock provider codec extracted
  from Sophia `fd6ea7538` (see "Lock provider codec (draft)" above). The three
  lock contract copies are unchanged at that master merge, which this release
  names as their contract revision. Merging the implementation did not accept
  the contract: the copied text still marks itself revision 1 (draft), so the
  crate is experimental and can change incompatibly until Sophia accepts the
  contract explicitly. The crate renders all of Sophia's golden
  lock records byte for byte. It has no client.
- **`sophia-9p-client`**: `Pipeline::write_secret` queues the same Twrite
  frame as `write`, then zeroes the request body once it is queued, and the
  output buffer's spare capacity after each send until the buffer empties.
  Dropping the pipeline zeroes the whole buffer. Before a secret is queued,
  the buffer reserves the full capacity its bound allows, so no later
  request can reallocate and free a copy unzeroed. `scrubbing_output` and
  `output_capacity` report that state for tests. Sophia's session lock sends
  the unlock password to its authentication agent through this path (t034
  security review, finding 3).

API compatibility with 0.2.0: no public item was removed or changed. The
additions are the new crate and `write_secret`, `scrubbing_output` and
`output_capacity`. `Pipeline` gains a `Drop` impl, but its fields are
private, so no caller can move out of it and no existing code breaks.

## t317 authentication foundation (development)

The authentication records and pipeline operations were developed alongside
Sophia's t317 candidate on base
`e855fd75acd8ac38b7b779cc5377acd5d2c0c223`. The profile copy comes from that
candidate, not the base commit. Sophia's subsequent immutable SDK pin binds
the two trees without a circular commit reference.

This adds Tauth/Rauth and explicit-afid attach, including bounded fid and tag
custody through refusal and both flush orderings. Scripted peers check exact
wire fields and reject malformed authentication qids. The closed-peer test
also accepts ConnectionReset, which is a valid result when a peer closes with
unread request bytes. There is no role-specific authentication policy here.
Public enums gain variants, so exhaustive downstream matches must be updated;
this is a development revision, not a new compatibility release.

## Normal lock-provider admission contract

`spec/sophia-lock-files.md` and the newly copied
`spec/factotum-launch-authentication.md` come from signed Sophia
`49eacf6462cfbe1eec8195ddeed5a1090c932447`. They specify the protected launch
channel, Factotum afid exchange, immutable identity and current custody selected
by the candidate's normal lock-provider startup. Record layouts and Rust code
are unchanged. This SDK still provides only the generic afid primitives;
the reusable launch exchange and a real protected Rust peer remain t317 work.

## Shell authenticated-root handoff

`ShellConnection::from_attached` takes exclusive ownership of the role pipeline
and root returned by the launch client. It refuses poisoned, busy or multi-fid
pipelines, then shares the ordinary shell negotiation beginning at `api`,
without Tversion or a second attach. It does not select claims or automatically
adopt environment descriptors. The generation channel remains the caller's.
The shell contract copy is from signed Sophia
`bd9ee9460a28870ab15876e447ef5139e462efe5`; its record layout is unchanged.

Scripted supplied-admission tests negotiate all four shell profiles, confirm
the single attach and epoch, and observe both custody acknowledgements. They
do not qualify the production Factotum join. A reattach mutation fails at
`shell handoff must not attach again`; all captured fixture bytes are restored.
The restored full workspace passes 352 tests with zero ignores, all-target
Clippy with warnings denied, and formatting. The contract hash check passes
from `spec/`; an earlier invocation from the repository root failed to locate
the files and is retained in the log.

| Package | SHA256SUMS SHA-256 |
| --- | --- |
| `t317-rust-shell-control-01` | `3e907f85d5aab68c45a473919e321047c37386185993dbc7633446116bd1ee2f` |
| `t317-rust-shell-handoff-01` | `8831bf871112944b935bdfc55f47c0f50af5268ed77818883bd18eed49881f4c` |

The positive package retains executable paths/hashes, never binary copies.
This provenance addition follows the captured candidate; implementation and
tests are unchanged. Production interoperability and deployed-client migration
remain separate gates.

## Shared WM contract reconciliation

The WM file contract copy now matches signed Sophia
`bd9ee9460a28870ab15876e447ef5139e462efe5`, including normal authenticated
WM startup and replacement. No codec or client implementation changes.
This aligns the shared contract snapshot before the Rust shell production join.

## Recipe authentication audience sets

The authentication contract is copied from signed Sophia `c57cc30eedce72dfeacea6ced3d49df78b26da52`.
It adds a bounded approved-audience set on the private Session-to-Factotum
owner interface. Public afid and launch-relay bytes are unchanged; no SDK
implementation changes or new authentication qualification are claimed here.

## Namespace discovery foundation

`spec/namespace-discovery.md` is copied without modification from signed Sophia
`2ef7e39b984dfba12f346c1f4d2b98e08edb6d65`. The discovery parser and Linux directory lookup are
implemented independently against that contract. Local socket tests cover pinned
directory identity, long endpoint names, symlink refusal and descriptor flags.
These tests do not claim Session launch integration or authentication; endpoint
visibility never grants permission.

Qualification: the full Rust workspace tests, formatting and all-target Clippy with warnings denied pass; the new discovery crate has six tests. Removing the declared endpoint-count check makes the complete-line truncation test fail. The source and all candidate files were restored byte-for-byte. This control qualifies truncation refusal, not authentication or a live Session launch.

- `t318-rs-discovery-01`: SHA256SUMS `33505120dfb446fac6cf96216c1e8d4b2ffb25cd67733ef4db08618d7d58a3f9`.
- `t318-rs-discovery-count-control-01`: SHA256SUMS `2accef4c8b395582c20439ac9cc07470e16a0d7bac688b73b739d0e191f994ac`.

## Sequential recipe endpoint authentication

The launch helper can reuse the completed conversation's exclusive, quiescent
relay for another endpoint without resetting the first role transport. Pending
relay work refuses. Independent scripted peers exercise both properties; no
production multi-endpoint launch is claimed by this SDK-only run. All ten launch-client tests and all-target Clippy pass.

Evidence `t318-rs-relay-01` has SHA256SUMS SHA-256
`5f977b28ea9941fe9459fb65b8efef5850adb0648e4cf84d74b76aa69e650625`. The candidate hashes cover
all modified/untracked files at the run; this provenance addition follows it.

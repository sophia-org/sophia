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

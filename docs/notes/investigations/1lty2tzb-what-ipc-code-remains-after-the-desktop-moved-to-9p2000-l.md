---
id: 1lty2tzb
date: 2026-09-27
kind: investigation
status: investigating
tags: [investigation, architecture, 9p]
---
# What IPC code remains after the desktop moved to 9P2000.L

## Question

The live desktop now runs Hagia (WM), Lom (bar) and Bemenu (launcher) over
9P2000.L only. The goal is a desktop that is 100% 9P: no Sophia-owned IPC
socket protocol left in production. Which IPC code can be deleted now, which
needs a preparatory change first, and which still has no 9P replacement?

## Evidence

- Candidate: Sophia master `2d69924a9` (the installed release
  `niltempus-583dcced3b89e9319866` pins it; manifest schema 7).
- Live session, read-only check on 2026-09-27: Hagia runs with
  `SOPHIA_WM_9P_SOCKET` and the `SOPHIA_WM_POLICY_*` names; Lom and Bemenu
  run from the release with only `SOPHIA_SHELL_9P_SOCKET`, no
  `SOPHIA_SHELL_SOCKET`.
- Survey method: read-only `git grep` and source reading at `2d69924a9`; no
  build or run. The full survey with file and line references is retained at
  `~/.local/state/sophia/development-evidence/ipc-removal-inventory/inventory-2d69924a9.md`
  and reproduced below.

Observation versus hypothesis: the per-surface callers below are observed in
source. Items marked *uncertain* were not confirmed.

## Finding and resolution

Almost nothing can be deleted outright yet. Three things hold IPC in place:

1. The 9P code still borrows IPC code. The WM file codec calls the
   `ipc::wm_v1_records`, `ipc::policy_records` and `ipc::policy_scalars`
   encoders and `IpcCodecError`; the shell owners build IPC frames and size
   budgets with `SOPHIA_IPC_HEADER_LEN`.
2. Three roles have no 9P replacement and run in the live session: output
   (`SOPHIA_OUTPUT_SOCKET`), the control bus (`sophia msg`) and the metadata
   broker and portal.
3. The transport selections still default to `current-ipc`, and the plan keeps
   IPC as an explicit rollback and benchmark path until t250 and t252 close.

Classification used below: **A** removable now; **B** removable after a
stated preparatory change; **C** must stay until a named 9P replacement exists.

IPC removal inventory for Sophia master 2d69924a9 (read-only; nothing built or run)

**Headline:** Almost nothing can be deleted outright today. The WM and shell 9P paths still share code with IPC: the WM file codec imports `ipc::` row codecs, and the shell owners encode IPC frames and charge budgets in IPC header bytes. The output role, the control bus (`sophia msg`) and the metadata broker have no 9P replacement, and all three run in the live session. Every migration task is still open in todo.md: t249, t250, t252, t253, t254, t255 and t263. The plan note (`docs/notes/plans/jlftaw00-…`, section "Implementation and qualification policy (2026-09-27)") says the operator keeps IPC "for explicit rollback and benchmark comparison". t250 also requires "explicit current-IPC relaunch rollback".

**What the live session actually runs** (release `/opt/sophia-niltempus-desktop/releases/niltempus-583dcced…`, whose manifest pins sophia 2d69924a9):
- It is launched with `--wm-transport=9p2000.L` and `--native-scanout`.
- Its generated `desktop.kdl` sets `transport "9p2000.L"` for Lom and Bemenu, and `control host-admin`.
- The launcher still passes `--shell-process-default=…/narthex`. It has no effect because shell components are configured, but the flag is still parsed.
- Your source profile `~/.config/sophia/desktop.kdl` has shell components with **no** `transport` line, so it resolves to current-ipc.

### 1. Transport selection enums and defaults — category B
- **Shell:** `crates/sophia-config/src/shell_components.rs:46-77,157-160`. `ShellTransportSelection` defaults to `CurrentIpc` and maps it to `SOPHIA_SHELL_SOCKET`.
- **WM:** `crates/sophia-session/src/live_session/config.rs:22-37` (`WmTransportSelection`, default `CurrentIpc`). `config/arguments.rs:506-509`: omitting `--wm-transport` selects current-ipc. `arguments.rs:576-585` handles `--shell-transport`, same default.
- **Other callers:**
  - `shell_component_connections.rs:110` (`add()` hardcodes CurrentIpc) and `:268` (negotiation branch).
  - `shell_component_processes.rs:98`.
  - `wm/public_policy/transport.rs` (the `CurrentIpc` variant, 120 lines).
  - `inspection.rs:248` and `sophia-protocol/src/inspection.rs:23` (`InspectionWire::CurrentIpc` is only a label; the inspection service itself is already 9P).
  - CLI help at `help.rs:60,84`.
- **Change needed:** make 9p2000.L the default, or delete the variants.
- **Knock-on effects:**
  - About 22 test files select CurrentIpc explicitly, e.g. `sophia-config/tests/shell_components.rs:203` and `sophia-session/tests/support/{wm_transport_config,policy_transport_selection,shell_startup,component_reconnect,metadata_shell_tests,policy_inspection}.rs`.
  - `docs/configuration.md:738-757` and `docs/sophia-wm-files.md:12` ("Omission selects current-ipc") need rewording.
  - Rollback breaks: a relaunch with your `~/.config` profile, or with no `--wm-transport`, changes meaning.

### 2. WM IPC (sophia_wm_v1 socket) — B for the transport, C for the shared codecs
- **Transport code (no production caller when 9P is selected):**
  - `policy_transport_worker/current_ipc.rs` (221 lines).
  - runtime `policy_ipc.rs` (846).
  - most of `policy_transport.rs` (797) and `PolicyWmSessionTransport`.
  - `ipc/wm_v1.rs` (2345, generated), `wm_v1_profile.rs`, `packets/wm.rs`.
- **Tools and bindings:**
  - `bindings/c/sophia_wm_v1.{c,h}` and its tests, `protocol/archive/sophia-wm-v1-r3/`.
  - the generator `tools/sophia-policy-protocol-gen`, `docs/generated/sophia-wm-v1-wire.md`, golden `sophia-wm-v1*.frames`.
  - `examples/policy_c_conformance_host.rs` (359).
  - `tools/check_policy_protocol.sh`, `check_archived_policy_client.sh`, `check_policy_client_matrix.sh`; xtask gate "wm-independent" at `native_protocol_family.rs:170`.
- **`crates/sophia-wm-demo`** (1414 lines): an IPC-only demo client. Correction (t264): sophia-cli never used it, but IPC policy tests do (`sophia-runtime/tests/policy_transport.rs` and sophia-session's live-control support use its `PolicyV1Client`), as do `tools/check_policy_protocol.sh`, `tools/check_policy_client_matrix.sh` and the xtask output-client stage. t264 dropped the cli dependency and made the session one dev-only; deleting the crate belongs to t269 with those IPC gates.
- **Blocking coupling:**
  - `sophia-protocol/src/wm_files/*` call `encode/decode_policy_{configuration,projection,snapshot}_records`, `decode_rgb` and `validate_policy_*` from `ipc/wm_v1_records*`, `ipc/policy_records.rs` and `ipc/policy_scalars.rs`. They also use `IpcCodecError` (`wm_files/payload.rs:5`, `records.rs:123`).
  - `protocol/sophia-wm-files-v1.kdl:207` says the row layouts "remain those in sophia-wm-v1.kdl".
  - These must move to wire-neutral homes first (`docs/sophia-shell-files.md:1008-1009`, "relocate, not delete").
- **Shared pieces to keep or rename:** the 9P worker uses `PolicyRoleEndpoint` (`policy_socket.rs`), `PolicyTransportError` and `PolicyProfileHandoff*` (`ninep/startup.rs`, `pending.rs`, `typed_codec.rs`).
- **Name only:** `--wm-interface=sophia_wm_v1` / `ExternalWmInterface::SophiaWmV1` is a policy name, not the IPC wire.
- **Gate:** t250 qualification plus the rollback recipe.

### 3. Shell socket (sophia_shell_v1) for components (Lom, Bemenu, Provlita) — B, parts C
- **Socket-only code:**
  - in `crates/sophia-runtime/src/shell_transport.rs` (907): the stream/inbox/frame decode at about :536-620 and :244-400.
  - `negotiation_service.rs` (socket hello).
  - `accept_and_negotiate*` in `negotiation.rs:43,59` and `legacy_methods.rs:197,205`.
- **Codecs:** `ipc/shell_*` (about 2.8k lines: shell_v1 723, launcher 380, reference 360, indicators 322, tabs 262, …) and `packets/shell_*`.
- **Not removable in isolation:** the owners still encode IPC frames and size budgets with `SOPHIA_IPC_HEADER_LEN`:
  - `content_resources.rs:187-190`, `native_launcher/control.rs:114-483`, `catalog_responses.rs:117`.
  - `content_candidates.rs:9`, `content_allocations.rs:7-8`, `accounting.rs:92`, `shell_content/allocations.rs:95`.
  - `max_frame_payload` in `catalog_candidates.rs:119`.
  - `ShellOutbox` carries both kinds (`outbox.rs`, `file_kind: None` = socket frame).
  - This is the "Shell file contract" row of the purge table (`docs/sophia-shell-files.md:1007`): socket-shaped `Limits` fields and budgets must become per-record first.
- **Keep:** `ShellSessionTransport` and `ShellComponentTransport` themselves. They also carry the 9P path (`accept_files_with_content_policy`); only their socket branches go.
- **Tests (tests-only callers):** runtime `shell_transport.rs`, `shell_content_transport.rs`, `shell_content_admission.rs`, `shell_component_transport.rs`, `shell_component_reduced_limits.rs` (about 2.3k lines), and backend-live `tests/support/lifecycle_tests/content_peer.rs:63`.
- **Gate:** t252 (with t263).

### 4. Single-process metadata shell (`--shell-process`) and Narthex descriptor profile — C
- **Where:** `live_session/metadata_shell.rs:702-708` (wire branch) and `metadata_shell/{reference,tabs,launcher}.rs`. The 9P rule is enforced at `arguments.rs:618-620`: "descriptor profile has no file contract yet".
- **Clients:** Narthex (Nim) is the only descriptor client. `docs/sophia-shell-files.md:513-518` keeps it on the socket "until that profile moves to files".
- **Tools:** `examples/shell_descriptor_conformance_host.rs` (717), `shell_launcher_conformance_host.rs` (241), session `shell_indicator_conformance_host.rs` (150), and `tools/check_shell_protocol.sh` (xtask gate "shell-independent", `native_protocol_family.rs:174`; it needs a Narthex checkout).
- **Blocks removal:** the missing descriptor/tabs/shortcuts file contract, or an operator decision to drop Narthex (t252 names "Narthex rollback").
- **Live hazard:** the launcher passes `--shell-process-default`, so the argument must keep parsing, or the external launcher (sophia-niltempus-desktop `run_desktop_session.sh`) must change.

### 5. GPU content proof and content conformance host — B
- **Where:** `sophia shell-gpu-content-proof` has a required `--transport=current-ipc|9p2000.L` (`cli/commands/backend/gpu_proof.rs:101-105`) and branches in `metadata_shell/gpu_content_proof.rs:160-166`. `examples/shell_content_conformance_host.rs:27-32` defaults to IPC.
- **Change needed:** drop the current-ipc value, and move the check_shell_protocol content-host invocation and its test clients to 9P.
- **Test clients:** `vendor/c-desktop-sdk/source/src/tests/sophia_shell_content_live_client.c`, `sophia_shell_v1_client.c` and `sophia_shell_launcher_client.c` are IPC clients.

### 6. Output role (SOPHIA_OUTPUT_SOCKET, sophia_output_v1) — C
- **Where:**
  - `ipc/output_v1.rs` (669).
  - runtime `output_transport.rs` (412) and `output_ipc.rs` (208). `output_service.rs` (445) is the owner, and parts of it may be neutral (uncertain).
  - `packets/output*.rs`, `protocol/sophia-output-v1.kdl`, golden output frames, `docs/sophia-output-v1.md`.
- **Production:** `wm/public_policy.rs:500-508,804-807` binds it and exports it to the WM whenever `--native-scanout` is on (true live), regardless of WM wire. `profile_reload.rs:421` also uses it.
- **Consumers:** the only known one is `sophia-wm-demo live-mixed-output-proof`, per the plan ("the only current client is the in-repository proof client"). Whether Hagia reads it is **uncertain**.
- **Pinned string:** the WM file `api` advertises `output_transport=current_ipc` (`ninep/owner.rs:7`). The pinned C SDK checks that exact string (`vendor/c-desktop-sdk/source/src/wm_session/bootstrap.c:4`, `tests/wm_session_peer.h:215`).
- **Missing:** the 9P output role and its peer (t253).

### 7. Control bus / `sophia msg` (SOPHIA_CONTROL_SOCKET, sophia_control_v1) — C
- **Where:**
  - `ipc/control_v1.rs` (428).
  - runtime `control.rs`, `control/client.rs` and `control/transport.rs` (about 930).
  - `cli/commands/msg.rs` (151), `session/live_session/control.rs` (316).
  - `bindings/python/sophia_control_v1.py` and its test, `protocol/sophia-control-v1.kdl`, `tools/check_control_protocol.sh`, `docs/sophia-control-v1.md`.
- **Production:** live (`control host-admin`). The socket is also exported to host applications (`application_catalog/execution.rs:26`).
- **Missing:** the 9P administration export and CLI (t254). The C SDK `COVERAGE.md:27` also records the gap.

### 8. Metadata broker and portal (SOPHIA_BROKER_SOCKET, broker_v1, portal) — C
- **Where:** `ipc/broker*.rs` (647), `ipc/portal.rs` (265), runtime `broker_transport.rs` (259), `sophia-portal/src/socket.rs` (332), `session/live_session/metadata_broker.rs` (333), `cli/commands/runtime/brokers.rs` (`metadata-broker-serve`), `protocol/sophia-broker-v1.kdl`.
- **Production:** started whenever a WM process exists (`run.rs:429-433`), so it runs live. It is a Sophia-internal helper.
- **Missing:** no 9P plan. t255 only requires listing it as a remaining public IPC.

### 9. Vendored SDK IPC features — B, but only through upstream SDK releases
- **Rust SDK:**
  - `sophia-shell-ipc` (about 1.5k lines) and the optional `ipc-compat` feature of `sophia-shell-client` (its socket and wire modules).
  - `sophia-shell-protocol` uses `sophia-shell-ipc` only as a dev-dependency (parity tests `tests/shell_files.rs`, `shell_file_candidates.rs`).
  - Sophia enables `ipc-compat` only in **dev-dependencies**: `sophia-runtime/Cargo.toml:19`, `sophia-session/Cargo.toml:32`. `sophia-protocol` has a dev-dependency on `sophia-shell-ipc` for `tests/sdk_ipc_parity.rs` (244).
  - Root `Cargo.toml:84-86` has path deps. **The Sophia-side flags can be dropped now** once those tests are retargeted.
- **C SDK:**
  - `GNUmakefile:6` sets `WITH_IPC ?= 1` and builds `libsophia-desktop-ipc.a` (`src/shell_wire/*`, `src/sophia_wm_v1.c`), plus the `check-ipc` target.
  - The file libraries do not include the IPC headers (checked).
  - `tools/check_shell_c_wire.sh` runs the full SDK `make check` in `xtask check all` (`check.rs:75`).
- **Pinning:** the SDKs are pinned by digest (`vendor/*/manifest.json`, `xtask/src/{c,rust}_desktop_sdk.rs`). Their `same_contract` checks (`c_desktop_sdk.rs:65-89`, `rust_desktop_sdk.rs:18-40`) require byte-identical copies of `protocol/sophia-shell-v1.kdl`, `sophia-wm-v1.kdl`, `golden/sophia-wm-v1.records`, the shell golden frames, `bindings/c/sophia_wm_v1.{c,h}`, `docs/sophia-shell-files.md` and `docs/sophia-wm-files.md`. So deleting or editing those Sophia files needs a new SDK release and re-vendor, or a change to the mapping lists.

### 10. Docs
`docs/sophia-policy-ipc.md` (659), `docs/generated/*-wire.md` and the `configuration.md` shell-transport section are docs-only. They go with their surface.

### Recommended removal order
1. **Now, small:**
   - Remove the unused `sophia-wm-demo` dependency from sophia-session and sophia-cli.
   - Retarget the Sophia tests that use `ipc-compat` / `sophia-shell-ipc` / `sdk_ipc_parity` to 9P, then drop those dev-deps.
   - Add a 9P mode to the content conformance and GPU proof tools.
2. **Relocate the shared codecs:** move `ipc::wm_v1_records`, `policy_records`, `policy_scalars`, `IpcCodecError` and `decode_rgb` to a neutral module. Give `sophia-wm-files-v1.kdl` its own row layouts. Rename or move `PolicyRoleEndpoint` and `PolicyTransportError`.
3. **Neutralize the shell owners:** per-record budgets and typed outbox (no `SOPHIA_IPC_HEADER_LEN`, `max_frame_payload` or frame encoding in owners).
4. **After t250/t252 plus a published rollback recipe:**
   - Flip the defaults to 9p2000.L. Add `transport` lines to your `~/.config` profile.
   - Delete the `CurrentIpc` variants, the WM current-ipc worker and runtime policy IPC, the shell socket branch and `ipc::shell_*`/`wm_v1*`, the generator, the C bindings, the r3 archive, and the check scripts and gates.
   - Ship SDK releases without `sophia-shell-ipc`, `ipc-compat` and `WITH_IPC`, re-vendor, and prune the `same_contract` lists.
5. **Narthex descriptor profile:** needs a file contract or a decision to drop Narthex. Stop passing `--shell-process-default`.
6. **Output (t253):** also change the `api` `output_transport=` string, in step with the C SDK and Hagia.
7. **Control (t254).**
8. **Broker/portal:** separate design, outside this milestone.

**Rollback:** older releases under `/opt/sophia-niltempus-desktop/releases/*` and `~/.local/state/sophia/desktop-releases/20260918-*` are self-contained binaries, so deleting code in master does not break them. Their pinned Hagia, Lom and Bemenu IPC builds cannot run against a post-removal Sophia.

**Uncertain:**
- Whether the pinned Hagia build reads `SOPHIA_OUTPUT_SOCKET` or links `libsophia-desktop-ipc`.
- Whether Provlita is 9P-only.
- How much of `output_service.rs` is wire-neutral.

### Follow-up: the three survey questions (t274, 2026-09-27)

Source audit at Sophia `32aa3c0d7`, Hagia
`69f427abb0565d10c04dab302915396048252dcf`, and Provlita
`e52a7e051587c8f396328dc8843516ebdd83b287` resolves the uncertainties above.
The original survey remains a record of what was known at `2d69924a9`.

**Hagia does not consume the output socket or link the SDK IPC library.**
`src/hagia.nim:158-162` rejects the retired WM socket variable and selects
`SOPHIA_WM_9P_SOCKET`. There is no `SOPHIA_OUTPUT_SOCKET` reader in `src/`.
The sole C SDK build boundary, `src/sophia/desktop_sdk.nim:4-20`, explicitly
compiles the `nine_p`, `wm_files`, and `wm_session` C sources. It neither
compiles the legacy wire sources nor links `libsophia-desktop-ipc`.
The tracked `hagia.nimble` supplies no alternate IPC link, and
`tools/check_sophia_policy.sh` builds the SDK with `WITH_IPC=0`.
The pairing fixture's assertion that Sophia exports `SOPHIA_OUTPUT_SOCKET`
does not establish a consumer. Likewise, the SDK's check of the WM `api`
string `output_transport=current_ipc` validates an advertisement; it does not
open that endpoint. This is an audit of the pinned source/build inputs, not
a claim about every historical installed binary.

**Provlita is 9P-only.** `Cargo.toml:15-17` pins the standalone Rust SDK at
`ea9cf651` without `ipc-compat`; neither the manifest nor its lock contains
`sophia-shell-ipc`. `src/serve.rs:26-31` rejects the retired variable even
when empty, requires a nonempty `SOPHIA_SHELL_9P_SOCKET`, and calls
`ShellConnection::connect_files` at line 51. `tests/cli.rs` covers the
endpoint refusals, and the service tests use a scripted 9P peer. The
contributor gate at `e52a7e0` passed 25 Rust and 12 tooling tests with strict
clippy, including the signal/no-replay controls. Evidence is retained under
`~/.local/state/sophia/development-evidence/component-sigterm/`.
This does not close Provlita's separate production-export/live acceptance.

**The output service mixes reusable role behavior with a concrete socket
adapter.** Preserve these behaviors when implementing t253/t272:

- `output_service.rs` owns the optional-client worker, typed command/event
  channels, bounded accept/intake turns, latest snapshot, reply/settlement
  dispatch, reconnect epochs, and supervised-assignee replacement.
- `pause_acceptance` is a synchronous barrier across the replacement
  process's spawn-to-PID handoff. Removing it would reopen an authorization
  race; it is not an IPC framing detail.
- `output_ipc.rs::OutputConnectionState` owns revision/capability admission,
  unique transaction identities, one active proposal plus one replaceable
  queued proposal, promotion only after settlement, and disconnect custody.
  Its algorithm is reusable, but its public types and constants still name
  `OutputV1*`. Move those to role-owned types with the new contract.
- The concrete `OutputSessionTransport`/`OutputTransportError`, socket
  negotiation, frame send/receive calls, and write-error translation are the
  service's wire-specific adapter. Replace them with the accepted file
  export; do not delete the whole service or retain the socket underneath it.

The retained regression map is
`crates/sophia-runtime/tests/output_service.rs`: candidate/settlement exchange,
replacement hardware snapshot, departed readers, no-client stop, supervised
PID replacement, and acceptance pause during handoff. The state reducer has
separate tests in `tests/output_ipc.rs`. Port these assertions to the new
role, including negative controls, before deleting their socket fixtures.
This classification required source inspection only; no output-role or live
session test was run. The output role still needs its 9P contract and peer.

## Validation and remaining work

Each removal is gated by the existing checks (`cargo xtask check`, the SDK
`same_contract` digests, the native protocol-family gates) and, for default
flips, by a published rollback recipe. The inventory maps to these task IDs;
`todo.md` and the monthly completion files own their current state:

- t264: drop the unused `sophia-wm-demo` dependency (A).
- t265: retarget Sophia tests off `ipc-compat`, `sophia-shell-ipc` and
  `sdk_ipc_parity`, then drop those dev-dependencies (B).
- t266: 9P modes for the content conformance host and GPU content proof,
  then retire their `current-ipc` value (B).
- t267: move the shared WM codecs and policy scalar validation to a
  wire-neutral home and give `sophia-wm-files-v1.kdl` its own row layouts (B).
- t268: per-record budgets and a typed outbox in the shell owners (B).
- t269: flip the WM and shell transport defaults to 9p2000.L, then delete the
  WM and shell IPC paths, codecs, generator, bindings, archive, scripts and
  gates (B, after t250, t252, t267, t268).
- t270: SDK releases without `sophia-shell-ipc`, `ipc-compat` and
  `WITH_IPC`; re-vendor and prune the `same_contract` lists.
- t271: the Narthex descriptor profile and single-process metadata shell (C).
- t272: retire the output IPC and the `output_transport=current_ipc` API
  string after t253 (C).
- t273: a 9P design, or an explicit decision, for the metadata broker and
  portal (C).
- t274: settle the survey's uncertainties.

The administrative commands are already t254, and the per-role default and
compatibility retirement umbrella is t255.

## Connections

- [Migrate desktop roles to a daily-driver 9P control bus](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
  owns t250 and t252 to t255; this note is the concrete code inventory t255
  asks for.
- [Shell file contract](../../sophia-shell-files.md) records the purge table
  and the rule that shared codecs are relocated, not deleted.
- [WM file contract](../../sophia-wm-files.md) still states that omission
  selects current-ipc; that line changes with t269.

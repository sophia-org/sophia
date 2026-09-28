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

### Shared-code relocation, first slice (t267, 2026-09-27)

The first change moves scalar policy validation from `ipc/policy_scalars.rs`
to `policy_scalars.rs` and the common error definition/conversions to
`codec_error.rs::BinaryCodecError`. WM file payload errors now name that
neutral type directly. Legacy socket callers retain `IpcCodecError` as an
alias to the same definition; the neutral owner does not import that alias.
This preserves variant payloads, Debug output, golden bytes, and both strict
file validation and the socket adapters' two historical exceptions.

Validation on the change based on `d29f7ed84`: all 269 protocol tests passed,
including scalar, WM file, malformed/golden and SDK IPC parity tests. Strict
all-target protocol clippy, workspace formatting and the layout gate passed.
Evidence: `~/.local/state/sophia/development-evidence/ipc-retirement/`.
The first clippy run caught a duplicate re-export; the corrected run passed.
No wire contract or SDK snapshot changed in this slice. The row encoders,
generated capability/outcome constants, row-layout contract, and runtime
endpoint/error ownership still need relocation; t267 remains open. The full
workspace gate belongs to the completed relocation before its main merge.

### File-owned row layouts (t267, 2026-09-27)

The generated fixed-row codec now lives in `sophia-protocol::wm_rows` and
imports only neutral byte-cursor helpers and `BinaryCodecError`. The legacy
envelope module imports those rows. The file schema now owns the complete
ordinary and capability-gated row layouts, constants and aggregate maxima;
the generator reads them directly, with a separate compatibility comparison
against the frozen socket schema. KDL boolean literals use the standard
`#true` spelling so the whole file contract can be parsed by the generator.

The byte layouts are unchanged: generated C bindings, golden frame and row
corpora, and the legacy generated documentation have no diff. All 269 protocol
tests and five generator tests pass, including refusals for row width, gate,
capability, outcome, limit and revision drift. Strict clippy, formatting and
generator freshness checks pass. Evidence remains in `ipc-retirement/`.
The contract text changes require coordinated SDK snapshot updates before the
full combined gate or main merge; no SDK snapshot was patched in place. The
semantic codec and runtime endpoint/error relocations remain in progress.

### Semantic codecs and admission separation (t267, 2026-09-27)

The semantic snapshot, projection, configuration, presentation, tab,
translation, output-key and launch-context codecs now live in `wm_records`.
They import generated values from `wm_rows` and report `BinaryCodecError`.
Only the old Begin/Chunk/End adapters and their historical error ordering
remain in `ipc`. The combined protocol suite passes all 269 tests.

Peer admission is now `role_endpoint::RoleEndpoint`; the old discovery
environment names and compatibility aliases stay in `policy_socket` for
remaining adapters. The 9P Session worker uses the neutral endpoint directly.
The shared profile executor now accepts an adapter-owned error type through
`PolicyProfileHandoffIo::Error`, converting only neutral `PolicyProfileIoError`
failures. Its 9P adapter no longer imports `PolicyTransportError`. The socket
adapter retains its existing error variants and maps the shared failures back
to them. No admission, timeout, retry or profile-state rule changed.

The isolated runtime endpoint/profile/socket tests passed 22/22. Session's
9P worker tests passed 51/51 (three separately gated tests stayed ignored),
including protected peer admission and profile exchange. Runtime and Session
all-target/all-feature clippy passed. An initial Session filename filter ran
zero tests; that log is retained and is not test evidence.

A read-only mount overlay excluded the entire protocol `ipc` module and its
re-export. The production library compiled, proving the file codecs do not
need the legacy module. The probe reported unused compatibility helpers;
ordinary strict clippy remains clean. The file test binaries still pulled
socket constructors through a shared fixture, so this probe did not prove
independent test execution. Those fixture dependencies are being separated
without deleting their legacy assertions. Probe builds used the private
target; it was cleaned before the ordinary combined test run to avoid any
overlay fingerprint reuse. SDK contract alignment and the full combined
gate remain prerequisites to the main merge.

### SDK alignment and independent file tests (t267, 2026-09-27)

The contract copies now come from signed SDK revisions: C
`4a90120a268b1b4846338adeb3b55790f289cd94` and Rust
`aa388e2561bd73e07f6a4f1c46028813188ab029`, vendored by `4fdd033aa`.
Both snapshot checks pass. The C row generator reads the file-owned layouts
and checks compatibility with the old rows; its generated executable code
is unchanged. The standalone C gates pass with and without IPC, and the Rust
SDK gates pass with default and all features. These revisions retain the
legacy adapters; removing them remains t270.

The fixture split `75308a64d`, merged as `6e336f724`, moves legacy frame
construction into explicit IPC test companions. All old golden assertions
remain. With the protocol IPC module and its re-export excluded by a
read-only mount, all 43 selected file and neutral profile tests pass on the
combined branch. This probe uses a separate target directory. The ordinary
protocol suite at the fixture commit passes 272 tests (269 before the split,
plus two neutral profile tests and one relocated golden test).

The first combined workspace gate at `4fdd033aa` stopped in
`normal_session_lifecycle` with `RuntimeDirUnset`: the isolated runner had
cleared the environment without supplying a private runtime directory. The
corrected runner creates a private mode-0700 `/run/user/1000` and sets
`XDG_RUNTIME_DIR` to it. The failed test then passes without any product-code
change. The failed gate log is retained; a complete combined gate is still
required before promotion. Evidence is under `ipc-retirement/`, including
`t267-runtime-dir-control.log` and `t267-combined-noipc-6e336f724.log`.

The next full gate at `79c4dc876` reached the independent Go oracle and
stopped because its default build cache was inside the read-only checkout.
The helper now defaults to its caller-owned scratch directory, preserving
an explicit `GOCACHE` override. The isolated runner mounts the already
provisioned Go modules read-only and keeps networking disabled. Both oracle
tests then pass, including the independent client's checks against the
production export (`t267-go-oracle-readonly.log`). This is a test-harness
correction; the export and oracle assertions are unchanged.

### Combined relocation gate (t267, 2026-09-28)

Candidate `54a9aca0333387bbf7392e0c5f237dfae5f7bcc9` passes the full
`cargo xtask check` in the isolated, offline runner: 443 test-result groups,
6,396 passed, zero failed and 62 ignored, followed by strict clippy, snapshot,
layout and verifier checks. The terminal exit was zero. The complete log is
`ipc-retirement/t267-combined-full-54a9aca03-2.log` in development evidence.
The SDK dependencies were published first: C `4a90120a` on `origin/master`
and Rust `aa388e25` on `origin/main`; existing release tags are unchanged.

The preceding run passed the tests and clippy but stopped in the archive
self-test because the isolated home had no public signing key. The final
runner mounts a public-only keyring read-only. It neither accesses private
keys nor changes the signing agent. The failed log and the passing focused
archive control are retained alongside the final gate.

The fixed rows, semantic records, scalar validation, codec error, endpoint
admission and shared profile I/O now have neutral owners. The file contract
owns its row layouts; both generators check legacy compatibility separately.
The old C bindings and WM golden corpora remain byte-identical to `d29f7ed84`.
The independent file-module build and its 43 tests also pass without compiling
the IPC module. These results satisfy the relocation scope; the retained
socket adapters, their defaults and SDK compatibility features still await
the separate retirement tasks. No installed or running component changed.
The gate hides devices and has no real archive corpus, so it claims neither
hardware acceptance nor re-verification of operator archives.

### Content hosts and combined owner gate (t265/t266/t268, 2026-09-28)

Candidate `bf9ba4093f2db596392ae4c3f3c4d7c42d4e8405` passes the full
`cargo xtask check`: 470 test-result groups, 6,515 passed, zero failed and
62 ignored. Strict clippy, SDK snapshots, layout and tool verifiers also
pass; the terminal exit is zero. The log is
`ipc-retirement/t265-t266-t268-combined-full-settlement.log` in development
evidence. The runner is offline, device-hidden, with private targets, no
display or session sockets, and a read-only public signing keyring.

The content conformance host and `shell-gpu-content-proof` now serve only
9P2000.L. Their CLI rejects `current-ipc` and the retired socket environment
variable. The content portion of `check_shell_protocol.sh` builds the
independent C SDK peer without its IPC library and runs it against the real
export. It proves allocation, upload, candidate, renderer refusal, retirement
and release; malformed-record controls prove no owner delivery or custody on
refusal. The backend popout tests also have file twins covering stale parent
receipts, anchoring, input acknowledgement, invalidation and retirement. The
proof-loop tests run without a GPU and explicitly report
`native_presentation=false`. This satisfies t266's host and wire migration;
it does not claim GPU execution or a full Narthex protocol-family run. The
descriptor portion of that script remains part of t271.

The migrated protocol tests also pass with `pub mod ipc` and its re-export
hidden: 25 selected binaries, 163 tests. The log is
`ipc-retirement/t265-neutral-protocol-noipc-bf9ba4093-2.log`. SDK compatibility
dev-dependencies still build in that experiment; this proves independence
of those test bodies, not deletion of the dependency graph. The preceding
probe could not execute rustc inside its nested sandbox. Its failed log is
retained; the passing run uses the existing single sandbox with a read-only
source overlay.

Shell output ownership now uses typed records and native body charges.
Socket encoding lives in the socket adapter. Settlement includes accepted
file input, partially consumed candidates and socket publications waiting
outside the FIFO; accounting counts the latter once without charging
admission twice. The settlement follow-up `fb987d9c5` has seven assertion-killed
mutants and 1,431 focused tests. Evidence is in
`ipc-removal-inventory/t268-settlement-followup.md` and `t268-logs/`.

The vendored SDKs are C `1526a30bec4dacb19e7e285f3c1ada0704472fdc` and
Rust `f92902e434f347d3d0dab1ce50b2c85a0cee9414`, both published before this
checkpoint. Native uploads use `max_chunk_bytes` directly; existing Limits
validation and wire layout remain unchanged. The removed frame-derived
minimum was equal on every admitted Limits, so restoring it is not a failing
mutation. SDK validation now also enforces welcome bounds and distinct
persistent catalog identities. Maximal-catalog fixtures were corrected to
use distinct equal-length identities; their size and acknowledgement
assertions remain unchanged. The initial combined test failure and the
17-test export retest are retained in development evidence.

This checkpoint does not close t265 or t268. Descriptor families still need
file records. A newly confirmed Rust SDK defect validates ResourceBegin's
chunk count against prototype Limits rather than the negotiated grant;
reduced-chunk upload tests and a fix are being developed separately. The
socket ordering counter's overflow and optional EINTR retry also remain
reported. No installed or running component changed, no hardware acceptance
was run, and no operator archive corpus was available to re-verify.

### Negotiated resource upload validation (t268, 2026-09-28)

The reduced-chunk defect above is fixed in Rust SDK
`c1323401b7e336606408499b13097d1270a2319d`. The file codec checks the
description's structural bounds. The content owner checks the exact chunk
count against the admitted grant. The retiring socket codec retains its
previous prototype-layout check; no wire field or normative contract changed.

Before the fix, the new real-export regression failed in the SDK's enqueue
path with `InvalidRecord("content resource chunk count")`, before sending
the upload. After re-vendoring, two 4 MiB resources uploaded using 32 KiB
chunks and retained the expected pixels and accounting. A separate real-export
test sends the prototype count under reduced limits: the owner refuses it
without reserving staging, then admits the correct count on the same connection.

The SDK's default and all-feature suites passed 566 tests. Strict clippy in
both configurations, formatting and all 13 spec digests passed. Sophia's
focused gate passed six reduced-limit tests, all nine B6c custody tests, the
vendored SDK suites, focused strict clippy, layout and formatting. Evidence:
`ipc-retirement/t268-reduced-upload-production-before.log` (expected red),
`t268-reduced-sdk-full.log`, `t268-reduced-sdk-final.log`,
`t268-reduced-sdk-checks.log`, and `t268-reduced-upload-production-after2.log`.
The first SDK clippy run found a test type-complexity warning; the follow-up
uses a named type. One spec check ran from the wrong directory; the corrected
check passed. The first Sophia launcher invocation lacked script execute
permission and ran no tests; the explicit bash invocation passed.

This is deterministic SDK/export evidence, with devices, display and network
hidden. It does not claim a live component update or completion of t268's
remaining descriptor work.

### Output-owner isolation and combined gate (t268/t272, 2026-09-28)

The output owner now retires only the offending connection after a malformed
frame, leaving the listener available for the admitted process to reconnect.
Session profile reloads use the output owner's epoch and retain their private
transaction identity through settlement. They no longer send an outcome to an
output client that did not submit the reload. Signed commits `a5ebb4b49`,
`d88b13d41` and `834cfc693` were reviewed and merged at
`1669b308545024731120339729260e633382cdbd`.

The focused gate passed eight runtime and three Session tests, strict clippy,
layout and formatting. Three mutations restored the old behaviors and each
failed its regression: listener loss, unsolicited client settlement, and a
reload rejected when WM and output epochs differ. The first epoch mutation
invocation reused a cached binary and is not evidence; the second forced a
rebuild and failed the assertion as intended. Logs are
`ipc-retirement/t272-output-owner-focused.log`, `t272-mutant-malformed.log`,
`t272-mutant-settlement.log` and `t272-mutant-epoch2.log`.

The full `cargo xtask check` at `1669b3085` exited zero: 476 test groups,
6,538 passed, zero failed and 62 ignored, followed by strict clippy and the
offline tool verifiers. The private sandbox hid devices, display and network.
Device pixel proofs were explicitly unproved and the direct-scanout archive
corpus was absent. Evidence: `ipc-retirement/t268-t272-combined-1669b3085.log`.

The SDK upload fix was also adopted and published by Lom `2fb451d5` and
Provlita `1444ebb1`, both pinned to Rust SDK `c1323401`. Their full offline
checks passed (64 Rust and 17 tooling tests for Lom, with one ignored Rust
test; 26 Rust tests for Provlita), including formatting and strict clippy.
Their logs are `t268-lom-c132340-check.log` and
`t268-provlita-c132340-check.log` under `ipc-retirement/`.

These results do not implement the output file role or complete t268/t272.
The descriptor replacement, other output-owner findings and output acceptance
remain. No installed component, live process or display configuration changed.

### Descriptor replacement proposal (t271, 2026-09-28)

The [descriptor file proposal](../decisions/4oapm903-carry-descriptor-families-as-native-shell-file-records.md)
records the full retained feature set, native byte layouts, family-specific
validation, capability dependencies and proposed owner outcomes. Its KDL
fragment passes an independent parse/offset/size check and three malformed
layout controls. Review corrected the descriptor prefix arithmetic, preserved
the distinct text rules, retained indicator access without content admission
and included those feeds in snapshot accounting. These are design checks;
the production export, both SDK roles and independent commit-boundary peer
remain required. The current descriptor socket and its tests stay in place.

The first implementation slice moves the passive descriptor, tab, reference,
shortcut and revision-4 launcher model into Rust SDK
`9fafa9176f8c828f2aad6fa2855964fd36101dd9`. Sophia's historical packet paths
re-export those same types. Socket validators delegate to the SDK while
retaining their public error facade; frame encodings and contract copies are
unchanged. Tab validation no longer constructs a temporary standalone snapshot
per row, and its total-entry check cannot overflow before comparing the cap.

The SDK's default and all-feature suites passed 594 tests, with strict clippy
in both configurations and formatting clean. Fourteen new model tests exercise
boundary values, selection and reservation coherence, scoped action identity,
different text rules and outcome epochs. Independent mutation runs rejected
an accidental sixteen-row tab cap, reuse of launcher text rules for descriptor
labels, and removal of the action's target-generation check. They used separate
targets and read-only source overlays; each compiled and failed its assertion.

Sophia's existing protocol suite passed 325 tests, and eight Engine suites
passed another 53, including descriptor presentation, tabs, work areas,
reference sheets and the launcher. Strict clippy for protocol/Engine, the
vendored SDK gate, formatting and layout passed. The first follow-on command
named a nonexistent Engine test target; the corrected invocation passed its
tests, then clippy found an obsolete import. Removing that import completed
the checks. No test assertion or socket vector was changed.

Evidence: `ipc-retirement/t271-sdk-model-{focused,full}.log`,
`t271-sdk-model-mutant-{tab-cap,label-rule,target-generation}.log`, and
`t271-sdk-model-sophia-{focused,focused2,checks}.log`. This slice establishes
shared types and validation only. Native descriptor file codecs and the
production export remain to be implemented; no descriptor client is switched.

The combined Sophia candidate `7c807603e58272791bbf4af2747363fcb60284ed`
then passed the full offline `cargo xtask check`: 480 test-result groups,
6,566 passed, zero failed and 62 ignored, followed by strict clippy, SDK
snapshot, layout and verifier checks. The terminal exit was zero; the log is
`ipc-retirement/t271-sdk-model-combined-7c807603e.log`. The source remained
clean during the gate. Its sandbox hid devices, networking, displays and live
session sockets. No installed component changed, and no hardware or operator
archive acceptance is claimed.

### Descriptor file client and export custody candidate (t271)

Rust SDK candidate `33e01b3d4102350dbc6c7918de22922e07ab8402` implements
the seventeen proposed native values and envelopes and the descriptor client
role. The client checks the admitted role separately from capability bit 0,
holds acknowledgements until snapshot reads finish, and uses the existing
typed submission/custody lane. Metadata-only readiness never fetches Limits;
combined content admission binds both Limits epochs to the attach. The
default and all-feature suites pass 664 tests in total, with strict clippy and
formatting clean. Three compiled mutants fail the role, unfinished-read ack
and Limits grant-epoch assertions. The published contract copies are unchanged;
the layout proposal remains a separate file and the SDK branch is unpublished.

Sophia `70a0c9aa63bf8837f54c82be17791edba6011e30` imports that exact signed
snapshot. `bf78aa404a3cf48d65d94c3b96101fb19cd7261c` adds the private export
support: capability-gated Descriptors, Tabs and Shortcuts nodes, immutable
pins, typed descriptor candidate custody and validated event publication. The
qid span grows to 32 to cover the fixed nodes without overlapping allocations.
Refusals leave the qid, journal and submission watermark unchanged. The
export fixture controls saturation and pinned-object replacement directly;
its narrow private mount is documented in the style guide.

The focused run passed 35 runtime tests (seven new export controls, five
journal tests, twelve file-transport tests, nine B6c tests and the existing
output-custody and C-role tests), plus the 664 vendored SDK tests and runtime
strict clippy. That run stopped at layout because the new private mount was
not yet recorded. After documenting the mount, the twelve file-owner tests,
strict clippy, layout, formatting and diff checks passed. A compiled export
role-guard mutant admits a bar's descriptor candidate and fails the exact
EACCES assertion. Failed and successful logs are retained under
`ipc-retirement/t271-{sdk-descriptor-*,descriptor-export-*}`.

These commits do not expose descriptor admission through Session, route the
new typed inputs into the existing presentation and action owners, or reserve
the proposed descriptor snapshot footprint. Those changes and an independent
C SDK peer must precede acceptance of the contract and retirement of socket
tests. Neither candidate is published or installed, and this evidence makes no
claim about the running desktop or independent descriptor interoperability.

The next candidate, `cabedd5880f1db2628345a8981d1af081154e68e`, adds explicit
runtime descriptor-file negotiation. It selects the descriptor API role and
352-byte journal sizing before accepting the peer, admits revisions 1–8 with
only requested optional families, and rejects attempts to reuse native-launcher
or persistent-catalog stores. The Rust SDK connects to the production export
for both metadata-only and combined content grants. The raw 9P control also
checks that content names are hidden before and after metadata-only negotiation.
Protection identity is supplied for the test process; this is not a protected
child launch or a Session profile-selection test.

The gate passes 29 file-path tests (seven new admission tests, twelve existing
file-transport tests, nine B6c tests and the existing C-role test), followed by
strict runtime clippy, layout, formatting and diff checks. Another sixteen
existing transport/negotiation tests pass. A compiled mutation lowering the
descriptor revision ceiling to 6 fails the revision 1–8 bootstrap control.
Logs are `ipc-retirement/t271-descriptor-admission-{gate,legacy,mutant-revision}.log`.
The production Session still does not select this role. Descriptor footprint
reservation, neutral owner routing, presentation-commit evidence and the
independent C peer remain required before the socket can be retired.

### Descriptor file presentation owner candidate (t271, 2026-09-28)

Sophia candidate `3f762e8cc` moves descriptor exchange state out of the socket
adapter and routes descriptor file candidates, outcomes, activations and
acknowledgements through the shared owner. A snapshot request reserves two
response credits before publication. Prepared transfers one credit; Presented,
Rejected or Superseded ends the exact request and releases the remaining
obligation. A stale transaction receives a rejection without stealing another
request's credits. Stale activation acknowledgements are consumed and counted
without discharging the current activation.

The production 9P export and Rust SDK pass six new owner tests: preparation
does not authorize activation; presentation does; a prepared replacement keeps
the previous presentation eligible until the terminal outcome; a presented
withdrawal makes both generations ineligible; stale generations, wrong outputs
and descriptor generations reject without an action; exact response credits
survive shared content-budget pressure and are released on rejection or
disconnect. Indicator admission uses the selected wire's control-credit size.

The focused final gate passes 83 tests, strict runtime clippy across all targets
and features, layout, formatting and diff checks. The existing socket,
negotiation, content-credit and B6c controls remain green. Two compiled mutations
fail their named assertions: allowing activation on Prepared, and omitting
descriptor credits from the shared content budget. Logs are
`ipc-retirement/t271-descriptor-owner-final.log` and
`t271-descriptor-owner-mutant-{prepared,credits}.log`. Mutants use read-only
source overlays and a separate target; they do not change the working tree.

This proves transport-owner behavior with supplied process protection identity,
not an Engine work-area commit or a protected Session launch. The production
Session selector, selected snapshot footprint reservation, tabs/reference and
launcher owner routing, independent C SDK peer and descriptor client migration
remain required. The proposal and SDK candidate remain unpublished; descriptor
IPC has not been removed.

### Selected snapshot custody (t271, 2026-09-28)

Sophia candidate `7401f8115` records each file export's encoded snapshot
ceiling when negotiation selects its feeds: two copies of each disclosed
feed's cap plus one shared 4 MiB build scratch. It derives the union from the
same root vocabulary used for lookup, including `outputs` on a combined
descriptor/content connection and `indicators` only when selected. The
content registry's existing storage reservation remains separately accounted.
This ceiling is a per-component protocol bound; it does not preallocate heap
memory or establish a process-wide RSS limit.

The export observes pinned objects through weak references while open fids
retain ownership. Accounting counts a current object shared with its pin once,
and an older pin plus a replacement separately. Replacing the unpinned current
object releases that predecessor; closing the old fid releases its pin. Both
snapshot fields become zero after transport disconnect, and quiescence now
includes them. Publication refuses every undisclosed feed before decoding or
allocating a replacement, including a bar's unselected catalog, and checks
the whole encoded cap before creating the new buffer.

The final focused gate passes 89 tests, strict runtime clippy, layout,
formatting and diff checks. Fourteen additional launcher, dock, indicator,
C-role, socket-publication and output-custody controls pass. Logs are
`ipc-retirement/t271-descriptor-snapshots-{final,roles}.log`. Compiled mutations
omitting indicator capacity and old-pin retention fail the expected assertions
(`t271-descriptor-snapshots-mutant-{indicators,pin}.log`). The initial invocation
used the nonexistent test target `shell_files_transport` and stopped before
building; its log is retained. The corrected target is `shell_file_transport`.

The typed tabs/reference/launcher owner paths, production Session selector,
independent C peer and descriptor client migration remain open. No descriptor
IPC was removed and neither candidate branch was published.

### Typed tab presentation and acknowledgement routing (t271, 2026-09-28)

Candidate `e14706b22` gives tabs their own snapshot, candidate, presentation
and activation state in the shared transport owner. File publication reserves
Prepared and terminal response credits before exposing a snapshot. Replacing an
unanswered snapshot reuses those credits; a candidate already handed to Session
must receive its terminal outcome first. Stale snapshot identities, reordered
groups, non-increasing generations and the reserved generation bit receive
Superseded without replacing a newer request. Metadata and combined content
connections account for tab credits alongside descriptor credits.

Session's tab path now uses typed transport methods. A scene change revokes tab
interaction without emitting a second terminal outcome for an already Presented
candidate. Base descriptors and tabs share one acknowledgement record kind;
the transport preserves acknowledgements belonging to the other owner and
consumes only an exact pending transaction/activation pair. Unknown pairs are
counted and cannot authorize an action.

The final runtime gate passed 99 tests and strict runtime/Session clippy across
all targets and features, layout, formatting and whitespace checks. Its Session
test filter initially selected zero tests because `native-session` was absent;
the corrected invocation passed 94 metadata-shell tests. Logs are
`ipc-retirement/t271-tabs-owner-final.log` and
`ipc-retirement/t271-tabs-session-native.log`. These 193 tests do not establish
an actual protected Session tab scene commit over the new file role.

Compiled mutations removing the reserved-generation check and allowing one
owner to discard another owner's acknowledgement failed their named controls
(`t271-tabs-mutant-high-bit.log` and
`t271-tabs-mutant-ack-owner-rebuilt.log`). The first acknowledgement mutation
attempt reused a cached artifact and is not counted as evidence. Cleaning only
the private mutant target's runtime package forced compilation of the recorded
overlay, after which the control failed. Repository sources were unchanged.

Reference and launcher owners, production descriptor role selection, the
independent C SDK peer and Narthex migration remain unfinished. This candidate
is unpublished; no live session was changed and t271 remains open.

### Broker and portal survey correction (t273, 2026-09-28)

The production statement in section 8 applies to the metadata broker only.
The portal socket server and the X clipboard coordinator are exercised by
tests; production does not bind a portal endpoint or admit multiple X
namespaces. Broker and portal file-contract drafts are under review, with
separate admission and no new authority inferred from a path or attach.
They are not implemented exports. Existing wire values and implemented
operations must be retained; absent portal executors must not be described
as live capabilities.

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
  gates (B, after t250, t252, t267, t268 and t271). The descriptor shell still
  uses the socket adapter, so its file replacement must precede deletion.
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

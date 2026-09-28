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

### Typed reference-sheet custody (t271, 2026-09-28)

Candidate `c0ed7ab3b` adds typed shortcut publication, reference requests,
candidate intake and presentation outcomes. Admission reserves the request
record and both response credits before transfer. A request requires the
published catalog generation; replacement cannot discard an outstanding
request or candidate. Disconnect and negotiation reset the reference owner,
and all shared queue checks include its credits.

The file owner validates transaction, catalog, request, output identity and
increasing candidate generation. A cancelled request receives Superseded even
when its reply is stale; other stale replies receive Rejected. A refusal for
another transaction cannot consume the current request's response capacity.
Session receives an explicit refusal event so it ends only the corresponding
wait. Prepared and terminal outcomes must echo the exact candidate identity;
Presented requires Prepared and a second terminal outcome is refused.

Session's reference path now uses typed methods, retaining its output-generation
and projection checks. Invalid current output or failed projection returns
Rejected before presentation. The existing pending-configuration invalidation
and disconnect paths still need review with the production descriptor selector;
this slice does not claim a protected Session reference presentation proof.

The isolated gate passed 105 runtime tests and 94 native Session metadata tests,
strict runtime/Session clippy with all targets/features, layout, formatting and
whitespace checks (`ipc-retirement/t271-reference-gate.log`). Six new tests use
the actual Rust SDK and production file export. Compiled mutations reserving
only two records instead of three and suppressing cancellation each failed
their named control (`t271-reference-mutant-{capacity,cancel}.log`). Each
mutation used a read-only overlay after cleaning the runtime package only in
the private mutant target. The source tree was not modified by those controls.

Launcher ownership, production descriptor selection, independent C SDK peers
and Narthex migration remain unfinished. The contracts remain proposed, the
candidate is unpublished and no live session changed.

### Typed descriptor launcher custody (t271, 2026-09-28)

Candidate `07d3e2070` routes the descriptor launcher's catalog, requests,
candidates, presentation outcomes, activation acknowledgements and launch
outcomes through typed transport methods. The application worker, launch
queue, filesystem verification and process admission remain Session-owned.
The launcher catalog uses the existing bounded publication owner; Session
waits for a socket publication to enter the output order before issuing a
request. File catalog publication remains one atomic snapshot announcement.

The file owner reserves the request plus Prepared and terminal responses
before transfer. Candidate identity, output, increasing generation, visibility
and membership in the catalog are checked before handing a candidate to
Session. Stale or revoked replies receive Superseded without consuming an
unrelated request. Session retains query/output-generation and projection
checks; projection refusal produces Rejected without presentation.

Only an exact Presented candidate permits activation. Activation reserves its
own event and a LaunchOutcome credit; the credit survives acknowledgement and
Session verification. Acknowledgements must echo the complete grant and
transaction, and duplicate or stale acknowledgements are counted without
releasing the pending grant. Unconsumed or revoked grants cannot report Started
or Failed; Rejected settles them. Cancellation revokes interaction but retains
terminal obligations until settlement or disconnect.

The isolated gate passed 112 runtime tests and 94 native Session metadata
tests, strict clippy for runtime/Session across all targets/features, layout,
formatting and whitespace checks (`ipc-retirement/t271-launcher-gate.log`).
The existing C role and socket publication parity suites added five passes
(`t271-launcher-publication-parity.log`). Seven new tests drive the real Rust
SDK and production export, without launching applications. Compiled mutations
accepting a partial acknowledgement or allowing Presented before Prepared
failed their named controls (`t271-launcher-mutant-{ack,prepared}.log`).

Initial failures are retained: `t271-launcher-check2.log` found a Vec where
catalog identities require a BTreeMap; `t271-launcher-focused.log` found a test
helper assuming two total credits in a case correctly retaining three. The
helper was replaced by explicit combined-accounting checks, including refusal
when activation and descriptor credits would exceed capacity. The first check
log preceded module wiring and is not counted as launcher compilation evidence.

Production descriptor selection, protected presentation/action acceptance,
independent C SDK peers and Narthex migration remain incomplete. No task is
closed by this slice, nothing is published and no live session changed.

### Protected descriptor profile selection (t271, 2026-09-28)

Candidate `9a4d0f184` adds the explicit single-shell development selection
`--shell-transport=9p2000.L --shell-file-profile=descriptor`. The default file
profile remains content. Configuration refuses a file profile without a single
9P shell and refuses unknown profile names. Descriptor selection admits
metadata-only negotiation without Limits and permits combined content only
through the existing content policy. Independent components retain their own
role selection. The CLI help and configuration guide describe the proposed
contract's current acceptance limits.

Session selects the descriptor admission method before negotiation and retains
the normal protected launch plan, endpoint environment, supervisor and deadline.
The new fixture executes a real SDK peer under that plan, with only its test
harness arguments replacing `--serve`. It checks that the child has the 9P
endpoint, no retired endpoint, no display variables and no DRM devices. The
parent negotiates revision 8 without content, receives a native descriptor
candidate, ends that epoch and launches a new child at the next epoch. A
content-profile control refuses the same descriptor peer. This is protected
startup and transport evidence, not an Engine presentation/work-area commit or
application launch proof; the parent drives transport directly after startup.

The final isolated gate passed 53 configuration, 96 native Session metadata and
50 runtime tests (199 parent tests; nested child summaries are not counted).
Strict clippy across config/runtime/Session/CLI with all targets/features,
layout, formatting and whitespace checks passed. Evidence is
`ipc-retirement/t271-selector-gate2.log`. The separate `component_reconnect`
filter selected zero tests, but those eight tests ran under their actual
`metadata_shell::component_session::reconnect_tests` names in the metadata run.
The compiled selector mutation admitting a descriptor under the content profile
failed the explicit refusal assertion (`t271-selector-mutant-role.log`).

Earlier fixture failures are retained: the focused run unwrapped PeerClosed
during cleanup, then its cleanup edit accidentally removed SDK I/O polling
and caused a candidate timeout in `t271-selector-gate.log`. The final fixture
polls I/O, handles only PeerClosed as a normal end and panics on other errors.
Nothing was retried against a live session. Independent C descriptor peers,
protected presentation/action acceptance, pending reference invalidation review
and Narthex migration still precede t271 completion and IPC deletion.

### Independent C descriptor controls (t271, 2026-09-28)

C SDK candidate `fb2b701136363b7b223525bce53bb82b58708d8e` implements the
eight descriptor control events and two activation acknowledgements. It pins
the proposed KDL and ADR from Sophia `0cbb7ea5b` under `spec/proposed/`, with
separate checked digests. Published contract copies and compatibility claims
remain unchanged. This is the first C implementation slice, not descriptor
session support or contract acceptance.

The codec uses native file envelopes and passive typed values. It checks the
body/envelope epoch, nonzero identities, exact size, reserved fields, query
UTF-8/control/bidi rules and per-family outcome relationships. Descriptor
outcomes require zero presentation epoch except Presented; reference and
launcher outcomes preserve their specified non-Presented epoch allowance.
Activation acknowledgements validate their shape without creating authority.
Their Submitted receipts are recognized; unimplemented descriptor kinds still
refuse. Encoding and decoding leave caller output unchanged on failure.

Evidence under `development-evidence/ipc-retirement/`:

- `t271-c-descriptor-controls-check1.log` and
  `t271-c-descriptor-controls-final.log`: full C SDK strict checks pass with
  `WITH_IPC=0` and `WITH_IPC=1`. The new test covers ten independent literal
  vectors and 1,295 malformed-wire refusals, plus invalid-value and capacity
  checks. Vectors come from the proposed file layout, not a Rust encoder or
  old socket corpus.
- `t271-c-descriptor-mutant-epoch.log` and
  `t271-c-descriptor-mutant-boolean.log`: separately compiled read-only
  overlays accepting mismatched epochs or `consumed=2` each fail assertions.
  Fresh private build directories prevent reuse of unmutated objects.
- `t271-c-descriptor-controls-ubsan-trap.log`: the focused test passes with
  undefined-behavior instrumentation in trap mode. The preceding
  `t271-c-descriptor-controls-sanitized.log` failed at link because this host
  lacks `libasan_preinit.o`, `libasan` and `libubsan`; no address-sanitizer
  result is claimed and no packages were installed.

All builds ran with hidden devices, no network/display, read-only source,
private outputs and nice 19/j2. The C SDK branch is signed but unpublished;
Sophia is not re-vendored yet. The three whole objects, four presentation
candidates, descriptor negotiation and independent C production-export peer
remain next. Protected Engine presentation/work-area acceptance and Narthex
thin bindings are also outstanding. No socket path is removed by this slice.

### Independent C presentation candidates (t271, 2026-09-28)

C SDK candidate `aa57dcd803cb380d26aadaf4d8a5e15bf5abd956` adds native
DescriptorCandidate, TabsCandidate, ReferenceCandidate and LauncherCandidate
records and their Submitted kind values. The proposal wording is clarified
at Sophia `348dee082`: a hidden descriptor candidate has no entries, while
distinct slots may have equal generations. The SDK pins that revised prose;
the layout fragment is unchanged and the contract remains proposed.

Descriptor and launcher candidates use bounded inline arrays. Tab order and
reference rows borrow immutable encoded storage, with public row helpers.
Tab uniqueness uses fixed scratch and heapsort without changing the requested
order; reference uniqueness uses a bounded slot bitmap. Neither codec decides
whether a generation is fresh, a page is an owner's projection, an activation
is authorized or a candidate has been presented. The tab high-bit generation
remains a semantic owner refusal, not a malformed record. Hidden/empty
launcher and reference candidates keep their less restrictive value rules.

Evidence in `development-evidence/ipc-retirement/`:

- `t271-c-candidates-check1.log`: full strict C SDK checks pass without IPC.
- `t271-c-candidates-final.log`: full strict checks, including compatibility,
  pass after the additional complete one-past-bound controls. Four independent
  literal vectors, 867 malformed-wire refusals and invalid-value tests cover
  exact maxima (16 descriptor entries, 1,024 tab groups, 256 reference entries,
  32 launcher rows), full extra rows, duplicates, selection, reservations,
  style/text bounds, borrowed lifetimes and unchanged output on errors.
- `t271-c-candidates-ubsan-trap.log`: candidate and control tests pass with
  undefined-behavior instrumentation in trap mode. Address-sanitizer support
  remains unavailable as recorded above.
- `t271-c-candidates-mutant-hidden.log` and
  `t271-c-candidates-mutant-duplicate.log`: fresh compiled overlays admitting
  hidden descriptor entries or duplicate tab groups each fail assertions.

The earlier control suite now reports 1,291 rather than 1,295 malformed-wire
refusals: four candidate kinds moved out of its unknown-kind list into this
suite's typed coverage. Its ten literal vectors and remaining controls pass.

The C SDK branch is signed, clean and unpublished. Its existing 8 KiB client
staging area still cannot submit maximum tab/reference candidates; this slice
does not claim client support. The three whole snapshot objects and descriptor
negotiation/storage integration remain before the independent C export peer.
Protected presentation/work-area proof and Narthex migration remain open. All
tests were isolated from devices, display, network and the running desktop.

### Independent C descriptor snapshot codecs (t271)

C SDK candidate `0f3f4e40dca8dd2b757040dfd0869e8e4d0db88d` adds the three
whole snapshot objects, completing passive codecs for all seventeen proposed
descriptor envelopes. The proposal remains unaccepted. Sophia
`731c5295bb2bf5bc875a1704a27f76a9e44e5086` states shortcut uniqueness and
mandatory chord/action text explicitly; the copied layout is unchanged.

Snapshots borrow immutable row storage. Validation covers action/connection
bindings, selected entries, distinct slots across tab groups, group partition
totals, optional shortcut fields and text rules. Descriptor labels forbid
Unicode controls but retain bidi characters; shortcut text has the stricter
rule. Complete maximum and one-past objects exercise the global row bounds,
including 1024 tab groups and 2048 entries together. No heap allocation or
quadratic uniqueness scan was introduced.

Evidence in `development-evidence/ipc-retirement/`:

- `t271-c-objects-check1.log` and `t271-c-objects-final.log`: strict checks pass
  without and with IPC respectively. Three independent literal snapshots and
  1379 malformed-wire refusals pass, alongside the existing suites. The control
  suite now has 1289 refusals because newly supported object kinds moved out
  of the unknown-kind list; its ten literal vectors are unchanged.
- `t271-c-objects-ubsan-trap.log`: the full file suite passes with undefined
  behavior traps enabled.
- `t271-c-objects-mutant-bidi.log`: incorrectly rejecting bidi in descriptor
  labels fails the positive row assertion.
- `t271-c-objects-mutant-cross-group.log`: clearing slot history between tab
  groups fails the duplicate-slot refusal.
- `t271-c-objects-mutant-ack.log`: accepting descriptor announcements in the
  existing content client fails its unsupported-publication refusal. That
  client has no descriptor fetch holds yet, so it must refuse before advancing
  consumption or acknowledgements.

Each mutant compiled in a fresh private target with a read-only source overlay
and exited 134 at the intended assertion. All gates hid devices, network and
display access. The signed SDK branch is unpublished. Descriptor negotiation,
large candidate staging and feed acknowledgement holds remain client work;
the independent C production-export peer, protected Engine work-area commit
proof and Narthex migration are still required before retiring IPC.

### C file client transaction storage (t271)

C SDK `0a09c298fd36d7d09c0f23a35116985deadd8c4e` adds an initializer with
caller-owned object and transaction scratch. Existing initializers keep their
inline defaults. External transaction storage is bounded at 4 MiB and checked
for overlap with the client, wire, wire storage and object scratch before
initialization changes state. No allocator or extra inline record array was
added. The queued session still has its 8 KiB per-record limit; this change is
the low-level transport prerequisite for descriptor client work.

The scripted peer receives complete 8260-byte tabs and 52488-byte reference
candidates through both value and encoded-byte submission, with 127-byte
writes. Changing the caller's input immediately after submission does not alter
the staged record. Submitted custody holds the transaction reopen until its
acknowledgement; EAGAIN waits for explicit retry and retains the same id and
bytes. Undersized, malformed and wrong-epoch submissions consume no id.

`t271-c-staging-check1.log` and `t271-c-staging-final.log` pass full strict
checks without and with IPC. `t271-c-staging-ubsan-trap.log` passes the full
file suite. `t271-c-staging-mutant-capacity.log` compiles a fresh overlay with
the old fixed-size encoding limit and exits 134 at the maximum-candidate
submission assertion. Device/network/display isolation and private outputs
match the snapshot gates above. These tests do not establish descriptor role
authorization or real-export interoperability. Negotiation, feed holds, queued
large records and the independent C peer remain unfinished.

### C descriptor negotiation and snapshot custody (t271)

C SDK `4816e5e9c275a30efca2a218a427c5ab6c2e2a8e` adds the development
descriptor profile. The api must explicitly name that role; an ordinary bar's
bit 0 remains inert. Offers obey the proposed revision/dependency table, and
the welcome must select exactly the required bits plus reservation bit 1.
Metadata readiness waits for consumed bootstrap Submitted and Negotiated
without reading Limits. Combined content also requires valid Limits; a failed
bootstrap fetch fails the session. Validated welcome accessors expose the
selected revision and capabilities without changing role authority.

The client refuses unselected objects, events and submissions before
consumption or local admission. Metadata catalog reads reject persistent
identity disclosure. Descriptor, tab and shortcut feeds join the existing
per-kind fetch holds. Partial reads, matching qid/generation, complete decode
and a separate EOF probe precede release. Superseding publications preserve
the earliest outstanding acknowledgement bound. Queued candidates stay local
while those holds prevent acknowledging earlier Submitted custody.

The disconnect scan now uses the same disclosure check as ordinary intake.
A buffered Submitted after an undisclosed event cannot establish custody;
the paired positive control still recognizes Submitted behind a valid event.
This preserves UnknownDisconnected rather than inventing custody from bytes
the negotiated role could not consume.

Evidence in `development-evidence/ipc-retirement/`:

- `t271-c-profile-final4.log` and `t271-c-profile-final5.log`: full strict
  checks with and without IPC pass. The latter follows formatting only.
- `t271-c-profile-ubsan-trap2.log`: full file suite passes.
- `t271-c-profile2-mutant-{readiness,holds,disclosure,buffered}.log`: fresh
  compiled overlays each exit 134 at the intended assertion when bootstrap
  custody is skipped, descriptor holds are omitted, unrequested feeds are
  disclosed, or the disconnect scan crosses an undisclosed event.
- Earlier failed `t271-c-profile-check{1,2,3}.log` files are retained: a test
  initializer lacked braces; the malformed-welcome fixture tried to encode an
  invalid value instead of injecting malformed bytes; and the hidden-candidate
  fixture incorrectly set a reservation edge. No assertion was weakened.

All gates used private outputs, hidden devices/network/display and nice 19/j2.
The SDK branch remains signed and unpublished. Its queued session still needs
large record support; only the low-level client currently stages maximum tab
and reference candidates. Independent C production-export interoperability,
protected Engine commit/work-area proof, Narthex migration and proposal
acceptance remain required. No IPC path was removed by this slice.

### Queued C descriptor candidates (t271)

Signed C SDK `2b00a7766c856b36e3604d68087b90f747bc1296` removes the
queued session's descriptor staging gap. `sophia_ss_open_fd_staging` accepts
caller-owned transaction storage separately from the queue and object scratch.
It checks all buffer regions before starting I/O; existing initializers retain
their 8 KiB inline storage. The total queue remains bounded to 512 KiB/64 slots,
and per-kind record limits are unchanged. No large inline array or allocation
was added to the session.

Candidate sizing now validates without an encoded scratch buffer and uses a
descriptor's body connection epoch when assigning temporary header context.
Actual admission still validates against the live epoch. Maximum tab/reference
candidates (8,260/52,488 bytes) can be admitted atomically as a group, copied
before caller mutation, and copied again before queue compaction at hand-off.
Undersized staging never issues tickets or submission IDs. Reservations survive
invalid groups, and queued versus issued records retain their distinct custody
outcomes on disconnect.

Evidence in `development-evidence/ipc-retirement/`:

- `t271-c-queue-check3.log` and `t271-c-queue-final2.log`: complete strict
  checks without and with IPC pass, including the new session staging test.
- `t271-c-queue-ubsan.log`: complete file suite passes with undefined-behavior
  traps enabled.
- `t271-c-queue-mutant-{cap,overlap}.log`: fresh compiled overlays exit 134
  when the old 8 KiB cap is restored or the session/transaction overlap guard
  is removed. The latter catches state mutation on rejected buffer arguments.
- The new tests exercise 127-byte fragmented writes, three queued maximum
  candidates, acknowledgement ordering, source mutation, exact and insufficient
  reservations, unchanged transaction bytes/ID across paced EAGAIN retry, and
  custody versus dropped-unsent outcomes.
- Earlier failures are kept: `check1` did not execute because the evidence
  launcher lacked an executable bit (subsequent calls use Bash); `check2`
  rejected a malformed test color with nonzero channels and zero alpha. The
  fixture now uses valid colors; no validation or assertion was relaxed.

These are scripted-peer gates in device/network/display-hidden isolation,
with private build outputs and nice 19/j2. The SDK branch is unpublished.
Independent C production-export conformance, protected Engine presentation
and work-area checks, Narthex migration and contract acceptance remain pending.
The desktop and published repository heads were unchanged.

### Independent C descriptor production-export exchange (t271)

Sophia `40b88fc5e` vendors signed C SDK
`2b00a7766c856b36e3604d68087b90f747bc1296`. The snapshot verifier now also
compares both proposed descriptor references to this worktree's proposal;
its contract-drift tests include them. These remain proposed inputs, not an
accepted or published contract.

`shell_file_descriptor_c` compiles a C-only peer against that snapshot and
drives `ShellComponentTransport` and its production 9P export. The peer uses
the C session and native codecs; it does not use Rust encoders or socket-frame
helpers. Both metadata-only and combined-content negotiations pass with exact
selected capabilities. Metadata never receives Limits; combined negotiation
uses the reserved content grant. The harness supplies process authorization
evidence and semantic presentation calls, so this is not a protected Session
launch or Engine work-area proof.

All seventeen proposed record families cross the independent boundary. The
test fetches the maximum 16 descriptors, 1,024 tab groups/2,048 entries, and
256 shortcuts, plus a 32-entry plain launcher catalog. The C client checks
decoded rows, epochs, generations and action identities, and keeps each
publication unacknowledged until its complete object fetch. It submits maximum
descriptor/tab/reference/launcher candidates (276/8,260/52,488/168 bytes).
The Rust owners compare the complete resulting values, then send exact
Prepared/Presented outcomes. Descriptor activation is refused before both
preparation and presentation; a stale activation acknowledgement cannot settle
the correct one. Both descriptor and launcher activation acknowledgements
complete, and disconnect leaves accounting quiescent. No application launch
or display access occurs.

Evidence in `development-evidence/ipc-retirement/`:

- `t271-c-export-final.log`: the independent C test passes both modes.
- `t271-c-export-related.log`: 38 tests pass across the new C test, descriptor
  negotiation/owners, existing C file tests and C r7/r8 role interoperability.
- `t271-c-export-vendor-tests.log`: snapshot/contract refusal tests pass;
  `t271-c-queue-vendor-check.log` proves the imported tree and revision.
- Focused clippy, workspace formatting and layout pass in
  `t271-c-export-{clippy,fmt,layout}.log`.
- `t271-c-export-mutant-cap.log`: a freshly compiled C overlay restoring the
  8 KiB cap fails at maximum tab admission (phase 5).
- `t271-c-export-mutant-wire.log`: changing a descriptor body's connection
  epoch immediately before its 9P write, after C-side value validation, gets
  Refused with remote `EINVAL` (22), not Submitted custody. The positive test
  fails at phase 1 as required. An earlier encoder-level mutation in
  `t271-c-export-mutant-epoch.log` was caught by C validation itself; that log
  is not server-refusal evidence.
- Failed `check1` and `check2` logs are kept: the new fixture initially used
  incorrect Rust field types/names, then treated the C event API's `AGAIN` as
  `BUSY`. Only the fixture changed; no production assertion was relaxed.

All runs are bounded, offline and device/network/display-hidden, with private
outputs and nice 19/j2. The live desktop and published main branches remain
unchanged. Protected Engine presentation/work-area evidence, Narthex's thin
C SDK migration, contract acceptance and IPC removal remain required.

### Protected C descriptor presentation and work area (t271)

The Session test
`protected_c_descriptor_work_area_changes_only_after_matching_presentation`
closes the CPU presentation boundary left open by the production-export test.
It compiles a C-only peer against the same vendored SDK, then launches it with
`LiveMetadataShell::start`, the production supervisor and default protected
domain. The child requires the 9P endpoint and refuses the old socket variable,
display variables and a visible DRM directory. Broker disclosure is an
already-sanitized one-row fixture; no broker process or application is run.

The real Session request/poll path resolves the C candidate into an Engine
descriptor overlay. The test verifies that Prepared leaves the work area
unchanged, as do an unrelated presented generation and staging the matching
overlay without presenting it. A CPU production cycle presents the matching
overlay; only then does Session apply the 24-pixel reservation. A rejected
32-pixel replacement preserves 24 pixels; a presented replacement changes it
to 32. Withdrawal preserves 32 until its own presentation restores the full
work area. The C peer independently decodes the ordered outcomes and checks
strictly increasing nonzero presentation epochs. The parent flushes the final
outcomes and requires a normal zero-status child exit before cleanup.

Evidence in `development-evidence/ipc-retirement/`:

- `t271-session-c-presentation-final-layout.log`: 70 selected Session tests pass,
  including this protected C test and the existing C WM export test sharing
  the bounded compiler helper.
- `t271-session-c-presentation-mutant-early.log`: committing the claim at
  Prepared fails the full-work-area assertion (24 pixels were consumed).
- `t271-session-c-presentation-mutant-generation.log`: removing the backend's
  candidate-generation check fails when the unrelated frame is presented.
- `t271-session-c-presentation-clippy-final.log`, `layout-final.log` (same
  prefix) and `fmt.log`: all-target/all-feature Session clippy, layout and
  workspace formatting pass.
- Failed harness iterations are retained: `check1` used the wrong SurfaceId
  constructor; `check2` passed the reservation assertions but tore down before
  the C child finished its final outcome. `check3` adds the checked normal exit.
  `clippy1` found a duplicate module inclusion and a collapsible conditional;
  the compiler helper is now included once and shared by both SDK tests.
  `layout1` required the repository's test-file-level cfg convention; the
  attributes moved to the support files without changing test behavior.

Mutants are read-only mount overlays, rebuilt before running; production
sources are unchanged. All runs use private outputs with devices, network and
display access hidden. This proves protected Session admission and its real
CPU presentation boundary, not KMS/page-flip timing, broker disclosure policy
or a product UI. The descriptor contract remains proposed. Narthex's thin C
SDK migration, contract acceptance and IPC retirement remain required.

### Independent descriptor hosts over files (t271, 2026-09-28)

Both the descriptor and launcher conformance hosts now use the production 9P
owner. `shell_descriptor_modes` builds an independent C peer against the pinned
C SDK with `WITH_IPC=0`; it replaces the Rust IPC fixture. The peer covers all
three descriptor modes and the launcher host. The ordinary test gate now proves
the previously missing independent tab/reference lifecycle and bar reservation
commit/withdrawal cases. No product checkout is needed.

The five descriptor/launcher tests and the four existing content-file tests
pass (`descriptor-file-modes-1.log`). Wrong tab activation IDs and transactions
cannot satisfy the owner's pending response; a refused disposition is rejected.
Stale presentation grants are refused by the file owner before disclosure, so
the old IPC fixture's stale-event injection is deliberately replaced by that
host refusal assertion. The launcher checks a 4096-row catalog, presentation,
activation, replay rejection and a new query. Persistent peers handle SIGTERM
and exit successfully before disconnection. This does not prove production
Session restart behavior.

Narthex's separate signed `c49dd92` migrates its executable and reducers to the
public C SDK at `2b00a776`, removing its four handwritten IPC wire modules.
Its 20 local checks pass, as do protected descriptor proof, bar proof, persistent
serve and launcher runs against these hosts. Development Nim-generated C emits
const-qualifier warnings; the SDK binding header passes strict C compilation.
This is development interoperability evidence, not a reviewed release build.
Product logs are under `development-evidence/narthex-descriptor-9p/`.

`check_shell_protocol.sh` now invokes the independent C file tests in place of
its descriptor/launcher socket clients and sibling Narthex build. Its unrelated
legacy codec and popout checks remain until their owning migrations. Focused
clippy and layout pass (`descriptor-file-clippy-1.log` and
`descriptor-file-layout-1.log`, under `development-evidence/ipc-retirement/`).
The complete shell script passes (`descriptor-shell-gate-2.log`), including
the retained codec, file-export, protected popout and indicator checks. The
first run caught two callers of the renamed C compiler helper; both were
updated. Session GPU-proof tests pass 14/14 (`descriptor-peer-session-tests.log`),
and clippy passes for the affected Session/backend test targets
(`descriptor-peer-callers-clippy.log`). The failed first gate log is retained.
Contract acceptance and retirement of the legacy single-shell configuration
remain outstanding; this slice does not complete t271.

### Retired fallback shell selection (t271, 2026-09-28)

Niltempus `b21eb20` stops emitting `--shell-process-default`; its five recipe
tests and focused clippy pass. Sophia now refuses that argument before loading
configuration, including its bare, empty, relative and absolute forms and when
an explicit shell is also supplied. Shell resolution no longer contains a
launcher fallback. Explicit component profiles remain unchanged. The separate
legacy `--shell-process` selection is still present pending its replacement.

The full Session library gate passes 703 tests, with 22 ignored
(`retire-shell-default-session-2.log`); focused clippy and workspace format pass.
The first test run caught one action-catalog fixture relying on the removed
fallback. It now selects its inert test executable explicitly, preserving its
WM and action assertions. The first clippy run caught a collapsible conditional;
both failed logs remain in `development-evidence/ipc-retirement/`. This change
does not update any installed launcher, prepared release or running session.

### Explicit descriptor component selection (t271, 2026-09-28)

The desktop profile can select a sole `shell-component` with role `descriptor`.
It uses 9P exclusively, takes its executable and private config from that
component, and preserves explicit combined content/input grants. Descriptor
reservations still use the global panel allowance. Mixing descriptor authority
with independent content owners is refused, as are legacy CLI overrides and
global GPU grants. The content aggregate cannot admit or launch this role.

The protected independent C work-area test now selects the peer through this
profile path. Its presentation, replacement and withdrawal assertions remain
unchanged. The Session library passes 704 tests (22 ignored); the content
connection suite passes 13, including refusal before creating an endpoint.
Configuration tests and workspace all-target/all-feature compilation pass.
CLI preflight checks the descriptor executable without executing it. Logs are
`descriptor-component-*` in `development-evidence/ipc-retirement/`.

This supplies the replacement selection path. Legacy shell arguments and profile
fields still await removal; the descriptor contract remains proposed. No running
session or installed profile changed.

### Retired shell launch selectors (t271, 2026-09-28)

Following the explicit descriptor path at `bb5961b47`, Session refuses
`--shell-process`, `--shell-transport` and `--shell-file-profile` before reading
configuration, alongside the already retired fallback argument. The profile
parser refuses `shell-client` and `shell-config`; their passive fields are
removed. Shell selection and private config now come only from declared
components. CLI inspection uses `--component=descriptor` for that role.

Configuration, reload, composition and preflight fixtures use the replacement
path. The shared panel probe declares a 9P bar and per-component GPU permission;
its action-catalog assertions are unchanged. Bare, empty and valued legacy
arguments all refuse, and descriptor inspection does not run the executable.

The Session library passes 704 tests (22 ignored), configuration passes, CLI
preflight passes 13 and profile inspection passes 5. Workspace all-target and
all-feature compilation and focused clippy pass. Evidence is
`retire-shell-selection-*` in `development-evidence/ipc-retirement/`; the two
failed Session runs retain the obsolete fixture error expectation and the
panel fixture's global GPU selection, both corrected on the new path.

The full `cargo xtask check` stops at C SDK contract drift: the descriptor ADR's
review-evidence section has changed since its vendored snapshot. That failure
is retained as `descriptor-selection-full-check.log`; there is no full-gate
pass for this candidate. Normative contract acceptance and SDK pin reconciliation
must resolve it. The descriptor owner's internal socket branch and its remaining
socket fixtures still await retirement. No task completion or live acceptance
is claimed by this configuration cut.

### Descriptor owner startup fixed to 9P (t271, 2026-09-28)

After selector retirement at `ac54c1504`, the descriptor owner's constructor
has no wire or file-profile argument. Its production startup always uses the
descriptor file negotiation and emits only `SOPHIA_SHELL_9P_SOCKET`. The unused
`ShellFileProfile` configuration enum and stored selections are removed.
Content-only independent components continue through their own owner.

The deferred-ready/reconnected reporting test now uses the Rust SDK over 9P.
Its first run exposed a fixture error: returning from server negotiation does
not mean the client has consumed its bootstrap custody and acknowledgement
replies. The corrected fixture services those replies until bounded client
readiness. The protected descriptor reconnect test and the negative that a
content-only export cannot admit the same descriptor child retain their
assertions. The independent C work-area test still uses the fixed production
startup path. Session passes 704 tests (22 ignored); focused clippy passes.
Evidence: `descriptor-only-session-2.log` and `descriptor-only-clippy.log`.
The first misfiltered run ran zero tests and is not evidence.

The runtime compatibility adapter remains for the broader IPC retirement, as
does a combined-content disconnect fixture that injects the old wire directly.
Neither is selectable by production descriptor startup. Their removal belongs
with the remaining owner/fixture migration; this does not close t271 or repair
the SDK contract-drift failure recorded above.

### Normative descriptor contract and SDK pins (t271, 2026-09-28)

Signed `3330ecf77` accepts ADR `4oapm903` on the implemented native wire and
recorded independent C evidence. All seventeen kind declarations and twenty-four
body/prefix/row layouts now live in `protocol/sophia-shell-files-v1.kdl`;
`docs/sophia-shell-descriptors.md` owns the role, scalar, semantic-refusal,
custody and presentation rules. The original layout fragment remains historical.
SDK builds no longer read a proposed-layout fragment or an evolving ADR.

C SDK `88347eb7b37816450863e4de582f1c01e80d446a` and Rust SDK
`f6b177c` copy the three normative shell file inputs from that signed Sophia
commit, with provenance and digest manifests. The C strict gate passes with
and without IPC compatibility. Rust passes 326 default-feature tests and 338
all-feature tests, plus strict clippy and formatting. The native KDL completeness
tests now read the normative file alone, retaining all descriptor cap and
literal-layout assertions. Evidence is `descriptor-contract-{c,rust}-*.log`
in `development-evidence/ipc-retirement/`.

Both SDKs are re-vendored from their signed commit objects. Sophia's same-contract
checks now bind the descriptor rules document and full KDL, removing the ADR
drift identified above. The full repository gate is being rerun against those
snapshots; no full-gate, publication or task-completion claim is made here.

The first full rerun reached the runtime tests and caught a boundary regression:
descriptor, tab, reference and launcher owners still named socket codecs. The
existing `shell_owner_wire_neutrality` check failed without being changed.
Those codecs now sit behind typed calls in `socket/descriptor_records.rs`.
Owner presentation state and file custody are unchanged. The guard also names
the legacy codecs whose names lack a `_frame` suffix, closing that blind spot.
The focused runtime gate passes 42 tests across both wires, including the
independent C descriptor exchange and the original socket lifecycle tests.
Logs: `descriptor-contract-full-check-1.log` (failed boundary check) and
`descriptor-adapter-focused-3.log` (pass). Two earlier focused invocations
failed before tests: a support module was named as a test binary, then the new
adapter imported a type from the wrong module. Both logs are retained.

Narthex `dc34720` now pins the accepted C SDK. Its 20 local checks and all four
protected 9P host modes pass again, with the same development-build limits.
Evidence: `narthex-descriptor-9p/accepted-contract-{local,build,conformance}.log`.
The full Sophia gate is rerunning after the adapter correction.

### Descriptor replacement acceptance (t271, 2026-09-28)

Candidate `77888779b` passes the complete isolated `cargo xtask check`, exit 0:
494 test-result groups report 6,689 passed, zero failed and 63 ignored. Strict
clippy, SDK snapshot checks, layout and tool verifiers pass. The log is
`ipc-retirement/descriptor-contract-full-check-2.log`. No device, display or
installed session was available; hardware proofs and the absent archive corpus
were reported separately rather than claimed.

Documentation correction `086cd6e75` removes the obsolete provisional descriptor
row, gives its accepted 352-byte event and 22,528-byte terminal reserve, and
replaces stale product migration observations with the current role contract.
The resulting SDK pins are C `74498734` and Rust `6e6c852d`, published by
fast-forward and independently read back from their remotes. These follow-ups
change only prose, provenance and digest lists; library code, tests and layouts
are identical to the full-gate pins. Sophia `310fcda46` imports those snapshots,
whose checks pass again. Release tags are unchanged.

This satisfies the descriptor replacement scope: an accepted native contract,
independent C coverage of all kinds and all four host modes, protected matching
presentation before work-area commit, a 9P-only client migration, explicit
descriptor component admission, and retirement of the old single-shell selectors.
Niltempus `b21eb20` no longer emits the fallback selector. Narthex `1cd0f9a`
contains the tested adapter and the final documentation-only SDK pin; its
`overview` branch is untouched. No component is installed or reloaded here.

The generic runtime socket compatibility adapter and remaining socket fixtures
are still present for t265/t269. They are not selectable by descriptor production
startup. Their final deletion, SDK compatibility retirement, independent content
default flips and the output/broker/portal work remain separate exits.

### Sophia SDK compatibility dependency retirement (t265, 2026-09-28)

The Session action harness now connects the real Rust SDK to the production
9P export. Its five tests retain receipt/activation independence, the 1,000
alternating acknowledgement rounds, WM admission/refusal, snapshot identity
and event high-water checks. Both response tickets must reach Submitted
before the fixture services the semantic acknowledgement and activation;
local enqueue success is not treated as remote custody. The focused run passes
5/5 (`ipc-retirement/t265-client-actions-files-2.log`).

The redundant socket SDK fixtures are removed with their assertions retained
in `shell_component_files_registry` (six), `shell_content_session_files` (five)
and `shell_component_files_reduced_limits` (six, including two additional
negotiated-chunk controls). Those suites and `shell_file_transport` pass all
29 tests (`t265-retained-files.log`). Protection evidence and presentation
facts are supplied by these fixtures; no physical acceptance is claimed.

`sdk_ipc_parity` compared two copies of the retiring socket codec. That
compatibility-copy obligation is retired, rather than replaced by a tautological
comparison of Sophia's file codec to the same SDK codec it re-exports. Sophia's
socket golden/malformed tests remain until t269. Native record literal vectors,
malformed inputs, independent C/Go peers and production-export SDK tests retain
file conformance coverage.

Sophia no longer enables `ipc-compat` or depends on `sophia-shell-ipc`, including
its test graph with every workspace feature enabled. The lockfile drops the
package; `t265-feature-tree-2.log` contains neither dependency nor feature.
The vendored SDK still carries compatibility for its own separately gated
all-feature suite until t270. An initial graph refresh used an incorrectly
ordered read-only mount and changed no lockfile; the corrected refresh passes.
Signed candidate `680f0516e` passes the complete isolated `cargo xtask check`,
exit 0: 490 test-result groups, 6,671 passed, zero failed and 63 ignored.
Strict clippy, SDK snapshot checks, layout and tool verifiers also pass.
Evidence: `ipc-retirement/t265-full-680f0516e.log`. This closes the Sophia
test dependency scope of t265. It does not remove Sophia's production socket
adapters or the SDKs' standalone compatibility targets. Devices and display
were hidden, and the archive corpus was absent; no live acceptance is claimed.

### Shell owner and outbox completion (t268, 2026-09-28)

The accepted descriptor records complete the native vocabulary used by the
typed output FIFO. Content, native launcher, catalog, indicator and descriptor
records have native body charges; control credits cover whole native records.
Snapshot objects remain separately bounded. Socket framing, message kinds and
frame-shaped bounds are confined to `shell_transport/socket.rs` and its
children. The strengthened `shell_owner_wire_neutrality` test enforces this
across the transport and content owners. Compatibility dispatch remains until
t269; retiring that dispatch no longer requires replacing owner encodings or
budget arithmetic.

`c82553a59` fixes the previously reported internal admission-counter overflow.
These stamps only merge two bounded queues; they are not protocol identities.
The shared record cap is at most u32-sized (64 without content), and output
never skips a retained earlier record. Thus their live span is less than half
the u64 range. Wrapping increment plus modular comparison preserves that order
without changing file journal sequences, transaction IDs or custody.

The new real-socket regression starts immediately before each wrap boundary.
The original increment panics (`t268-order-wrap-before.log`); increment-only
repair produces the wrong observed record order
(`t268-order-wrap-comparison-before.log`). The complete fix passes all 56
runtime library tests and 45 focused transport tests, including the independent
C descriptor exchange, native record bounds and owner isolation. Strict runtime
clippy for all targets/features, formatting and layout pass. Logs are
`ipc-retirement/t268-order-wrap-after.log` and `t268-final-{focused,clippy,fmt,layout}.log`.

The preceding full gate at `680f0516e` passed 6,671 tests and all offline checks.
Only the counter/comparison repair and its regression changed executable code
after that gate. Earlier credit/settlement mutations and the reduced-upload
regression remain part of the evidence above. These results satisfy t268's
owner-budget and typed-output scope. Optional socket EINTR retry was not added;
it is not needed for this separation and the adapter remains slated for t269.
No production transport default, SDK release, installed component or live
session changed.

### Output reconnection fencing (t272, 2026-09-28)

The remaining output survey's late-response defect is reproduced: after an
admitted process reconnects, a delayed Reply or Settle for its previous epoch
ends the service with `InvalidConnectionEpoch`, closing the new connection.
The new tests admit transactions 2 and 3 in the replacement epoch while the
old response names transaction 2. Both tests fail before the repair with EOF
at the subsequent publication (`t272-late-epoch-before.log`).

The service now drops responses naming a valid older epoch before writing or
settling anything. A publication queued behind the old response proves forward
progress and no stale bytes; the replacement's active and queued proposals
then settle in order. Zero and future epochs still fail as owner errors, with
no bytes delivered. All eleven output-service tests pass, along with strict
runtime clippy (`t272-late-epoch-{after,clippy}.log`). These tests use bounded
private sockets with supplied peer identity and no devices or live session.

This fixes a wire-independent reconnect rule needed by the output file role.
The output export, independent SDK peer, bounded retained identities/channels,
remaining failure and restart observations, and applicable topology acceptance
are still required. It does not close t272 or claim output IPC retirement.

### Neutral output-role foundation (t272, 2026-09-28)

The output negotiation values, proposal, snapshot envelope and outcome now
live in `sophia-protocol/src/output_role.rs`. The old socket module re-exports
them to preserve its API. The owner and its five unchanged tests move from
`output_ipc.rs` to `output_connection.rs`; none of their admission or settlement
logic changes. A verbatim comparison verifies both relocations.

A disposable private probe compiles the actual owner source and runs those five
tests with the protocol's IPC module and re-exports removed by a read-only
overlay. The only test-body change in that probe is the crate import. All five
pass (`ipc-retirement/t272-neutral-probe-final.log`), proving the owner/model
can support the future file export without compiling the socket codecs.
This is not a file-wire implementation or peer interoperability result.

The ordinary gate passes 18 runtime owner/service/transport tests and eleven
protocol schema/configuration/topology tests. Strict clippy for protocol and
runtime, all targets/features, formatting and layout pass. Logs:
`t272-neutral-output-{focused,protocol,clippy,fmt,layout}.log`. The first normal
compile warned about a redundant root glob re-export; an explicit public export
list removes the warning while preserving both facades. The final no-IPC probe
was regenerated from that corrected source and passes again.

### Native output record foundation (t253/t272, 2026-09-28)

`sophia_protocol::output_files` now implements the native 32-byte envelope,
submit/ack controls, Negotiate, Submitted, Proposal and Outcome codecs. The
proposal uses bounded fixed rows, including zeroed unused member slots. It
preserves separate file-submission, domain-transaction and journal identities.
The [implementation draft](../../sophia-output-files.md) and
[proposed decision](../decisions/vkkjmufd-use-native-records-for-the-separate-output-file-role.md)
describe this slice and the remaining custody decisions. No live export is
advertised, no existing transport is selected differently, and no socket codec
is called by the new module.

Eight codec tests check literal bytes, each truncated prefix, reserved fields,
identity classes, enum/count bounds and the 1,784-byte candidate maximum. Two
new owner tests prove that decoded unknown requested capability bits still
intersect at negotiation and that semantic topology refusals still consume the
domain transaction identity. All seven owner tests and eight codec tests also
pass with the protocol IPC module removed in a read-only build overlay.

Evidence under `development-evidence/ipc-retirement`:
`t272-file-codec-final.log` (19 protocol checks, including the existing output
schema/topology checks), `t272-file-admission-final.log` (7 owner checks),
`t272-file-codec-noipc.log` (15 checks), and
`t272-file-codec-{clippy,fmt,layout}.log` (pass). A read-only overlay mutant
which accepts nonzero reserved bytes fails three named refusal tests in
`t272-file-codec-reserved-mutant.log`. The private protocol build artifacts were
cleared afterward and the final normal tests rebuilt the original source.

The remaining object/event bodies, bounded export, independent SDK peer,
protected recovery and applicable native acceptance are still required. This
slice does not close t253 or t272. In particular, acknowledged journal records
do not bound the owner's domain-transaction replay history; the export design
must address that separately.

### Native topology and output events (t253/t272, 2026-09-28)

The native codec now covers Topology, Negotiated, Refused and ObjectPublished.
Topology encodes the existing authoritative snapshot as bounded fixed rows,
with a mode table tiled exactly once in head order. Decoding and encoding apply
the existing snapshot validator; labels require valid UTF-8 and zero padding,
and every unused member slot is zero. A maximum topology record is 52,216 bytes,
including its 32-byte envelope. Disabled heads with no current mode remain
representable. `Some(INVALID)` is refused on encode to avoid silently changing
it to `None` on decode.

The event codecs preserve revision/capability negotiation, distinguish a
terminal negotiation refusal from a topology outcome, and identify a published
topology by both its domain epoch and immutable Qid path. The
[implementation draft](../../sophia-output-files.md) documents every row and
requires a readable refusal-drain phase before export revocation. Limits and
export custody are still subsequent work.

Evidence under `development-evidence/ipc-retirement`: 31 focused protocol
checks pass in `t272-topology-focused-2.log`; 27 codec/owner checks pass with
the IPC module removed in `t272-topology-noipc.log`. Strict protocol/runtime
clippy, formatting and layout pass in `t272-topology-{clippy,fmt,layout}.log`.
A separate private-target, read-only mutant allowing a gap in the mode table
fails `mode_ranges_must_tile_the_table_once_in_head_order` as intended
(`t272-topology-mode-mutant.log`). The source tree was never mutated for that
control. The earlier `t272-topology-focused.log` records a test-only borrow
error in the four-member case, corrected without changing its assertions.

These checks do not provide an independent-language client exchange or a live
output export. They leave the remaining t253/t272 acceptance scope unchanged.

### Bounded output custody primitives (t253/t272, 2026-09-28)

The native Limits codec completes the record vocabulary and explicitly binds
domain replay history: default/ceiling 4,096 IDs per epoch, independent of
journal acknowledgement. `OutputConnectionState::with_transaction_limit`
preserves arbitrary ID order and counts semantically rejected IDs. Capacity
refusal changes no custody; accepted active/queued proposals still settle, and
only a newer connection epoch clears history. The retiring socket adapter
continues using its unchanged default. The file export must use the bounded
constructor and enforce the advertised deadline fields.

The shared 9P journal now prepares nonempty batches atomically, with checked
record/byte/sequence/offset bounds and no mutation on refusal or guard drop.
`OutputFileJournal` reserves one 56-byte terminal record for each of at most
two pending proposals. Publication cannot consume those credits. Queued
replacement batches the new receipt with the old Stale outcome; immediate
semantic refusal batches its receipt with Rejected without borrowing another
proposal's credit. Refused negotiation remains readable until acknowledged;
the export's terminal drain deadline is not implemented yet.

The domain owner and journal are exercised together through the public APIs,
including journal refusal before domain mutation, replacement, settlement,
history exhaustion and a dropped reservation. No served export or live service
selection is claimed by these primitives.

Evidence under `development-evidence/ipc-retirement`:

- `t272-custody-9p-final.log`: all 106 9P crate checks pass, including atomic batch
  capacity refusal, guard drop, partial reads, acknowledgements and overflow.
- `t272-custody-codec.log`: 23 native output codec checks pass, including the
  literal Limits record and every interval/truncation refusal.
- `t272-custody-final-2.log`: 31 runtime owner/journal/service/transport checks
  pass after clearing the private runtime build artifacts used by the mutants.
- `t272-custody-noipc-final.log`: 41 codec and owner/custody checks pass with the
  protocol IPC module excluded. The disposable probe points at the production
  owner/journal sources and adds only test import aliases.
- `t272-custody-clippy-final.log` and `t272-custody-{fmt,layout}.log`: strict
  checks pass. Final batch review also added a refusal for a byte bound below
  existing retention even if the new payload is empty; the final 9P gate covers
  that edge.
- `t272-custody-mutants.log`: a read-only overlay disabling the replay ceiling
  fails three owner checks; independently removing terminal byte reservations
  fails the byte-budget test. Four failures are expected. The source tree was
  not changed by either mutant.
- `t272-custody-full.log`: the complete offline `cargo xtask check` passes on
  the code signed as `80df2c4ce`: 496 test groups, 6,714 passed, zero failed,
  63 ignored, followed by SDK snapshot checks, workspace clippy, formatting,
  layout and tool verifiers. The sandbox hides devices and live-session paths;
  buffer-age and GLX/EGL physical proofs are explicitly unproved there, and the
  direct-scanout archive corpus is absent. This is deterministic acceptance
  of the code change, not physical output acceptance.

The [implementation draft](../../sophia-output-files.md) records limits and
custody rules. File-node lifetime, immutable snapshot retention, exact
submission replay, deadline enforcement, service-channel bounds and protected
transport integration remain required before t253/t272 can close.

### Broker and portal survey correction (t273, 2026-09-28)

The production statement in section 8 applies to the metadata broker only.
The portal socket server and the X clipboard coordinator are exercised by
tests; production does not bind a portal endpoint or admit multiple X
namespaces. Broker and portal file-contract drafts are under review, with
separate admission and no new authority inferred from a path or attach.
They are not implemented exports. Existing wire values and implemented
operations must be retained; absent portal executors must not be described
as live capabilities.

The [corrected broker/portal decision proposal](../decisions/xa78u03g-keep-broker-and-portal-file-authority-and-custody-separate.md)
preserves rejection 5 and optional-text representation, specifies refusal
draining, accounts for every receipt and reserved terminal record, and separates
portal requester admission from the decision-owner role. It proposes finite
transfer history across reconnects without requiring ordered caller IDs. The
earlier evidence-only KDL is not a finished contract and still needs those
changes before SDK consumption. This is design progress, not t273 acceptance.

### Broker and portal contract reconciliation (t273, 2026-09-28)

The subsequent [decision](../decisions/xa78u03g-keep-broker-and-portal-file-authority-and-custody-separate.md)
is accepted as a design. The [broker](../../sophia-broker-files.md) and
[portal](../../sophia-portal-files.md) contracts now carry the corrected native
KDL; the earlier evidence-only drafts above remain historical. This satisfies
t273's design scope without asserting an implemented portal, removing a socket,
or bypassing t255 qualification.

All eight root review findings are resolved in the design: rejection 5 and
optional values retained; terminal refusal draining; full broker receipt
accounting; explicit PortalRequester admission; every portal receipt and
reserved terminal counted; the Session WM owner reference corrected; and a
finite retained identity history across reconnect. Only Clipboard has a payload
encoding. Replacement admission waits for old executor custody and its buffer
to settle. Partial acks cannot extend refusal draining. The portal budget
explicitly backpressures before 322 lifecycle records exceed its 256-record
ceiling; it does not claim byte room implies record room.

The source test `broker_portal_file_layouts` parses both actual KDL files and
checks complete byte coverage, kind classes, preserved rejection values,
candidate sizes and custody arithmetic. Four tests pass, including eight
malformed-layout controls. The first focused run found a test-helper lookup
error (object declaration selected instead of body); its log is retained.
These are design checks, not independent client, executor or admission proofs.

Evidence under
`~/.local/state/sophia/development-evidence/ipc-retirement/`:
`t273-contract-review.md`, `t273-layout-focused.log`,
`t273-layout-focused-run2.log`, and `t273-layout-full.log`.
The full `cargo xtask check` exits 0: 496 reported result groups, 6,717 passes,
zero failures and 63 ignored, with SDK checks, clippy, layout and tool verifiers
passing. It ran offline in the device-hidden sandbox. The count includes
reported fixture-child results and is not a count of unique tests. Native
pixel/scanout evidence is not proved in this sandbox; no live session was used.
The contracts list the real-export, SDK, protected admission, replay, pressure,
two-row atomicity and execution-race evidence required before replacement.

### Protected control-owner fixture uses WM files (t269 preparation)

The generic `real_owner_commits_actions_and_confirms_replacement_commit` test
now launches an independent C SDK peer over the protected WM 9P endpoint.
It retains action and replacement commit ordering, two replacement epochs,
stale-action refusal, reload settlement and logout assertions. The peer rejects
the retired WM socket environment. Administrative requests still use control-v1;
this change does not claim t254 migration or remove the remaining demo users.

The new fixture exposed an initial-startup state bug: profile activation had
already consumed `Negotiated`, but the running owner reset its flag to false.
The [startup investigation](qqdj8dfu-initial-policy-activation-consumes-negotiation-before-session-records-it.md)
records the failing-before/passing-after pointer-focus assertion and the repair.
Ordinary transport-selection coverage checks that negotiation is preserved
without prematurely enabling configuration-dependent capability use.

### Session no longer depends on the IPC WM demo (t269 preparation)

The remaining Session reference to `sophia-wm-demo` was a comparison between
the owner's rearm table and the demo's stateless retry table. Session now checks
all five contract outcomes explicitly and exercises recovery with the independent
C SDK peer on a real protected 9P connection. Its dev-dependency and corresponding
lockfile edge are removed; the runtime IPC tests and other demo users remain.

The new ordinary test
`protected_c_sdk_recovers_after_stale_and_timed_out_projections` supplies two
layout failures to the production settlement owner. The first has an advanced
canonical generation, producing RejectedStale; the second produces TimedOut.
The peer requires those exact outcomes, then waits for new owner cycles with
fresh snapshot, request and submission identities. Its third projection commits.
Zero child exit proves it decoded the final Committed outcome. No WM restart
or new epoch is allowed, and neither failed projection increments commit state.
This tests recovery over the real export, not elapsed resize expiry or native
scanout. The old demo's own policy tests stay with that compatibility crate.

Evidence under `development-evidence/ipc-retirement/`:

- `t269-file-recovery-focused.log`: first focused run passes.
- `t269-recovery-mutant-stale.log`: a read-only overlay disabling stale rearm
  fails at fresh cycle 1. `t269-recovery-mutant-timeout-fresh.log` freshly
  compiles the timeout-only mutant and fails at cycle 2. The earlier
  `t269-recovery-mutant-timeout.log` reused the stale mutant artifact and is
  excluded. Package artifacts were cleared before the fresh timeout run and
  again before normal validation; no source mutation was applied to the tree.
- `t269-recovery-session-final.log`: the complete all-feature Session suite
  passes (1,062 reported passes, zero failures, 39 ignored, including child
  fixture reports). The new recovery test is non-ignored.
- `t269-recovery-control-final.log`: the existing ignored protected control
  test also passes with the shared C peer's unchanged committed-outcome checks.
- `t269-recovery-{clippy,fmt,layout}.log`: all pass. Locked metadata in
  `t269-recovery-metadata.json` confirms the removed dependency edge.

These checks extend the full repository gate at `6df3602d8`; they do not
claim a second full repository run. No installed or running session changed.

### Session WM IPC worker retirement (t269, 2026-09-28)

Following the accepted source-retirement decision, Session now defaults to
9P2000.L and refuses `--wm-transport=current-ipc`. The socket worker and its
selection branches are deleted. Restart, profile rollback and inspection use
the same file endpoint owner as initial startup. Output and administrative
transports are unchanged. Runtime compatibility libraries and the shell socket
path are subsequent cuts; this slice does not close t269.

Coverage for the deleted worker tests is retained as follows:

| Retired assertion | Retained file/owner check |
| --- | --- |
| Admission before internal Negotiated and Configuration | `supplied_stream_orders_negotiated_before_exact_profile_reducer_exchange`, `full_worker_transports_all_commands_without_owning_their_outcomes` |
| Rejected profile closes before internal Negotiated | `profile_identity_mismatch_or_peer_refusal_closes_admission` (now both wrong identity and exact client refusal), `semantic_adapter_refuses_profile_before_negotiated` |
| IPC partial-transfer markers and discarded frames | Retired with the IPC worker. The file export validates complete candidates before semantic delivery; typed out-of-phase Configuration/SessionOperation and premature Projection refusals remain in `policy_adapter_driver` |
| Protected startup, restart and recovery | `policy_transport_selection`, the independent C SDK `live_control` and `policy_recovery` fixtures |

The pregraphics failure test now expects the file admission timeout rather than
the retired socket's `AcceptTimedOut`; its deadline, participant rollback and
directory-removal assertions are unchanged.

Evidence under `development-evidence/ipc-retirement/`:

- `t269-wm-retirement-session-2.log`: all-feature Session tests pass (1,060
  reported passes including child fixtures, zero failures, 39 ignored). The first
  run's timeout-string mismatch is retained in `t269-wm-retirement-session-1.log`.
- `t269-wm-retirement-clippy-2.log`: strict Session/CLI clippy passes. The first
  run caught a one-element test loop; its log is retained.
- `t269-wm-retirement-control.log`: the independent C SDK protected control
  startup/restart test passes, 1/1.
- `t269-wm-retirement-full.log`: workspace and SDK tests and clippy pass; layout
  then refuses four production files containing test-only constructors. Those
  helpers were moved to their existing `tests/support` modules without changing
  their assertions. `t269-wm-retirement-layout-2.log` passes, and
  `t269-wm-retirement-helper-tests.log` passes 59 tests with three opt-in peers
  ignored. The layout debt ledger is unchanged.
- `t269-rollback-current-verify.json` and `t269-rollback-previous-verify.json`:
  niltempus `b21eb20` verifies both installed releases read-only. The explicit
  retained target is `niltempus-4f498ff25c3a6d89c16f` (Sophia `2d69924a9`), whose
  activation ledger binds manifest `6c0de096e50eac78739d64c44aada29a832fcab45271c99cd7803419f2f7091d`
  and SHA256SUMS `1c7740f1cdd30e45848e85f8172c8d147c77270fba922f53773e0439b0c6494b`.
- `t269-rollback-private-mounts-2.log`: the real installer performs initial and
  repeat installation, selects a distinct release, rolls back, and preserves
  the user WM in private `/opt` and display-manager mounts. The first attempt
  used an artifact path hidden by that private `/opt`; it failed before install
  and is retained. The rerun binds the same artifact at `/rollback-release`.
- `t269-rollback-private-tests.log`: selection damage, missing selection,
  activation-history and release-tamper checks pass separately from that first
  private-mount fixture failure.

Whole-release rollback remains an explicit subsequent-login operation, never a
failed-negotiation fallback. No host installation, selection or running session
was changed by these checks. The detailed [recovery recipe in niltempus](https://github.com/sophia-org/niltempus/blob/ff6da98a20cb584cf17dba53a322683b003389bd/docs/release-recovery.md)
distinguishes ordinary rollback, which preserves personal component selections,
from explicitly using the retained release's sealed WM and desktop profile.
The latter command has source/identity evidence, not a new graphical run.

SDK contract copies are refreshed and re-vendored from signed C SDK
`0670e68b6425cdd4623f383bc368690d3f8a0b5c` and Rust SDK
`d0ac4111a47da3396e0f10fbcbbf0fbf6d878fb5`. Both revisions change only the WM
lifecycle reference, its digest and provenance; executable SDK sources are
unchanged. The Rust checksum list is relative to `spec/`, unlike the C list's
repository-relative paths; the initial wrong-directory check was corrected in
`t269-wm-rust-sdk-contract-2.log` before vendoring.

The final `cargo xtask check` at signed `e72fa171b` passes in
`t269-wm-retirement-full-2.log`: exit 0, 496 reported result groups, 6,715
reported passes, zero failures and 63 ignored tests. SDK snapshot checks,
workspace/SDK tests, clippy, layout and the remaining offline verifiers pass.
The device-hidden gate reports buffer-age and GLX/EGL hardware evidence as not
proved here; no physical result is inferred. The combined private installer
selection, rollback and tamper run also passes in
`t269-rollback-private-final.log`. This completes the Session WM worker slice;
the remaining runtime and shell compatibility source is still tracked by t269.

## Runtime WM IPC retirement (t269, 2026-09-28)

After Session's worker removal, `PolicyWmSessionTransport` and
`PolicyConnectionState` had no production caller. This slice removes those
runtime modules, their IPC-only test targets and `policy_c_conformance_host`.
The checkpoint-parent lifetime test now binds the same `RoleEndpoint` directly;
its checkpoint preservation and endpoint cleanup assertions are unchanged.
Pure capability selection, profile handoff, bounded profile I/O, generic peer
admission and Engine policy reducers remain in their existing owners.

The retained behavioral mapping is:

| Retired assertion family | Retained file/neutral controls |
| --- | --- |
| Complete policy cycle, exact startup profile prepare/activate/rollback, wrong phase and typed identity refusal | `policy_file_c_sdk::c_sdk_drives_profile_configuration_snapshot_and_policy_exchange`; `policy_file_startup::{supplied_stream_orders_negotiated_before_exact_profile_reducer_exchange,profile_identity_mismatch_or_peer_refusal_closes_admission,wrong_completion_kind_preserves_staging_and_stop_wakes_the_profile_receive}`; unchanged `policy_profile_handoff` and `policy_profile_io` |
| Capability offer/ceiling/dependencies, selection once, required capability refusal | Both pure `policy_capabilities` tests; `policy_file_startup::{missing_required_dependency_closes_without_successful_negotiation,canonical_selection_binds_once_within_ceiling_and_limits_remain_immutable}` |
| Pointer-focus and other request-cause outbound capability gate | `wm_file_controls::every_cause_uses_its_own_complete_body_and_selected_capabilities` checks every selected cause on encode and decode; `policy_file_adapter::full_worker_transports_all_commands_without_owning_their_outcomes` exercises the real worker |
| Indicator, tab, translation, launch-origin, presentation and output-launch-context section capabilities and counts | `wm_file_arrays::{every_snapshot_extension_is_gated_by_the_selected_capabilities,unnegotiated_projection_sections_refuse_before_domain_delivery,complete_prefix_and_array_truncations_cannot_be_repaired_by_the_outer_length}`; real decoder refusal in `policy_file_custody` |
| Epoch/transaction custody, duplicate submissions, no incomplete semantic delivery | `policy_file_startup::wrong_header_epoch_is_refused_before_custody_without_closing_profile_wait`; `policy_file_custody::{complete_submit_requires_permit_and_duplicate_does_not_spend_the_next_one,no_permit_never_decodes_and_accepted_replay_never_decodes_again,reactor_receive_timeout_withdraws_permit_without_delivering_fragments}` |
| Snapshot publication/pinning and bounded transfer | `policy_file_atomic_cycle` checks publication refusal is atomic; `policy_file_custody::snapshots_pin_metadata_and_qids_continue_across_epochs`; the C SDK test reads a 64-surface snapshot exceeding its 4096-byte msize |
| Silent peer, stop and fresh endpoint admission | `policy_file_pending` absolute accept expiry, exact protected PID/UID and cancellation tests; `policy_file_custody::actual_reactor_send_without_ack_has_a_bounded_deadline`; protected C recovery and replacement tests |
| IPC revision negotiation, begin/chunk/end ordering and uncounted extension tails | Retired with the wire. Files use versioned complete records, sections and immutable Limits; `wm_file_envelope` and `wm_file_arrays` enforce their bounds. This is not a claim that IPC framing survives in files |

`tools/check_policy_protocol.sh` now runs those WM file/neutral suites plus
the protected C SDK recovery and control replacement tests. Its schema-6
result identifies the 9P wire and makes no physical presentation claim.
`check_policy_client_matrix.sh` is a thin entry to the same generic gate; the
old sibling Hagia build and archived r3 runner are retired. The native-family
collector no longer requires unused Hagia/Narthex checkout arguments. Its
schema-2 report binds Sophia (including its vendored SDKs) and distinguishes WM
files, mixed shell retirement checks and the still-retained output IPC.

The old profile model instrumentation maps and WM freeze survey retain their
historical source references, explicitly marked as such. No formal refinement
claim is transferred to the file reactor. The frozen IPC codec, generator and
demo/output-client closure remain for subsequent t269 work; this slice does
not delete output coverage or satisfy t272.

Focused evidence: `development-evidence/ipc-retirement/t269-runtime-wm-file-gate-2.log`
passes, including the C SDK exchange, protected recovery and replacement.
The first launcher attempt was refused because the evidence isolation script
is not executable; invoking that same script with Bash runs the unchanged
isolation contract. That failed launch log is retained and ran no build.

At signed `a9e17ed4b`, the full repository gate passes in
`t269-runtime-wm-full.log`: 494 reported result groups, 6,693 reported passes,
zero failures and 63 ignored tests (including child-fixture results). Strict
affected-target clippy also passes in `t269-runtime-wm-clippy.log`. Hardware
buffer-age and GLX/EGL checks report no device and make no pixel claim.

The first native-family run at that head passes the WM phase, then catches a
stale shell invocation: `check_shell_protocol.sh` still names
`shell_content_transport`, removed in t265 (`680f0516e`). Its retained file
replacement is `shell_content_session_files`, introduced by `eafa78bfc` with
the original owner assertions. The invocation is corrected without changing
those tests. The failed family report and phase logs remain under
`~/.cache/sophia-ipc-retirement/family-a9e17ed4b/`.

The corrected native-family run at signed `2dcc1122a` passes all eight phases:
isolation, WM independent client, shell independent clients, protocol/runtime,
Engine owners, output owner, output client and control service. Its schema-2
report records the same clean source identity before and after. Evidence is
in `t269-runtime-wm-family-2.log` and
`~/.cache/sophia-ipc-retirement/family-2dcc1122a/report.json`; both family runs
are also retained under `development-evidence/ipc-retirement/`. The optional
operator-supplied content client is absent; the required independent C SDK
content peer passes. Output's independent full lifecycle and native acceptance
remain explicitly unclaimed. No installed release or running session changed.

The next WM deletion closure includes `sophia-wm-demo`'s output client, which
must retain its tests outside that crate, and niltempus packaging/physical
runner callers. Output is still a separate role. Those dependencies are not
permission to drop output evidence while retiring the WM IPC codec.

## Validation and remaining work

The operator accepted the [source-retirement decision](../decisions/twkn9fsp-retire-wm-and-shell-ipc-with-release-rollback-while-latency-qualification-remains-open.md)
on 2026-09-28. WM/shell compatibility removal may proceed with independent
coverage, complete code gates and verified whole-release rollback while latency
qualification stays open. Numeric budgets and the failed verdict are unchanged.

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
  gates (B, after t267, t268 and t271, under the accepted retirement decision). Descriptor production startup
  now uses files; the remaining generic socket adapter still needs deletion.
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
- [WM file contract](../../sophia-wm-files.md) now specifies the file-only
  Session launch and whole-release rollback.

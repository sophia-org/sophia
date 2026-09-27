---
id: 9cd1ie0x
date: 2026-09-26
kind: plan
tags: [plan, 9p, tooling]
---
# Publish native C and Rust desktop SDKs

## t263 — Publish versioned client libraries

Promoted by niltempus on 2026-09-27 under the
[accepted SDK decision](../decisions/gs6l7tuk-publish-native-c-and-rust-desktop-sdks-with-pinned-contracts.md).
Extract from merged master `987cfd39` (t252 source `79a09a302`). The first
release supplies the shell SDKs needed for t252 application adoption; it does
not wait for t252's attended acceptance.

Repositories: `sophia-org/sophia-desktop-sdk-c` (Codex) and
`sophia-org/sophia-desktop-sdk-rs` (Claude). Keep generic 9P separate from shell,
WM, output and administration modules. Shell ships first; releases declare
actual coverage. Use `libsophia-desktop` for the C library and reserve `-dev`
for distribution development packages. Neither SDK needs a compositor checkout.

## Scope and exit

- Inventory the Rust, C and Nim implementations and their consumers: Lom, Bemenu
  and Hagia. Preserve supported behavior and record each extraction's source commit.
- Nim can consume the C API through its foreign function interface, so a separately
  published Nim SDK is outside this plan. Evaluate C interop for Hagia against
  retaining its existing Nim client, including WM role coverage, ownership and
  build dependencies; this does not authorize an immediate Hagia rewrite.
- Define public APIs, ownership, licensing, versioning and the supported contract
  revisions. Publish independently buildable packages with examples and tests.
- Migrate consumers to pinned releases and verify each against the production
  exports, including negotiation, events, submit/ack, uploads where applicable,
  and revocation. Keep transport free of Sophia-specific role semantics.
- Keep authoritative KDL/specifications and server integration tests in Sophia.
  Keep the Go oracle independent of the SDK codecs so it can detect shared errors.
- Document dependency updates and compatibility checks across repositories.

Implementation and publication are authorized by niltempus's approved plan.
The application SDK, compositor portability and broker/portal migration remain separate.

## SDK platform policy

Approved by niltempus on 2026-09-27: design both native SDKs for Linux and BSD,
with Linux as the first supported platform and FreeBSD as the next qualification
target. Keep byte codecs, value validation, queueing and lifecycle logic free of
OS-specific APIs. Confine peer credentials, socket flags and signal handling to
small platform adapters. Preserve the distinction between the 9P2000.L wire's
error numbers and the host's errno values.

FreeBSD support requires native build, protocol, socket, retry, revocation and
peer-authentication tests in CI. Cross-compilation alone is not qualification.
Record OpenBSD and NetBSD separately when native runners and equivalent tests
exist. No general BSD support claim follows from portable source or a Linux
test pass. Initial Linux daily-driver adoption does not wait for BSD runners;
broader Sophia server/compositor portability remains t256.

## Extraction and completion

Claude extracts Rust blocking/pipelined 9P clients and neutral value types, and
the shared shell protocol crate used by the SDK and Sophia server. Server
connection/export/journal/admission owners stay in Sophia. B6c completes
catalog/indicator objects, outcomes, candidates and activation responses, and
fixes submit EAGAIN handling and custody tracking. Keep staging available after
EAGAIN; clunk after a successful submit reply cannot undo transferred custody.

Codex extracts C transport/codecs/session support, preserving the existing
same-ID EAGAIN retry. Add reusable lifecycle handling, bounded multi-record
queue admission, explicit connection selection, poll interests/deadlines and
terminal errors. Share lifecycle logic through typed values; keep UI/rendering
in applications. Retain IPC compatibility backends in both SDKs until t255.

Local queue admission, Submitted custody, semantic outcome and presentation
remain distinct. A sent request without observed custody can have unknown
outcome; never replay across epochs. Resolve EALREADY from tracked state or
fail closed. Bound raw object buffers and owned decoded data separately; never
ack past unresolved retention obligations.

SDK tests carry immutable specification copies with source revisions and
digests, verified by Sophia's integration gate. Pin Rust dependencies by exact
revision and lockfile and provision verified sources before offline gates.
C consumers use immutable snapshots with upstream revision and per-file hashes.
No gate fetches sources or reads a mutable external worktree. SDK release ->
explicit source/spec pin update -> full gate. Compatibility manifests include
SDK version, source hash, dialect, file API version and role revisions/masks.

After a tested SDK commit is pinned, Codex adopts C in Bemenu; Claude assigns
disjoint Lom and Provlita lanes. Preserve Bemenu `7d2d239` and its five unpushed
commits. Avoid Lom's active `test/t099-content-lifecycle` branch; coordinate
Provlita's Lom GPU-helper pin. Add production-export tests to all three clients.

Exit of the first release: both SDKs build independently, declared shell
coverage passes against the production export, source/spec provisioning is
reproducible offline, and applications pin the required releases. Retain literal
vectors, pipeline regressions, malformed/partial traffic, retry/disconnect,
bounded-memory, maximum-object and revocation controls, and the independent
Go oracle's exact 96-check verdict. Native rendering, installed rollout and
performance acceptance remain t252 gates.

Codex directs tracking, normative changes, integration review and compile slots.
Claude owns Sophia-side Rust dependency integration; Codex owns C snapshots,
Bemenu and the oracle. Use isolated worktrees, nice 19/jobs 2/private targets,
signed commits and herdr handoffs. Never reset gpg-agent. Authentication may
block publication but does not block local implementation.

## Dependencies and connections

### Repository boundary cleanup before the installed session

On 2026-09-27 niltempus required Sophia to remain independent of particular
shell and WM products, including tests and release tooling. The required rule
is in `docs/style-guide.md` and the Sophia agent instructions. Generic SDK and
server conformance stays in Sophia; actual desktop assembly belongs in
`sophia-desktop-integration` or the personal installer, and UI behavior belongs
in the client repository.

The first relocation moves Bemenu artifact preparation and the live Bemenu
test to the independent integration workspace at signed revision
`d39c351fa72d30eb19e1da9d064bc49e23ad8373`. It pins public Sophia crates at
`9fcaec782ce4fe9978568c0466ee17a78b3d4571`, with a committed lockfile and
explicit offline provisioning. Its artifact refusal tests pass 4/4, pin tests
5/5, and the live Bemenu check passes with two openings, three candidates,
one edit and one activation. The source is Bemenu `52a6e309`, the SDK snapshot
is `a0ab8c8`, and the prepared binary SHA-256 is
`c2b16fabd9564d506548e3181f8903abb6ee0a72a0c56651b890276f37f5d00d`.
The live log is `bemenu-files/live-52a6e30-integration.log` under development
evidence. Session decisions remain scripted; this does not prove physical
rendering, expiry behavior or launch policy.

The remaining cleanup includes product-specific discovery and preflight,
WM launch environment names, named-stack build and installed-session tooling,
and product evidence gates. Relocate each gate with its inputs and negative
controls before removing its Sophia entry point. Preserve generic policy,
shell, protected-GPU and configuration checks. New release preparation waits
for matching external adapters and WM support; an explicit checker must
produce `policy=validated` wherever the desktop previously required validation.
Publication and attended acceptance are separate from these local gate results.

The [boundary cleanup checkpoint](../milestones/htxttn94-desktop-boundary-cleanup-checkpoint-before-integration-relocation.md)
records the explicit policy-checker seam, generic WM environment and discovery,
personal installer compatibility checks, and sibling-free verifier fixtures.

The [extraction checkpoint](../milestones/i7pfnyzy-desktop-sdk-extraction-checkpoint-before-application-adoption.md)
records the first merged C snapshot, validation and the scope still unclaimed.

Prerequisites are the accepted t251 contract and reviewed t252 foundation, now
merged. This replaces the former dependency on completed t252 acceptance.
The initial SDK release supports t252 application adoption. Keep t249/t250
qualification open and t254's dependency unchanged. Later SDK role modules
follow their accepted contracts without claiming unimplemented coverage.

- [9P migration plan](jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
  owns role acceptance and compatibility retirement.
- [Shell file contract](../../sophia-shell-files.md) owns protocol behavior.
- t258 developer documentation and t260 reference clients complement packaging;
  this task owns library extraction, releases and consumer dependencies.

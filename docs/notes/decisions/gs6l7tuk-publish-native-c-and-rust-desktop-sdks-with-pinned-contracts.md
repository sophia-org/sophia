---
id: gs6l7tuk
date: 2026-09-27
kind: adr
status: accepted
tags: [adr, 9p, tooling]
---
# Publish native C and Rust desktop SDKs with pinned contracts

## Context
The clients currently live in Sophia. Rust applications pin the compositor
repository, and Bemenu vendors C sources from it. The file transport works,
but the Rust role adapters and reusable C lifecycle layer still need completion.
Independent packages let applications consume a supported client without a
compositor checkout.

## Decision

Maintain two native, independently versioned implementations:
`sophia-org/sophia-desktop-sdk-c` and `sophia-org/sophia-desktop-sdk-rs`.
Separate reusable 9P transport from shell, WM, output and administration modules.
Ship shell first and disclose actual coverage in each release. Nim may use the
C API; no separate Nim SDK or immediate Hagia rewrite is required. An
application SDK and broker/portal migration are outside this decision.

Rust's neutral shell records, validators and codecs live in the Rust SDK as a
protocol crate consumed by both client and server. Extract its blocking and
pipelined 9P clients and shared value types; keep server connection/export and
journal owners in Sophia. Neither SDK depends back on the compositor tree.

Sophia owns authoritative specifications, production integration tests and the
independent Go oracle. SDK conformance tests carry immutable specification
copies with source revisions and digests. Integration checks those digests.
Pin SDK sources by exact revision, provision verified sources before offline
gates, and require an explicit pin update and full gate for a new SDK release.
Keep IPC as an optional, tested compatibility backend until t255.

## Alternatives

Keeping SDKs inside Sophia retains unnecessary application coupling. A C-only
implementation with Rust bindings would reduce duplication, but niltempus chose
native C and Rust SDKs. A separate Rust client codec would add another
implementation; the C client and Go oracle already provide independent checks,
so the Rust client and server share a conformance-pinned codec.

## Consequences

The server gains an external protocol dependency, so exact source and contract
pins are required for reproducible offline builds. Both native implementations
must meet the same custody, retry, revocation and bounded-memory contracts.
SDK versions identify supported role revisions/capabilities as well as the
file API version; an api version alone does not establish compatibility.

Complete integration and recovery before broad optimization. Use 9P as the
experimental daily driver with explicit IPC rollback and benchmark comparison.
The refused t249 measurement remains refused; performance qualification and
IPC retirement are separate gates. Publishing an SDK does not attest native
rendering, Session admission policy or physical desktop acceptance.

## Acceptance and connections

Accepted by niltempus on 2026-09-27 in the director planning session, including
the repository names, two native implementations, role scope and daily-driver
policy. Implementation authorized by “Implement the plan.” This records that
acceptance retrospectively; it does not claim the extraction has shipped.

- [SDK implementation plan](../plans/9cd1ie0x-publish-independent-sophia-client-libraries.md)
- [Daily-driver plan](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
- [Shell file contract](../../sophia-shell-files.md)

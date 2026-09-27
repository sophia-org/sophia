---
id: 9cd1ie0x
date: 2026-09-26
kind: plan
tags: [plan, 9p, tooling]
---
# Publish independent Sophia client libraries

## t263 — Publish versioned client libraries

Requested by niltempus on 2026-09-26. After the role contracts stabilize, extract
the Rust and C clients from Sophia into independently versioned repositories
under sophia-org. Consumers should be able to use a small
client package without depending on the compositor repository.

Expose generic 9P transport separately from Sophia role records, negotiation and
session handling. Review suitable existing transport libraries before choosing
what to maintain. A language SDK may initially contain both exposed layers;
repository names and the final split are implementation decisions.

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

This backlog entry authorizes tracking only. Repository creation, publication and
consumer migration follow promotion with an owner and an agreed release plan.

## Dependencies and connections

Depends on t252 shell contract acceptance. Coordinate extraction with the remaining
role migrations before promising coverage beyond accepted contracts.

- [9P migration plan](jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
  owns role acceptance and compatibility retirement.
- [Shell file contract](../../sophia-shell-files.md) owns protocol behavior.
- t258 developer documentation and t260 reference clients complement packaging;
  this task owns library extraction, releases and consumer dependencies.

---
id: izw9opes
date: 2026-09-24
kind: investigation
status: investigating
tags: [investigation]
---
# Private Session tests can move without widening the production API

## Question

Can the Session portion of [t026](../plans/queue-11-parallel-production-readiness.md#t026)
leave production `src` without exposing private runtime authority for tests?

## Change

Starting from signed master `bf968409`, branch `layout/t026-session` moves 38
test files into `crates/sophia-session/tests/support`. Private `#[path]` loaders
keep their original logical modules and compile conditions. Relative fixture
and helper paths follow the moved files. No visibility or production API is
expanded, and existing test names remain available to external gates.

The oversized test modules are divided by scheduling, pointer gestures,
desktop profiles, snapshot visibility, private-worker lifetime, admission
refusal, receipt custody and collection/backpressure. Existing same-module
`include!` conventions preserve private helpers and stable test names. Five
test debt rows are removed; this is not a claim that the remaining production
source debt is gone. X authority's separate t026 changes belong to the other
agent and are not included here.

## Validation

All-feature Session unit tests run with devices and installed session sockets
hidden, and private config/runtime/temp directories. They pass 579 tests;
18 existing explicit opt-in tests remain ignored. This is deterministic
refactoring evidence, not fresh physical acceptance. Worktree artifacts retain
`t026-session-tests.log`, `t026-clippy.log` and `t026-layout.log`.

The first extraction exposed doc comments attached to the following item at a
file boundary. Moving comments and attributes with their items fixed those
compile errors; no test body was weakened. The production loader diff consists
only of relocated private module paths.

## Remaining work

A second slice on the same branch separates WM control/snapshot/projection
records, Engine scanout verdicts/scene checksums/projection geometry/frame
presentation, diagnostic record reduction, and desktop comparison sample
parsing/storage/attestation/process sampling. Expanding the new same-module
includes reproduces the original six files exactly apart from whitespace.
Their public facades and private helper access remain unchanged. Six more
production debt rows are removed.

This slice passes 676 protocol, Engine and conformance tests and the same
579 Session tests (18 existing ignores), all-feature/all-target Clippy for
the four affected crates, formatting, and the layout gate. Logs are retained
in the worktree under `.artifacts/t026-domain-{tests,session,clippy,layout}.log`.

The other Session/backend/renderer/Engine/protocol/conformance production
source rows and the other agent's X-authority rows still require cohesive
splits and their own validation. The task stays open until those portions
are integrated. This note retains the Session test move as one reviewable part.

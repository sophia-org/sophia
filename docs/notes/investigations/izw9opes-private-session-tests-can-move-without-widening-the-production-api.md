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

The first two signed checkpoints are `252d9801` (private tests) and `8ca09bad`
(protocol, composition and diagnostic domains).

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

A third slice extracts the persistent native-scanout module, retaining its
existing module identity, into owner records and separate construction, head
access, watchdog, singleton service, mirror retirement/presentation, frame
retirement, callbacks, renderer initialization, queue and observation files.
Output frame queuing leaves renderer-image custody in its own file. Session
argument parsing also moves behind its existing private configuration facade.
Three additional size-debt rows are removed. Existing test-only re-exports
follow the extracted owner; the layout exception records that there are no
test bodies in that production file.

The device-hidden backend/Session run passes 1,701 tests across 77 result
groups, with 37 existing opt-in tests ignored. Three source-reachability tests
initially read the old monolithic path; their source inputs now follow the
extracted functions, with all assertions retained. The successful rerun is
`.artifacts/t026-native-session-tests-2.log`; Clippy and layout logs are
`t026-native-session-clippy-2.log` and `t026-native-layout.log` in the same
directory. This does not claim physical scanout acceptance.

The next backend/renderer slice separates topology planning, resource cohorts,
apply coordination, semantic startup, preparation, installation and publication.
Visual runtime CPU/GPU cycles and renderer CPU/DMA-BUF/mixed export paths keep
their existing owner types in separate implementation files. Atomic validation
tests move beside scanout submission tests, preserving their names and helpers.
Four more size-debt rows retire. Device-hidden backend/renderer tests pass 928
tests across 49 groups, with two existing ignores; all-feature/all-target Clippy,
formatting and layout pass. Evidence is retained in
`.artifacts/t026-topology-{tests,clippy-2,layout}.log`.

The final Session slice separates public output responses/publication from
policy projection, worker startup/restart, requests and work-area handling;
layout observation/ownership from staging/settlement; and key routing from
pointer routing. The owner loop keeps its existing ordering while seat service,
native service, completion proofs and resource reporting move to their own
expressions. Private helpers handle profile evidence and route-lease service.
No test-only production API is added. All seven remaining Session size rows
retire, leaving only the separately owned X-authority rows in the debt ledger.

Device-hidden Session tests pass 911 tests across 43 groups, with 37 existing
opt-in ignores. The source-based work-area regression check also reads the
extracted policy files, retaining the original negative assertion's coverage.
It passes separately after that fixture update. All-feature/all-target Clippy,
formatting and layout pass. Evidence is retained in
`.artifacts/t026-phases-tests.log`, `t026-phases-clippy-final.log`,
`t026-phases-layout.log` and `t026-work-area-source-test.log`.

The other agent's X-authority portion and final integration remain. The task
stays open until both portions are integrated; these checks do not close any
physical acceptance gate.

## Integration candidate

The five source commits were rebased with signatures onto master `b0f7c4d7`;
the resulting source candidate is `d5299e39`. Its unified native-family gate
passes all eight phases: isolation, independent WM and shell clients,
protocol/runtime, Engine, output live owner, output client and control service.
That includes the independent C/Hagia/Narthex clients and the retained eleven
WM behavior scenarios. The gate does not supply a Lom content client and is
not t099 popout acceptance.

Durable evidence is retained at
`~/.local/state/sophia/development-evidence/t026-source-layout-d5299e39/`.
`native-family/report.json` records the clean candidate and peer identities;
the adjacent logs retain each phase. The top-level logs retain the affected
crate suites and Clippy/layout checks, including earlier failed extraction
attempts separately from the successful reruns. Formatting, metadata and
signature checks also pass. Only X-authority rows remain in the size-debt
ledger; the task remains open for that agent's separately gated portion.

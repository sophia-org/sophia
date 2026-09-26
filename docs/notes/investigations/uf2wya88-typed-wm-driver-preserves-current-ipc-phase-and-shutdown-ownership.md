---
id: uf2wya88
date: 2026-09-25
kind: investigation
status: implemented
tags: [investigation]
---
# Typed WM driver preserves current IPC phase and shutdown ownership

## Question

Can t248 expose semantic WM commands and results to a future file adapter
without moving policy authority or duplicating the current driver phases?

## Evidence

Signed source `45e61551622a3b5c1d84c36dea4e9668dbf30581` on
`session/t248-wm-adapter` extracts five Session files from accepted `611507b5`.
Logs are retained in `sophia-borders/.artifacts/t248-wm-adapter`.
All compilation and tests used the device-hidden isolation wrapper, nice19,
two jobs and the exclusively allocated disk target
`sophia-t027/.artifacts/t026-target`. Compiler paths identify sophia-borders.

## Finding and resolution

The private `PolicyAdapter` accepts semantic commands and yields decoded
configuration, dirty, projection and session-operation results. Profile
admission carries neutral identity and transaction correlation. The current
IPC adapter retains negotiation, profile handoff, transfer assembly and codecs.
Existing constructors and supervised endpoints remain unchanged.

The driver still owns command order, one-slot owner queues, response deadlines,
capability checks and shutdown. `LivePublicPolicyState` remains the only policy
reducer and settlement owner. Output-role IPC is unchanged. A malformed completed
projection is represented separately so moving its decode does not replace an
earlier phase refusal with a decode error.

The six scripted controls exercise the actual worker and driver, including
profile refusal before Negotiated, cycle/outcome/operation/receipt ordering,
transfer-phase refusals, rejected outcomes, malformed-message phase precedence,
and shutdown while the owner event queue is full. Scripted messages are supplied
semantic values, not proof of wire decoding, protected admission, a valid
reducer proposal or native completion. The existing two real IPC profile tests
and existing queue shutdown control also pass.

## Validation and remaining work

Focused worker controls: 9 passed. Full native-session lib: 620 passed,
0 failed, 18 ignored. Strict Session native-session all-target Clippy passed;
format and diff checks passed. The initial cached layout command also returned
success, but the freshly rebuilt t249 xtask later flagged the production-file
`#[cfg(test)]` mount as inline tests. That earlier result is insufficient for
layout acceptance. Matching the existing shutdown fixture, the attribute now
lives inside the external test-support file; no test body or ledger changed.
Fresh-root layout then passed; both logs are retained in the t249 evidence.
No additional worker suite was run for that attribute-only relocation before
releasing the serial build slot. These are affected-owner checks, not a full
workspace or paired Hagia acceptance claim.

The first focused run was 8 passed and 1 failed: the new refusal fixture observed
the adapter's disconnect trace just before the worker dropped its event sender,
then incorrectly required immediate channel closure. Both terminal assertions
now wait at most two seconds for actual channel disconnection. The failed log
`focused-initial-fixture-race.log` is preserved beside the passing `focused.log`;
no production behavior changed for that fixture repair.

The 9P adapter, independent record codec, staging/submit admission and paired
Hagia integration remain subsequent work. No live, device or physical run was
performed. The file-contract review requires opened-handle snapshot metadata,
bounded send pressure, no fragment-driven phase changes and cancellation that
preserves already acknowledged staging bytes. Those rules do not change this
independently reviewable current-IPC extraction.

## Connections

The accepted [9P interface decision](../decisions/1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
motivates the semantic seam while preserving admission and ownership boundaries.
The director owns task tracking and the subsequent WM file contract; this note
does not close t248 or authorize role integration.

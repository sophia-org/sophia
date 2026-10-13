---
id: 8mqltan4
date: 2026-10-13
kind: investigation
status: closed
tags: [tooling, validation]
---
# Bounded task context for agent resumption

## Question

Can agents resume from existing task and notebook records without loading entire
investigations or parent conversations? niltempus approved a bounded retrieval
pilot before evaluating Mem0's additional cost and discovery benefit.

## Implementation

`zk context` delegates to `cargo xtask context` through the managed Cargo slot.
It resolves exact open task IDs, outlines Markdown headings, and reads selected
sections with nested constraints. Full-source SHA-256 hashes identify the local
bytes. The default 12,000-byte output bound refuses oversized reads without
partial output. This is a local reader, with no model calls or memory service.

Five long task descriptions (t191, t194, t211, t220, t228) move verbatim into
owning investigations. Their shorter queue rows retain metadata and link to the
preserved narrative. Existing lane omissions are outside this change; no task
is promoted or completed by the compaction. t194 gains an owning investigation.

The t249 agent owns the plan and current investigation in its separate worktree.
Coordination through Herdr reserved those two files to that agent and t326 to
this tooling change. Its current resumption must distinguish the qualified
external pair from later master source and preserve all acceptance limits.

## Initial measurement

At Sophia `fa8aa8588ebd26202e75de0be3040395d40735fe`, the t249 plan and
investigation together contain 215,228 UTF-8 bytes and 26,439 whitespace-separated
words. `context task t249` plus a read of the investigation's
`#profile-gated-pointer-focus-2026-10-13` returns 12,792 bytes and 1,541 words,
including provenance: 94.1% fewer retrieved bytes. The task section includes its
nested acceptance conditions; the result includes supplied-boundary limits and
the next action. This comparison precedes the other agent's new resumption summary.

The five historical queue rows shrink from 16,887 to 1,916 bytes. Their complete
descriptive text is retained in the linked notes. The queue as a whole shrinks
from 53,094 to 38,416 bytes including the new t326 row.

These are text-size measurements, not tokenizer counts or measured API savings.
They omit additional contracts an agent may need, repeated reads, reasoning,
cached-input pricing and output. Mem0 processing would also need counting in a
later comparison. No provider has been configured and no notes uploaded.

## Validation and remaining work

After the t249 agent integrated signed documentation commit `0beb54d0c`, the
task section links directly to `#current-resumption` and the row dispositions.
The task read plus that resumption returns 11,425 bytes and 1,364 words including
provenance, compared with 234,299 bytes for the whole updated plan/investigation:
95.1% fewer retrieved bytes. The summary read alone is 3,069 bytes. Inspection
confirms it retains Sophia/Hagia/runner identities, the distinct `fa8aa8588`
generic evidence audit, supplied-boundary limits and the remaining measurement
gate. Implementation still requires the applicable row and contract sections.

Four focused regression tests pass: exact task selection and nested constraints;
missing/duplicate/oversized reads; fenced-code and duplicate-heading handling;
and content identity plus checkout containment. The full `cargo-slot sophia cargo
xtask check` gate exits zero against the t326 working changes, starting on
`fa8aa8588` and finishing after the disjoint t249 documentation merge `0beb54d0c`.
No runtime source changed during the gate. It includes workspace tests, SDK
checks, strict Clippy, layout, script/fixture checks and retained archive checks.
The short `zk context` alias produces byte-identical output to the direct command.
All five compacted task links resolve, and a real oversized task read exits with
empty stdout. Narrative and task-metadata preservation checks pass.

Notebook indexing succeeds; the broken-link report names 25 existing notes,
none introduced or edited by this change. Task IDs remain unique across open
and monthly completion files. Whitespace checks pass. t326's bounded-retrieval
pilot is complete; measured API-token savings and Mem0 comparison remain future
evaluation, not claimed results or newly admitted implementation work.

## Connections

- [Context workflow](../../agent-context.md) describes startup, resumption and delegation.
- [Notebook contract](../README.md) keeps normative contracts above historical evidence.
- [Work tracking](../../work-tracking.md) owns task state and evidence retention.
- [t249 qualification](5rzn1zue-wm-qualification-after-source-retirement-and-attended-output-recovery.md) is the bounded-retrieval pilot, not a new qualification run.

# Project notes and agent recall

Keep zk useful to people. Decisions, normative contracts, acceptance criteria
and evidence belong in the source documents. Keep a readable `Current resumption`
in the owning investigation, even when Mem0 is available. `todo.md` remains the
task ledger; mandatory working rules belong in `AGENTS.md`.

Write for human readers using *Dreyer's English*: clear, concise, idiomatic prose
and concrete wording. Omit needless jargon and agent-oriented boilerplate.
Preserve exact contracts, candidate identities and evidence limits.

Start with the agent instructions and README, then select the assigned task or
use `zk queue`. Read the task's scope and exit before acting. Read architectural
contracts relevant to the change; the command below does not select them for you
or override required instructions.

## Retrieve only the needed section

Run from the Sophia checkout. The command reads local Markdown without a model,
network connection, persistent memory database or new index:

```sh
cargo-slot sophia cargo xtask context task t249
cargo-slot sophia cargo xtask context outline docs/architecture.md
cargo-slot sophia cargo xtask context read 'docs/notes/investigations/5rzn1zue-wm-qualification-after-source-retirement-and-attended-output-recovery.md#current-resumption'
```

`zk context task t249` is the short alias. It accepts the same `read`, `outline`
and size-limit arguments.

`task` resolves an exact open task ID in `todo.md` and follows its `[details]`
link. `outline` lists heading anchors and line numbers. `read` returns a heading
and its nested sections, stopping at the next heading of the same or higher
level. Without an anchor, it returns the introduction before the first level-two
or deeper heading. Paths are relative to the checkout root. To follow a relative
link inside a note, resolve it against that note's directory first.

Each source has a SHA-256 digest of its full current contents, and reads include
line ranges. These identify local bytes, including uncommitted changes; they do
not attest that a candidate was tested, signed or merged. Use `git rev-parse HEAD`
and `git status --short` when preparing a cross-machine handoff. Re-read a source
whose digest changes. Git carries notes across machines; Tailscale alone does
not synchronize them.

Output defaults to at most 12,000 UTF-8 bytes, including provenance. Oversized
reads fail without partial text. Choose a narrower section with `outline`, or
explicitly request `--max-bytes=24000`. The byte bound is not a tokenizer count.
The reader supports ATX headings (`#` through `######`), ignores fenced code,
and disambiguates repeated anchors with `-1`, `-2`, and so on. Use its outline
anchors; it is not a full Markdown/HTML renderer. Setext and explicit HTML
anchors are not supported.

Use zk to discover related decisions and investigations, and fff for source and
content searches. Imported `sources/` notes are historical evidence. Check the
current contract and dated successor before using an old finding as guidance.

## Resume and delegate

Keep one short `Current resumption` section near the top of the owning active
investigation. Aim for 300–500 words: exact candidates, latest result or blocker,
what remains unproved, next bounded action, and links to the relevant evidence
and acceptance criteria. Update it at a meaningful checkpoint; retain the dated
evidence below. Do not create a second task ledger or overwrite historical results.

For another agent, send a bounded assignment:

```text
Task and question:
Checkout, branch and source revision:
Owned files and excluded scope:
Current result/blocker and remaining gate:
Contract and evidence section links:
Requested result and verification:
```

The recipient reads the referenced sections rather than inheriting the whole
conversation. Return findings, changed paths, checks and evidence limits. Record
durable findings in their owning note before handing off. Agree on file ownership
through Herdr when available; a note or task lane is not a distributed lock.

## Use Mem0 to find sources

Mem0 provides concise agent recall for project `sophia`. Store short source
pointers, useful search terms, recurring pitfalls and summaries that will help
future work. Include the source path and heading, plus its revision. For
uncommitted text, use a content hash and make clear that it identifies local
bytes, not a tested or merged candidate.

Search at task start when prior work may help, normally returning at most three
results. Verify relevant source sections with `zk context` before acting. Read
working rules from the current `AGENTS.md`; a remembered summary cannot replace
them. If memory is unavailable or unhelpful, use zk and fff.

After updating a durable source, save concise recall under a stable key when
it adds value. Read an existing entry before replacing it and pass its returned
version as `expected_version`. On a conflict, re-read the entry and source before
deciding what to save. Avoid repeated searches and unchanged writes without a
reason. Memories do not update themselves when files change.

Do not create a second task ledger, copy whole notes or transcripts, or store
credentials. Memory is a source-discovery aid, not acceptance evidence or task
ownership. Keep the human-readable handoff and exact evidence in zk.

## Measure the benefit

For t249, compare the bytes and words of its whole linked plan and current
investigation with the selected task section and current resumption section.
Check that the selected text preserves the candidate, next action, and physical
acceptance limits. This measures retrieved context size, not total API tokens or
avoided reasoning. Actual savings require agent usage records on comparable work,
including repeated reads, output, cache accounting and any memory extraction.

Include memory searches, writes and processing costs in the comparison. The
local Mem0 service is available, but availability and successful recall do not
establish token savings. Claim savings only when comparable usage measurements
support them.

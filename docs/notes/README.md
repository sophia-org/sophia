# Development notes and decisions

Sophia keeps investigations, reusable ideas, and architectural decisions in
linked Markdown files. Use `zk` to create, find, and connect them. Start with
the question you are working on; read its evidence and related decisions before
opening a new note.

## Pinned observations

- 📌 [Prove the shell lifecycle before spending operator time](concepts/t972gtpa-prove-the-shell-lifecycle-before-spending-operator-time.md) — lessons from bringing up Lom, repeated live tests, and incomplete integration evidence. Find pinned notes with `zk list docs/notes --tag pinned`.

## Find your way

- [Investigations and concepts](indexes/current.md) connects the first maintained notes.
- [Architecture Decision Records](indexes/decisions.md) explains the decision trail.
- [Milestone history](indexes/milestones.md) preserves completed work and changes of direction.
- [Milestone plans](indexes/plans.md) hold task criteria, dependencies, and exits.
- Imported research is indexed by [date](indexes/date.md) and [topic](indexes/topic.md).
- [Migration record](migration.md) explains provenance, missing dates, and old links.
- [Documentation map](../README.md) identifies the authoritative contracts.
- [Todo](../../todo.md) owns execution order and outstanding acceptance work.

Tasks use the todo.txt format inside `todo.md`. The [work-tracking contract](../work-tracking.md)
defines `zk tasks`, automatic monthly completion files, and the single source of
task state. `done.md` stays a short history guide. The [task cutover record](todo-cutover.md)
preserves the previous roadmap and its task mapping.

The notebook configuration lives at the repository root in `.zk/`. This lets
`zk` follow links between notes and current specifications. Build trees, runtime
source trees, and frozen document archives are excluded from indexing. Notes
live here; being searchable in `zk` does not change a document's authority.
The SQLite index is local, ignored by Git, and rebuildable.

## Choose the right record

| Kind | Use it for | Status |
| --- | --- | --- |
| Investigation | A coherent incident, experiment, or open question | `investigating`, `implemented`, `awaiting-physical-acceptance`, `closed` |
| Concept | One reusable insight with supporting evidence and limits | `draft`, `established`, `superseded` |
| ADR | A lasting architectural choice and its consequences | `proposed`, `accepted`, `rejected`, `superseded` |
| Milestone | A completion, retargeting, deferral, or evidence review | `draft`, `recorded` |
| Plan | Task criteria, dependencies, and measurable milestone exits | Task state stays in the task files |
| Source | An imported log entry whose claims retain their original context | `historical` |

An investigation can contain several observations. Split it when the question
changes, not after an arbitrary number of paragraphs. Extract a concept only
when it helps explain another problem. Write an ADR for decisions about authority,
ownership, lifecycle, protocol contracts, or compatibility; ordinary fixes usually
need only an investigation.

Architecture and specifications define current behavior. ADRs explain why a
choice was made. Neither an old source note nor an accepted ADR overrides a
newer normative contract. If they disagree, investigate and reconcile the
documents rather than silently treating one as permission to change behavior.

## Use zk

Run these from the repository root. `zk` is already available on the development
host; it is an optional documentation tool, not a build or runtime dependency.

```sh
zk investigate --title "Describe the incident or question"
zk concept --title "State the reusable insight"
zk adr --title "Describe the architectural choice"
zk milestone --title "Milestone N: the result or change"
zk plan --title "The milestone scope and exit"
zk queue
zk tasks ls +parallel
zk completed

zk recent
zk list docs/notes --match "startup readiness"
zk list docs/notes --tag session --sort created-
zk list docs/notes --created 2026-09-06
zk list docs/notes/decisions
zk list --link-to docs/notes/concepts/readiness-readiness-must-name-an-obligation.md
zk edit docs/notes --match "startup" --interactive
zk index
```

Creation opens `$EDITOR` or `$VISUAL`. Agents and scripts should add
`--print-path --no-input`, then edit the returned file. Interactive selection
uses `fzf`; ordinary creation, search, and indexing do not require it.
The creation aliases also work from subdirectories of this notebook.

Templates supply the ID, creation date, kind, and initial status. Keep the ID and
filename stable even if the title changes. New IDs are random to avoid a shared
number counter across concurrent checkouts. The first two ADRs have fixed seed
IDs; they do not establish a numbering requirement. An ID collision during a
merge requires renaming the newer note and updating its links.

Add one or two useful topic tags, such as `session`, `x11`, `rendering`, `policy`,
`shell`, `security`, `validation`, `tooling`, or `architecture`. Tags help discovery;
they do not confer ownership. Use ordinary relative Markdown links with `.md`
extensions and descriptive labels. Explain the relationship in the surrounding
sentence. `zk list --link-to PATH` provides backlinks without copying lists into
every note. Curated index pages supply the useful starting points.

The configuration uses zk's documented [templates and groups](https://zk-org.github.io/zk/config/config.html)
and [frontmatter](https://zk-org.github.io/zk/notes/note-frontmatter.html).

## Maintain the record

1. Search first. Continue an existing investigation when the question and
   evidence chain are the same. Add dated follow-ups; preserve failed hypotheses
   and the evidence that disproved them.
2. Record the trigger, candidate identity, relevant configuration, finding,
   correction, and verification limits. Keep raw logs and large captures in
   evidence artifacts and link their retained paths. A `/tmp` path records where
   evidence was observed; it does not promise that evidence is still available.
3. Link the investigation to related concepts and decisions. For a cross-repository
   incident, choose one owning investigation and link to the other repositories'
   commits or documents rather than maintaining competing narratives.
4. Keep a new ADR `proposed` until acceptance is established. Record the acceptance
   date and basis. An existing, documented decision may be recorded retrospectively,
   but say so and cite its original evidence. Implementation and physical acceptance
   remain separate claims. Record a rejected proposal instead of deleting it.
5. When an accepted decision changes, write a successor ADR. Mark the earlier
   record `superseded` and link both ways. Preserve the earlier reasoning.
6. Update affected normative documents and `todo.md` in the same change. Put
   actionable follow-ups in the roadmap; a note's unresolved question is not a
   second execution queue. Record milestone history with `zk milestone`, linking
   its evidence and decisions. Completion, retargeting, and deferral are different
   outcomes; a history record must say which occurred. Add useful entries to the
   curated indexes, including the milestone map.
7. Run `zk index`, inspect the changed links, and run `git diff --check`.
   Documentation changes do not need a new graphical session or a Rust build.

Task completion also requires updating its linked evidence before using
`zk tasks do N`. That command archives the short task line directly into the
current monthly completion file and refreshes zk. Do not maintain duplicate
task status in note checkboxes. The [tracking contract](../work-tracking.md)
defines ordering and safe use of the CLI's changing line numbers.

Imported source notes are frozen evidence. Append a linked investigation or
concept instead of rewriting their bodies or claiming that every old experiment
is still open. Their date and topic indexes are frozen migration views; `zk list`
provides the live date and topic views for new notes. Dates absent from historical
headings were recovered from the first commit adding each heading. Metadata and
the [date audit](indexes/recovered-dates.md) identify this basis. Archived roadmap
sections use their snapshot date. Neither basis asserts an event or completion date.

Do not append to the former research logs or roadmap-history files. They only
preserve old anchors. The <a href="../history/notebook-migration.json">migration manifest</a>
and original snapshots are also frozen.

Snapshot and manifest links use HTML anchors so they remain clickable repository
artifacts without appearing as missing Markdown notes in zk's backlink graph.

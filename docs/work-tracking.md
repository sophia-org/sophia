# Track work with todo.txt and zk

**Role:** repository work-tracking contract. Architecture and specifications
retain their authority over product behavior.

`todo.md` uses the [upstream todo.txt format](https://github.com/todotxt/todo.txt):
one task per line. Its filename stays familiar, but it contains no Markdown
headings, multiline task descriptions, checklists, or development diary.
Monthly `done-YYYY-MM.md` files beside `todo.md` use the same format for completed
tasks. `done.md` is a short history guide. The task files are indexed by `zk`.

The [milestone plans](notes/indexes/plans.md) hold scope, dependencies, and
measurable exits. Investigations hold diagnoses and evidence; concepts explain
reusable ideas; ADRs record architectural decisions; milestone records explain
completion, deferral, or a changed exit. The [notebook guide](notes/README.md)
defines those records and their lifecycle.

## One owner for each fact

| Fact | Owner |
| --- | --- |
| Open task, lane, priority, stable ID, execution order | `todo.md` |
| Dated completion and retained task link | `done-YYYY-MM.md` |
| Task detail, milestone scope, dependencies, measurable exit | Linked plan or investigation |
| Progress evidence, failed hypotheses, validation limits | Investigation or milestone note |
| Architectural choice and consequences | ADR, with current behavior in the normative contract |

Do not copy task checkboxes or an active queue into notes. A note can explain
what was observed or implemented without asserting that the linked task is
complete. Milestone completion requires the full exit, including physical
acceptance where required. An implemented slice, retargeting, and deferral are
different outcomes.

## Task format and order

A task looks like this:

```text
(A) Accept panel-only login +critical +cp14-3 @physical id:t005 order:005 [details](docs/notes/plans/queue-02-cp-14-3-development-session-readiness-and-milestone-14-c.md#t005)
```

- `(A)` is the critical-path priority; `(B)` is admitted parallel work.
  Candidates and deferred tasks have no priority until promoted.
- Exactly one lane project is required: `+critical`, `+parallel`, `+candidate`,
  or `+deferred`. Other projects identify milestones or domains, such as
  `+cp14-3` and `+cp15-1`.
- Contexts describe the work: `@physical`, `@development`, or `@planning`.
- `id:` is a stable, repository-unique task identity. Keep it when a task moves
  or completes. Never use the CLI's changing display line number as a durable ID.
- `order:` is the reviewed ordering key. Choose the lowest open order in
  `+critical` unless the user names another scope. Preserve the CP-14.3 stage
  ordering, then CP-15.1 and CP-15.2, and their linked exit conditions.
- The Markdown link points to the task's full criteria and evidence. It is
  ordinary task-description text to todo.txt and a traversable link to `zk`.
- `depends:tNNN,tMMM` names local prerequisites. `peer:lom/tNNN,tMMM` names
  paired work in Lom; identities remain repository-local. Shared evidence may
  satisfy distinct exits, but completing a peer never closes a local task.

The repository's `id:` and `order:` conventions use todo.txt's permitted
`key:value` metadata. Upstream clients preserve them but do not enforce Sophia's
gates. Priority sorting alone does not establish readiness or satisfy a dependency.
The configured CLI keeps file order; keep that order consistent with `order:`.
If an external client reorders the file, use the keys and restore the reviewed
order before committing. IDs stay stable when ordering keys change.

Parallel work may proceed only within its admitted scope and without delaying or
weakening the critical path. Candidate work must be explicitly promoted with a
driver and measurable exit before implementation. Deferred work is outside the
current scope. A historical unchecked item does not become a new task by being
imported into the notebook. A later row does not authorize bypassing an earlier
gate. User-selected work takes precedence over the default queue selection.

## Commands

Run these from this checkout or a subdirectory:

```sh
zk queue
zk tasks ls +parallel
zk tasks ls +candidate
zk tasks ls id:t005
zk edit docs/notes/plans/queue-02-cp-14-3-development-session-readiness-and-milestone-14-c.md
zk investigate --title "Describe the incident" --print-path --no-input
zk milestone --title "Milestone 14: retained evidence review"
```

`zk tasks` runs the unmodified [upstream CLI](https://github.com/todotxt/todo.txt-cli)
with `.todo/config`, then runs `zk index --quiet` after a successful command.
`zk queue` lists the critical lane through the same path. Configuration derives
the task paths from its own checkout, so commands from an isolated checkout do
not edit another checkout's queue. No global todo configuration is changed.

The development host has the CLI at `~/src/todo.txt-cli/todo.sh` and the format
reference at `~/src/todo.txt`. Set `SOPHIA_TODO_CLI` to another installation's
`todo.sh` path if needed. The inspected references are CLI commit `105fae6d`
and format commit `1d90c086`; they are external documentation tools, not Sophia
build dependencies or vendored product code. The aliases use `/bin/sh`, so
they also work when the user's login shell is Fish.

For a new task, search existing notes first. Reuse the relevant plan or create
an investigation with `zk`; fill in its scope and criteria, then add a short task
line with an unused stable ID, its admitted lane, ordering key, and Markdown link.
The CLI prepends the creation date on new tasks. Imported tasks omit creation
dates because cutover does not establish when the work was first proposed.

## Finish and retain evidence

Before completion, update the linked note with the result, exact candidate and
configuration where relevant, validation, and remaining limits. Update affected
normative documents when behavior or an architectural decision changes. A new
ADR starts proposed; record its acceptance basis and preserve later changes in
a linked successor.

Find the task again by stable ID, inspect its current line number, then complete
that line through the CLI. For example, after `zk tasks ls id:t005`, run
`zk tasks do N` with the displayed number. The CLI adds today's completion date,
preserves the task ID and note link, and moves the line directly to the current monthly completion file. The alias
then refreshes `zk` so backlinks reflect the move. Re-read the row before any
number-based mutation when another agent may be editing the queue.

This is synchronization through a single task record and retained links, not
two independently editable status lists. Do not mark a task complete merely
because its note exists. The CLI cannot decide whether evidence satisfies a
milestone exit. Direct editor changes remain supported; run `zk index` afterward.

Keep `todo.md` limited to short, actionable lines. Put narratives and progress
updates in notes when they are written, rather than waiting for completion.
`done.md` remains a small guide. The CLI selects
`done-YYYY-MM.md` from the current local month and writes
completed task lines there directly. Monthly rollover needs no manual cleanup
or copied ledger. `zk completed` lists these files; detailed results stay in the
linked notes. If an unusually busy month's file becomes awkward to read, split
its older complete lines into dated `done-*.md` files in the same directory, retaining IDs,
dates, and links, then reindex. Only the current month's canonical file receives
new completions. Keep each completion record in one place. Completion files stay
beside `todo.md` because the upstream CLI copies the Markdown link unchanged;
moving them into a subdirectory would change the link's base path.

## Validation and handoff

For product work, preserve architecture, authority, metadata-disclosure, passive
data, and protocol boundaries. Model temporal or ownership changes before
implementation where required. Keep meaningful regressions outside production
`src`; do not weaken privacy to make tests convenient. Run `cargo xtask check`
for code changes and the named physical gate for hardware claims. Documentation
changes need inspection, whitespace checks, and link checks, not a new session.

Retain exact source, binary, configuration, topology, and evidence identity for
promotion claims. Review the impact of a new candidate before reusing old
evidence. Keep old evidence under its original identity and rerun affected
workflows; an ordinary defect does not reset unrelated completed work or restart
the 36-row comparison campaign. Use the [validation contract](validation.md).

For each handoff, state the task ID, result or blocker, remaining gate, and note
path. Inspect `zk tasks ls id:ID`, the linked note, and the diff before committing.
Run `zk index`, `zk list docs/notes --broken-links`, and `git diff --check` for
tracking changes. Task IDs must be unique across `todo.md` and all `done-*.md` completion files;
each open task needs one lane, one ordering key, and a resolvable note link.

The [cutover record](notes/todo-cutover.md) preserves the former roadmap and maps
every imported task. The old checked rows remain historical evidence in `zk`;
they were not assigned fabricated completion dates in the new completion files.

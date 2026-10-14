# Administrative file records

Status: t254 codec foundation. No administrative 9P listener, client migration
or default change is implemented by this document. The existing control service
remains active. The shared-core export must preserve its HostDomain admission,
command catalog and execution owners before these records can be used.

## Version 1 line grammar

This version names the file syntax, independently of legacy control-wire
revision 2. It does not downgrade the current command catalog or add commands.

The future `ctl` file accepts one invocation record. Its corresponding `status`
record describes an outcome; accepting bytes is not a successful command result.
These are separate files with one versioned grammar each, not the old socket
envelope inside a 9P write.

```text
v1 invoke CONNECTION REQUEST GENERATION OWNER LENGTH:NAME\n
v1 outcome CONNECTION REQUEST GENERATION OUTCOME LENGTH:DETAIL\n
```

Here `\n` means one LF byte. Fields before the counted text have exactly one
ASCII space between them. CONNECTION, REQUEST and GENERATION are nonzero u64
values written in canonical unsigned decimal: no sign or leading zeroes.
LENGTH uses the same decimal spelling but allows zero and counts UTF-8 bytes,
not characters. It must equal the complete remaining text before LF. There is
no escape processing, shell expansion or trailing padding.

Every record is at most 512 bytes including LF. NAME is 1–128 bytes, using the
existing control-name rules: ASCII letters, digits, spaces, hyphens, underscores
and dots, with no leading or trailing whitespace. OWNER is `policy` or
`session`. Syntactically valid names still require catalog membership and an
admitted grant; parsing a new name never creates an operation. Existing session
operations and registered policy actions retain their existing owners.

DETAIL is at most 256 UTF-8 bytes and may be empty. Control characters are
forbidden, including CR, embedded LF, tab and NUL. OUTCOME is exactly one of
`committed`, `completed`, `unchanged`, `rejected`, `stale`, `denied`,
`unavailable`, `overloaded`, `timed-out` or `indeterminate`. Preserve the existing
distinction between policy commit, Session completion and uncertain effects.
The codec does not decide which outcomes an operation may legally produce.

Examples, each followed by LF:

```text
v1 invoke 7 9 12 policy 8:next tab
v1 invoke 7 10 12 session 10:restart-wm
v1 outcome 7 9 12 committed 0:
v1 outcome 7 10 12 indeterminate 19:completion not seen
```

## Export obligations still to implement

The connection value must match the admitted attach, not select another caller.
The owner must verify catalog generation and action identity immediately before
dispatch, preserve monotonically scoped request identities and refuse replay.
Current code keeps at most sixteen pending administrative tickets; migration
must retain bounded custody rather than introducing an unbounded line queue.

Pin catalog/discovery and initial status schemas, open modes, bounded partial
write staging, nonzero-offset behavior, result retention and acknowledgement
before implementing the export. A codec consumes exactly one complete record;
it is not a stream accumulator. Oversized or malformed writes must fail before
dispatch. No 9P flush, clunk or disconnected reader may undo a committed effect
or authorize a retry after an indeterminate result. Admission must be rechecked
at the existing claim boundary. Read-only inspection grants no command access.

Use existing `ControlCommand` and `ControlOutcome` values. They currently live
with the legacy control codec and must move to a neutral module before the old
wire is retired; no duplicate command executor belongs in the file adapter.
The deferred output-process invocation extension is excluded.

## Validation

`sophia-protocol/tests/admin_files.rs` checks independent literal records,
non-ASCII byte counts, command spaces, exact bounds, every outcome, zero and
overflow identities, noncanonical numbers, unknown versions/owners/outcomes,
missing or duplicate line endings and embedded control characters. These are
codec controls, not proof of admission, replay prevention or live execution.

The [t254 investigation](notes/investigations/t75i0gx8-define-bounded-administrative-file-records-before-migrating-control.md)
records implementation progress and remaining gates.

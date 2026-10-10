# Session lock coverage diagnostics

This is a Session diagnostic contract. The separate
[lock-provider contract](sophia-lock-files.md) defines provider records and
authority; SDK snapshots carry that wire contract, not this internal record.

Session's `sophia_live_session_lock schema=1 status=covered` record is an
observation of Engine retirement, not a provider record or a new lock-state
transition. It names `epoch`, `topology_epoch`, `owner`, `outputs` and `heads`.
`owner` is the nonzero, session-local native retirement owner identity, read
from the same current native owner that supplies the proof. Every
enabled head of every current output must have retired a frame containing
the current lock's cover; prepared frames, an older lock, duplicate heads,
suspended scanout and an unavailable frame service cannot supply proof.

The topology must be settled and publicly committed: no installation
transition, hardware publication or policy candidate may be pending, and
the installed and published epochs must agree. Session deduplicates by lock
epoch, topology epoch and current native owner. A replacement owner on an
unchanged topology owes its own cover proof and record. A repeated observation
of the same tuple, an older topology within the lock, or an absent or zero
owner emits nothing. A lock already in the Locked phase can therefore report
coverage again after output loss or return, without changing lock or unlock
authority. This diagnostic does not replace the initial all-head proof required
to enter Locked.

Daily capture retains the approved lock status, source, reason and verdict
words, and bounded numeric fields including `owner`, `outputs` and `heads`.
It drops free-form errors and unapproved values. This is a bounded capture
filter, not a complete record validator: consumers must require their expected
schema and all fields needed for their proof.

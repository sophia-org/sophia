---
id: eh89zpqc
date: 2026-10-15
kind: milestone
status: recorded
tags: [milestone]
---
# Administrative commands accepted on the shared 9P core

## Result

[t254](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t254--migrate-administrative-commands)
meets its exit. Existing administrative commands now use a separately authorized
export on the shared 9P core. Session and the CLI select it by default while
preserving HostDomain admission, command owners and explicit legacy rollback.
Read-only inspection gains no command authority.

## Evidence and decisions

The [investigation](../investigations/t75i0gx8-define-bounded-administrative-file-records-before-migrating-control.md)
records the lifecycle models, denial/replay/revocation controls, bounded client,
independent Go oracle, process-loss tests and final repository gate. All eight
performance blocks pass the declared limits at `639dab440`; the administrative
runtime is unchanged in installed source `ed0caa4f254438925038872aaf08f959a3ab7551`.

On October 15, release `niltempus-b7f017d6e4cdb2056faa`, prepared by desktop
integration `a6eb56d7ff32f0dd900eb61575d91c47fb1da538`, passed discovery,
`focus-next` with a committed outcome, and WM replacement with a completed
outcome. The Session survived. niltempus confirmed continued normal desktop,
keyboard and mouse operation. This is the t036 smoke required by t254.

Raw live evidence is in
`~/.local/state/sophia/development-evidence/t254-installed-20261015/`.
The investigation links qualification samples, full checks, release checksums
and installer rollback tests. The verified preceding release
`niltempus-eb8be6b503fa8bca3815` remains installed as `previous` for rollback.

## Limits

This accepts administrative transport and its rollout, not broader engine or WM
latency qualification. The live smoke did not switch back to the old release.
Service hot replacement and durable exactly-once results across a crash are not
provided; clients preserve uncertainty and never replay automatically. The
output invocation extension remains deferred. Further work stays in the
[task ledger](../../../todo.md).

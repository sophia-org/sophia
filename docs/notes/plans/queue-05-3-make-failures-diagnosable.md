---
id: queue-05
date: 2026-09-06
kind: plan
tags: [plan, milestone]
---
# 3. Make failures diagnosable

This plan retains the scope, constraints, and task details from the roadmap
cutover. Task status and order live only in [todo.md](../../../todo.md)
and the [monthly completion history](../../../done.md). Follow the
[work-tracking contract](../../work-tracking.md).
Historical candidate identities in the details require revalidation before use.

[Parent scope](queue-02-cp-14-3-development-session-readiness-and-milestone-14-c.md).



## t015

Reuse existing telemetry for identifiable per-session logs and bounded
resource observations; preserve diagnostics after abnormal exit.


## t016

Provide a simple incident-time marker and document how to find the
matching build, configuration, session, and surrounding events. Keep expensive
tracing and pixel inspection opt-in; retain metadata-disclosure boundaries.


Exit: a reported problem can be investigated without reproducing it merely to
recover an overwritten log. Extend existing session/tooling owners rather than
building a separate monitoring platform.

## Implementation and acceptance

The implementation and deterministic evidence are recorded in the
[daily diagnostics investigation](../investigations/e84g9ivq-durable-daily-session-diagnostics-and-incident-markers.md).
The [operator contract](../../operations.md#mark-and-investigate-a-problem)
owns command syntax, limits, privacy, and the distinction from proof archives.

Both tasks require the same physical exit: in a replacement installed session, mark
an event from an independent TTY, log out, log in again, and inspect/preserve
the earlier session by ID. The current session must remain usable while marking.
This is a normal-use canary, not a resumption of the comparison matrix. A passed
deterministic test does not close this physical exit or earlier milestone gates.

The first installed attempt ended during VT preparation and exposed missing
CLI installation and failure fields. The [incident investigation](../investigations/tnf5xqrb-vt-handoff-failure-exposed-missing-diagnostic-causes.md)
records the repaired diagnostics and the subsequent multi-output renderer
handoff correction. The installed `4b4f2841` round trip and marker written while
the seat was suspended passed. The subsequent logout/login and retrieval of
that exact marked record also passed, as documented in the
[acceptance record](../milestones/v4ycp9ba-daily-session-diagnostics-accepted-across-logout-and-login.md).

## t309

Preserve a structured cause when startup refuses an output profile on the
ordinary supervised login route. The 2026-10-07 daily login returned to
greetd with only a failed Session result. A bounded proof-route retry with
private raw output captured `UnknownConnector("DP-2")` after that monitor
was unplugged; the original daily cause cannot be recovered. See the
[incident and profile correction](../investigations/ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md#subsequent-hardware-change-and-login-failure).

Keep the existing profile refusal semantics. Record a bounded failure kind
and phase before returning the error, through the ordinary diagnostic
capture. Decide the connector field under the existing metadata-disclosure
rules; do not retain arbitrary error strings or client data as a shortcut.
The fix must not depend on raw stderr or on the proof launcher.

Exit: a missing named connector produces a classified cause in the durable
ordinary-session record, without a successful startup or an opt-in trace.
Controls must cover that route and keep a valid profile accepted. Retain the
failed login and proof retry as separate evidence; neither proves the
ordinary route is repaired. Card isolation and automatic fallback to another
profile are outside this task.

The subsequent live login reports suppressed Present-delivery records. The
recorder names `sophia_x_present_delivery` as having spent its 3.75 MiB share
of a 15 MiB segment; the source is identical to installed `825d91460`. This
is a per-name byte quota, not a time-based limit or a shared failure quota.
The failed login had `suppressed=0`, so this does not explain its missing
cause. Read-only evidence: `igpu-login-exit-20261007/RECORDER-SUPPRESSION-01`.

Quota independence does not guarantee terminal-cause persistence. Failure
records use the ordinary 256-entry nonblocking queue; only identity/profile
records have the separate priority queue. Extend the controls above to a
spent Present quota and queue pressure, and verify the approved failure kind
and phase survive record reduction. Keep terminal-cause storage bounded and
TTY recovery independent of a wedged diagnostic writer.

---
id: queue-08
date: 2026-09-06
kind: plan
tags: [plan, milestone]
---
# CP-15.1 — Native protocol-family lifecycle audit

This plan retains the scope, constraints, and task details from the roadmap
cutover. Task status and order live only in [todo.md](../../../todo.md)
and the [monthly completion history](../../../done.md). Follow the
[work-tracking contract](../../work-tracking.md).
Historical candidate identities in the details require revalidation before use.

[Parent scope](queue-01-critical-path.md).



## t022

Audit `sophia_wm_v1`, `sophia_shell_v1`, and `sophia_output_v1` against
`docs/sophia-policy-ipc.md`.

The [2026-09-18 native desktop audit](../investigations/vle7mt47-native-desktop-capability-audit-separates-contracts-from-client-ui.md)
adds a source-backed developer capability matrix, evidence classification and
deduplicated follow-ups. It does not close this family-level audit. Remaining
work must reconcile every role's lifecycle, extract the missing declarative
output schema and test its codec equivalence, and document each intentional
transport difference. No new desktop UI or toolkit port is needed.


Required exit:

- align hello/welcome negotiation, effective bounds, capabilities, epochs,
  transaction identity, complete transfers, outcomes, recovery, and extension
  handling;
- document every intentional role-specific difference in its role contract;
  and
- remove or explicitly version accidental transport forks without weakening
  the frozen WM revision.

The [2026-09-24 family audit](../investigations/v2yrv8je-native-family-audit-makes-output-layouts-explicit-without-changing-frozen-wm-bytes.md)
records the extracted output schema, codec equivalence tests, role lifecycle
differences and validation. It preserves experimental shell/output status;
the family conformance entry and independent lifecycle proof belong to t023.

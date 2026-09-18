---
id: n8r3d6qp
date: 2026-09-18
kind: investigation
status: repaired-pending-native-acceptance
tags: [investigation, native-shell, dock, launcher, placement]
---
# Catalog launch must capture the clicked output workspace

The operator reported a DP-1 dock click opening Terminal on DP-2 unless they
first changed the focused desktop. Retained capture
`.artifacts/lom-panel-native/20260918T161411Z` records four persistent catalog
launches, one on logical output 1 and three on output 2. These records establish
launch provenance, not physical connector mapping or eventual window placement.

Source shows that native catalog payloads retained the clicked output, but no
placement bookmark was captured. Hagia's ordinary new-window placement uses
the active output. Explicit registered placement classes also use that output;
native catalog intents themselves have no class. This distinguishes the gap
from incorrect dock hit-testing and from the earlier LookupColor failure.

The repair extends the existing opaque launch-origin mechanism with a committed
per-output bookmark, including empty workspaces. Session captures it at queue
admission and joins the verified process to its first top-level. Hagia alone
resolves workspace membership. Content and WM generations remain separate.
See [the normative contract](../../sophia-policy-ipc.md#output-bound-catalog-launches).

Validation logs are `.artifacts/dock-smoke/placement-*`. Private protocol/WM
fixtures use simulated surface admission; they are not owner-loop/KMS acceptance.
The actual dock/menu socket controls retain supplied presentation evidence.
The next attended smoke must prove the launch-to-committed-placement join on
both monitors, without switching desktops first. No physical run, installation
or live endpoint probe is part of implementation validation.

Scoped validation passed: 939 Rust checks in the affected-library run (17
ignored), then 30 focused boundary checks and 30 diagnostics/verifier/private
socket checks after the final refinements. These overlap; do not sum them as
unique tests. Strict affected Clippy, formatting and layout passed. Hagia's
178 policy controls and four independent wire controls passed, with its layout
gate. C and Rust consume the same regenerated 17-record WM corpus.

The explicit private Hagia socket test covers an empty destination, focus change,
workspace change during startup and rejected-then-committed placement. The
surface/process admission is supplied by the fixture. A separately compiled
Hagia mutant disabling origin interpretation fails its exact output assertion;
the original binary passes afterward. Initial compiler/fixture setup failures
are retained separately in the logs and are not passing evidence. Hagia's signed
implementation is `90590a5`; exact Sophia canonical validation follows its signed
checkpoint and is recorded separately under `.artifacts/`.

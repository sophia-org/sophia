# Shell reconnect budget: bounded admission model

Source baseline: signed `69e16358a6a576a6668b0d74f5a5c18f8cb4a329`.
Owning task: Sophia t100. The investigation and retained production-owner
reproduction are in
[vup982br](../../docs/notes/investigations/vup982br-retained-panel-pixels-can-block-fresh-component-admission-after-disconnect.md).
This document reviews a proposed arithmetic change; it does not close t100.

## System and analysis scope

Category B: runtime ownership and reclamation. Session serializes grant
reservation and disconnect. Renderer consumers can outlive a connection;
their eventual release is independent of admission and peer notification.
The registry accounts full active allowances and actual disconnected storage.
The proposed change chooses tighter initial allowances at the existing atomic
reservation boundary. It introduces no owner, message, grant mutation or new
retirement transition.

The two external implementation/review lanes independently inspect resource
storage and Session admission, and client consumption of welcome limits.
Local history has seven commits affecting the registry or component connection
owner, from `c7dea19a` through `06fa46b8`; these establish shared custody,
independent component admission, three-owner capacity and role partitions.
The current uncorrected circular wait has a compiled owner regression. This is
a bounded repair review, not a whole-repository bug hunt: no external issue or
PR claims are used.

## Scenarios

### Retained pixels prevent the grant needed to replace them

`shell_component_connections.rs:140` reserves a fresh full role profile.
`shell_content/epoch_registry.rs:125` admits only if active reservations plus
actual retained storage fit. The two-role profiles total 64 MiB before any old
storage is added. Four retained bytes therefore prevent a replacement even
after old native work has completed. The existing component reconnect test
drives the real service, private transport and backend custody, with explicit
simulated native completion.

Model the initial allowance decision, collection and later replacement as
separate operations. Never turn a resource accounting adjustment into evidence
that a frame completed or an input receipt became current.

### One slot consumes another disconnected slot's replacement allowance

Fitting against global free space alone can let one slot consume a second
slot's future allowance. Session already selects unique roles
(`shell_component_connections.rs:107`), mapped to unique store profiles
(`:172`). Preserve each profile's nominal envelope, subtracting *all* of its
retired generations. This also covers repeated successors that fail before
their old storage drains. No extra per-slot accounting ledger is needed.

### A valid grant admits an image but cannot upload its replacement

`ContentLimits::validate` permits resident capacity equal to one resource.
Lom's scheduler includes currently resident bytes when checking its next upload.
A one-resource resident floor can therefore accept a panel and strand its next
update. Keep the existing maximum resource size and require resident capacity
for two such resources. This is a generic minimum, not a promise that arbitrary
multi-output workloads fit. Existing clients must honor all advertised limits.

## Proposed allowance selection

For the requested nominal profile, let `M` be its unchanged maximum resource
size, `S0/R0/T0` its staging/resident/retiring ceilings, and `Q=S0+R0+T0`.
Let `P` be the sum of every retained source charge for this profile, not just
the immediately preceding grant. Let `U` and `V` be all existing source and
backing reservations. Global source and backing caps are `C` and `D`.

After real collection, use checked subtraction and four-byte alignment:

```text
A = min(Q - P, C - U)
B = min(R0 + T0, D - V)
require A >= 4*M and B >= 3*M
R = min(R0, A - 2*M, B - M)
S = min(S0, A - R - M)
T = min(T0, A - R - S, B - R)
require S >= M, R >= 2*M, T >= M
```

Round usable capacities down, never up. Validate the entire resulting
`ContentLimits`, then use the existing registry reservation transaction.
`max_session_retiring_bytes` remains exactly the registry source cap.
All non-byte fields, maximum resource size and fresh grant identity remain
unchanged. Failed admission does not publish a transport allowance or alter an
existing grant. Session's already-minted attempt numbers remain burned.

## Safety argument and executable obligations

* `S+R+T <= A <= C-U`: a successful grant cannot exceed global source credit.
* `R+T <= B <= D-V`: resource backing credit stays within its independent cap.
* `S+R+T+P <= Q`: repeated same-profile predecessors stay in that role's
  envelope. If all selected nominal envelopes sum to at most `C`, another
  role cannot consume this role's envelope by reconnecting first.
* Disconnect cannot increase that envelope. `ContentResourceStore::revoke`
  (`resources.rs:486`) aborts all transfers. `abort` (`:364`) returns staging,
  reserved-resident and associated backing credit. Remaining source storage is
  resident plus retiring, bounded by the old `R+T`.
* Retired resource collection (`resources.rs:453`) requires the last actual
  consumer to have ended. Retired usage can decrease, but fresh requests cannot
  grow it. Native claims, source consumers and displayed reservations retain
  their existing independent owners.
* No fresh profile appears at a failed arithmetic or count check; successful
  admission still reserves its future retirement slot. Three active grants and
  sixteen total active/retained epochs remain the existing bounds.

Pure controls must check unchanged cold grants, exact four-byte deductions,
alignment, both global caps, the full-resource floor, stale identities and
atomic refusal. Enumerated role/order/retention cases should check the envelope
inequality, not merely reproduce the selection expressions.

Joined controls must turn the original two-role progress red green, keep its
old evidence, exercise all three connected roles and repeated retained epochs,
and verify that only actual replacement changes reservations/input. Saturated
admission must have a positive counterpart after genuine collection. Compiled
mutations should remove the profile debit and restore full-profile admission;
the corresponding conservation/progress controls must fail for their stated
reason.

## Progress and limits

If a profile's retained charge leaves at least its useful floor and other
global/count constraints permit it, a fresh grant fits without waiting for
the displayed predecessor to disappear. Completion still depends on a valid
client candidate and real renderer progress. Capacity recovery is not implied
when consumers never release, all retirement slots are occupied, or a workload
exceeds the negotiated profile. Refusal must be explicit and observable.

The existing scheduler provides a bounded retry *rate*, up to a minute between
attempts; it does not bound time to successful recovery. This repair must not
label indefinitely unavailable capacity as guaranteed eventual progress.

Registry backing is conservative credit per resource, not measured copies or
VRAM. Native head buffers have separate owners and bounds. Mirrored heads do
not multiply the source lease's byte charge. This arithmetic cannot establish
physical KMS completion, native memory residency or driver behavior.

## Verification method and exclusions

The new decision is pure integer arithmetic at an existing serialized owner.
Exhaustive bounded admission controls and the algebra above target that change;
the existing `ShellContentLifecycle` model still owns resource conservation and
revocation. Its byte-unit abstraction does not prove this arithmetic. No new
TLA transition is proposed while the repair leaves lifecycle ordering intact.
If implementation adds mutable allowance history, collection-triggered
scheduling or different release authority, revise this brief and model those
transitions before implementation.

Source-only review must confirm role/profile uniqueness for the entire Session
lifetime and inspect actual client handling of initial limits. Tests, rather
than a model, establish wire welcome values, real private-owner invocation,
diagnostic reduction and the two-role reproduction. GPU acceptance, arbitrary
client throughput, new image-size policy and mid-connection renegotiation are
excluded. The broader t069/t097/t100 exits stay open.

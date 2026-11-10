---
id: p8cn6w2r
date: 2026-11-09
kind: investigation
status: investigated
tags: [investigation, performance, portability, custody, sft]
---
# Sealed images do not by themselves remove the validation service

## Finding

Stop the proposed sealed-image substitution at this feasibility read. No runtime
change or native run is selected. Immutability can remove a source-mutation
hazard; it does not establish pixel validity, grant authority, budget ownership
or renderer eligibility. A design that removes the validation return must name
where each of those obligations runs, not merely substitute the backing file.
This does not reject every immutable-source design.

The current shell CPU path is the reusable owned-copy ring, not the older t338
sealed-per-image path. [experimental-owned-ring](../../experimental-owned-ring.md)
records its selection as the sole shell CPU upload path and the r10 refusal of
sealed shell ingress. The older [image-channel diagram](../../sophia-image-channel.md)
remains useful history for shell r9 and separate role contracts; its own diagram
includes a worker validating premultiplied BGRA before retaining immutable backing.
It therefore does not supply an existing worker-free acceptance path.

## Whole-lifecycle comparison

| Obligation | Current owned-copy ring | Historical sealed-image alternative |
| --- | --- | --- |
| Producer storage | Reusable registered slots; exact source release permits reuse | Prepare and fill a fresh immutable backing; source release does not unseal it |
| Transfer | One registration descriptor; per-offer control | Per-image admission and descriptor transfer in the historical protocol |
| Source safety | Worker preads into private storage; producer mutation cannot alter the accepted destination | Owner checks exact backing and required seals; immutable backing may be retained |
| Pixel validity | Validate copied bytes under bounded worker credit | Historical path still validates BGRA on a worker |
| Authority and budgets | Owner sequences grant, resource, destination and work-credit obligations | Same obligations need a defined owner even if storage is immutable |
| Rendering | Existing composition/upload and render-result handoff | Seals alone change neither renderer access nor render-result publication |
| Reclamation | Slot release distinct from resource release after consumer lifetime | Source-release receipt distinct from final retained backing/resource lifetime |

Removing the private copy is not equivalent to removing its worker service. Moving
validation into the renderer would change the intervening owner admission and
settlement boundary. Moving it onto the owner would change the thread/responsiveness
contract. Neither is selected by this note. A producer-created validity assertion
cannot replace the owner's validation of untrusted pixels.

## Prior measurement, with its boundary

Read-only input: `~/.local/state/t331/d4p-03/FINDINGS.txt`, October 21.
It reports sealed mean [1872.5,1912.5] and ring mean [1580,1620] combined
Session+peer microseconds per frame, ring-minus-sealed [-332.5,-252.5].
The run used RX7900 GRE / DP-1, a 1920x24 BGRA panel, one outstanding frame,
two reusable slots and the older protocol/source pair. Both processes improved.
The finding attributes no individual syscall or copy cost and proves no latency
equivalence. This note reads the saved findings; it does not independently recount
the raw run or requalify it on the current lean iGPU lane.

That result is a reason to include producer preparation, per-image descriptor
lifecycle and reclamation in any renewed comparison. It is not a prediction that
all future immutable-image designs lose, nor a bound on their performance. Do not
subtract its numbers from current hp-01/dcp-01 observations.

## Portability and reopening

Follow [fp9o6pdb](../concepts/fp9o6pdb-portable-plan-9-interfaces-with-native-frame-mechanisms.md):
keep portable custody meaning separate from backing/transfer/wakeup mechanisms.
Linux admission requirements do not establish equivalent protection on a BSD,
Redox or a future OS. Each proposed adapter must prove its backing cannot change
for the entire accepted lifetime, exact extent, revocation and reclamation rules.
A ring, URL or handle name alone supplies none of those guarantees. This read
adds no platform trait, dependency or support claim.

Reopen only with a specific alternative that names an eliminated service, places
all validation and fresh authority obligations, accounts for producer and owner
lifecycle costs, and preserves responsiveness. First review that contract; then
write a device-free proof. Any later native comparison needs separate selection.
The [T4 checkpoint](l4ruwq60-worker-side-native-submission-needs-a-new-authority-and-lifetime-contract.md)
remains parked; this study does not solve its stalled-worker progress, retained
submission ownership or pre-ioctl completion coverage. M1/M2 remain deferred.

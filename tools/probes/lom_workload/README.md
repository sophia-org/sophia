# Lom causal workload evidence

This verifier checks the action-latency part of the planned two-output workload.
It also requires the exact final content-shutdown accounting record.
It is **not yet the complete attended gate**: it does not establish measured cadence,
process/GPU memory plateau, session recovery, GPU grant correctness
or healthy process lifetimes. Run those separate checks too. The current tty4
launcher has not yet been upgraded to this workload.

The accepted workload shape is 60 seconds with 40 state-changing workspace
actions, 20 per output. Warmup and numerical ACK/native latency limits must be
written to a budget JSON file before the run. No historical numerical approval
was recovered; the numbers in `tests/fixture.py` are test values, not such an
approval. The budget file requires exactly these positive integer keys:

```text
warmup_usec, duration_usec, actions_per_output, output_count,
ack_p95_usec, ack_max_usec, native_p95_usec, native_max_usec
```

The observation window starts at the first issued action and lasts the declared
60 seconds. Every output must have completed its warmup before that first action
and keep presenting through the end. No clicks belong in warmup. All 40 intended
state-changing actions must complete; missing, rejected, no-op and timed-out
actions fail rather than disappearing from percentiles. Cancellation, stale and
no-op controls belong in their separate acceptance cases. Per-output p95 uses
the nearest-rank estimator, `ceil(0.95 * sample_count)`, and maxima are checked
independently. Timing values are microseconds in the shared monotonic epoch.

```sh
python3 tools/probes/lom_workload/verify.py \
  --host EVIDENCE/session/events.0.log \
  --client EVIDENCE/session/untrusted-session-output.log \
  --budgets EVIDENCE/workload-budgets.json
```

Host records establish issuance and exact validated ACK, actual WM queue serial,
committed policy revision, and native identity/completion. Lom's separate record
names the revision captured by the render job, candidate and presentation epoch.
The join follows those exact identities. It does not use a later snapshot,
queue time, ACK enqueue time or log-arrival timestamp as native completion.
Publication state proves the clicked slot was inactive before the action and
active in the causally committed revision. Identical repeated publication rows
are permitted; contradictory rows for the same identity fail.

Native binding requires complete head coverage of one current owner/frame and
stable targets. All-head completion is separate from Sophia's primary-driven
logical publication. Only exact kernel timestamps qualify for this latency
metric; local out-fence/missing-UST observation fallback does not. A missing
converged action result fails even when a later unrelated revision completes.
An unrelated candidate without qualifying all-head timing remains unavailable
and is counted as `unqualified_candidates`; it cannot supply a timing sample.
This matters when logical primary publication precedes a lagging mirror sibling,
including during final quiescence. It does not excuse any missing action result
or establish complete candidate/teardown settlement. Host evidence
cannot be supplied through the untrusted client log.

Run the synthetic transcript and real-command controls without any display:

```sh
python3 -B -m unittest discover -s tools/probes/lom_workload/tests -v
```

They are also registered in `tools/check_lom_gpu_content_proof_verifiers.sh`.
Fixtures do not run Session, Lom, WM, a GPU worker or native scanout. Passing them
establishes verifier behavior against the given transcript, not native latency.

## Native mode qualification

Each owned candidate binding includes `mode_refresh_millihz` from the current
native head render target. The verifier requires every head, including mirror
siblings, to have a stable mode of at least 60 Hz. Missing, zero, overflowing,
duplicate or changing refresh evidence fails; no 60 Hz default is supplied.
The report retains each head's selected mode rate. This is mode qualification,
not measured rendering cadence, VRR minimum cadence or a driver-health claim.
Kernel completion latency, process health and teardown remain separate checks.

## Final content accounting

Exactly one host shutdown record must follow the full workload and every
qualifying native completion, name the same connection/grant, confirm current
renderer workers joined and report zero for every actual owned store, byte,
input and response credit. The `quiescent` label alone is insufficient. Missing,
duplicate, malformed or mismatched fields fail; no empty default is supplied.
The reported settled-candidate count may be nonzero: these were disconnected
obligations resolved after rendering ended, not newly presented frames.

This validates the final protocol/storage snapshot emitted by Session, not
process RSS, global historical worker reclamation or GPU residency. It does not
replace the native gate's normal exit, cleanup, fatal/restart and recovery checks.

## In-run protocol storage

The existing bounded five-second owner-loop sampler observes actual content
stores and aggregate input/output charges without collecting owners or draining
responses. It records the actual immutable negotiated ceilings alongside each
sample. Workload verification requires samples bracketing the entire window,
no gap over six seconds and no grant or limit change.

For this one-panel-per-output workload the declared ceiling is two resource
slots per output (four total), with storage/backing bounded by twice the sum of
the exact panel pixel sizes. Allocations, candidates, permits, demands and
transport inventory have explicit numeric ceilings, also subject to negotiated
limits. Resource-ID population must not grow past its observed warmup high-water
mark. The report retains every inventory high-water and the bounds used. Final
shutdown still requires zero, including retiring owners and reserved credits.

These are sampled protocol/storage bounds and warmed ID stability, not proof
against every between-sample excursion or arbitrary-duration leak. Existing
production admission enforces its negotiated bounds continuously. Real retained
cache/consumer coverage in the separate 1000-cycle tests is not replaced by
these observations, and neither evidence class establishes process or GPU
memory residency.

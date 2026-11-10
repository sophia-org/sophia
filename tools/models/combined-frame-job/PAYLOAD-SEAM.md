# Stage 2: opaque payload and forwarding witness

Status: source proposed for bounded review before any compilation. Stage 1 at
2269c4d33 and its validation a5e5f616 remain frozen. Only the existing cfg(test)
route and tests change; no production generic worker, feature or cross-crate
bridge is introduced. CPU/frame and security remain priorities, not measured
outcomes of this fixture.

## Type boundary and concrete instantiation

Route<P>, ReturnedValidation<P>, forward<P> and return_plan<P> have only
P: Send + 'static. They move P without inspection, Clone, Debug, Any, callbacks or
other trait bounds. Route's fields are private to the support module. The existing
Job has a cfg(test) Route<ProbePlan> field: the real worker loop instantiates the
generic transport with one private fixture type. It does not become a generic
production scheduler. A second move-only payload exercises the generic helper
directly, using pixels from real validation; that is not a second worker-loop
instantiation. No admission table, validation loop or owner is copied.

ProbePlan carries an identity value and an owned process-local descriptor used as
a lifetime sentinel. It has no methods, Clone or Debug. Tests observe descriptor
identity and socket EOF after disposal. These harmless fixture handles do not
define the actual scene plan's permitted references. Geometry, predecessor leases,
composition record and renderer placeholder are absent. Production plan eligibility
and the prohibition on GPU/device handles and mutable authority remain unproved.

## Ownership and failure route

Submission takes the plan by value. An invalid reservation or a failed job enqueue
returns the exact source descriptor and plan without marking the row submitted.
Before enqueue, the fixture reserves a private one-place owner-return channel.
Only the moved Route owns its sender; the caller receives its receiver. Neither
the generic route nor the payload is Clone, and there is one return attempt.

Successful validation plus successful downstream try_send moves plan and pixels
together. The exact returned envelope settles the row once and returns both to
the owner. Validation failure, cancellation, downstream Full or Disconnected puts
the plan in the private owner-return slot before producing the unchanged ordinary
completion. This slot emits no wake; the existing completion wake orders the
owner's drain. Full/Disconnected still return actual Ok(pixels) on that path, as
stage 1 did, without inventing a peer error. Final reserved renderer-inbox failure
classification remains a later composed obligation.

The private plan-return slot cannot fill twice through this API: the sole sender
and plan are consumed by return_plan. Full is an internal assertion, not a hostile
capacity decision. If the owner abandons the receiver, the plan is dropped where
the failed return occurs (or when the buffered receiver is dropped). That is safe
only for eligible payload references; it is not a device-retirement proof. A test
abandons the receiver and checks ordinary cancellation completion and descriptor
closure. No blocking send or retry is added.

This fixture allocates an extra bounded channel per combined attempt. Its cost is
not hidden or claimed free: the eventual retained owner-row implementation and
all allocation/retention cost need separate review before runtime integration.
No framing, CPU or service-count comparison is licensed by this test shape.

## Forwarding fact, not release authority

The existing fresh Arc reservation identity now contains an initially false
AtomicBool. The worker stores true with Release only after successful try_send;
the reservation observes it with Acquire. Failure leaves it false. Cancellation
does not erase it, and equal numeric ids still cannot bind different allocators.
No new success wake is emitted and take_completed is unchanged.

Receiving the result may precede that store. Tests use an ordinary FIFO job after
the combined job when asserting the final flag, rather than assuming receive is
a witness-store barrier. The flag neither clears credit nor authorizes rendering,
publication or resource release. Exact returned-envelope settlement remains valid
without consulting it. A false flag cannot prove no transfer: a thread can stop
after moving the envelope and before the store. A true flag alone cannot reclaim
the lost-return row; qualified native destruction and matching identity are still
missing here. Stage 1's receiver-loss reclamation gap therefore remains pinned.
Validator-death/result/retirement interleavings are not discharged by this stage.

## Written checks and post-review gates

The seven stage 1 tests retain their credit/backing assertions and add plan return
and witness checks on the real worker. They cover stale reservation, deadline,
size, malformed validation, cancellation, downstream Full/Disconnected and receiver
loss. Three additional checks cover real-worker descriptor ownership through owner
settlement, generic transport of a distinct non-Clone/non-Debug payload, and an
abandoned owner-return receiver. The unchanged no-notify check remains a point
observation only. These checks have not run at this source-review checkpoint.

After signed frozen-source acceptance: run all ten tests, ordinary/shutdown
controls, and named mutations. Move the witness store before try_send: Full must
fail the not-forwarded assertion. Omit the successful store: the FIFO-barrier
witness assertion must fail. Omit the failure plan return: the original validation
error must still arrive but its plan-before-completion assertion must fail. Keep
mutations in a separate cargo-slot worktree, retain failures, restore and recheck.
Then run the required isolated full gate and fmt on the unchanged signed source.
No production source, native run, timing, push or three-service claim is admitted.

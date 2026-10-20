# Experimental ACK handoff

`experimental-ack-handoff` is an independent, disabled-by-default feature of
`sophia-shell-client`. It is a t331 experiment, not a release-qualified SDK
policy. No Sophia vendor pin or protocol revision changes. Enable it explicitly
with `--features sophia-shell-client/experimental-ack-handoff`. The earlier
`experimental-write-read-scheduling` feature is independent and is not enabled
by this feature.

## Behavior and ownership

At an ordinary quiescent yield, a drive may leave an eligible cumulative ACK
unqueued when it has added a new typed item to the application inbox. The
application can consume that event and enqueue its next record. On the next
drive the existing ordering can queue the covering ACK before record/submit,
without waiting for the ACK reply. Sharing a write is a hypothesis to measure;
this does not promise fewer messages, syscalls or owner passes.

Eligibility requires no current submission, no pending outbound record, no
outstanding ACK, automatic ACK enabled, no unfetched-object holds, an advancing
ACK limit, and neither budget exhaustion nor the last allowed round. It does
not defer merely because an old inbox item remains unconsumed. A drive entered
with ACK debt cannot hand it off again. Explicit ACK queues the obligation;
disabling automatic ACK transfers control to the existing manual policy.

ACK debt keeps immediate interest set even after the inbox is drained. Both
ordinary and image-enabled wait paths check immediate work after their leading
drive and before blocking. Budgets still limit I/O: a queued or partial ACK may
need further entries and remains owned by the pipeline. Immediate interest is
an obligation on the caller, not a background task or permission to ignore
byte budgets.

## Observable contract changes

A successful `poll_io` or `dispatch` can return an application event before it
queues an otherwise eligible ACK. A caller that stops driving has no wall-clock
ACK-delivery guarantee. Journal retention, transaction reuse, refusal publication
and the server's existing progress deadline can therefore be affected. The
experiment adds no timer, extends no server deadline and removes no authority
check.

The existing read phase still runs on entry. Returning before the trailing ACK
round can move the reads and ACK-write errors that that round would have
observed to a later entry. This is not same-call equivalence. Partial messages,
object holds, ACK sequences, submission custody, EOF recovery and normal error
handling retain their existing mechanisms. Late errors remain errors; read
results are never used as authorization.

## Controls and remaining gates

The scripted peer pins the absence of an ACK at the first handoff, immediate
interest after draining the inbox, and exact next-write order `ack`,
`transaction`, `submit` while the ACK reply is held. Disabling handoff makes the
first assertion fail. Other dedicated controls cover no repeated handoff with
another event, zero and one-byte budgets, partial ACK output, MAX_ROUNDS,
manual/explicit ACK, EOF with retained content, ACK refusal, and an enabled
image socket. Existing burst coalescing, object-hold, reuse, custody and retry
suites run with the feature both off and on.

The image wait control uses a zero timeout after zero-budget dispatch; it
proves ACK progress with the image path present. It is not a wall-clock proof
that every possible immediate-work state avoids blocking. The common pre-block
check is also source-reviewed. No new production saturation/deadline capture
or native CPU/latency pair has been run. SDK tests do not qualify a change to
server retention behavior under an application that stops driving.

C counterpart inspected: `sophia-desktop-sdk-c` at
`30cc519a4f17607880fcf1cdb5da0cf4efeea320`, `src/shell_session/session.c::ss_pump`
and `src/shell_files/client.c::sophia_sf_client_ack_through`. Its pass-0 I/O and
later zero-budget passes are a different scheduling structure. That is not proof
of equivalent ACK-handoff behavior. There is no corresponding C experiment or
new paired conformance result. Default C/Rust APIs are unchanged; experimental
parity and release qualification remain open. Do not promote this feature or
update production pins on the strength of this implementation alone.

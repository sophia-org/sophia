# Coverage audit

Scenarios 1 to 3 run in `X11ClientOutputSpill.cfg`; they share one state
space because the client's choice to keep reading, never read, or stop is a
branch of the same run. `X11ClientOutputSpillBlockingWriter.cfg` is scenario
4 and must violate `ReaderNeverWaitsOnRecipient`.
`X11ClientOutputSpillUnbounded.cfg` removes the byte bound and must violate
`OutstandingBounded`. `X11ClientOutputSpillNoSilence.cfg` removes the silence
allowance and must fail `SilentClientIsEnded`. Every control retains the
positive model's other checks.

Observed with pinned TLA+ Tools 1.7.4 (jar SHA-256
936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88):
positive: 1,353 generated / 722 distinct states, depth 21, no errors.
BlockingWriter: `ReaderNeverWaitsOnRecipient` violated at depth 5, the first
record the full kernel refuses: today's deadlock. Unbounded:
`OutstandingBounded` violated at depth 9, a spill past its bound with no
ending. NoSilence: temporal counterexample, a client that stopped reading
with a full kernel and a record still owed, never ended.

Two properties were corrected while writing the model, each after a TLC
counterexample: a gone client whose last owed record fits into the kernel
owes the server nothing and is not ended, and the drain having room to move a
record is activity within one slice, so the allowance clock does not pass
while it has work.

No emitted implementation trace has been checked against this model. The wire
tests in `crates/sophia-x-authority/tests/x11_wire/flooding_client.rs` are
the concrete evidence; runtime acceptance is recorded separately.

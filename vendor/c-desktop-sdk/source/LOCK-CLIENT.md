# Experimental lock clients

The lock file contract remains draft. A provider renders the lock display; it
does not receive secrets, check passwords or unlock the desktop. These clients
have scripted-peer and paired production-export coverage, including protected
idle revocation. In-flight revocation, reauthentication and release-wide
qualification remain open before adopting new SDK pins.

Rust starts with Connecting::direct for a supplied-admission socket, or
Connecting::attached for the launch client's exact admitted root. C uses
sophia_lc_init or sophia_lc_init_attached on its caller-owned wire. Protected
launches must not fall back to anonymous attach after authentication failure.
Keep one owner of the connection; do not interleave raw requests or reset it.

Drive Rust dispatch with monotonic milliseconds and a byte budget. It shares
one I/O budget across at most 32 rounds. Monitor fd and io_interest, service
immediate work before sleeping, and honor wake_deadline during startup. C
exposes its wire readiness and bounded sophia_lc_service; timers belong to its
caller. An EAGAIN submit waits for explicit retry in both SDKs, including during
negotiation. Do not spin on retry or invent a timer inside the protocol client.
Rust transfers a ready Session once through take_session.

C's sophia_lc_obligations and Rust's obligations expose consumed/acked
watermarks, pending events/objects/uploads, outstanding upload writes and
ACK/retry waits. A consumed watermark is not proof that its ACK reached the
server. Terminal inspection retains watermarks while clearing pending work.

Ready means negotiation succeeded. The single transaction may still be busy
recycling after its prior submission. A Busy admission changes nothing; drive
the connection and retry admission when its obligations permit it. Submit only
Present/Candidate, FrameDemand or ResourceRetire through the public submission
method. Startup owns Negotiate; the upload helper owns Begin, End and Cancel.

Events have retained heads. Inspect and consume each one explicitly. Nothing
behind a held head is decoded, including custody controls. Rust processes the
next head on dispatch; C also parses after event_consume. Neither may acknowledge
past an unconsumed application event. ResourceStatus remains visible even though
the upload helper also applies it internally.

A Lock publication blocks delivery until its exact object reaches EOF and its
fid is clunked. A newer Qid supersedes the announcement and is consumed internally,
without exposing stale contents. The latest verified object remains available
by reference. Rust moves its decoded allocation list into Session without cloning.
The object is evidence about the published lock state, not authority to unlock.

One upload may be active. Set its window between uploads: 1–8 writes, with five
additional request slots reserved for controls. Begin waits for admission before
opening the slot. Rust takes ownership of each Vec chunk without copying its
payload; failed admission returns the same Vec. take_upload_chunk returns
completed storage for reuse after all issued writes settle. C borrows the caller's
chunk; keep it valid and unchanged until upload_ready or upload_pending becomes
false. Do not infer storage release merely from a cancellation request.

End requires exactly the declared byte count. Cancel stops new writes, drains
issued replies and submits Cancel. A positive short write can continue with a
one-write window; a pipelined short write cancels because later offsets may
already have reached the peer. Remote slot errors end or cancel the upload,
while malformed replies terminate the session. Final resource status controls
cleanup; a submission ACK alone does not prove resource acceptance.

Inspect the latest submission result after errors. Confirmed Submitted custody
survives disconnect; an issued but unconfirmed Rust submission becomes
UnknownDisconnected, and an unsent one becomes DroppedUnsent. C retains its
CUSTODIED, ISSUED or STAGED classification; interpret ISSUED as uncertain after
terminal failure and STAGED as unsent. Neither classification authorizes replay.
Refusal and stale state are terminal. Recovery needs a fresh authorized
connection and fresh state; there is no hidden reconnect or IPC fallback.

Fixed journal/object storage, borrowed write slices and owned-buffer reuse avoid
specific local copies. The transport may still copy payloads, and body/object
codecs may allocate. These are implementation properties, not measured CPU gains.
See [lock parity](LOCK-PARITY.md) for exact coverage and remaining gates.

# Private diagnostic output-capture delivery

This default-off diagnostic is for untimed consumer-pixel qualification. It is
not a public protocol, a filesystem grant to an observer, or a timed benchmark
path. Build the diagnostic host with `native-session`,
`experimental-shell-capture` and `diagnostic-capture-delivery`. Timed hosts must
have a separate build receipt proving the delivery feature is absent.

## Admission and destination

`session run --diagnostic-capture-directory=ABSOLUTE_PATH` creates one new
Session-owned directory, mode 0700. Existing paths refuse; parent traversal is
refused. Session opens and retains the directory FD. This is a new **Session
write path**. The observer still has its existing confined filesystem and no
writable host destination. Private files are exclusive, mode 0600, and opened
relative to that FD with NOFOLLOW. No file is overwritten.

The configured recipes must include a developer-launch observer with one
capture-output. A private launch must be a developer request with a nonzero
transaction, that observer recipe, a protected process, the exact Session
executable, and exactly `capture OUTPUT_ID --diagnostic-stream`. OUTPUT_ID must
be a canonical nonzero integer. The recipe must name a fully qualified connector;
no suffix-only connector is accepted. At most three attempts are consumed per
Session, with no overlapping attempt in the same namespace. Failed attempts
remain consumed; preparation errors close their namespace occupancy.

Session rejects the diagnostic flag outside this selection, including builds
without the feature. The CLI also refuses ordinary pipe/file stdout, relay mode
and additional arguments. No fallback writes binary data to Session's shared
stdout, session.log or the operator terminal.

## Custody, activation and server evidence

Session creates two private Unix socketpairs and installs their child endpoints
as stdout and stderr at spawn, after the protection command's stdio setup. A
protected domain that does not inherit fd 1 refuses. The parent's writer copies
close with the spawn command. The private stderr replaces the ordinary
application diagnostic stderr sink; errors belong in `capture-N.stderr`.

After authenticated startup, before serving the portal, Session opens a pidfd
for the inner child, requires it to be the wrapper's sole child, compares the
post-exec executable device/inode with its own executable, rereads starttime,
and checks the pidfd has not exited. Protocol-unavailable startup refuses.
The binding is authenticated launch **custody**, not per-message writer proof.
Socketpair SO_PEERCRED would identify the creator, so it is deliberately not
used as an observer identity.

At successful immutable capture publication, Session records request serial,
output ID, output incarnation, presentation generation and a SHA256 of pixels.
It rechecks the fully qualified recipe connector against the live backend's
unique primary-head mapping at that publication. The diagnostic frame must
match that server record exactly. A second publication for the attempt refuses.
The qualifier must additionally bind the receipt connector and incarnation to
its own pinned connector and held scene; approval of another output is not enough.

## Private framing and lifecycle

The private stream is `SCAP0001`, a nonzero u64 little-endian request serial,
the existing 64-byte OutputCaptureFrameInfo, tightly packed RGBA8 bytes, and a
u64 little-endian FNV-1a64 checksum, followed by EOF. Metadata validation retains
the existing 64 MiB capture bound. The receiver allocates from that bounded
metadata before comparing against the server publication. Ordinary CLI metadata
is suppressed in this mode. The writer shuts down its write half explicitly.

Concurrent bounded readers drain stdout and stderr off the Session owner thread.
Both use the same 20-second deadline from preparation; stderr is limited to
16 KiB and must be empty for success. Reader failures and deadline expiry set the
failure state and notify the existing owner wake. The owner retires the launch
through its existing off-owner terminate/reap path. Once preparation returns,
failure is checked before the namespace registry try-lock and startup gate.
This does not impose a hard wall-clock bound on filesystem preparation or
thread scheduling before that handoff; the controller retains its own timeout.
Acceptance also requires a
successful clean reap and the retained identity binding. A failed spawn or a
cancelled launch closes endpoints/senders and yields refusal rather than retry.

## Publication and controller acceptance

Each PAM and JSON is written to an exclusive `.partial` file, synced, renamed
without replacement, then directory-synced. This makes each file complete at
publication; it does **not** make the pair crash-atomic. A JSON failure may leave
a complete PAM and a `.refused` file. Worker threads are bounded but detached;
Session shutdown is not a completion barrier.

The diagnostic controller **must wait** for the attempt's receipt or refusal
before stopping Session. Its own deadline means refusal, never a retry in place.
It accepts only all of the following together:

- Complete `capture-N.json`, schema 1, status delivered, with the expected launch,
  transaction, executable identity, output, connector and live incarnation.
- No `capture-N.refused`, no attempt `.partial` files, and an existing
  `capture-N.stderr` file of exactly zero bytes.
- Complete `capture-N.pam` whose SHA256 equals `pam_sha256` in that JSON.
- The package's separately verified held-scene and composed-output pixel oracle.

A PAM alone, a partial file, an exit status alone, or child metadata alone is no
acceptance evidence. If Session dies before terminal publication, retain the
files as an incomplete refused attempt. Do not harvest pixels from shared logs.

## Device-free coverage and remaining qualification

Private framing tests cover truncation, checksums, extra bytes, bounds and
expired reads. Store fixtures cover supplied activation, exact server joins,
clean reap, stderr refusal, owner notification, attempt limits and publication
collisions. They do not claim authenticated native launch coverage. Supervisor
tests exercise actual host and bubblewrap children, private stdout/stderr, EOF
before reap, refusal when fd 1 is unavailable, and spawn failure.

Still required before native pixel qualification: an independently reviewed
controller implementing the acceptance rule above; exact diagnostic artifact
receipts; the real authenticated observer/executable/output joins; and all three
consumer pixel, import-refusal and release-lifetime gates. No device was opened
by these fixtures and no timed performance conclusion follows from them.

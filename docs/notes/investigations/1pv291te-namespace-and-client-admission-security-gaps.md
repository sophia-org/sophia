---
id: 1pv291te
date: 2026-09-20
kind: investigation
status: investigating
tags: [security, session]
---
# Namespace and Client Admission Security Gaps

## Question

How does Sophia ensure secure namespace placement at connection admission, and what gaps, escape vectors, or security boundary challenges exist in this model under native Linux process isolation?

## Evidence

The September 20 threat hypotheses below are historical. The October 10 source
review corrects their admission and ancestry claims; use that review and the
[current boundary plan](../plans/esnqxpqw-pidfd-and-namespace-admission-optimizations.md)
for implementation scope.

- `crates/sophia-session/src/live_session/x_frontend.rs:12` implements `LiveXAdmissionPolicy`. Vetting is done by inspecting `request.peer_credentials` (retrieved via `SO_PEERCRED` on Unix domain sockets).
- `crates/sophia-session/src/launch_origin.rs:73` contains the implementation of `process_ancestors`, which reads parent and grandparent relationships via /proc in a separate worker thread.
- `crates/sophia-runtime/src/session/namespace.rs` maintains the session-owned in-memory `NamespaceRegistry`.

## Finding and resolution

An audit of the connection admission and launch origin tracking logic reveals several security vectors:

### 1. The Re-parenting (Orphan / Daemon Escape) Gap
* **The Vector:** When a child process is spawned and its immediate parent exits, the Linux kernel re-parents the orphaned child process to init (PID 1) or a configured subreaper.
* **The Risk:** If a sandboxed application double-forks, the connecting process's immediate parent is dead. The `process_ancestors` crawler will traverse the chain up to init. Because the original authorized launcher process is missing from the active chain, Sophia cannot associate the connection with its original `LaunchOrigin`. 
* **Boundary Consequence:** This forces Sophia to either fail-closed (denying connections for daemonizing applications) or fail-open (relegating them to a default, un-sandboxed namespace).

### 2. Unix FD Passing (SCM_RIGHTS) Hijacking
* **The Vector:** Unix domain sockets support transferring open file descriptors between unrelated processes using sendmsg with SCM_RIGHTS auxiliary data.
* **The Vulnerability:** Once a trusted/classic client connection is successfully admitted to the X server, the socket file descriptor is active.
* **The Risk:** If a malicious or confined process can communicate with a trusted process via some other local IPC channel (e.g., DBus, shared pipes, or local files), the trusted process could be coerced to pass its open, already-admitted X11 socket FD to the untrusted process.
* **Boundary Consequence:** Because admission is checked strictly at setup, Sophia does not re-verify credentials on the active socket. The untrusted process gains full access to the trusted namespace.

### 3. PID Namespace Boundaries & Containment Mismatch
* **The Vector:** Sandboxing environments (Flatpak, Bubblewrap, Docker) isolate applications inside nested PID namespaces, which translates PIDs when crossed.
* **The Vulnerability:** SO_PEERCRED returns the PID of the peer as mapped into the receiver's namespace, but /proc visibility might not align.
* **The Risk:** If the Sophia session supervisor runs inside its own sandbox or with restricted /proc visibility, or if mount namespaces do not align perfectly, /proc/[translated-pid]/stat will be unreadable.
* **Boundary Consequence:** When read_process fails, process_ancestors aborts the connection, causing legitimate sandboxed apps to fail to launch.

### 4. Resource Satiation DoS via /proc Traversal
* **The Vector:** Finding ancestors requires reading /proc/[pid]/stat up to MAX_ANCESTRY_DEPTH times, performing file opens, parses, and comparisons on every X11 connection attempt.
* **The Vulnerability:** Sockets can be connected to rapidly without performing setup authentication.
* **Boundary Consequence:** A malicious client could spam connection attempts to the X11 Unix socket. Although the admission worker is threaded, high-frequency /proc parsing can induce severe CPU load, locking the NamespaceRegistry and blocking new window and input mappings for legitimate clients.

## Validation and remaining work

To address these gaps, the following design mitigations and validation gates are proposed:

1. **Verify Double-Fork Handling:** Implement a conformance test in `crates/sophia-x-authority/tests` where a client process double-forks and connects, asserting that the admission policy handles the re-parented child safely.
2. **Limit FD Passing Exposure:** Ensure that frontends periodically audit active connections or utilize connection-bound unique tokens instead of relying solely on one-time connection setup peer credentials.
3. **Bound /proc Traversal Rate:** Implement a connection-rate limiter per UID to prevent resource exhaustion from fast-reconnecting socket attacks.

## Source reconciliation (2026-10-10)

At Sophia `b0ca8280a`, `LiveXAdmissionPolicy::admit` checks the peer UID and
calls `NamespaceRegistry::admit` before looking up ancestry. The lookup runs
on the admission worker, outside the registry lock. `process_ancestors` returns
an empty hint on unreadable proc, reparenting during observation or PID/start-time
mismatch. It does not abort X admission or fall back to a more privileged
namespace. The earlier claims of denied admission and a held namespace-registry
lock during proc traversal are not supported by this implementation. Reparenting
can lose launch/workspace attribution; that remains distinct from admission.

The descriptor-transfer threat also needs a precise limit: a passed connection
retains its original authority. Neither repeated original-peer checks nor an
ordinary connection token proves the identity of its new holder. Confinement
must exclude unwanted descriptor/credential transfer paths, and a cooperating
trusted proxy remains a trust-boundary limitation. The accepted immutable
connection rule does not claim otherwise.

The core still refuses Tauth and non-NOFID attach, and the protected lock
transport still requires pidfd checks. The
[t133 design](../plans/esnqxpqw-pidfd-and-namespace-admission-optimizations.md)
now separates authenticated principal, supervisor launch custody, namespace
selection and grants; it withdraws pidfd-as-ancestry and unmeasured timing claims.
No production checks were removed and no new authentication was exercised.

## t133 local mechanism review (2026-10-10)

Read-only source review at `b089c15d5` confirms the earlier corrections and
resolves the next implementation boundary in the
[local authentication design](../plans/esnqxpqw-pidfd-and-namespace-admission-optimizations.md#local-authentication-design-and-t317-handoff-2026-10-10).
No code was changed and no build, authentication exchange, sandbox probe or
performance experiment was run for this planning slice.

The current Factotum agent adopts only its private Session stream.
`agent.rs::serve` checks its parent relationship and registers that connection
as `ChannelClass::Session`; `export.rs::attach` refuses unregistered connections.
The only protocols remain `pass` and `pam`. Password retrieval has no server
authentication side, and PAM is a Session-only unlock attempt whose result has
no role-connection proof. A public `rpc` path must not be invented by exposing
the existing key-administration tree.

The selected implementation direction is a private launch-generation channel
with conversation-only access, provisioned by the actual supervisor spawn.
Factotum verifies an afid challenge relayed through that channel and sends its
verdict to Session over the trusted owner channel. The verdict names the exact
transport, wire generation and attempt. Session joins the delegated principal
to a separate still-live launch assignment and policy-selected namespace.
The plan records nonce lifetime, immutable connection identity across `Tversion`,
Reserved-to-Active spawn ordering, bounded quotas and the controls for each.
It introduces no password-derived or custom cryptographic proof.
Session retains the trusted end of the child channel and attests its registration
when relaying to Factotum; no descriptor is passed on the 9P stream. Factotum
trusts that Session attestation rather than directly inspecting the child fd.
Custody-required path endpoints retain the current cheap peer filter before
allocating authentication slots. Removing `SO_PEERPIDFD` as a hard requirement
does not authorize an anonymous path on which another process can squat ahead
of the lock provider; lock-provider failure must leave the lock enforced.

Two review corrections are material. Rust `Command` does not universally close
all descriptors except standard I/O; CLOEXEC controls exec inheritance.
`ProtectionDomainSpec::inherited_fds` currently validates only 0–2 and the spawn
code nulls excluded standard streams. Bubblewrap's exact descriptor behavior
has not been tested by this review: an explicit named handoff and a sentinel-fd
leak control are implementation prerequisites. Also, `Tversion` must invalidate
afids and their results, while the transport's pinned principal and launch
binding survive. Reauthentication uses a new challenge through the same live
generation capability, not a stale verdict.

Possession is not proof of the current holder's process identity. A launched
child can fork, transfer a descriptor or proxy its authority; the recipient
still has that launch's authority and cannot choose another principal or role.
Revocation must end that authority even if the descriptor remains open.
Host containment belongs to t142, with X admission unchanged. The initial
observer/operator tool must also be an authorized Session launch; adding a
same-UID shortcut for an ad-hoc shell client would violate this design.

The first local mechanism borrows the external-credential principle of
[RFC 4422 Appendix A](https://www.rfc-editor.org/rfc/rfc4422.html#appendix-A),
without claiming SASL framing or remote authentication. The
[9P auth/attach contract](https://9p.io/magic/man2html/5/attach) supplies the afid
exchange but deliberately leaves the authentication mechanism unspecified.
On 2026-10-10 niltempus approved this t133 design result: supervised desktop
components first, an authorized developer launch command under t318, and local
credential provisioning for independently launched tools and applications as
the explicit [t324 follow-on](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#t324).
Components remain independent executables; this is not a permanent requirement
that Sophia launch all applications. The reviewed source is `b089c15d5`; this
result changes documentation only. No exchange, build or confinement control
was run for this design review. Actual exchange, state-machine, independent
SDK, overload and replacement-peer-check controls remain t317 acceptance work;
t142 owns the confined-group proof. Existing peer checks and X admission remain
unchanged until their respective implementation gates pass.

## t317 implementation reconciliation (2026-10-12)

Source reviewed at `a794e0bc8910711ab6d1ac58544e9fbb3e072f53` replaces the
earlier t133 design-only observations above. Those dated observations remain
historical evidence. The [implementation investigation](etyvymxi-authenticated-attach-transport-state-and-factotum-handoff.md)
records individual source candidates, executed suites, negative controls and
limits; this reconciliation is a source and retained-evidence review, not a new
execution or a claim that the final candidate-wide gate passed.

| Threat or boundary | Implemented defense and evidence |
| --- | --- |
| Claimed uname or peer-written success becomes authority | The core's authentication wire tests refuse ordinary, unverified and forged attaches. Session's role-afid join requires a correlated Factotum result plus current launch custody. Claims select neither principal nor namespace. |
| Reset permits a different identity | Core authentication tests reject changes to principal, namespace and launch after reset. The wire test destroys the old afid with EBADF. The protected admission-context join reauthenticates through the same generation channel and preserves the exact context; its reset control discriminates context loss. |
| Partial attach pins authority without a root | Checked root preparation precedes identity commit. A full fid table cannot pin an identity; abandoned preparation publishes nothing. Lock, WM and shell root checkpoints cover their respective owners. |
| Stale verdict or retained fid survives revocation | Session checks custody before attach and on retained operations. The role-afid controls discriminate both checks. Protected upload controls revoke launch, namespace or admission between fragments, before idle cleanup can mask a missing guard. |
| An unrelated launch acquires custody | A consumed channel handoff and exact protected spawn generation gate activation. Unprotected spawn and configured descriptor claims are refused. Worker custody observes launch exit and is retired before reaping. Possession still permits intentional delegation within that launch. |
| Anonymous connectors starve custody-role admission | The expected-peer filter remains before authentication allocation. Session bounds endpoint/global transport slots and retains cleanup custody; core pre-auth fid/request bounds also apply. This provides bounded admission, not a general fair-access guarantee. |
| Lock requires a socket pidfd on every platform | The authenticated lock branch checks the live assignee, then afid and current custody; it does not call SO_PEERPIDFD. The explicit legacy branch retains that requirement. Supervisor pidfds and the expected-peer availability filter remain. |
| Authentication silently expands X authority | X11 still uses its existing admission owner. Launch ancestry remains an attribution hint, not namespace selection or a substitute for authentication. |

Manifest integrity was rechecked for `t317-role-afid-01`,
`t317-lock-wire-01`, `t317-admission-context-01`,
`t317-wire-reset-context-control-01`,
`t317-retained-admission-check-control-01` and
`t317-component-clients-01`: every manifest entry matched its retained file.
That check establishes package integrity, not execution at the current commit.
Their original runs and candidate bindings remain as recorded in the linked
investigation; later source changes require impact review or rerun.

Normal supervised WM, lock, output, descriptor-shell, bar, launcher and dock
owners now select authentication. Generic C/Rust SDK tests and independent
client checkpoints distinguish authentication from rendered or physical
acceptance. Metadata broker and administrative control still have their
existing admission models; their legacy transport migrations belong to t314
and t254. The live portal uses the accepted protocol-authority socketpair
exception, not a protected-process or afid claim. Its out-of-process requester
variant remains unintegrated. These boundaries are summarized in the
[launch authentication contract](../../factotum-launch-authentication.md#transport-boundaries).

Session and the supervisor remain trusted. Factotum verifies Session-attested
nonce correlation and registration; it does not independently prove the
holder's executable, PID or human identity. A cooperating holder can relay a
challenge for another process's transport within the original launch's
authority. The SDK relays its own afid challenge, but that is not a constraint
on a malicious holder. Revocation ends authority even if the channel remains
open. There is no remote-authentication claim, same-UID fallback or new access
for independently launched tools; t324 retains credential provisioning and
t318 retains authorized developer launch and namespace composition.

Final SDK snapshot alignment, candidate-wide validation and integration remain
required before t317 closure. This reconciliation changes no task state and
claims no live installation.

## Connections

- Links to [Namespaces and Portals](../../namespaces-and-portals.md)
- Links to [Sophia X Authority](../../sophia-x-authority.md)
- Links to [Native Desktop Capability Audit](vle7mt47-native-desktop-capability-audit-separates-contracts-from-client-ui.md)

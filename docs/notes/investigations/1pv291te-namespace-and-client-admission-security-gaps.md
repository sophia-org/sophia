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

## Connections

- Links to [Namespaces and Portals](../../namespaces-and-portals.md)
- Links to [Sophia X Authority](../../sophia-x-authority.md)
- Links to [Native Desktop Capability Audit](vle7mt47-native-desktop-capability-audit-separates-contracts-from-client-ui.md)

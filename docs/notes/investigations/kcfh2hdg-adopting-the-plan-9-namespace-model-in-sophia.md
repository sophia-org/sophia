---
id: kcfh2hdg
date: 2026-09-28
kind: investigation
status: investigating
tags: [architecture, security]
---
# Adopting the Plan 9 namespace model in Sophia

## Question and scope

How should Sophia adopt the Plan 9 namespace model so each process or admitted
client group receives a composable view of files and services?

niltempus requested this investigation on September 28 while reviewing the
README's target architecture, then clarified that the subject is adopting the
namespace model itself. Merely using 9P for role messages or hiding sockets in
Linux mount namespaces does not answer that request.

The investigation should define which Plan 9 namespace semantics Sophia adopts,
how clients use them, and how they map onto a Linux implementation. It should
cover bind, mount, unmount, union ordering, inheritance, and private versus shared
namespace state. No particular implementation mechanism is selected yet.

## Questions to resolve

1. **Naming and composition.** Define a client's namespace root, service naming,
   bind and mount operations, replacement and union lookup rules, and where
   creation lands in a union. Decide which operations clients can perform
   themselves and which require Session admission.
2. **Identity and authority.** Map a filesystem namespace to Sophia's existing
   `NamespaceId`, immutable admission contexts and role grants. Identify when
   two clients share resources even if their path views differ. Path visibility,
   service identity and permission to perform an operation need separate rules.
3. **Process lifecycle.** Define inheritance at launch, private copies versus
   shared views, subsequent changes, service restart, revocation and cleanup.
   Specify what happens to existing fids, file descriptors and in-flight work
   when a name is rebound or a service is removed.
4. **Desktop composition.** Work through applications, WM, shells and
   administration seeing different service views. Allow both X11 and future 9P
   applications in a trust domain; protocol choice must not define trust.
   Keep the Engine's rendering and transaction interface protocol-neutral.
5. **Transfers.** Define how an authorized clipboard, drag-and-drop or capture
   transfer exposes a specific object or service without joining whole
   namespaces. Relate this to the broker/portal decision tracked by t273.
6. **Linux feasibility.** Compare kernel mounts, userspace filesystem mounts,
   and client-side namespace resolution for the chosen semantics. Re-check
   privilege requirements and compatibility with ordinary applications. Cover
   same-UID escape paths, inherited descriptors, runtime sockets and credentials;
   a naming view alone is not an isolation proof.
7. **Contributor interface.** Propose a small declarative namespace recipe and
   readable inspection interface. Keep named desktop policies in their own
   repositories; Sophia should provide generic composition and admission.

## Evidence to collect

Read the original Plan 9 namespace documentation and cite the specific semantics
being adopted. Audit current Sophia namespace allocation, admission, service
exports and process launch code with file references. Distinguish current
behavior, earlier proposals and the new target model.

Use a bounded, device-free prototype to resolve uncertain Linux mechanisms if
needed. Exercise two private clients with overlapping names, an explicit shared
service, a rebound service, and a revoked transfer. Record expected visibility,
operation permission and retained-handle behavior separately. No live-session
changes are required for this investigation.

## Investigation exit

Produce a proposed ADR naming the adopted semantics, ownership and threat model;
a worked application namespace recipe; and a migration plan with concrete tests
for composition, inheritance, isolation and revocation. Identify implementation
gaps and any semantics that cannot be supported on the chosen Linux mechanism.
The investigation can finish with that reviewed design; implementation and
acceptance require their own tasks and evidence.

## Connections

- [Namespaces and portals](../../namespaces-and-portals.md) owns Sophia's
  existing resource isolation and admission contract. Reconcile it with the
  proposed naming model rather than treating identical terminology as identical
  semantics.
- [Earlier Plan 9 and mount investigation](5kqzwmi5-plan-9-belongs-in-the-session-control-plane-not-the-engine.md)
  records the role-transport discussion and a host-specific v9fs limitation.
  This investigation broadens the question to namespace composition.
- [Accepted 9P public-interface decision](../decisions/1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
  governs the transport direction; it does not by itself settle namespace APIs.
- [Socket-directory and frontend plan](../plans/ooy00zjd-socket-directory-and-frontend-multiplexer-architecture.md)
  supplies related isolation work and its review. A directory of sockets is one
  possible ingredient, not the full namespace model.
- [IPC removal inventory](1lty2tzb-what-ipc-code-remains-after-the-desktop-moved-to-9p2000-l.md)
  tracks t273's broker/portal transport decision. Coordinate the designs without
  conflating that narrower migration with this investigation.

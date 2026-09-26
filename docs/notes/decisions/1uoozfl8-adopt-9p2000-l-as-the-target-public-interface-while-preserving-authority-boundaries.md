---
id: 1uoozfl8
date: 2026-09-25
kind: adr
status: accepted
tags: [adr, architecture, protocol]
---
# Adopt 9P2000.L as the target public interface while preserving authority boundaries

## Context

Sophia's current desktop roles share a binary envelope and lifecycle contract
but require Sophia-specific message handling. During the 2026-09-25
brainstorming session, niltempus explored 9P2000.L as a common interface for WMs,
shells, administration and applications. The earlier draft mixed full public
IPC replacement with a permanent binary/9P split and overstated filesystem
confinement, performance and Plan 9 compatibility.

The existing owners already enforce metadata blindness, admission, coherent
proposals, presented input and retirement. The new interface must preserve those
semantics rather than create another scene or authorization owner.

## Decision

Adopt 9P2000.L as the target common public protocol for replaceable desktop
components, with a 9P application frontend alongside X authority. Replace the
custom public desktop IPC progressively after equivalent behavior, recovery and
acceptable performance are demonstrated. Existing X11 support remains.

Use separate authorized role interfaces even when transport code is shared.
Linux isolation, Sophia resource namespaces, admission, disclosure and revocation
remain enforced. Engine's internal typed transactions and execution mechanisms
are outside the public IPC replacement scope.

Support the design direction of application-owned service exports, available to
explicitly granted consumers. Application semantics remain with their owners;
blind WM policy does not acquire access to those exports.

The detailed file API, operation subset, encoding, attach identity, compatibility
and performance thresholds remain open. Coexistence is a migration mechanism,
not an accepted permanent division into a compiled fast path and a script path.

## Alternatives

- Keep all existing public role protocols indefinitely and add 9P only for
  administration. This would leave the WM/shell developer interface split
  across transport families and is not the accepted destination.
- Replace every internal channel and driver interface with 9P. This exceeds the
  public-interface goal and does not follow from the developer benefits.
- Treat mount visibility as sufficient authorization. This cannot replace
  admission, retained-handle revocation or actual presented-input validation.

## Consequences

The expected benefit is a consistent, inspectable interface usable through
generic 9P clients or mounted file I/O. Sophia still owes versioned domain
contracts, independently implemented clients and conformance evidence. A common
protocol does not grant Plan 9 application compatibility or graphics performance.

Migration must preserve the current contracts until replacement evidence is
available, including an independent WM, content shell and graphical application,
production lifecycle joins, negative controls and measured performance. Physical
acceptance remains separate. The existing 9P scaffold establishes none of those
integration claims by itself.

## Acceptance and connections

Created with `zk adr` as proposed, then accepted on 2026-09-25 on the basis of
niltempus's explicit agreement in this conversation: "I agree with this
approach", followed by the request to revise the documentation and continue
brainstorming. Acceptance covers architectural direction; it does not freeze an
API or resume implementation, builds, gates or live-session work. No task is
closed or reprioritized by this record.

Later on 2026-09-25, niltempus approved compact binary runtime records with
readable inspection and instructed "Implement the plan" for the Hagia-first
milestone. The [execution plan](../plans/80blhke8-migrate-the-hagia-wm-role-to-admitted-9p2000-l-files.md)
records that narrower implementation authorization. The initial pause above
remains the history of this ADR's acceptance, not the current execution state.

On 2026-09-26 niltempus reaffirmed that the goal is replacing the separate
public WM, shell and other role protocols with 9P. An investigation written
without the Hagia development context had proposed administration-only 9P and
permanent WM IPC; its scope was corrected. Engine's internal interfaces,
per-role admission and separate physical acceptance remain unchanged.

- [Public 9P interface design](../../sophia-9p-control-bus.md) owns the evolving
  design, migration criteria and open questions.
- [9P application frontend](../../sophia-9p-authority.md) separates application
  protocol authority from desktop role services.
- [Architecture](../../architecture.md) retains the authority boundaries and
  distinguishes the accepted target from current support.
- [Current native protocol family](../../sophia-policy-ipc.md) remains the
  implementation and compatibility contract during migration.
- [Namespaces and portals](../../namespaces-and-portals.md) retains the trust
  and transfer model.

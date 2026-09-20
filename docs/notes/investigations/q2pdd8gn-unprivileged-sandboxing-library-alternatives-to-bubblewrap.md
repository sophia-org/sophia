---
id: q2pdd8gn
date: 2026-09-20
kind: investigation
status: investigating
tags: [security, architecture, session]
---
# Unprivileged Sandboxing Library Alternatives to Bubblewrap

## Question

What mature, well-maintained Rust-native sandboxing and containerization libraries exist in the ecosystem that can replace Bubblewrap (`bwrap`) or the stagnant `Pnut` library for Sophia's unprivileged process-supervisor and role-containment layers?

## Evidence

- Our evaluation of the stagnant `mikedanese/pnut` project (`docs/pnut-evaluation.md`) revealed a blocking public API (`Sandbox::run()`) and a fail-open configuration bug in its Landlock builder.
- Sophia's supervisor requires a nonblocking unprivileged spawn API returning a `Child`-like process handle, allowing socket verification against the exact host PID.
- Online research identified several robust Rust-native sandbox libraries (`unshare`, Youki's `libcontainer`, and `extrasafe`).

## Finding and resolution

Rather than invoking an external C binary like Bubblewrap via shell command-line strings, Sophia can migrate to a compiled, native Rust sandboxing backend. The following three alternatives are evaluated:

### 1. `unshare` (Dedicated Process Namespace Sandbox)
* **Design & Alignment:** This is the closest native-Rust match to Bubblewrap. It provides a clean, builder-style API modeled directly after `std::process::Command`.
* **Why it Fits:**
  - It natively exposes a nonblocking `spawn()` API returning a normal `Child` handle, allowing Sophia to retrieve the host PID for peer-credential authentication.
  - It automates the orchestration of unprivileged namespaces (`CLONE_NEWUSER`, `CLONE_NEWNS`, UTS, PID, Net), UID/GID mapping, `chroot`, and `pivot_root`.
* **Status:** Highly focused, mature, and perfectly suited for process-supervisor wrapping without OCI/container runtime overhead.

### 2. `libcontainer` (CNCF Youki-Core Library)
* **Design & Alignment:** The official library core of Youki (the Rust OCI container runtime matching `runc`).
* **Why it Fits:**
  - Extremely active corporate backing, robust maintenance, and extensive production hardening.
  - First-class, unprivileged (rootless) namespace and cgroups management, seccomp filters, and Landlock integration.
  - Includes advanced parent-child synchronization mechanisms during container startup.
* **Status:** The best choice for a heavy, enterprise-grade, highly maintained containerization engine.

### 3. `extrasafe` (In-Process Compartmentalization)
* **Design & Alignment:** A highly active library specializing in in-process sandboxing and thread-level compartmentalization.
* **Why it Fits:**
  - Its `Isolate` feature uses unprivileged namespaces to containerize single Rust functions rather than external binaries.
  - Provides a simple DSL for declaring seccomp filter profiles.
* **Status:** Perfect for in-process sandboxing of untrusted internal services (e.g., parsing user desktop files in the launcher, or image decoding).

## Validation and remaining work

1. **Keep Bubblewrap for Now:** Since `bwrap` is highly stable, well-audited, and ubiquitous across Linux distributions, it remains the primary deployable backend for Milestone 15.
2. **Prototyping `unshare`:** Once the core preflight checks are accepted, prototype an alternative `unshare` backend under the `ProtectionDomainSpec` trait, compiling process sandboxing directly into the Sophia binary.

## Connections

- Links to [Pnut Protection-Backend Evaluation](../../pnut-evaluation.md)
- Links to [PIDFD and Namespace Admission Optimizations](../plans/esnqxpqw-pidfd-and-namespace-admission-optimizations.md)
- Links to [Namespaces and Portals](../../namespaces-and-portals.md)

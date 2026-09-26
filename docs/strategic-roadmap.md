# Sophia Strategic Roadmap: Monorepo Incubation to Multi-Repo Ecosystem

**Role:** strategic architecture roadmap and organizational evolution policy.

**Status:** non-normative strategic plan; active architectural direction.

## 1. Executive Summary

Sophia separates visual authority, application protocols, window management and
desktop shells. X authority is the current application frontend; the 9P crate
is a scaffold. Process and repository extraction below are strategic targets,
not claims that all components already run as standalone daemons.

The accepted [9P2000.L direction](sophia-9p-control-bus.md) targets common public
WM, shell and administrative interfaces and a 9P application frontend alongside
X11. Application-owned service exports use explicit grants. Namespaces and
existing semantic owners remain; Engine's internal typed execution is outside
the public transport replacement. The file API and migration schedule remain
open, and implementation stays paused during the current brainstorming session.

This roadmap outlines the evolution of the Sophia codebase from its current
**Phase 1: Monorepo Incubation** into **Phase 3: Autonomous Satellite Repositories**
under the `sophia-org` organization.

```text
 PHASE 1: INCUBATION (Current)                PHASE 3: SATELLITE ECOSYSTEM (Target)
 ┌──────────────────────────────────────┐     ┌──────────────────┐  ┌──────────────────┐
 │ sophia-org/sophia (Cargo Monorepo)   │     │sophia-org/       │  │sophia-org/       │
 │  ├── sophia-engine                   │     │sophia-x-authority│  │sophia-9p-author  │
 │  ├── sophia-session                  │     └─────────┬────────┘  └────────┬─────────┘
 │  ├── sophia-protocol (Iterating)     │               │                    │
 │  ├── sophia-x-authority (Incubating) │               ▼ (admitted records) ▼
 │  └── sophia-9p-authority (Incubating)│     ┌────────────────────────────────────────┐
 └──────────────────────────────────────┘     │ sophia-org/sophia                      │
                                              │  • sophia-engine (DRM/KMS visual core) │
                                              │  • sophia-session (supervisor/admission│
                                              │  • sophia-protocol (v1.0 wire schemas) │
                                              └──────────────────┬─────────────────────┘
                                                                 │
                                                                 ▼ (target 9P WM API)
                                                      ┌──────────────────┐
                                                      │ sophia-org/hagia │
                                                      │ (Reference WM)   │
                                                      └──────────────────┘
```

---

## 2. Architectural Duality: Velocity vs. Purity

The decision to house protocol authorities in the main repository versus dedicated
repositories represents a balance between **engineering velocity** and
**architectural isolation**:

### The Case for Multi-Repository Separation (The Architectural Target)
1. **Unassailable Protocol Neutrality:** Moving frontends out of the core tree
   proves beyond doubt that `sophia-engine` has zero coupling to X11 or any
   specific client protocol. It cements the engine’s status as a pure visual
   kernel.
2. **Enforcing Boundaries "By Force of Law":** Within a single Cargo workspace,
   shared convenience functions and type leakage are constant risks. Physical
   repository separation forces authorities to communicate exclusively through
   versioned public interfaces over Unix domain sockets.
3. **Divergent Lifecycles and Scale:**
   - `sophia-engine` follows Linux DRM/KMS, DMA-BUF, and Vulkan/Mesa evolution.
   - `sophia-x-authority` is a massive legacy compatibility project with heavy
     dependencies (font parsers, XKB, XRender, Xcursor).
   - `sophia-9p-authority` has a distinct application-protocol lifecycle. Its
     production size, maintenance cost and performance are not established by
     the current scaffold.

### The Risk of Premature Extraction (Why Monorepo Incubation Wins Today)
- **Cross-Repository Friction:** When wire protocols, transaction structures, or
  admission handshakes are in active flux, monorepo incubation enables atomic Git
  commits across the engine, supervisor, and frontends.
- **Dependency Churn:** Premature extraction introduces the overhead of constant
  Cargo git/path dependency bumps, tag releases, and fragmented CI pipelines,
  slowing down daily development.

**Strategic Consensus:** Keep authorities as independent crates within the
monorepo during incubation; extract them cleanly to `sophia-org` repositories once
their wire interfaces reach a verified freeze.

---

## 3. The Three Evolution Phases

### Phase 1: Monorepo Incubation (Current Tranche)
* **Structure:** All authorities (`sophia-x-authority`, `sophia-9p-authority`)
  reside as isolated crates under `crates/` in the `sophia` repository.
* **Goals:**
  * Drive `sophia-x-authority` to full daily-driver maturity, validating complex
    workloads (Firefox, Steam, Kitty, Alacritty) and passing automated X11
    conformance suites.
  * Define the accepted 9P direction's role and application contracts before
    assigning implementation slices. Independent clients, joined lifecycle
    controls and performance comparisons precede protocol retirement.
  * Refine the `SurfaceTransaction` and `RoutedInputRequest` data contracts to
    ensure they remain strictly protocol-neutral.
* **Rules:** Authorities must not link against private engine internals; all
  interaction must flow through `sophia-protocol` types and session routing
  abstractions.

### Phase 2: Protocol Freeze & Surface Stabilization
* **Structure:** Monorepo codebase, but the public wire contracts are formally
  stabilized and sealed.
* **Goals:**
  * Achieve a **Transaction Protocol Freeze**: the schema defining how an
    authority submits anonymous buffers, damage regions, and layout epochs to the
    session supervisor is declared stable.
  * Extract and publish `sophia-protocol` as an independent, semver-governed crate
    with strict backwards-compatibility guarantees.
  * Establish out-of-process integration harnesses: test `sophia-x-authority` and
    `sophia-9p-authority` exclusively as standalone executables connecting over
    simulated Unix domain sockets.

### Phase 3: Autonomous Satellite Repositories (`sophia-org`)
* **Structure:** Frontends migrate to dedicated repositories under `sophia-org`:
  * `sophia-org/sophia`: The core repository containing `sophia-engine`,
    `sophia-session`, `sophia-portal`, and `sophia-protocol`.
  * `sophia-org/sophia-x-authority`: The standalone Phoenix X server frontend.
  * `sophia-org/sophia-9p-authority`: The standalone Plan 9 synthetic filesystem
    frontend.
  * `sophia-org/hagia`: The external reference window manager (already in an
    external repo speaking `sophia_wm_v1`, with a target 9P role interface).
  * `sophia-org/sophia-surface-v1`: Future native GPU client SDK (winit/SDL3
    integration).
* **Goals:**
  * Independent release cycles and tags for each authority.
  * Independent issue tracking, fuzzing pipelines, and community contributors.
  * Automated end-to-end integration testing in `sophia-conformance` pulling
    pinned release binaries.

---

## 4. Extraction Criteria: The Gate to Separate

An authority crate will not be moved to an external repository until it meets all
four **Extraction Gates**:

| Gate | Requirement | Verification Method |
| :--- | :--- | :--- |
| **G1: Wire Stability** | No breaking schema changes to the authority's transaction contract for at least 3 consecutive releases. | Git log analysis of `sophia-protocol` transaction definitions. |
| **G2: Zero Private Linkage** | The authority crate depends *only* on `sophia-protocol`, `sophia-input-authority`, `sophia-portal`, and standard OS crates. No linkage to `sophia-engine`, `sophia-backend-live`, or internal session code. | Cargo dependency audit via `cargo xtask check`. |
| **G3: Standalone CI** | The authority compiles, passes unit tests, and executes smoke suites in headless CI without requiring physical DRM hardware or GPU access. | Offline test execution in unprivileged container. |
| **G4: Conformance Evidence** | The authority passes cross-namespace isolation tests, demonstrates clean recovery from session supervisor restarts, and cleanly manages client teardowns. | `sophia-conformance` integration report. |

---

## 5. Ecosystem Component Matrix

| Component | Current Location | Target Location | Protocol Boundary | Readiness |
| :--- | :--- | :--- | :--- | :--- |
| **`sophia-engine`** | `crates/sophia-engine` | `sophia-org/sophia` | DRM/KMS, DMA-BUF, Internal Scene Graph | Core visual kernel |
| **`sophia-session`** | `crates/sophia-session` | `sophia-org/sophia` | Supervisor lifecycle, Namespace admission | Core supervisor |
| **`sophia-protocol`** | `crates/sophia-protocol` | `sophia-org/sophia` (or standalone crate) | KDL/Binary Wire Schemas | Iterating towards v1.0 freeze |
| **`sophia-portal`** | `crates/sophia-portal` | `sophia-org/sophia` | Deterministic cross-namespace reducers | Implemented |
| **`Hagia`** | External repository | `sophia-org/hagia` | Current `sophia_wm_v1`; target 9P WM role (opaque spatial layout) | External satellite (Active); migration unimplemented |
| **`sophia-x-authority`** | `crates/sophia-x-authority`| `sophia-org/sophia-x-authority` | X11 Wire ──► `SurfaceTransaction` | Phase 1 (Incubating) |
| **`sophia-9p-authority`**| `crates/sophia-9p-authority`| `sophia-org/sophia-9p-authority`| 9P2000.L ──► `SurfaceTransaction` | Phase 1 (Stubbed) |
| **9P desktop role services** | Design only | Crate/process split open | Separate authorized WM, shell and administrative exports | Accepted direction; API unimplemented |

The earlier permanent `sophia-wm-9p-bridge` proposal is replaced by the common
public-interface direction. Any temporary migration adapter requires an explicit
ownership and removal plan; blind policy does not gain a listening service.
Other frontend rows are candidates, not promoted by the 9P decision. Repository
extraction and public IPC migration are independent acceptance decisions.

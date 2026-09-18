# Native Shell Reference-Client Audit

**Role:** requirements and feasibility evidence for the shell preparation
tranche. The architecture, native protocol-family contract, shell direction,
and role schema remain authoritative.
**Status:** source audit and downstream build baseline; no native content
capability or Quickshell Sophia backend is implemented.

**Historical scope:** the status and “current” matrix below describe the
original Quickshell/revision-1–4 feasibility tranche. They are preserved as
baseline evidence. They do not describe today's content or modular admission;
use the [2026-09-18 capability map](native-desktop-capabilities.md) for that.

The [content-shell proposal](content-shell.md) now collects the behavioral
requirements for the panel/popout workflow. This audit owns its source evidence
and feasibility questions; the proposal specifies the intended lifecycle and
authority boundary. Neither admits runtime implementation or assigns wire types.

## Baseline And Purpose

The first downstream reference is [sophia-org/quickshell](https://github.com/sophia-org/quickshell),
forked from the official GitHub mirror. The canonical upstream remains
<https://git.outfoxxed.me/quickshell/quickshell>. The audited baseline is
`2d3b3e9c70ef380dff751b61d334dc88df016c29`; adapter work belongs on the
downstream `sophia` branch, with its own build and dependencies.

Quickshell supplies a demanding toolkit consumer. Narthex remains the
independent descriptor reference, and the [Noctalia survey](sophia-shell-v1-direction.md#what-noctalia-is)
remains evidence from another implementation. None defines Sophia's wire.
The later content prototype must also have an independently written C client
using the published protocol without Qt, Quickshell, or Sophia libraries.

X11 remains the application priority. The first native workflow is a panel
with a button that opens and closes an anchored popout. A control inside the
popout changes its own visual state. This establishes content, local
interaction, reservation, and lifecycle needs without requiring a launcher,
application metadata, a desktop-service port, or a new application frontend.

## Capability And Ownership Matrix

Current means implemented shell revisions 1–4: descriptors, tabs, reference
sheets, and the application launcher. Proposed means a content requirement,
not an allocated capability or wire record.

| Workflow need | Current support or gap | Owner and required boundary |
| --- | --- | --- |
| Admission and negotiation | Current bounded hello/welcome, revision/capability selection, connection epochs | Session admits a separately confined shell; no frontend or toolkit-specific endpoint |
| Panel content | Current descriptors cannot carry arbitrary raster content | Proposed bounded shell-owned content resources and complete visual candidates; shell rasterizes widgets, Engine validates and composites |
| Work-area reservation | Current bounded, output-scoped reservation candidates | Session caps the claim; Engine coordinates coherent presentation/work area; WM chooses application layout |
| Output and panel geometry | Current snapshot supplies opaque output identity/generation; geometry stays Engine-private | Proposed shell-local geometry and scale facts must expose only what the content role needs; frontend resource identities stay private |
| Anchored popout | No arbitrary content surface, parent/anchor, or dismissal vocabulary | Proposed generic shell-owned roles and bounded placement intents; Engine resolves authoritative geometry and stacking |
| Button and popout interaction | Current discrete descriptor activations; no general Qt event stream | First content workflow uses coordinate-free discrete targets; adapter maps actions to widgets. Any later local-coordinate input requires a separate grant |
| Text entry and keyboard modes | Not needed by the first control; no generic content keyboard contract | Deferred; future keyboard disclosure/focus requires explicit admission, not an X11 grab or ambient key feed |
| Content updates and pacing | Prepared/presented outcomes exist for descriptor candidates; no content upload or pacing wire | Proposed resource generations, bounded uploads and feedback; Engine owns the presentation clock, damage and backpressure |
| Hot reload and disconnect | Current recipient epochs, complete snapshots, stale-work rejection, inert retained descriptor pixels | Specify content-resource retirement and reservation cleanup together; Qt generations cannot extend revoked authority |
| Window activation or workspace controls | Broker-issued descriptor actions and blind WM policy are separate roles | Outside the first workflow; richer controls must use their owning broker/session contracts |
| Lock, capture, clipboard and system services | Separate authorities and grants; not implied by content admission | No shortcuts through toolkit APIs or an application's X11/Wayland connection |

An opaque Engine output reference is not a `QScreen`, X output identifier, or
Wayland object. The adapter may maintain local mappings after receiving
authorized facts. It cannot make its platform types part of the protocol.

## Quickshell Integration Findings

At the audited commit, `src/x11/CMakeLists.txt` builds a small XCB-backed
`Quickshell.X11` module. `src/x11/panel_window.cpp` layers panel policy onto
`ProxyWindowBase`. The shared `src/window/proxywindow.cpp` and header create
`ProxiedWindow`/`QQuickWindow` objects and rely on native visibility, screen,
geometry, event delivery, and private Qt Quick APIs. A new socket module alone
does not replace those assumptions.

The downstream adapter therefore has two responsibilities: mapping the generic
shell contract, and integrating Qt's rendering/window lifecycle. Its audit
must include screen models, panel/popout creation, exposure, device scale,
input masks, local focus, scenegraph errors, QML reload, and resource teardown.
QML object ownership and reload generations remain local implementation facts.

Qt's [QQuickRenderControl](https://doc.qt.io/qt-6/qquickrendercontrol.html)
supports hardware-accelerated rendering into an offscreen target while
retaining a QQuickWindow for scene management and event delivery. That is the
candidate for a bounded probe. Its lack of an on-screen window does not prove
that graphics-device initialization is independent of a platform connection,
or solve transport between processes. Both need evidence on the selected Qt
version. [QPA](https://doc.qt.io/qt-6/qpa.html) is a broader platform integration
surface with private interfaces; a full Sophia QPA plugin is not admitted by
this preparation tranche.

The working hypothesis is to retain Qt's rasterizer behind generic content
resources. Engine still owns final GPU composition and presentation. No Qt
scenegraph nodes, GL objects, QRhi objects, shader programs, or process-local
handles become wire types. Future authorized region-local input can feed
Qt's own widget hit testing; Engine's physical target selection and capture
remain authoritative.

## Transport And Performance Questions

Start from the shell direction's content-addressed cached textures. Measure a
static panel, a small changed region, and continuous content animation. Record
uploaded and reused bytes, cache residency, bounded queue depth, CPU raster or
GPU readback costs, Engine upload/composition cost, idle wakeups, frame pacing,
and power where physical instrumentation exists. Retain dimensions, scale,
update rate, hardware and driver identity with each measurement.

Qt's render-control update requests must be reconciled with Engine pacing.
Content-generation updates may not introduce an independent compositor clock
or an unbounded producer queue. Prepared content cannot become an input target
before its matching presented geometry.

FD or DMA-BUF transfer remains a candidate only if measured cached-byte
transport is insufficient. Before admission, specify allocation ownership,
format/size limits, synchronization, damage, lifetime, fallback and renderer
failure. Existing X11 DRI3/Present support does not establish this shell trust
boundary. GPU composition does not require a GPU-handle ABI for shell clients;
the independent C client may produce a simple CPU raster of its own content.

## Subsequent Prototype Acceptance

These are requirements for the next separately admitted tranche, not claims
about the current implementation:

1. Publish the smallest generic content/interaction contract and corpus. Both
   Quickshell and the independent C client render a panel, claim its bounded
   work area, open a popout, change local state, and dismiss it through that
   same contract. Narthex and the retained revision-1/2 corpora keep passing.
2. Run protocol/lifecycle cases in an Engine test host with no X application
   frontend. Run the adapter probe without `DISPLAY` or `WAYLAND_DISPLAY` and
   prove it needs no application-facing display socket. Any platform/device
   initialization blocker must be resolved downstream or recorded as a failed
   hypothesis before claiming native feasibility.
3. Reject unsupported capabilities, malformed or oversized content, stale
   output/resource/connection generations, and input against unpresented
   content. Test bounded resource and queue exhaustion without losing the last
   coherent presentation.
4. Exercise resize, scale/output changes, popout dismissal, QML reload, peer
   crash/reconnect, and renderer failure. Revoke old interactions immediately,
   retire resources safely, and reconcile reservations atomically. Neither
   client receives foreign pixels, namespace identities, or WM authority.
5. Retain the performance measurements above and a short physical panel/popout
   check in an X11 Sophia session once installation is explicitly scheduled.
   Offline rendering and existing X11 Quickshell smokes are distinct evidence;
   neither substitutes for physical presentation and input acceptance.

## Preparation Evidence

The fork and local branch are established. The pinned Nix development shell
could not start because the environment has no Nix daemon socket. The operator
installed the Void build dependencies, and native CMake configuration succeeded
with GCC 14.2.1 and Qt Core/Quick/ShaderTools 6.11.1, Debug, tests enabled, and
the default X11/Wayland/service features. The build passed all 1,361 initial
steps. Eight of nine offscreen/software CTest suites passed. The upstream
`TestPopupWindow::moveWithParent` assertion failed at line 115 (x = 12,
expected 20); the same case also failed on Qt's minimal platform (x = 10,
expected 20). This is recorded baseline debt, with no test changes or skips.
Its cause and behavior under an isolated X11 server remain unverified.

The downstream `SOPHIA.md` retains the exact commands, source identity and
results; local build artifacts retain compiler warnings and the complete test
log. Qt Core/Quick/ShaderTools are 6.11.1 and CMake is 4.2.2. The fork's `sophia`
branch is published at the upstream baseline, and its issue tracker is enabled
for downstream crash reports. Documentation and crash-default changes are
tracked separately from the upstream baseline; publishing those commits does
not establish native backend support.

No native backend, content wire, application frontend, live profile change,
or shell installation is part of this preparation evidence.

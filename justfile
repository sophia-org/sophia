# Sophia's human entry points.
#
# `just --list` is the optional, memorable surface someone runs by hand.
# Recipes contain no workflow logic: they delegate to `cargo xtask`, which is
# also the canonical CI interface. Installed sessions invoke Sophia directly;
# neither production nor repository scripts depend on `just`.

_default:
    @just --list --unsorted

# One GPU client filling one head with nothing else drawn: the configuration in
# which a frame can be one opaque client DMA-BUF, which is what direct scanout
# requires.
#
# It runs no window manager. It cannot -- `sophia-wm-demo` lost its serving
# mode in 83596bfc with the experimental WM API v7 -- and it does not need one:
# a session without a WM honours the client's own geometry, so a client asked
# for the head's size fills it, and no WM means no focus ring and no border
# over the frame.
#
# The client is Kitty, because it is the one this stack is known to hand
# DMA-BUFs: every promoted Hagia archive carries hundreds of DMA-BUF frames
# from it, while vkcube on this machine presents through the software path --
# 389 Presents, every one a CPU layer.
#
# The input-latency harness cannot answer this question. It proves input
# reaches a terminal, so it needs an input proof, and the session refuses
# `--terminal-exec` alongside one; its client is always xterm, which draws
# through X core rendering and never presents a DMA-BUF at all.

# Prove a client's own buffer can reach the plane uncomposed. Run from tty3.
direct-scanout-probe width='2560' height='1440' hold='20' workload='kitty':
    @cargo --quiet xtask conformance run direct-scanout "{{ width }}" "{{ height }}" "{{ hold }}" "{{ workload }}"

# Binds a signed identity, runs the probe, verifies and archives it. Run from
# tty3. The probe recipe above is the same session without the archive.

# Promote a direct-scanout run as immutable evidence. Run from tty3.
direct-scanout-gate:
    @cargo --quiet xtask conformance gate direct-scanout

# Prove a directly scanned output returns to composition when an overlay opens,
# and that eligibility comes back only through a fresh atomic test. Run from
# tty3. The overlay is opened by the session itself: the shell that would open
# one in a product session is what this session does not run.
direct-scanout-overlay-gate:
    @cargo --quiet xtask conformance gate direct-scanout --overlay-proof

# Measure whether a direct frame costs less than a composed one, on one head
# in one session: direct flips outside the overlay window, composed frames
# inside it. Run from tty3. Holds the overlay far longer than the transition
# proof does, because a composed population needs to be a distribution.
direct-scanout-cost-gate:
    @cargo --quiet xtask conformance gate direct-scanout --cost

# Prove the legacy hardware cursor keeps working while the plane scans a
# client's buffer directly. Run from tty3. This is the baseline the atomic
# cursor plane has to match, and the claim no run has yet tested: every
# direct-scanout archive so far had a cursor that was visible and never moved.
direct-scanout-cursor-gate:
    @cargo --quiet xtask conformance gate direct-scanout --cursor

# The same proof with the cursor on an atomic plane instead of the legacy
# ioctl. Run from tty3. Compare its motion-to-submit against archive 0004,
# which is the same sweep on the path this one replaces.
direct-scanout-atomic-cursor-gate:
    @cargo --quiet xtask conformance gate direct-scanout --atomic-cursor

# The bounded glxgears benchmark, hands off the mouse. Run from tty3.
# This is the unshaken baseline for the recipe below.
glxgears-benchmark:
    @tools/benchmark_sophia_glxgears_tty3.sh

# The same benchmark with a virtual mouse shaking the pointer at 1 kHz from
# the moment the client holds focus. Run from tty3; keep hands off the mouse.
# The summary lands in ~/.local/state/sophia/standalone-session/shake.log.
glxgears-shake hz='1000' amplitude='8':
    @SOPHIA_GLXGEARS_SHAKE_HZ="{{ hz }}" SOPHIA_GLXGEARS_SHAKE_AMPLITUDE="{{ amplitude }}" tools/benchmark_sophia_glxgears_shake_tty3.sh

# Re-verify an archived direct-scanout run, newest by default.
direct-scanout-archive run='':
    @cargo --quiet xtask conformance verify direct-scanout-archive "{{ run }}"

# Read what the last direct-scanout probe measured.
direct-scanout-verify log='':
    @cargo --quiet xtask conformance verify direct-scanout-standalone "{{ log }}"

# The check none of the four failed physical runs had: three died assembling a
# vector nothing validated until the display manager was already down.

# Build and validate every tool profile's session argument vector.
check-profiles:
    @cargo --quiet xtask profile check

# Fail if the exact source-layout debt differs from its reviewed ledger.
check-layout:
    @cargo --quiet xtask check layout

# Run the canonical offline, non-hardware repository gate.
check:
    @cargo --quiet xtask check

# The one recipe that does not route through `cargo xtask`: installing a
# session is an operator action on a physical machine, not a gate CI can run,
# and putting it behind the canonical CI interface would misrepresent it as
# one. It delegates to `tools/`, where the packaging and installation scripts
# it composes already live.
#
# Everything is derived from the worktree, so there is no commit hash to keep
# up to date, and it is idempotent: installing what is already installed
# reports that and stops.

# Package this commit and install it as the live session. Prompts for sudo.
install-session:
    @tools/install_session_from_head.sh

# Needs no privileges. The policy client is a blind client the Engine
# validates, so it lives where its owner can replace it rather than inside the
# checksummed release, and a reload is an ordinary file replacement followed by
# a supervised restart. A reload that does not settle is rolled back.

# Rebuild Hagia and restart it in the running session, keeping the windows.
reload-wm:
    @tools/reload_policy_client.sh

# Prepare the signed, immutable diagnostic matrix.
desktop-comparison-prepare run:
    @cargo --quiet xtask conformance desktop-comparison prepare "{{ run }}"

# Launch, capture, seal, and tear down exactly the next row from tty3.
desktop-comparison-row run:
    @cargo --quiet xtask conformance desktop-comparison gate "{{ run }}"

# Read the exact next row before selecting its local greetd session.
desktop-comparison-status run:
    @cargo --quiet xtask conformance desktop-comparison status "{{ run }}"

# Bind the active local supervisor and DP-1 CRTC to the exact next row.
desktop-comparison-attest run supervisor_pid crtc:
    @cargo --quiet xtask conformance desktop-comparison attest "{{ run }}" "{{ supervisor_pid }}" "{{ crtc }}"

# Check session, tool, topology, profile, and kernel-timing readiness.
desktop-comparison-preflight run:
    @cargo --quiet xtask conformance desktop-comparison preflight "{{ run }}"

# Capture and seal exactly one row. This may prompt for tracefs-only sudo.
desktop-comparison-capture run:
    @cargo --quiet xtask conformance desktop-comparison capture "{{ run }}"

# Register a clean pinned XLibre prefix and compile the isolated xmonad profile.
desktop-comparison-install-reference xlibre_source prefix:
    @cargo --quiet xtask conformance desktop-comparison install-reference "{{ xlibre_source }}" "{{ prefix }}"

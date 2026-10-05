#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${SOPHIA_QEMU_OUT_DIR:-$ROOT_DIR/.qemu}"
KERNEL_VERSION="${SOPHIA_QEMU_KERNEL_VERSION:-$(uname -r)}"
KERNEL_IMAGE="${SOPHIA_QEMU_KERNEL:-/boot/vmlinuz-$KERNEL_VERSION}"
INITRAMFS="${SOPHIA_QEMU_INITRAMFS:-$OUT_DIR/sophia-$KERNEL_VERSION.img}"
SCENARIO="${SOPHIA_QEMU_SCENARIO:-session}"
TWO_XTERM="${SOPHIA_QEMU_TWO_XTERM:-0}"
SHARED_RENDERER_WORKER="${SOPHIA_ENABLE_SHARED_RENDERER_WORKER:-0}"
DIRECT_SCANOUT="${SOPHIA_ENABLE_DIRECT_SCANOUT:-0}"
SINGLE_CARD="${SOPHIA_QEMU_SINGLE_CARD:-0}"
EXPECT_RENDERER_WORKERS="${SOPHIA_QEMU_EXPECT_RENDERER_WORKERS:-}"
GPU_MODE="${SOPHIA_QEMU_GPU_MODE:-software}"
RENDER_NODE="${SOPHIA_QEMU_RENDER_NODE:-/dev/dri/renderD128}"
XTEST_ROW="${SOPHIA_QEMU_XTEST_ROW:-}"

CPU_MODE="${SOPHIA_QEMU_CPU_MODE:-open}"
CPU_SECONDS="${SOPHIA_QEMU_CPU_SECONDS:-60}"
CPU_GRACE="${SOPHIA_QEMU_CPU_GRACE:-10}"
CPU_RATE="${SOPHIA_QEMU_CPU_RATE:-5}"
CPU_TARGET="${SOPHIA_QEMU_CPU_TARGET:-zero}"
CPU_CLIENTS="${SOPHIA_QEMU_CPU_CLIENTS:-2}"
CPU_SIZE="${SOPHIA_QEMU_CPU_SIZE:-small}"
CPU_DAMAGE="${SOPHIA_QEMU_CPU_DAMAGE:-absent}"
CPU_EVIDENCE="${SOPHIA_QEMU_CPU_EVIDENCE:-full}"
# The session-lock-provider scenario's stand-in provider: baseline, flood or stall.
LOCK_PROVIDER_MODE="${SOPHIA_QEMU_LOCK_PROVIDER_MODE:-}"
lock_provider_cmdline=""
if [[ "$SCENARIO" == session-lock-provider ]]; then
    case "$LOCK_PROVIDER_MODE" in
        baseline|flood|stall) ;;
        *)
            echo "SOPHIA_QEMU_LOCK_PROVIDER_MODE must be baseline, flood or stall" >&2
            exit 1
            ;;
    esac
    lock_provider_cmdline=" sophia.lock_provider_mode=$LOCK_PROVIDER_MODE"
elif [[ -n "$LOCK_PROVIDER_MODE" ]]; then
    echo "SOPHIA_QEMU_LOCK_PROVIDER_MODE is only for the session-lock-provider scenario" >&2
    exit 1
fi
# The output-unplug scenario's action (t306): one, one-return, all-return or
# input-return. The host takes away the last head or every head, and the
# guest the keyboard, and gives it back in the return modes;
# SOPHIA_QEMU_SINGLE_CARD picks two heads on one card or one head on each of
# two cards.
UNPLUG_MODE="${SOPHIA_QEMU_UNPLUG_MODE:-}"
unplug_cmdline=""
if [[ "$SCENARIO" == output-unplug ]]; then
    case "$UNPLUG_MODE" in
        one|one-return|all-return|input-return) ;;
        *)
            echo "SOPHIA_QEMU_UNPLUG_MODE must be one, one-return, all-return or input-return" >&2
            exit 1
            ;;
    esac
    unplug_cmdline=" sophia.unplug_mode=$UNPLUG_MODE"
    # Diagnostic only, off by default: the guest kernel logs its DRM ioctls,
    # atomic checks and probes (drm.debug core, atomic and KMS bits), so a
    # refused commit names its errno, and the guest copies its kernel log from
    # the action window into the evidence.
    # With 1, the session runs the generic test WM, so a loss settles through
    # a WM relayout and policy commit rather than the presentation deadline.
    case "${SOPHIA_QEMU_UNPLUG_WM:-0}" in
        0) ;;
        1) unplug_cmdline+=" sophia.unplug_wm=1" ;;
        *) echo "SOPHIA_QEMU_UNPLUG_WM must be 0 or 1" >&2; exit 1 ;;
    esac
    case "${SOPHIA_QEMU_UNPLUG_CONSOLE:-1}" in
        0|1) ;;
        *) echo "SOPHIA_QEMU_UNPLUG_CONSOLE must be 0 or 1" >&2; exit 1 ;;
    esac
    # dri3 adds a client that presents one DMA-BUF frame and then holds its
    # window still, so the removal meets content with no next frame.
    case "${SOPHIA_QEMU_UNPLUG_CLIENT:-none}" in
        none) ;;
        dri3) unplug_cmdline+=" sophia.unplug_client=dri3" ;;
        *) echo "SOPHIA_QEMU_UNPLUG_CLIENT must be none or dri3" >&2; exit 1 ;;
    esac
    case "${SOPHIA_QEMU_UNPLUG_KMSG:-0}" in
        0) ;;
        1) unplug_cmdline+=" drm.debug=0x15 log_buf_len=16M sophia.unplug_kmsg=1" ;;
        *) echo "SOPHIA_QEMU_UNPLUG_KMSG must be 0 or 1" >&2; exit 1 ;;
    esac
elif [[ -n "$UNPLUG_MODE" ]]; then
    echo "SOPHIA_QEMU_UNPLUG_MODE is only for the output-unplug scenario" >&2
    exit 1
fi
# virgl (a DMA-BUF client needs a 3D-capable guest GPU) is supported on one
# card with two heads only.
if [[ "$SCENARIO" == output-unplug && "${SOPHIA_QEMU_GPU_MODE:-software}" == virgl \
    && "${SOPHIA_QEMU_SINGLE_CARD:-0}" != 1 ]]; then
    echo "the output-unplug scenario runs virgl only with SOPHIA_QEMU_SINGLE_CARD=1" >&2
    exit 1
fi
# Diagnostic only, off by default: on the first native page-flip hard stall
# the guest's stamper asks the guest kernel for its blocked tasks (SysRq w)
# and copies that report into the evidence (tools/qemu_line_stamp.c).
SYSRQ_ON_HARD_STALL="${SOPHIA_QEMU_SYSRQ_ON_HARD_STALL:-0}"
case "$SYSRQ_ON_HARD_STALL" in
    0) ;;
    1)
        [[ "$SCENARIO" == session-lock-provider ]] || {
            echo "SOPHIA_QEMU_SYSRQ_ON_HARD_STALL is only for the session-lock-provider scenario" >&2
            exit 1
        }
        lock_provider_cmdline+=" sophia.sysrq_on_hard_stall=1"
        ;;
    *)
        echo "SOPHIA_QEMU_SYSRQ_ON_HARD_STALL must be 0 or 1" >&2
        exit 1
        ;;
esac

cpu_cmdline=""
if [[ "$SCENARIO" == cpu ]]; then
    if [[ "$CPU_EVIDENCE" != full && "$CPU_EVIDENCE" != aggregate ]]; then
        echo "invalid CPU evidence mode" >&2; exit 1
    fi
    if [[ "$CPU_MODE" != open && "$CPU_MODE" != closed ]] \
        || [[ "$CPU_TARGET" != zero && "$CPU_TARGET" != next ]]; then
        echo "invalid CPU workload switch" >&2; exit 1
    fi
    if [[ "$CPU_CLIENTS" != 1 && "$CPU_CLIENTS" != 2 ]] \
        || [[ "$CPU_SIZE" != small && "$CPU_SIZE" != head ]] \
        || [[ "$CPU_DAMAGE" != absent && "$CPU_DAMAGE" != full && "$CPU_DAMAGE" != patch ]] \
        || [[ "$CPU_SIZE" == head && "$CPU_CLIENTS" != 1 ]]; then
        echo "invalid CPU size/damage or overlapping head-sized windows" >&2; exit 1
    fi
    for value in "$CPU_SECONDS" "$CPU_GRACE" "$CPU_RATE"; do
        if [[ ! "$value" =~ ^[1-9][0-9]{0,2}$ ]]; then
            echo "invalid CPU numeric bound" >&2; exit 1
        fi
    done
    if (( CPU_SECONDS < 10 || CPU_SECONDS > 120 || CPU_GRACE < 5 || CPU_GRACE > 30 || CPU_RATE > 240 )); then
        echo "CPU bounds: seconds 10..120, grace 5..30, rate 1..240" >&2; exit 1
    fi
    cpu_cmdline=" sophia.cpu_mode=$CPU_MODE sophia.cpu_seconds=$CPU_SECONDS sophia.cpu_grace=$CPU_GRACE sophia.cpu_rate=$CPU_RATE sophia.cpu_target=$CPU_TARGET"
    cpu_cmdline+=" sophia.cpu_clients=$CPU_CLIENTS sophia.cpu_size=$CPU_SIZE sophia.cpu_damage=$CPU_DAMAGE"
    cpu_cmdline+=" sophia.cpu_evidence=$CPU_EVIDENCE"
fi

case "$SCENARIO" in
    session|emergency-recovery|gtk-classic|gtk-confined|session-lock|session-lock-provider|xtest-selection|cpu|output-unplug) ;;
    *)
        echo "SOPHIA_QEMU_SCENARIO must be session, emergency-recovery, gtk-classic, gtk-confined, session-lock, session-lock-provider, xtest-selection, cpu, or output-unplug" >&2
        exit 1
        ;;
esac
if [[ -n "$XTEST_ROW" && ( "$SCENARIO" != xtest-selection || ! "$XTEST_ROW" =~ ^[0-7]$ ) ]]; then
    echo "SOPHIA_QEMU_XTEST_ROW is a row 0-7 and only the xtest-selection scenario takes it" >&2
    exit 1
fi
if [[ "$TWO_XTERM" != 0 && "$TWO_XTERM" != 1 ]]; then
    echo "SOPHIA_QEMU_TWO_XTERM must be 0 or 1" >&2
    exit 1
fi
if [[ "$SCENARIO" != session && "$TWO_XTERM" != 0 ]]; then
    echo "SOPHIA_QEMU_TWO_XTERM is only supported by the session scenario" >&2
    exit 1
fi
if [[ "$SHARED_RENDERER_WORKER" != 0 && "$SHARED_RENDERER_WORKER" != 1 ]]; then
    echo "SOPHIA_ENABLE_SHARED_RENDERER_WORKER must be 0 or 1" >&2
    exit 1
fi
if [[ "$DIRECT_SCANOUT" != 0 && "$DIRECT_SCANOUT" != 1 ]]; then
    echo "SOPHIA_ENABLE_DIRECT_SCANOUT must be 0 or 1" >&2
    exit 1
fi
if [[ "$SINGLE_CARD" != 0 && "$SINGLE_CARD" != 1 ]]; then
    echo "SOPHIA_QEMU_SINGLE_CARD must be 0 or 1" >&2
    exit 1
fi
if [[ "$GPU_MODE" != software && "$GPU_MODE" != virgl ]]; then
    echo "SOPHIA_QEMU_GPU_MODE must be software or virgl" >&2
    exit 1
fi

case "$SCENARIO" in
    cpu) DEFAULT_EVIDENCE_FILE=/tmp/sophia-qemu-cpu.log ;;
    emergency-recovery) DEFAULT_EVIDENCE_FILE=/tmp/sophia-qemu-emergency-recovery.log ;;
    gtk-*|session-lock|session-lock-provider|xtest-selection|output-unplug) DEFAULT_EVIDENCE_FILE="/tmp/sophia-qemu-$SCENARIO.log" ;;
    *) DEFAULT_EVIDENCE_FILE=/tmp/sophia-qemu-session.log ;;
esac

EVIDENCE_FILE="${SOPHIA_QEMU_EVIDENCE:-$DEFAULT_EVIDENCE_FILE}"
QEMU_BIN="${SOPHIA_QEMU_BIN:-qemu-system-x86_64}"
QEMU_ACCEL="${SOPHIA_QEMU_ACCEL:-kvm:tcg}"
if [[ "$QEMU_ACCEL" != kvm && "$QEMU_ACCEL" != tcg && "$QEMU_ACCEL" != kvm:tcg ]]; then
    echo "SOPHIA_QEMU_ACCEL must be kvm, tcg, or kvm:tcg" >&2
    exit 1
fi
MEMORY_MIB="${SOPHIA_QEMU_MEMORY_MIB:-2048}"
VIRTUAL_CPUS="${SOPHIA_QEMU_CPUS:-2}"
VNC_SOCKET="${SOPHIA_QEMU_VNC_SOCKET:-$OUT_DIR/display.sock}"
QMP_SOCKET="${SOPHIA_QEMU_QMP_SOCKET:-$OUT_DIR/qmp.sock}"
SERIAL_FIFO="${SOPHIA_QEMU_SERIAL_FIFO:-$OUT_DIR/serial.fifo}"
DISPLAY_BUS_SOCKET="$OUT_DIR/display-bus.sock"
QEMU_PID=""
LOGGER_PID=""
DISPLAY_BUS_PID=""

cleanup() {
    if [[ -n "$QEMU_PID" ]] && kill -0 "$QEMU_PID" 2>/dev/null; then
        kill "$QEMU_PID" 2>/dev/null || true
        wait "$QEMU_PID" 2>/dev/null || true
    fi
    if [[ -n "$LOGGER_PID" ]] && kill -0 "$LOGGER_PID" 2>/dev/null; then
        kill "$LOGGER_PID" 2>/dev/null || true
        wait "$LOGGER_PID" 2>/dev/null || true
    fi
    if [[ -n "$DISPLAY_BUS_PID" ]] && kill -0 "$DISPLAY_BUS_PID" 2>/dev/null; then
        kill "$DISPLAY_BUS_PID" 2>/dev/null || true
        wait "$DISPLAY_BUS_PID" 2>/dev/null || true
    fi
    rm -f "$VNC_SOCKET" "$QMP_SOCKET" "$SERIAL_FIFO" "$DISPLAY_BUS_SOCKET"
}
trap cleanup EXIT

if ! command -v "$QEMU_BIN" >/dev/null 2>&1; then
    echo "missing qemu-system-x86_64; on Void install it with:" >&2
    echo "  sudo xbps-install -S qemu-system-amd64" >&2
    exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
    echo "missing python3; on Void install it with:" >&2
    echo "  sudo xbps-install -S python3" >&2
    exit 1
fi
if [[ ! -r "$KERNEL_IMAGE" ]]; then
    echo "guest kernel is not readable: $KERNEL_IMAGE" >&2
    exit 1
fi
if [[ ! -r "$INITRAMFS" ]]; then
    echo "guest initramfs is not readable: $INITRAMFS" >&2
    echo "build it first with tools/build_qemu_session_initramfs.sh" >&2
    exit 1
fi
if [[ ! "$MEMORY_MIB" =~ ^[0-9]+$ ]] || (( MEMORY_MIB < 512 || MEMORY_MIB > 16384 )); then
    echo "SOPHIA_QEMU_MEMORY_MIB must be from 512 through 16384" >&2
    exit 1
fi
if [[ ! "$VIRTUAL_CPUS" =~ ^[0-9]+$ ]] || (( VIRTUAL_CPUS < 1 || VIRTUAL_CPUS > 16 )); then
    echo "SOPHIA_QEMU_CPUS must be from 1 through 16" >&2
    exit 1
fi
if [[ "$GPU_MODE" == virgl && ! -c "$RENDER_NODE" ]]; then
    echo "Virgl QEMU mode requires a DRM render node: $RENDER_NODE" >&2
    exit 1
fi

if [[ "$GPU_MODE" == virgl ]]; then
    display_args=(-display "egl-headless,rendernode=$RENDER_NODE")
    if [[ "$SINGLE_CARD" == 1 ]]; then
        gpu_args=(-device virtio-vga-gl,max_outputs=2)
    else
        gpu_args=(-device virtio-vga-gl,max_outputs=1 -device virtio-gpu-pci,max_outputs=1)
    fi
else
    display_args=(-display none -vnc "unix:$VNC_SOCKET")
    if [[ "$SINGLE_CARD" == 1 ]]; then
        gpu_args=(-device virtio-vga,max_outputs=2)
    else
        gpu_args=(-device virtio-vga,max_outputs=1 -device virtio-gpu-pci,max_outputs=1)
    fi
fi

forced_connector=""
if [[ "$SINGLE_CARD" == 1 && "$SCENARIO" != output-unplug ]]; then
    forced_connector=" video=Virtual-2:1280x800@60e"
fi
# The output-unplug scenario takes heads away the way a display does: QEMU's
# D-Bus display sets a head's size to zero, which clears it from virtio-gpu's
# enabled outputs and raises the device's display event, so the guest kernel
# reprobes and sends the hotplug uevent Sophia rescans on. A forced connector
# would ignore that, so the second head is enabled by the host instead, while
# the guest is still paused (-S).
pause_args=()
if [[ "$SCENARIO" == output-unplug ]]; then
    if [[ "$GPU_MODE" == virgl ]]; then
        display_args=(-display "dbus,addr=unix:path=$DISPLAY_BUS_SOCKET,gl=on,rendernode=$RENDER_NODE")
    else
        display_args=(-display "dbus,addr=unix:path=$DISPLAY_BUS_SOCKET,gl=off")
    fi
    pause_args=(-S)
fi

mkdir -p "$(dirname "$EVIDENCE_FILE")"
: > "$EVIDENCE_FILE"
rm -f "$VNC_SOCKET" "$QMP_SOCKET" "$SERIAL_FIFO" "$DISPLAY_BUS_SOCKET"
mkfifo "$SERIAL_FIFO"
if [[ "$SCENARIO" == output-unplug ]]; then
    # A private bus that only QEMU and this harness use.
    cat > "$OUT_DIR/display-bus.conf" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path=$DISPLAY_BUS_SOCKET</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF
    dbus-daemon --config-file="$OUT_DIR/display-bus.conf" --nofork --nopidfile &
    DISPLAY_BUS_PID=$!
    for _ in $(seq 1 100); do
        [[ -S "$DISPLAY_BUS_SOCKET" ]] && break
        sleep 0.05
    done
fi

case "$SCENARIO" in
    emergency-recovery)
        echo "sophia_qemu_recovery schema=1 status=starting isolation=headless control=qmp-unix host_drm=none host_vt=none keyboard=virtio chord=ctrl-alt-backspace" | tee -a "$EVIDENCE_FILE"
        ;;
    gtk-*|session-lock|session-lock-provider)
        echo "sophia_qemu_gtk schema=1 status=starting isolation=headless control=qmp-unix host_drm=none host_vt=none keyboard=virtio mouse=virtio scenario=$SCENARIO" | tee -a "$EVIDENCE_FILE"
        ;;
    output-unplug)
        # virgl renders the guest on a host render node; no host card or
        # input device is passed through either way.
        host_drm=none
        [[ "$GPU_MODE" == virgl ]] && host_drm=render_node
        echo "sophia_qemu_unplug schema=1 status=starting isolation=headless control=none host_drm=$host_drm host_vt=none gpu=virtio-gpu mode=$UNPLUG_MODE single_card=$SINGLE_CARD gpu_mode=$GPU_MODE console=${SOPHIA_QEMU_UNPLUG_CONSOLE:-1}" | tee -a "$EVIDENCE_FILE"
        if [[ "$GPU_MODE" == virgl ]]; then
            # Which host GPU the guest renders on.
            echo "sophia_qemu_unplug schema=1 status=render_node node=$RENDER_NODE rdev=$(stat -c '%t:%T' "$RENDER_NODE") device=$(readlink -f "/sys/class/drm/$(basename "$RENDER_NODE")/device")" | tee -a "$EVIDENCE_FILE"
        fi
        ;;
    xtest-selection)
        echo "sophia_qemu_xtest_selection schema=1 status=starting isolation=headless control=none host_drm=none host_vt=none gpu=virtio-gpu input=xtest row=${XTEST_ROW:-0}" | tee -a "$EVIDENCE_FILE"
        ;;
    *)
        echo "sophia_qemu_session schema=3 status=starting isolation=headless display_sink=vnc-unix control=qmp-unix host_drm=none host_vt=none guest_network=none storage=none gpu=virtio-gpu gpu_devices=2 gpu_heads=2 keyboard=virtio mouse=virtio ticks=300" | tee -a "$EVIDENCE_FILE"
        ;;
esac

while IFS= read -r line || [[ -n "$line" ]]; do
    printf '%s\n' "${line%$'\r'}"
done < "$SERIAL_FIFO" | tee -a "$EVIDENCE_FILE" &
LOGGER_PID=$!

cpu_export_args=()
if [[ "$SCENARIO" == cpu ]]; then
    CPU_EXPORT_FILE="$OUT_DIR/cpu-export.log"
    cpu_export_args=(-device virtio-serial-pci
        -chardev "file,id=cpuevidence,path=$CPU_EXPORT_FILE"
        -device virtserialport,chardev=cpuevidence,name=sophia.cpu.evidence)
fi

"$QEMU_BIN" \
    -machine "q35,accel=$QEMU_ACCEL" \
    -smp "$VIRTUAL_CPUS" \
    -m "$MEMORY_MIB" \
    -nodefaults \
    -no-reboot \
    "${display_args[@]}" \
    -monitor none \
    "${pause_args[@]}" \
    -qmp "unix:$QMP_SOCKET,server=on,wait=off" \
    -serial stdio \
    "${cpu_export_args[@]}" \
    "${gpu_args[@]}" \
    -device virtio-keyboard-pci \
    -device virtio-mouse-pci \
    -kernel "$KERNEL_IMAGE" \
    -initrd "$INITRAMFS" \
    -append "console=ttyS0 quiet loglevel=3 rdinit=/sbin/sophia-qemu-init rd.driver.pre=virtio_pci rd.driver.pre=virtio_gpu rd.driver.pre=virtio_input panic=-1 sophia.scenario=$SCENARIO sophia.two_xterm=$TWO_XTERM sophia.shared_renderer_worker=$SHARED_RENDERER_WORKER sophia.direct_scanout=$DIRECT_SCANOUT$cpu_cmdline$lock_provider_cmdline$unplug_cmdline$forced_connector${XTEST_ROW:+ sophia.xtest_row=$XTEST_ROW}" \
    > "$SERIAL_FIFO" 2>&1 &
QEMU_PID=$!

if [[ "$SCENARIO" == cpu ]]; then
    # Guest input is autonomous. A host bound also covers a hung teardown.
    deadline=$((SECONDS + CPU_GRACE + CPU_SECONDS + 90))
    while kill -0 "$QEMU_PID" 2>/dev/null && (( SECONDS < deadline )); do sleep 0.2; done
    if kill -0 "$QEMU_PID" 2>/dev/null; then
        echo "sophia_qemu_cpu schema=1 status=failed reason=host_timeout" | tee -a "$EVIDENCE_FILE"
        exit 1
    fi
    wait "$QEMU_PID"
    QEMU_PID=""
    wait "$LOGGER_PID"
    LOGGER_PID=""
    grep -q '^sophia_qemu_cpu schema=1 status=exited session_exit=0 export_exit=0$' "$EVIDENCE_FILE"
    grep -q '^sophia_present_cpu_result ' "$CPU_EXPORT_FILE"
    exit 0
fi

if [[ "$SCENARIO" == emergency-recovery ]]; then
    guard_ready=false
    for _ in $(seq 1 600); do
        if grep -q '^sophia_session_input_guard schema=2 status=ready ' "$EVIDENCE_FILE"; then
            guard_ready=true
            break
        fi
        kill -0 "$QEMU_PID" 2>/dev/null || break
        sleep 0.05
    done
    if [[ "$guard_ready" != true ]]; then
        echo "sophia_qemu_recovery schema=1 status=failed reason=input_guard_readiness_timeout" | tee -a "$EVIDENCE_FILE"
        exit 1
    fi

    if ! "$ROOT_DIR/tools/qemu_qmp_emergency_chord.py" "$QMP_SOCKET"; then
        echo "sophia_qemu_recovery schema=1 status=failed reason=qmp_arm_input_send" | tee -a "$EVIDENCE_FILE"
        exit 1
    fi
    echo "sophia_qemu_recovery_input schema=1 status=sent phase=arm source=qmp device=virtio-keyboard chord=ctrl-alt-backspace events=6" | tee -a "$EVIDENCE_FILE"

    # The trigger must not land on a session that is still coming up. Focus
    # readiness says a surface can receive keys, not that the session ever put a
    # frame on screen, and a chord delivered in between ends the run with a
    # startup-readiness failure instead of the clean emergency exit this
    # scenario exists to prove -- the session reporting, accurately,
    # `in_flight_displayed=0`. `startup schema=2 status=ready` is the state that
    # settles it: surface, visual detail and a presented frame, all true.
    recovery_ready=false
    for _ in $(seq 1 600); do
        if grep -q '^sophia_session_input_guard schema=1 status=armed$' "$EVIDENCE_FILE" \
            && grep -q '^sophia_live_session_input_pipeline schema=4 status=poller_ready ' "$EVIDENCE_FILE" \
            && grep -q '^sophia_live_session_startup schema=2 status=ready ' "$EVIDENCE_FILE" \
            && grep -q '^sophia_live_session_input_pipeline schema=1 status=focus_ready$' "$EVIDENCE_FILE"; then
            recovery_ready=true
            break
        fi
        kill -0 "$QEMU_PID" 2>/dev/null || break
        sleep 0.05
    done
    if [[ "$recovery_ready" != true ]]; then
        echo "sophia_qemu_recovery schema=1 status=failed reason=armed_session_readiness_timeout" | tee -a "$EVIDENCE_FILE"
        exit 1
    fi

    if ! "$ROOT_DIR/tools/qemu_qmp_emergency_chord.py" "$QMP_SOCKET"; then
        echo "sophia_qemu_recovery schema=1 status=failed reason=qmp_trigger_input_send" | tee -a "$EVIDENCE_FILE"
        exit 1
    fi
    echo "sophia_qemu_recovery_input schema=1 status=sent phase=trigger source=qmp device=virtio-keyboard chord=ctrl-alt-backspace events=6" | tee -a "$EVIDENCE_FILE"

    set +e
    wait "$QEMU_PID"
    qemu_status=$?
    QEMU_PID=""
    wait "$LOGGER_PID"
    logger_status=$?
    LOGGER_PID=""
    set -e
    cleanup

    if [[ "$qemu_status" -ne 0 || "$logger_status" -ne 0 ]]; then
        echo "sophia_qemu_recovery schema=1 status=failed reason=guest_exit qemu_exit=$qemu_status logger_exit=$logger_status" | tee -a "$EVIDENCE_FILE"
        exit 1
    fi
    echo "sophia_qemu_recovery schema=1 status=complete qemu_exit=0" | tee -a "$EVIDENCE_FILE"
    "$ROOT_DIR/tools/verify_qemu_emergency_recovery_evidence.sh" "$EVIDENCE_FILE"
    exit 0
fi

wait_for_evidence() {
    local pattern="$1" reason="$2"
    for _ in $(seq 1 600); do
        if grep -Eq "$pattern" "$EVIDENCE_FILE"; then
            return 0
        fi
        kill -0 "$QEMU_PID" 2>/dev/null || break
        sleep 0.05
    done
    echo "sophia_qemu_gtk schema=1 status=failed reason=$reason scenario=$SCENARIO" | tee -a "$EVIDENCE_FILE"
    exit 1
}

if [[ "$SCENARIO" == session-lock ]]; then
    # The session locks itself once zenity is presented. Every key below is
    # physical (virtio keyboard), the only kind a lock takes; none may reach
    # zenity, whose stdout must later be exactly the post-unlock text.
    wait_for_evidence '^sophia_live_session_lock schema=1 status=locked epoch=1$' lock_timeout
    "$ROOT_DIR/tools/qemu_qmp_type.py" "$QMP_SOCKET" wrongpass
    echo "sophia_qemu_lock_input schema=1 status=sent source=qmp secret=wrong" | tee -a "$EVIDENCE_FILE"
    wait_for_evidence '^sophia_live_session_lock schema=1 status=failed epoch=1 attempt=1 ' failed_verdict_timeout
    "$ROOT_DIR/tools/qemu_qmp_type.py" "$QMP_SOCKET" sophialock
    echo "sophia_qemu_lock_input schema=1 status=sent source=qmp secret=right" | tee -a "$EVIDENCE_FILE"
    wait_for_evidence '^sophia_live_session_lock schema=1 status=unlocked epoch=1$' unlock_timeout
fi

# Whether a line matching `pattern` follows the first line matching `anchor`.
# shellcheck source=tools/qemu_evidence_barrier.sh
. "$ROOT_DIR/tools/qemu_evidence_barrier.sh"

wait_for_after() {
    local anchor="$1" pattern="$2" reason="$3"
    for _ in $(seq 1 600); do
        if awk -v a="$anchor" -v p="$pattern" \
            'found { next } $0 ~ a { seen = 1; next } seen && $0 ~ p { found = 1 } END { exit !found }' \
            "$EVIDENCE_FILE"; then
            return 0
        fi
        kill -0 "$QEMU_PID" 2>/dev/null || break
        sleep 0.05
    done
    echo "sophia_qemu_gtk schema=1 status=failed reason=$reason scenario=$SCENARIO" | tee -a "$EVIDENCE_FILE"
    exit 1
}

if [[ "$SCENARIO" == session-lock-provider ]]; then
    # As session-lock, with a stand-in lock provider and only the right
    # password, so PAM's failure delay is not in the run. The provider's own
    # state is established after the lock and before any key is typed: a
    # stalled provider has stopped, a flooding one is submitting.
    locked='^sophia_live_session_lock schema=1 status=locked epoch=1$'
    wait_for_evidence "$locked" lock_timeout
    case "$LOCK_PROVIDER_MODE" in
        stall) provider_state='^sophia_qemu_lock_provider schema=1 mode=stall state=stalled ' ;;
        flood) provider_state='^sophia_qemu_lock_provider schema=1 mode=flood state=serving .* submitted=[1-9]' ;;
        baseline) provider_state='^sophia_qemu_lock_provider schema=1 mode=baseline state=serving ' ;;
    esac
    wait_for_after "$locked" "$provider_state" provider_state_timeout
    "$ROOT_DIR/tools/qemu_qmp_type.py" "$QMP_SOCKET" qzv
    echo "sophia_qemu_lock_input schema=1 status=sent source=qmp secret=right" | tee -a "$EVIDENCE_FILE"
    wait_for_evidence '^sophia_live_session_lock schema=1 status=unlocked epoch=1$' unlock_timeout
fi

if [[ "$SCENARIO" == gtk-* || "$SCENARIO" == session-lock || "$SCENARIO" == session-lock-provider ]]; then
    input_ready=false
    for _ in $(seq 1 600); do
        if grep -q '^sophia_live_session_input schema=1 status=ready source=physical text=sophia$' "$EVIDENCE_FILE"; then
            input_ready=true
            break
        fi
        kill -0 "$QEMU_PID" 2>/dev/null || break
        sleep 0.05
    done
    if [[ "$input_ready" != true ]]; then
        echo "sophia_qemu_gtk schema=1 status=failed reason=input_readiness_timeout scenario=$SCENARIO" | tee -a "$EVIDENCE_FILE"
        exit 1
    fi

    "$ROOT_DIR/tools/qemu_qmp_pointer.py" "$QMP_SOCKET" 1 1 1
    echo "sophia_qemu_gtk_pointer schema=1 status=sent phase=entry_focus source=qmp clicks=1" | tee -a "$EVIDENCE_FILE"
    "$ROOT_DIR/tools/qemu_qmp_type.py" "$QMP_SOCKET" sophia
    echo "sophia_qemu_gtk_input schema=1 status=sent source=qmp text=sophia events=14" | tee -a "$EVIDENCE_FILE"

    pointer_ready=false
    for _ in $(seq 1 200); do
        if grep -q '^sophia_live_session_pointer schema=1 status=ready source=physical action=select$' "$EVIDENCE_FILE"; then
            pointer_ready=true
            break
        fi
        kill -0 "$QEMU_PID" 2>/dev/null || break
        sleep 0.05
    done
    if [[ "$pointer_ready" != true ]]; then
        echo "sophia_qemu_gtk schema=1 status=failed reason=pointer_readiness_timeout scenario=$SCENARIO" | tee -a "$EVIDENCE_FILE"
        exit 1
    fi

    if [[ "$SCENARIO" == session-lock-provider ]]; then
        # Session decides whether the pointer proof still holds Return back
        # once per input batch, before the batch is routed, so a Return in
        # the select click's batch is suppressed for good. Send it only
        # after the click has been routed: a button_routed record written
        # after this pre-click line count, never an earlier one.
        select_anchor="$(wc -l < "$EVIDENCE_FILE")"
    fi
    "$ROOT_DIR/tools/qemu_qmp_pointer.py" "$QMP_SOCKET" 0 0 1
    echo "sophia_qemu_gtk_pointer schema=1 status=sent phase=focused_select source=qmp clicks=1" | tee -a "$EVIDENCE_FILE"
    if [[ "$SCENARIO" == session-lock-provider ]]; then
        wait_for_after_line "$select_anchor" \
            '^sophia_live_session_pointer schema=2 status=button_routed count=[1-9][0-9]*$' \
            select_route_timeout
    fi
    "$ROOT_DIR/tools/qemu_qmp_type.py" "$QMP_SOCKET"
    echo "sophia_qemu_gtk_input schema=1 status=sent source=qmp action=submit events=2" | tee -a "$EVIDENCE_FILE"

    set +e
    wait "$QEMU_PID"
    qemu_status=$?
    QEMU_PID=""
    wait "$LOGGER_PID"
    logger_status=$?
    LOGGER_PID=""
    set -e
    cleanup

    if [[ "$qemu_status" -ne 0 || "$logger_status" -ne 0 ]]; then
        echo "sophia_qemu_gtk schema=1 status=failed reason=guest_exit scenario=$SCENARIO qemu_exit=$qemu_status logger_exit=$logger_status" | tee -a "$EVIDENCE_FILE"
        exit 1
    fi
    # Every GTK scenario proves a committed resize except the provider one,
    # whose public WM owns geometry. Session reports an unproven resize as
    # "disabled" whether or not one was asked for, so the provider scenario
    # also requires that Session never logged a resize request: it logs one
    # for every injected resize.
    surface_resize=committed
    if [[ "$SCENARIO" == session-lock-provider ]]; then
        surface_resize=disabled
    fi
    # Only a completed guest can show it; any other ending is reported below
    # as semantic evidence rather than as a resize.
    if [[ "$SCENARIO" == session-lock-provider ]] \
        && grep -q "^sophia_qemu_guest schema=1 status=complete scenario=$SCENARIO$" "$EVIDENCE_FILE"; then
        if ! grep -Eq '^sophia_live_session schema=[0-9]+ status=bounded_complete .* surface_resize=disabled ' "$EVIDENCE_FILE" \
            || grep -q '^sophia_live_resize ' "$EVIDENCE_FILE"; then
            echo "sophia_qemu_gtk schema=1 status=failed reason=resize_requested scenario=$SCENARIO" | tee -a "$EVIDENCE_FILE"
            exit 1
        fi
    fi
    if ! grep -q "^sophia_qemu_guest schema=1 status=complete scenario=$SCENARIO$" "$EVIDENCE_FILE" \
        || ! grep -q "^sophia_x_application_session schema=1 status=passed class=gtk3_software client=zenity .*protocol_errors=0 first_error=none physical_text=true pointer_button=true surface_resize=$surface_resize buffer_path=cpu_shm native_presentation=enabled cleanup=clean\$" "$EVIDENCE_FILE" \
        || { [[ "$SCENARIO" == session-lock ]] \
            && ! "$ROOT_DIR/tools/verify_qemu_session_lock_evidence.sh" "$EVIDENCE_FILE"; } \
        || { [[ "$SCENARIO" == session-lock-provider ]] \
            && ! "$ROOT_DIR/tools/verify_qemu_session_lock_provider.py" "$EVIDENCE_FILE" "$LOCK_PROVIDER_MODE"; }; then
        echo "sophia_qemu_gtk schema=1 status=failed reason=semantic_evidence scenario=$SCENARIO" | tee -a "$EVIDENCE_FILE"
        exit 1
    fi
    echo "sophia_qemu_gtk schema=1 status=complete scenario=$SCENARIO qemu_exit=0" | tee -a "$EVIDENCE_FILE"
    exit 0
fi

if [[ "$SCENARIO" == output-unplug ]]; then
    display_bus=(busctl "--address=unix:path=$DISPLAY_BUS_SOCKET")
    # head_size CONSOLE WIDTH HEIGHT: what QEMU's display reports for one head;
    # zero takes the head away.
    head_size() {
        "${display_bus[@]}" call org.qemu "/org/qemu/Display1/Console_$1" \
            org.qemu.Display1.Console SetUIInfo qqiiuu 0 0 0 0 "$2" "$3" > /dev/null
    }
    unplug_failed() {
        echo "sophia_qemu_unplug schema=1 status=failed reason=$1" | tee -a "$EVIDENCE_FILE"
        exit 1
    }
    display_ready=false
    for _ in $(seq 1 200); do
        if "${display_bus[@]}" status org.qemu > /dev/null 2>&1; then
            display_ready=true
            break
        fi
        kill -0 "$QEMU_PID" 2>/dev/null || break
        sleep 0.05
    done
    [[ "$display_ready" == true ]] || unplug_failed display_bus_timeout
    head_size 0 1280 800 || unplug_failed head_enable
    head_size 1 1280 800 || unplug_failed head_enable
    "$ROOT_DIR/tools/qemu_qmp_cont.py" "$QMP_SOCKET" || unplug_failed qmp_cont

    if [[ "$UNPLUG_MODE" != input-return ]]; then
        # The guest records its uevents from the session's readiness on.
        monitoring=false
        for _ in $(seq 1 1200); do
            if grep -q '^sophia_qemu_unplug schema=1 status=monitoring$' "$EVIDENCE_FILE"; then
                monitoring=true
                break
            fi
            kill -0 "$QEMU_PID" 2>/dev/null || break
            sleep 0.05
        done
        [[ "$monitoring" == true ]] || unplug_failed monitoring_timeout
        if [[ "${SOPHIA_QEMU_UNPLUG_CLIENT:-none}" == dri3 ]]; then
            # The static client's barrier: its DMA-BUF Present was captured,
            # promoted and retired (a mixed retirement), and the client now
            # holds without presenting. Only then is a head taken away.
            barrier=false
            for _ in $(seq 1 600); do
                if grep -q '^sophia_live_session_present schema=2 status=retired ' "$EVIDENCE_FILE" \
                    && grep -q '^dri3_layout stage=holding ' "$EVIDENCE_FILE"; then
                    barrier=true
                    break
                fi
                kill -0 "$QEMU_PID" 2>/dev/null || break
                sleep 0.05
            done
            [[ "$barrier" == true ]] || unplug_failed static_barrier_timeout
            echo "sophia_qemu_unplug schema=1 status=static_barrier present=retired client=holding" | tee -a "$EVIDENCE_FILE"
        fi
        # One head modes take away SOPHIA_QEMU_UNPLUG_CONSOLE (default 1, the
        # second head); 0 takes away the first, where new windows open.
        consoles=("${SOPHIA_QEMU_UNPLUG_CONSOLE:-1}")
        [[ "$UNPLUG_MODE" == all-return ]] && consoles=(0 1)
        for console in "${consoles[@]}"; do
            head_size "$console" 0 0 || unplug_failed head_disable
            echo "sophia_qemu_unplug schema=1 status=sent action=off target=Console_$console" | tee -a "$EVIDENCE_FILE"
        done
        sleep 5
        if [[ "$UNPLUG_MODE" != one ]]; then
            for console in "${consoles[@]}"; do
                head_size "$console" 1280 800 || unplug_failed head_enable
                echo "sophia_qemu_unplug schema=1 status=sent action=on target=Console_$console" | tee -a "$EVIDENCE_FILE"
            done
        fi
    fi

    # The session bounds its own runtime; this bounds one that never ends.
    deadline=$((SECONDS + 150))
    while kill -0 "$QEMU_PID" 2>/dev/null && (( SECONDS < deadline )); do sleep 0.2; done
    kill -0 "$QEMU_PID" 2>/dev/null && unplug_failed host_timeout
    set +e
    wait "$QEMU_PID"
    qemu_status=$?
    QEMU_PID=""
    wait "$LOGGER_PID"
    logger_status=$?
    LOGGER_PID=""
    set -e
    cleanup
    if [[ "$qemu_status" -ne 0 || "$logger_status" -ne 0 ]]; then
        unplug_failed "guest_exit qemu_exit=$qemu_status logger_exit=$logger_status"
    fi
    echo "sophia_qemu_unplug schema=1 status=guest_exited qemu_exit=0" | tee -a "$EVIDENCE_FILE"
    cd "$ROOT_DIR"
    exec cargo xtask conformance verify output-unplug "$UNPLUG_MODE" "$EVIDENCE_FILE"
fi

if [[ "$SCENARIO" == xtest-selection ]]; then
    # Nothing is sent from the host: the drag and the paste are XTEST inside
    # the guest, and the session bounds its own runtime. The host waits for
    # the guest to power off and judges the evidence it left.
    set +e
    wait "$QEMU_PID"
    qemu_status=$?
    QEMU_PID=""
    wait "$LOGGER_PID"
    logger_status=$?
    LOGGER_PID=""
    set -e
    cleanup

    if [[ "$qemu_status" -ne 0 || "$logger_status" -ne 0 ]]; then
        echo "sophia_qemu_xtest_selection schema=1 status=failed reason=guest_exit qemu_exit=$qemu_status logger_exit=$logger_status" | tee -a "$EVIDENCE_FILE"
        exit 1
    fi
    echo "sophia_qemu_xtest_selection schema=1 status=complete qemu_exit=0" | tee -a "$EVIDENCE_FILE"
    exec "$ROOT_DIR/tools/verify_qemu_xtest_selection_evidence.sh" "$EVIDENCE_FILE"
fi

input_ready=false
for _ in $(seq 1 600); do
    if grep -q '^sophia_live_session_input schema=1 status=ready source=physical text=sophia$' "$EVIDENCE_FILE"; then
        input_ready=true
        break
    fi
    kill -0 "$QEMU_PID" 2>/dev/null || break
    sleep 0.05
done
if [[ "$input_ready" != true ]]; then
    echo "sophia_qemu_session schema=3 status=failed reason=input_readiness_timeout" | tee -a "$EVIDENCE_FILE"
    exit 1
fi

if ! "$ROOT_DIR/tools/qemu_qmp_type.py" "$QMP_SOCKET" sophia; then
    echo "sophia_qemu_session schema=3 status=failed reason=qmp_input_send" | tee -a "$EVIDENCE_FILE"
    exit 1
fi
echo "sophia_qemu_input schema=1 status=sent source=qmp device=virtio-keyboard text=sophia events=14" | tee -a "$EVIDENCE_FILE"

pointer_ready=false
for _ in $(seq 1 100); do
    if grep -q '^sophia_live_session_pointer schema=1 status=ready source=physical action=select$' "$EVIDENCE_FILE"; then
        pointer_ready=true
        break
    fi
    kill -0 "$QEMU_PID" 2>/dev/null || break
    sleep 0.05
done
if [[ "$pointer_ready" != true ]]; then
    echo "sophia_qemu_session schema=3 status=failed reason=pointer_readiness_timeout" | tee -a "$EVIDENCE_FILE"
    exit 1
fi

if ! "$ROOT_DIR/tools/qemu_qmp_pointer.py" "$QMP_SOCKET"; then
    echo "sophia_qemu_session schema=3 status=failed reason=qmp_pointer_send" | tee -a "$EVIDENCE_FILE"
    exit 1
fi
echo "sophia_qemu_pointer schema=1 status=sent source=qmp device=virtio-mouse action=select commands=5" | tee -a "$EVIDENCE_FILE"

set +e
wait "$QEMU_PID"
qemu_status=$?
QEMU_PID=""
wait "$LOGGER_PID"
logger_status=$?
LOGGER_PID=""
set -e
cleanup

if [[ "$qemu_status" -ne 0 || "$logger_status" -ne 0 ]]; then
    echo "sophia_qemu_session schema=3 status=failed reason=guest_exit qemu_exit=$qemu_status logger_exit=$logger_status" | tee -a "$EVIDENCE_FILE"
    exit 1
fi

echo "sophia_qemu_session schema=3 status=complete qemu_exit=0" | tee -a "$EVIDENCE_FILE"
if [[ "$TWO_XTERM" == 1 ]]; then
    SOPHIA_QEMU_REQUIRE_TWO_XTERM=1 \
        SOPHIA_QEMU_EXPECT_RENDERER_WORKERS="$EXPECT_RENDERER_WORKERS" \
        "$ROOT_DIR/tools/verify_qemu_session_evidence.sh" "$EVIDENCE_FILE"
else
    SOPHIA_QEMU_EXPECT_RENDERER_WORKERS="$EXPECT_RENDERER_WORKERS" \
        "$ROOT_DIR/tools/verify_qemu_session_evidence.sh" "$EVIDENCE_FILE"
fi

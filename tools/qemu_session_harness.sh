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

case "$SCENARIO" in
    session|emergency-recovery|gtk-classic|gtk-confined|xtest-selection) ;;
    *)
        echo "SOPHIA_QEMU_SCENARIO must be session, emergency-recovery, gtk-classic, gtk-confined, or xtest-selection" >&2
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
    emergency-recovery) DEFAULT_EVIDENCE_FILE=/tmp/sophia-qemu-emergency-recovery.log ;;
    gtk-*|xtest-selection) DEFAULT_EVIDENCE_FILE="/tmp/sophia-qemu-$SCENARIO.log" ;;
    *) DEFAULT_EVIDENCE_FILE=/tmp/sophia-qemu-session.log ;;
esac

EVIDENCE_FILE="${SOPHIA_QEMU_EVIDENCE:-$DEFAULT_EVIDENCE_FILE}"
QEMU_BIN="${SOPHIA_QEMU_BIN:-qemu-system-x86_64}"
MEMORY_MIB="${SOPHIA_QEMU_MEMORY_MIB:-2048}"
VIRTUAL_CPUS="${SOPHIA_QEMU_CPUS:-2}"
VNC_SOCKET="${SOPHIA_QEMU_VNC_SOCKET:-$OUT_DIR/display.sock}"
QMP_SOCKET="${SOPHIA_QEMU_QMP_SOCKET:-$OUT_DIR/qmp.sock}"
SERIAL_FIFO="${SOPHIA_QEMU_SERIAL_FIFO:-$OUT_DIR/serial.fifo}"
QEMU_PID=""
LOGGER_PID=""

cleanup() {
    if [[ -n "$QEMU_PID" ]] && kill -0 "$QEMU_PID" 2>/dev/null; then
        kill "$QEMU_PID" 2>/dev/null || true
        wait "$QEMU_PID" 2>/dev/null || true
    fi
    if [[ -n "$LOGGER_PID" ]] && kill -0 "$LOGGER_PID" 2>/dev/null; then
        kill "$LOGGER_PID" 2>/dev/null || true
        wait "$LOGGER_PID" 2>/dev/null || true
    fi
    rm -f "$VNC_SOCKET" "$QMP_SOCKET" "$SERIAL_FIFO"
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
if [[ "$SINGLE_CARD" == 1 ]]; then
    forced_connector=" video=Virtual-2:1280x800@60e"
fi

mkdir -p "$(dirname "$EVIDENCE_FILE")"
: > "$EVIDENCE_FILE"
rm -f "$VNC_SOCKET" "$QMP_SOCKET" "$SERIAL_FIFO"
mkfifo "$SERIAL_FIFO"

case "$SCENARIO" in
    emergency-recovery)
        echo "sophia_qemu_recovery schema=1 status=starting isolation=headless control=qmp-unix host_drm=none host_vt=none keyboard=virtio chord=ctrl-alt-backspace" | tee -a "$EVIDENCE_FILE"
        ;;
    gtk-*)
        echo "sophia_qemu_gtk schema=1 status=starting isolation=headless control=qmp-unix host_drm=none host_vt=none keyboard=virtio mouse=virtio scenario=$SCENARIO" | tee -a "$EVIDENCE_FILE"
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

"$QEMU_BIN" \
    -machine q35,accel=kvm:tcg \
    -smp "$VIRTUAL_CPUS" \
    -m "$MEMORY_MIB" \
    -nodefaults \
    -no-reboot \
    "${display_args[@]}" \
    -monitor none \
    -qmp "unix:$QMP_SOCKET,server=on,wait=off" \
    -serial stdio \
    "${gpu_args[@]}" \
    -device virtio-keyboard-pci \
    -device virtio-mouse-pci \
    -kernel "$KERNEL_IMAGE" \
    -initrd "$INITRAMFS" \
    -append "console=ttyS0 quiet loglevel=3 rdinit=/sbin/sophia-qemu-init rd.driver.pre=virtio_pci rd.driver.pre=virtio_gpu rd.driver.pre=virtio_input panic=-1 sophia.scenario=$SCENARIO sophia.two_xterm=$TWO_XTERM sophia.shared_renderer_worker=$SHARED_RENDERER_WORKER sophia.direct_scanout=$DIRECT_SCANOUT$forced_connector${XTEST_ROW:+ sophia.xtest_row=$XTEST_ROW}" \
    > "$SERIAL_FIFO" 2>&1 &
QEMU_PID=$!

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

if [[ "$SCENARIO" == gtk-* ]]; then
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

    "$ROOT_DIR/tools/qemu_qmp_pointer.py" "$QMP_SOCKET" 0 0 1
    echo "sophia_qemu_gtk_pointer schema=1 status=sent phase=focused_select source=qmp clicks=1" | tee -a "$EVIDENCE_FILE"
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
    if ! grep -q "^sophia_qemu_guest schema=1 status=complete scenario=$SCENARIO$" "$EVIDENCE_FILE" \
        || ! grep -q '^sophia_x_application_session schema=1 status=passed class=gtk3_software client=zenity .*protocol_errors=0 first_error=none physical_text=true pointer_button=true surface_resize=committed buffer_path=cpu_shm native_presentation=enabled cleanup=clean$' "$EVIDENCE_FILE"; then
        echo "sophia_qemu_gtk schema=1 status=failed reason=semantic_evidence scenario=$SCENARIO" | tee -a "$EVIDENCE_FILE"
        exit 1
    fi
    echo "sophia_qemu_gtk schema=1 status=complete scenario=$SCENARIO qemu_exit=0" | tee -a "$EVIDENCE_FILE"
    exit 0
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

#!/bin/sh
set -eu

export HOME=/root
export PATH=/usr/sbin:/usr/bin:/sbin:/bin
export LC_ALL=C
export XDG_RUNTIME_DIR=/tmp/sophia-runtime
export LIBGL_DRIVERS_PATH=/usr/lib/dri
# The minimal guest deliberately has neither logind nor a seatd daemon.
# Select libseat's direct no-op VT/device backend explicitly; production sessions
# retain normal libseat backend discovery.
export LIBSEAT_BACKEND=noop

mkdir -p /proc /sys /dev /run /run/udev /tmp /tmp/.X11-unix "$XDG_RUNTIME_DIR"
mount -t proc proc /proc 2>/dev/null || true
mount -t sysfs sysfs /sys 2>/dev/null || true
mount -t devtmpfs devtmpfs /dev 2>/dev/null || true
mkdir -p /dev/pts
mount -t devpts devpts /dev/pts
mount -t tmpfs tmpfs /run 2>/dev/null || true
chmod 700 "$XDG_RUNTIME_DIR"

scenario="session"
two_xterm=false
cmdline=""
IFS= read -r cmdline < /proc/cmdline || true
case " $cmdline " in
    *" sophia.scenario=emergency-recovery "*) scenario="emergency-recovery" ;;
    *" sophia.scenario=gtk-classic "*) scenario="gtk-classic" ;;
    *" sophia.scenario=gtk-confined "*) scenario="gtk-confined" ;;
    *" sophia.scenario=xtest-selection "*) scenario="xtest-selection" ;;
esac
xtest_row=""
case " $cmdline " in
    *" sophia.xtest_row="*)
        xtest_row="${cmdline##* sophia.xtest_row=}"
        xtest_row="${xtest_row%% *}"
        ;;
esac
case " $cmdline " in
    *" sophia.two_xterm=1 "*) two_xterm=true ;;
esac
case " $cmdline " in
    *" sophia.shared_renderer_worker=1 "*)
        export SOPHIA_ENABLE_SHARED_RENDERER_WORKER=1
        ;;
esac
case " $cmdline " in
    *" sophia.direct_scanout=1 "*)
        export SOPHIA_ENABLE_DIRECT_SCANOUT=1
        ;;
esac

if [ "$scenario" = "emergency-recovery" ]; then
    echo "sophia_qemu_guest schema=1 status=booting gpu=virtio-gpu scenario=emergency-recovery"
elif [ "$scenario" = "gtk-classic" ] || [ "$scenario" = "gtk-confined" ] \
    || [ "$scenario" = "xtest-selection" ]; then
    echo "sophia_qemu_guest schema=1 status=booting gpu=virtio-gpu scenario=$scenario"
else
    echo "sophia_qemu_guest schema=1 status=booting gpu=virtio-gpu ticks=300"
fi

udevd --daemon
udevadm control --log-priority=err

modprobe virtio_pci
modprobe virtio_gpu
modprobe virtio_input
modprobe evdev
udevadm trigger --action=add
udevadm settle --timeout=5

attempt=0
while [ ! -e /dev/dri/card0 ] && [ "$attempt" -lt 100 ]; do
    sleep 0.05
    attempt=$((attempt + 1))
done

if [ ! -e /dev/dri/card0 ]; then
    echo "sophia_qemu_guest schema=1 status=failed reason=virtio_gpu_drm_missing"
    poweroff -f
fi

connector_count=0
connected_count=0
for connector in /sys/class/drm/card[0-9]-*; do
    if [ ! -f "$connector/status" ]; then
        continue
    fi
    connector_count=$((connector_count + 1))
    status=""
    IFS= read -r status < "$connector/status" || true
    if [ "$status" = "connected" ]; then
        connected_count=$((connected_count + 1))
    fi
done
echo "sophia_qemu_topology schema=1 status=observed requested_heads=2 connectors=$connector_count connected=$connected_count"

input_devices=""
for device in /dev/input/event*; do
    if [ -e "$device" ]; then
        if [ -z "$input_devices" ]; then
            input_devices="$device"
        else
            input_devices="$input_devices,$device"
        fi
    fi
done

guard_pid=""
guard_triggered_file="/tmp/sophia-input-guard.triggered"
if [ "$scenario" = "emergency-recovery" ]; then
    if [ -z "$input_devices" ]; then
        echo "sophia_qemu_guest_recovery schema=1 status=failed reason=input_devices_missing"
        sync
        poweroff -f
    fi
    guard_armed_file="/tmp/sophia-input-guard.armed"
    rm -f "$guard_armed_file" "$guard_triggered_file"
    /usr/bin/sophia session input-guard \
        "--input-devices=$input_devices" \
        "--armed-file=$guard_armed_file" \
        "--triggered-file=$guard_triggered_file" \
        "--owner-pid=$$" &
    guard_pid=$!
    guard_armed=false
    attempt=0
    while [ "$attempt" -lt 600 ]; do
        if [ -s "$guard_armed_file" ]; then
            guard_armed=true
            break
        fi
        if ! kill -0 "$guard_pid" 2>/dev/null; then
            break
        fi
        sleep 0.05
        attempt=$((attempt + 1))
    done
    if [ "$guard_armed" != true ]; then
        echo "sophia_qemu_guest_recovery schema=1 status=failed reason=input_guard_arm_timeout"
        sync
        poweroff -f
    fi
    set -- session run --display=:181 --native-scanout --max-runtime-ms=30000
    echo "sophia_qemu_guest_recovery schema=1 status=running chord=ctrl-alt-backspace"
elif [ "$scenario" = "gtk-classic" ] || [ "$scenario" = "gtk-confined" ]; then
    profile="classic"
    [ "$scenario" = "gtk-confined" ] && profile="confined"
    # Accessibility is outside this minimal image's GTK rendering/input proof.
    # Disable its bus lookup explicitly while retaining the real session bus.
    export GTK_A11Y=none
    expected_stdout="$(printf 'sophia\n.')"
    expected_stdout="${expected_stdout%.}"
    set -- session run --display=:181 --native-scanout --max-runtime-ms=30000 \
        --namespace-profile="$profile" --software-client-rendering \
        --client=zenity --client-arg=--entry --client-arg=--title \
        --client-arg='Sophia GTK proof' --client-arg=--text \
        --client-arg='Type sophia, then click OK' \
        --expect-client-stdout="$expected_stdout" --require-client-normal-exit \
        --expect-physical-text=sophia --expect-physical-pointer \
        --inject-surface-resize=640x360 --exit-after-input-proof
    echo "sophia_qemu_gtk schema=1 status=running profile=$profile"
elif [ "$scenario" = "xtest-selection" ]; then
    # The headless gate's session, on a scanned-out head: XTEST admitted,
    # the driver as the only client, its exact pass line required. Physical
    # input stays enabled, as on an installed desktop, but nothing is typed:
    # --admit-xtest cannot be combined with a physical proof, since a
    # synthetic source could satisfy it. sophia.xtest_row=N drags a blank
    # row instead of the marker and must fail; that is the red half.
    set -- session run --display=:181 --native-scanout --max-runtime-ms=120000 \
        --admit-xtest --client=/usr/bin/xtest_selection_driver \
        --expect-client-stdout='sophia_xtest_selection schema=1 status=pass owner_in_a=true matched=true pointer_in_a=true pointer_in_b=true' \
        --require-client-normal-exit
    if [ -n "$xtest_row" ]; then
        set -- "$@" "--client-arg=--row=$xtest_row"
    fi
    echo "sophia_qemu_xtest_selection schema=1 status=running row=${xtest_row:-0}"
else
    set -- session run --display=:181 --native-scanout --max-ticks=300 \
        --expect-physical-text=sophia --expect-physical-pointer
    if [ "$two_xterm" = true ]; then
        set -- "$@" --secondary-terminal
    fi
fi

if [ -n "$input_devices" ]; then
    set -- "$@" "--input-devices=$input_devices"
fi

set +e
# Give every application in the guest one session-scoped bus. Modern GTK
# acquires the bus before opening X, so a bus-less image can strand launchers
# without ever reaching the authority listener.
SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1 \
    /usr/bin/dbus-run-session -- /usr/bin/sophia "$@"
status=$?
set -e

if [ "$scenario" = "emergency-recovery" ]; then
    guard_done=false
    attempt=0
    while [ "$attempt" -lt 100 ]; do
        if ! kill -0 "$guard_pid" 2>/dev/null; then
            guard_done=true
            break
        fi
        sleep 0.05
        attempt=$((attempt + 1))
    done
    set +e
    if [ "$guard_done" = true ]; then
        wait "$guard_pid"
        guard_status=$?
    else
        kill -TERM "$guard_pid" 2>/dev/null || true
        wait "$guard_pid" 2>/dev/null || true
        guard_status=124
    fi
    set -e
    guard_pid=""
else
    guard_status=0
fi

if [ "$scenario" = "emergency-recovery" ]; then
    if [ "$status" -eq 0 ] && [ "$guard_status" -eq 0 ] \
        && [ -s "$guard_triggered_file" ]; then
        echo "sophia_qemu_guest_recovery schema=1 status=complete exit_status=0 guard_exit_status=0"
    else
        echo "sophia_qemu_guest_recovery schema=1 status=failed reason=recovery_exit exit_status=$status guard_exit_status=$guard_status"
    fi
elif [ "$scenario" = "gtk-classic" ] || [ "$scenario" = "gtk-confined" ]; then
    if [ "$status" -eq 0 ]; then
        echo "sophia_qemu_guest schema=1 status=complete scenario=$scenario"
    else
        echo "sophia_qemu_guest schema=1 status=failed reason=gtk_session_exit scenario=$scenario exit_status=$status"
    fi
elif [ "$scenario" = "xtest-selection" ]; then
    if [ "$status" -eq 0 ]; then
        echo "sophia_qemu_guest schema=1 status=complete scenario=$scenario"
    else
        echo "sophia_qemu_guest schema=1 status=failed reason=xtest_selection_exit scenario=$scenario exit_status=$status"
    fi
elif [ "$status" -eq 0 ]; then
    echo "sophia_qemu_guest schema=1 status=complete ticks=300"
else
    echo "sophia_qemu_guest schema=1 status=failed reason=session_exit exit_status=$status"
fi

sync
poweroff -f

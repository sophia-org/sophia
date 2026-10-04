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
    *" sophia.scenario=session-lock "*) scenario="session-lock" ;;
    *" sophia.scenario=cpu "*) scenario="cpu" ;;
    *" sophia.scenario=xtest-selection "*) scenario="xtest-selection" ;;
esac
cpu_mode=open
cpu_seconds=60
cpu_grace=10
cpu_rate=5
cpu_target=zero
cpu_clients=2
cpu_size=small
cpu_damage=absent
cpu_evidence=full
for arg in $cmdline; do
    case "$arg" in
        sophia.cpu_mode=*) cpu_mode="${arg#*=}" ;;
        sophia.cpu_seconds=*) cpu_seconds="${arg#*=}" ;;
        sophia.cpu_grace=*) cpu_grace="${arg#*=}" ;;
        sophia.cpu_rate=*) cpu_rate="${arg#*=}" ;;
        sophia.cpu_target=*) cpu_target="${arg#*=}" ;;
        sophia.cpu_clients=*) cpu_clients="${arg#*=}" ;;
        sophia.cpu_size=*) cpu_size="${arg#*=}" ;;
        sophia.cpu_damage=*) cpu_damage="${arg#*=}" ;;
        sophia.cpu_evidence=*) cpu_evidence="${arg#*=}" ;;
    esac
done
if [ "$scenario" = cpu ]; then
    case "$cpu_evidence" in full|aggregate) export SOPHIA_PRESENT_EVIDENCE="$cpu_evidence" ;;
        *) echo 'sophia_qemu_cpu status=failed reason=evidence_mode'; poweroff -f; exit 1 ;;
    esac
fi
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
    || [ "$scenario" = "session-lock" ] || [ "$scenario" = "xtest-selection" ]; then
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
if [ "$scenario" = "cpu" ] && ! modprobe virtio_console; then
    echo 'sophia_qemu_cpu schema=1 status=failed reason=export_device_module'
    poweroff -f
fi
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
elif [ "$scenario" = "gtk-classic" ] || [ "$scenario" = "gtk-confined" ] \
    || [ "$scenario" = "session-lock" ]; then
    profile="classic"
    [ "$scenario" = "gtk-confined" ] && profile="confined"
    # Accessibility is outside this minimal image's GTK rendering/input proof.
    # Disable its bus lookup explicitly while retaining the real session bus.
    export GTK_A11Y=none
    expected_stdout="$(printf 'sophia\n.')"
    expected_stdout="${expected_stdout%.}"
    # The lock adds two typed passwords and PAM's failure delay.
    runtime_ms=30000
    [ "$scenario" = "session-lock" ] && runtime_ms=90000
    set -- session run --display=:181 --native-scanout --max-runtime-ms="$runtime_ms" \
        --namespace-profile="$profile" --software-client-rendering \
        --client=zenity --client-arg=--entry --client-arg=--title \
        --client-arg='Sophia GTK proof' --client-arg=--text \
        --client-arg='Type sophia, then click OK' \
        --expect-client-stdout="$expected_stdout" --require-client-normal-exit \
        --expect-physical-text=sophia --expect-physical-pointer \
        --inject-surface-resize=640x360 --exit-after-input-proof
    if [ "$scenario" = "session-lock" ]; then
        # Real PAM on a test account: root's password is "sophialock". The
        # files are written here, as root, because the agent starts only on
        # a root-owned stack nobody else can write.
        umask 022
        mkdir -p /etc/pam.d
        cp /usr/share/sophia/pam.d/sophia-lock /etc/pam.d/sophia-lock
        echo 'root:x:0:0:root:/root:/bin/sh' > /etc/passwd
        echo 'root:x:0:' > /etc/group
        printf 'passwd: files\ngroup: files\nshadow: files\n' > /etc/nsswitch.conf
        echo 'root:$6$sophiaqemulock$0yAj5lhlkbYql3IR/RdeAxyKMuUneqNAuzv34feGFoNNv0K/XuBdCVIV3NTcHXc7/isi94OseMHhyTPDGjuJi0:20000:0:99999:7:::' > /etc/shadow
        chmod 600 /etc/shadow
        export LOGNAME=root USER=root
        set -- "$@" --factotum-agent=/usr/bin/sophia-factotum \
            --factotum-pam-helper=/usr/bin/sophia-factotum-pam --inject-session-lock \
            --physical-sequence-timeout-ms=60000
    fi
    echo "sophia_qemu_gtk schema=1 status=running profile=$profile"
elif [ "$scenario" = "cpu" ]; then
    # Per-CPU nanosecond runtime avoids guest tick-sampling error. Guest only;
    # fixed for every revision and enabled before warm-up or any measurements.
    if ! { echo 1 > /proc/sys/kernel/sched_schedstats &&
        mkdir -p /run/present-cpu &&
        mount -t tmpfs -o size=256m,mode=0700 tmpfs /run/present-cpu; }; then
        echo 'sophia_qemu_cpu schema=1 status=failed reason=measurement_setup'
        poweroff -f
    fi
    cpu_export_device=""
    for port in /sys/class/virtio-ports/*; do
        [ -r "$port/name" ] || continue
        read -r port_name < "$port/name"
        if [ "$port_name" = sophia.cpu.evidence ]; then
            cpu_export_device="/dev/${port##*/}"
        fi
    done
    if [ ! -c "$cpu_export_device" ]; then
        echo 'sophia_qemu_cpu schema=1 status=failed reason=export_device_missing'
        poweroff -f
    fi
    runtime_ms=$(((cpu_seconds + cpu_grace + 30) * 1000))
    set -- session run --no-config --session-mode=normal --display=:181 --native-scanout \
        "--max-runtime-ms=$runtime_ms" --session-app=cpu=/usr/bin/present_cpu_workload \
        --session-start=cpu --exit-when-startup-exits \
        "--session-app-arg=cpu=--mode=$cpu_mode" "--session-app-arg=cpu=--seconds=$cpu_seconds" \
        "--session-app-arg=cpu=--grace=$cpu_grace" "--session-app-arg=cpu=--rate=$cpu_rate" \
        "--session-app-arg=cpu=--target=$cpu_target" "--session-app-arg=cpu=--clients=$cpu_clients" \
        "--session-app-arg=cpu=--size=$cpu_size" "--session-app-arg=cpu=--damage=$cpu_damage" \
        --session-app-arg=cpu=--sample-pid=parent --session-app-arg=cpu=--guest-process-accounting=true \
        --session-app-arg=cpu=--output=/run/present-cpu/workload.json
    echo "sophia_qemu_cpu schema=1 status=running mode=$cpu_mode"
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
if [ "$scenario" = "cpu" ]; then
    # Serial port I/O and its IRQ work would dominate the measured guest CPU.
    # Preserve every record in tmpfs, then export only after the client snapshots.
    cat /proc/interrupts > /run/present-cpu/interrupts-before
    SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1 \
        /usr/bin/dbus-run-session -- /usr/bin/sophia "$@" > /run/present-cpu/session.log 2>&1
    status=$?
    cat /proc/interrupts > /run/present-cpu/interrupts-after
else
    SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1 \
        /usr/bin/dbus-run-session -- /usr/bin/sophia "$@"
    status=$?
fi
set -e
if [ "$scenario" = "cpu" ]; then
    # Preserve the result even if bulk-log export fails. Failure must not exit
    # PID 1 under set -e and replace the actual reason with a kernel panic.
    export_status=0
    /bin/sh /usr/bin/sophia-cpu-export /run/present-cpu > "$cpu_export_device" || export_status=$?
    echo "sophia_qemu_cpu schema=1 status=exited session_exit=$status export_exit=$export_status"
    if [ "$export_status" -ne 0 ]; then status=1; fi
fi

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
elif [ "$scenario" = "gtk-classic" ] || [ "$scenario" = "gtk-confined" ] \
    || [ "$scenario" = "session-lock" ]; then
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

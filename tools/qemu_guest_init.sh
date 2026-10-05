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
    *" sophia.scenario=session-lock-provider "*) scenario="session-lock-provider" ;;
    *" sophia.scenario=cpu "*) scenario="cpu" ;;
    *" sophia.scenario=xtest-selection "*) scenario="xtest-selection" ;;
    *" sophia.scenario=output-unplug "*) scenario="output-unplug" ;;
esac
# The kernel refuses bubblewrap's pivot_root(2) while / is the initramfs rootfs.
# This scenario starts protection domains, so PID 1 first re-roots onto a bind
# of the same rootfs (tools/qemu_reroot.c: no copy, no cleanup) and runs again
# from the top, remounting its filesystems there. The bind is not recursive,
# so they are unmounted first.
if [ "$scenario" = "session-lock-provider" ]; then
    if [ "${1:-}" != "--rerooted" ]; then
        if ! umount /run /dev/pts /dev /sys /proc; then
            echo "sophia_qemu_guest schema=1 status=failed reason=reroot_umount"
            poweroff -f
        fi
        exec /usr/bin/sophia-qemu-reroot /newroot /sbin/sophia-qemu-init --rerooted
    fi
    # Our root must now be a mount with a parent, not the namespace's root.
    root_count=0
    root_id=""
    root_parent=""
    root_source=""
    mount_ids=" "
    while read -r id parent _ mount_root mount_point _; do
        mount_ids="$mount_ids$id "
        if [ "$mount_point" = / ]; then
            root_count=$((root_count + 1))
            root_id="$id"
            root_parent="$parent"
            root_source="$mount_root"
        fi
    done < /proc/self/mountinfo
    case "$mount_ids" in
        *" $root_parent "*) root_parent_visible=true ;;
        *) root_parent_visible=false ;;
    esac
    if [ "$$" != 1 ] || [ "$root_count" != 1 ] || [ "$root_id" = "$root_parent" ] \
        || [ "$root_parent_visible" != false ] || [ "$root_source" != / ]; then
        echo "sophia_qemu_guest schema=1 status=failed reason=reroot pid=$$ root_mounts=$root_count root_mount=$root_id parent=$root_parent"
        cat /proc/self/mountinfo
        poweroff -f
    fi
    if ! bwrap --ro-bind / / --dev /dev --proc /proc /bin/sh -c 'exit 0'; then
        echo "sophia_qemu_guest schema=1 status=failed reason=bwrap_smoke"
        poweroff -f
    fi
    echo "sophia_qemu_guest schema=1 status=rerooted pid=1 root_mount=$root_id parent=$root_parent bwrap_smoke=pass"
fi
cpu_mode=open
cpu_seconds=60
cpu_grace=10
cpu_rate=5
cpu_target=zero
cpu_clients=2
cpu_size=small
cpu_damage=absent
cpu_evidence=full
lock_provider_mode=""
unplug_mode=""
unplug_kmsg=false
stamp_args=""
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
        sophia.lock_provider_mode=*) lock_provider_mode="${arg#*=}" ;;
        sophia.unplug_mode=*) unplug_mode="${arg#*=}" ;;
        sophia.unplug_kmsg=1) unplug_kmsg=true ;;
        # Diagnostic: the stamper requests SysRq w on the first hard stall.
        sophia.sysrq_on_hard_stall=1) stamp_args="--sysrq-on-hard-stall" ;;
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
    || [ "$scenario" = "session-lock" ] || [ "$scenario" = "xtest-selection" ] \
    || [ "$scenario" = "session-lock-provider" ] || [ "$scenario" = "output-unplug" ]; then
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
    || [ "$scenario" = "session-lock" ] || [ "$scenario" = "session-lock-provider" ]; then
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
    [ "$scenario" = "session-lock-provider" ] && runtime_ms=90000
    # The session-authored resize proof bypasses a public-policy WM, which owns
    # geometry, so Session cannot settle it there: the provider scenario, the
    # only one with a public WM, runs without it. Every other scenario's argv
    # is unchanged.
    resize_proof=--inject-surface-resize=640x360
    if [ "$scenario" = "session-lock-provider" ]; then
        resize_proof=""
    fi
    set -- session run --display=:181 --native-scanout --max-runtime-ms="$runtime_ms" \
        --namespace-profile="$profile" --software-client-rendering \
        --client=zenity --client-arg=--entry --client-arg=--title \
        --client-arg='Sophia GTK proof' --client-arg=--text \
        --client-arg='Type sophia, then click OK' \
        --expect-client-stdout="$expected_stdout" --require-client-normal-exit \
        --expect-physical-text=sophia --expect-physical-pointer \
        ${resize_proof:+"$resize_proof"} --exit-after-input-proof
    if [ "$scenario" = "session-lock" ] || [ "$scenario" = "session-lock-provider" ]; then
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
    if [ "$scenario" = "session-lock-provider" ]; then
        # A generic stand-in lock provider (tools/qemu_lock_provider_standin.c)
        # in one of three modes; the session starts it in its protection domain.
        case "$lock_provider_mode" in
            baseline|flood|stall) ;;
            *)
                echo "sophia_qemu_guest schema=1 status=failed reason=lock_provider_mode scenario=$scenario"
                poweroff -f
                ;;
        esac
        mkdir -p -m 0700 /run/sophia-qemu-lock
        echo "$lock_provider_mode" > /run/sophia-qemu-lock/mode
        printf 'schema 1\nsession {\n    lock-provider {\n        executable "/usr/bin/sophia-qemu-lock-provider"\n        config "/run/sophia-qemu-lock/mode"\n    }\n}\n' \
            > /run/sophia-qemu-lock/desktop.kdl
        chmod 600 /run/sophia-qemu-lock/desktop.kdl /run/sophia-qemu-lock/mode
        # A three-letter synthetic password ("qzv"): typed at the harness's
        # 0.2 s per key, the whole entry fits well inside Session's 2 s
        # provider acknowledgement timeout, which a stalled provider's first
        # unacknowledged entry event starts.
        echo 'root:$6$sophiaqemuprov$D9.j/TYuwhc29grNkukfTUrAZOqFdKQee8kxtPS1vZINOghlo12imqt2aULJPSo0rI5XGlXPlCMANBarSXL5X0:20000:0:99999:7:::' > /etc/shadow
        chmod 600 /etc/shadow
        # Session starts a lock provider only beside a WM that publishes the
        # output snapshot: the generic test WM (tools/qemu_generic_wm.c).
        set -- "$@" --desktop-profile=/run/sophia-qemu-lock/desktop.kdl \
            --wm-process=/usr/bin/sophia-qemu-generic-wm \
            --wm-interface=sophia_wm_v1 --wm-transport=9p2000.L
        echo "sophia_qemu_lock_provider_guest schema=1 status=configured mode=$lock_provider_mode"
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
elif [ "$scenario" = "output-unplug" ]; then
    # t306: the session runs on scanned-out heads with udev-managed input, as
    # an installed desktop does, while the guest takes away one connector,
    # every connector or the keyboard and, in the return modes, gives it back
    # (unplug_drive). No client: the verdict is read from the session's own
    # topology records and its bounded completion.
    case "$unplug_mode" in
        one|one-return|all-return|input-return) ;;
        *)
            echo "sophia_qemu_guest schema=1 status=failed reason=unplug_mode scenario=$scenario"
            poweroff -f
            ;;
    esac
    input_devices=""
    # Records written through tracing then carry no colour in the evidence.
    export NO_COLOR=1
    set -- session run --display=:181 --native-scanout --max-runtime-ms=40000 --input-seat=seat0
    echo "sophia_qemu_unplug schema=1 status=running mode=$unplug_mode"
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

# The output-unplug scenario's guest side. The host takes heads away through
# QEMU's display (tools/qemu_session_harness.sh); the guest takes the keyboard
# away through its virtio driver's unbind and gives it back with bind. Either
# way the guest counts its own DRM and input uevents, so a run whose action
# never reached Sophia is named as such instead of passing or failing.
unplug_keyboard() {
    action="$1"
    keyboard=""
    for input in /sys/class/input/input[0-9]*; do
        name=""
        IFS= read -r name < "$input/name" || true
        if [ "$name" = "QEMU Virtio Keyboard" ]; then
            keyboard="$(cd "$input/device" && pwd -P)" || keyboard=""
        fi
    done
    if [ "$action" = on ]; then
        keyboard="$unplug_keyboard_device"
        control=/sys/bus/virtio/drivers/virtio_input/bind
    else
        unplug_keyboard_device="$keyboard"
        control=/sys/bus/virtio/drivers/virtio_input/unbind
    fi
    if [ -n "$keyboard" ] && echo "${keyboard##*/}" > "$control"; then
        echo "sophia_qemu_unplug schema=1 status=sent action=$action target=${keyboard##*/}"
    else
        echo "sophia_qemu_unplug schema=1 status=failed reason=keyboard_$action"
    fi
}

unplug_drive() {
    udevadm monitor --kernel --property --subsystem-match=drm --subsystem-match=input \
        > /run/sophia-qemu-unplug/uevents 2>/dev/null &
    monitor_pid=$!
    kmsg_pid=""
    if [ "$unplug_kmsg" = true ]; then
        # From here on only: the log before the action is the boot's.
        echo "sophia_qemu_unplug schema=1 status=kmsg_window" > /dev/kmsg
        cat /dev/kmsg > /run/sophia-qemu-unplug/kmsg &
        kmsg_pid=$!
    fi
    sleep 1
    echo "sophia_qemu_unplug schema=1 status=monitoring"
    if [ "$unplug_mode" = input-return ]; then
        unplug_keyboard off
        sleep 5
        unplug_keyboard on
        sleep 6
    else
        # The host's removal, wait and return, with room to settle.
        sleep 18
    fi
    kill "$monitor_pid" 2>/dev/null || true
    wait "$monitor_pid" 2>/dev/null || true
    hotplug=0
    removed=0
    added=0
    while IFS= read -r line; do
        case "$line" in
            HOTPLUG=1) hotplug=$((hotplug + 1)) ;;
            "KERNEL["*"] remove "*"(input)") removed=$((removed + 1)) ;;
            "KERNEL["*"] add "*"(input)") added=$((added + 1)) ;;
        esac
    done < /run/sophia-qemu-unplug/uevents
    echo "sophia_qemu_unplug schema=1 status=uevents drm_hotplug=$hotplug input_remove=$removed input_add=$added"
    if [ -n "$kmsg_pid" ]; then
        kill "$kmsg_pid" 2>/dev/null || true
        wait "$kmsg_pid" 2>/dev/null || true
        window=false
        copied=0
        while IFS= read -r line; do
            case "$line" in
                *"sophia_qemu_unplug schema=1 status=kmsg_window"*) window=true; continue ;;
            esac
            if [ "$window" = true ] && [ "$copied" -lt 40000 ]; then
                echo "sophia_qemu_kmsg ${line#*;}"
                copied=$((copied + 1))
            fi
        done < /run/sophia-qemu-unplug/kmsg
        echo "sophia_qemu_unplug schema=1 status=kmsg_copied lines=$copied"
    fi
}

# Copies the session's output to the console and starts the guest actions
# once the session has presented on every head.
unplug_watch() {
    driving=false
    while IFS= read -r line; do
        printf '%s\n' "$line"
        case "$line" in
            "sophia_live_session_startup schema=2 status=ready "*)
                if [ "$driving" = false ]; then
                    driving=true
                    unplug_drive &
                fi
                ;;
        esac
    done
    wait
}

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
elif [ "$scenario" = "session-lock-provider" ]; then
    # Every lock and provider line is stamped where the guest observed it
    # (tools/qemu_line_stamp.c); the session's exit status stays its own.
    # The kernel's own records reach the evidence through the stamper's
    # /dev/kmsg copy, not the console, which "quiet loglevel=3" keeps terse.
    mkfifo /run/sophia-qemu-lock/output
    # shellcheck disable=SC2086 # empty, or the one diagnostic flag
    /usr/bin/sophia-qemu-line-stamp $stamp_args < /run/sophia-qemu-lock/output &
    stamp_pid=$!
    SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1 \
        /usr/bin/dbus-run-session -- /usr/bin/sophia "$@" > /run/sophia-qemu-lock/output 2>&1
    status=$?
    wait "$stamp_pid"
elif [ "$scenario" = "output-unplug" ]; then
    mkdir -p -m 0700 /run/sophia-qemu-unplug
    mkfifo /run/sophia-qemu-unplug/output
    unplug_watch < /run/sophia-qemu-unplug/output &
    watch_pid=$!
    SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1 \
        /usr/bin/dbus-run-session -- /usr/bin/sophia "$@" > /run/sophia-qemu-unplug/output 2>&1
    status=$?
    wait "$watch_pid"
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
    || [ "$scenario" = "session-lock" ] || [ "$scenario" = "session-lock-provider" ]; then
    if [ "$status" -eq 0 ]; then
        echo "sophia_qemu_guest schema=1 status=complete scenario=$scenario"
    else
        echo "sophia_qemu_guest schema=1 status=failed reason=gtk_session_exit scenario=$scenario exit_status=$status"
    fi
elif [ "$scenario" = "output-unplug" ]; then
    if [ "$status" -eq 0 ]; then
        echo "sophia_qemu_guest schema=1 status=complete scenario=$scenario"
    else
        echo "sophia_qemu_guest schema=1 status=failed reason=unplug_session_exit scenario=$scenario exit_status=$status"
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

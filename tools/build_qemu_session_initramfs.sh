#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${SOPHIA_QEMU_OUT_DIR:-$ROOT_DIR/.qemu}"
KERNEL_VERSION="${SOPHIA_QEMU_KERNEL_VERSION:-$(uname -r)}"
KERNEL_IMAGE="${SOPHIA_QEMU_KERNEL:-/boot/vmlinuz-$KERNEL_VERSION}"
INITRAMFS="${SOPHIA_QEMU_INITRAMFS:-$OUT_DIR/sophia-$KERNEL_VERSION.img}"

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "missing required command: $1" >&2
        exit 1
    fi
}

require_command cargo
require_command dbus-daemon
require_command dbus-run-session
require_command dracut
require_command zenity
require_command xterm
require_command readlink
require_command lsinitrd

if [[ ! -r "$KERNEL_IMAGE" ]]; then
    echo "guest kernel is not readable: $KERNEL_IMAGE" >&2
    exit 1
fi
if [[ ! -d "/usr/lib/modules/$KERNEL_VERSION" ]]; then
    echo "guest kernel modules are missing: /usr/lib/modules/$KERNEL_VERSION" >&2
    exit 1
fi

mkdir -p "$OUT_DIR" "$OUT_DIR/dracut-tmp"

# Candidate binaries supplied by their owner instead of built here: a
# sha256sum manifest naming sophia, sophia-factotum and sophia-factotum-pam,
# by absolute path or relative to the manifest's directory. They are verified, copied, and the copies verified again;
# dracut runs with --nostrip, and the built image is checked to hold exactly the bytes the manifest pins.
PINNED_MANIFEST="${SOPHIA_QEMU_PINNED_MANIFEST:-}"
if [[ -n "$PINNED_MANIFEST" ]]; then
    pinned_dir="$OUT_DIR/pinned"
    rm -rf "$pinned_dir"
    mkdir -p "$pinned_dir"
    [[ "$(wc -l < "$PINNED_MANIFEST")" -eq 3 ]] \
        || { echo "pinned manifest must name exactly three binaries" >&2; exit 1; }
    manifest_dir="$(cd "$(dirname "$PINNED_MANIFEST")" && pwd)"
    (cd "$manifest_dir" && sha256sum --check --strict --quiet "$(basename "$PINNED_MANIFEST")") \
        || { echo "pinned binaries do not match their manifest" >&2; exit 1; }
    while read -r digest path; do
        [[ "$path" = /* ]] || path="$manifest_dir/$path"
        name="$(basename "$path")"
        case "$name" in
            sophia|sophia-factotum|sophia-factotum-pam) ;;
            *) echo "unexpected pinned binary: $path" >&2; exit 1 ;;
        esac
        install -m 0755 "$path" "$pinned_dir/$name"
        [[ "$(sha256sum "$pinned_dir/$name" | cut -d' ' -f1)" == "$digest" ]] \
            || { echo "pinned copy of $name changed" >&2; exit 1; }
    done < "$PINNED_MANIFEST"
fi

(
    cd "$ROOT_DIR"
    if [[ -z "$PINNED_MANIFEST" ]]; then
        cargo build --release --offline -p sophia-cli --features native-session
        # The session-lock scenario's authenticator: the agent and its PAM helper.
        cargo build --release --offline -p sophia-factotum -p sophia-factotum-pam
    fi
    # The xtest-selection scenario's client: two real xterms driven by XTEST.
    # A pinned image holds only pinned binaries and the scenario's own tools.
    if [[ -z "$PINNED_MANIFEST" ]]; then
        cargo build --release --offline -p sophia-session --all-features \
            --example xtest_selection_driver --example present_cpu_workload
    fi
)

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
[[ "$TARGET_DIR" = /* ]] || TARGET_DIR="$ROOT_DIR/$TARGET_DIR"
SOPHIA_BIN="$TARGET_DIR/release/sophia"
FACTOTUM_BIN="$TARGET_DIR/release/sophia-factotum"
FACTOTUM_PAM_BIN="$TARGET_DIR/release/sophia-factotum-pam"
if [[ -n "$PINNED_MANIFEST" ]]; then
    SOPHIA_BIN="$pinned_dir/sophia"
    FACTOTUM_BIN="$pinned_dir/sophia-factotum"
    FACTOTUM_PAM_BIN="$pinned_dir/sophia-factotum-pam"
fi

# The session-lock-provider scenario's guest tools, on the vendored C SDK.
GUEST_TOOLS="$OUT_DIR/guest-tools"
mkdir -p "$GUEST_TOOLS"
SDK_SOURCE="$ROOT_DIR/vendor/c-desktop-sdk/source/src"
cc -std=c11 -O2 -Wall -Wextra -Werror -I"$SDK_SOURCE" \
    "$ROOT_DIR/tools/qemu_lock_provider_standin.c" \
    "$SDK_SOURCE"/lock_files/*.c "$SDK_SOURCE"/nine_p/*.c \
    -o "$GUEST_TOOLS/sophia-qemu-lock-provider"
cc -std=c11 -O2 -Wall -Wextra -Werror "$ROOT_DIR/tools/qemu_line_stamp.c" \
    -o "$GUEST_TOOLS/sophia-qemu-line-stamp"
cc -std=c11 -O2 -Wall -Wextra -Werror -I"$SDK_SOURCE" \
    "$ROOT_DIR/tools/qemu_generic_wm.c" \
    "$SDK_SOURCE"/wm_files/*.c "$SDK_SOURCE"/wm_session/*.c "$SDK_SOURCE"/nine_p/*.c \
    -o "$GUEST_TOOLS/sophia-qemu-generic-wm"
cc -std=c11 -O2 -Wall -Wextra -Werror "$ROOT_DIR/tools/qemu_reroot.c" \
    -o "$GUEST_TOOLS/sophia-qemu-reroot"
(cd "$GUEST_TOOLS" && sha256sum sophia-qemu-lock-provider sophia-qemu-line-stamp \
    sophia-qemu-generic-wm sophia-qemu-reroot > SHA256SUMS)
# What the guest tools were built from, recorded beside their hashes.
{
    echo "sdk=$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$ROOT_DIR/vendor/c-desktop-sdk/source/compatibility.json" | head -1)"
    echo "sdk_upstream=$(sha256sum "$ROOT_DIR/vendor/c-desktop-sdk/upstream.commit" | cut -d' ' -f1) (upstream.commit)"
    echo "sources=$(cd "$ROOT_DIR" && sha256sum tools/qemu_lock_provider_standin.c tools/qemu_lock_provider_service.h tools/qemu_line_stamp.c tools/qemu_generic_wm.c tools/qemu_reroot.c | tr '\n' ' ')"
    echo "bwrap=/usr/bin/bwrap $(/usr/bin/bwrap --version) sha256=$(sha256sum /usr/bin/bwrap | cut -d' ' -f1)"
} > "$GUEST_TOOLS/FIXTURE.txt"
XTEST_SELECTION_DRIVER="$TARGET_DIR/release/examples/xtest_selection_driver"
# The driver asks xterm for DejaVu Sans Mono; a proportional fallback would
# move the text row it aims at.
DEJAVU_MONO="$(fc-match -f '%{file}' 'DejaVu Sans Mono' 2>/dev/null || true)"
if [[ -z "$DEJAVU_MONO" || "$(basename "$DEJAVU_MONO")" != DejaVuSansMono.ttf ]]; then
    echo "DejaVu Sans Mono is not installed; the xtest-selection guest scenario needs it" >&2
    exit 1
fi
runtime_files=(
    "$(command -v dbus-daemon)"
    "$(command -v dbus-run-session)"
    /usr/lib/libEGL.so.1
    /usr/lib/libEGL_mesa.so.0
    /usr/lib/libGLdispatch.so.0
    /usr/lib/libgbm.so.1
    /usr/lib/libGLESv2.so.2
    /usr/lib/libgallium-*.so
    /usr/lib/libdrm.so.2
    /usr/lib/libinput.so.10
    /usr/lib/libudev.so.1
    /usr/bin/zenity
    /usr/lib/libpam.so.0
    /usr/lib/security/pam_unix.so
    /usr/lib/security/pam_faildelay.so
)
extra_includes=(
    --include "$ROOT_DIR/tools/present_cpu/export_guest_log.sh" /usr/bin/sophia-cpu-export
    --include "$DEJAVU_MONO" /usr/share/fonts/TTF/DejaVuSansMono.ttf
    --include "$FACTOTUM_BIN" /usr/bin/sophia-factotum
    --include "$FACTOTUM_PAM_BIN" /usr/bin/sophia-factotum-pam
    --include "$GUEST_TOOLS/sophia-qemu-lock-provider" /usr/bin/sophia-qemu-lock-provider
    --include "$GUEST_TOOLS/sophia-qemu-line-stamp" /usr/bin/sophia-qemu-line-stamp
    --include "$GUEST_TOOLS/sophia-qemu-generic-wm" /usr/bin/sophia-qemu-generic-wm
    --include "$GUEST_TOOLS/sophia-qemu-reroot" /usr/bin/sophia-qemu-reroot
    --include "$ROOT_DIR/examples/pam.d/sophia-lock" /usr/share/sophia/pam.d/sophia-lock
)
required_guest_paths=(
    /usr/bin/bwrap
    /etc/ld.so.cache
    /usr/bin/dbus-daemon
    /usr/bin/dbus-run-session
    /usr/share/dbus-1/session.conf
    /usr/bin/xterm
    /usr/share/fonts/TTF/DejaVuSansMono.ttf
    /usr/bin/sophia-factotum
    /usr/bin/sophia-factotum-pam
    /usr/bin/sophia-qemu-lock-provider
    /usr/bin/sophia-qemu-line-stamp
    /usr/bin/sophia-qemu-generic-wm
    /usr/bin/sophia-qemu-reroot
    /usr/bin/umount
    /usr/share/sophia/pam.d/sophia-lock
    /usr/lib/security/pam_unix.so
)
if [[ -z "$PINNED_MANIFEST" ]]; then
    extra_includes+=(
        --include "$TARGET_DIR/release/examples/present_cpu_workload" /usr/bin/present_cpu_workload
        --include "$XTEST_SELECTION_DRIVER" /usr/bin/xtest_selection_driver
    )
    required_guest_paths+=(/usr/bin/present_cpu_workload /usr/bin/xtest_selection_driver)
fi
# Protection domains (the session-lock-provider scenario's provider) start
# through Bubblewrap.
runtime_files+=(/usr/bin/bwrap)
install_files=()
runtime_files+=("$(command -v xterm)")
for file in "${runtime_files[@]}"; do
    if [[ -e "$file" ]]; then
        install_files+=("$file")
    fi
done

XKB_DATA_DIR="$(readlink -f /usr/share/X11/xkb)"
if [[ ! -d "$XKB_DATA_DIR" ]]; then
    echo "xkeyboard-config data is missing: /usr/share/X11/xkb" >&2
    exit 1
fi
dracut --force --nostrip --no-hostonly --no-hostonly-cmdline --no-early-microcode \
    --kver "$KERNEL_VERSION" \
    --tmpdir "$OUT_DIR/dracut-tmp" \
    --force-drivers "virtio_pci virtio_gpu virtio_input virtio_console evdev" \
    --install "/bin/sh /usr/bin/cat /usr/bin/gzip /usr/bin/base64 /usr/bin/sha256sum /usr/bin/wc /usr/bin/du /usr/bin/chmod /usr/bin/mount /usr/bin/umount /usr/bin/modprobe /usr/bin/pidof /usr/bin/poweroff /usr/bin/sleep /usr/bin/sync /usr/bin/mkfifo ${install_files[*]}" \
    --include "$ROOT_DIR/tools/qemu_guest_init.sh" /sbin/sophia-qemu-init \
    --include "$SOPHIA_BIN" /usr/bin/sophia \
    "${extra_includes[@]}" \
    --include /usr/lib/dri /usr/lib/dri \
    --include /usr/lib/gbm /usr/lib/gbm \
    --include /etc/fonts /etc/fonts \
    --include /usr/share/fonts/cantarell /usr/share/fonts/cantarell \
    --include /usr/share/fonts/noto/NotoSans-Regular.ttf \
      /usr/share/fonts/noto/NotoSans-Regular.ttf \
    --include /var/lib/dbus/machine-id /var/lib/dbus/machine-id \
    --include /usr/share/dbus-1/session.conf /usr/share/dbus-1/session.conf \
    --include /usr/share/glvnd /usr/share/glvnd \
    --include /usr/share/libinput /usr/share/libinput \
    --include /usr/share/glib-2.0/schemas /usr/share/glib-2.0/schemas \
    --include /usr/share/icons/Adwaita /usr/share/icons/Adwaita \
    --include "$XKB_DATA_DIR" "$XKB_DATA_DIR" \
    "$INITRAMFS"

initramfs_listing="$(lsinitrd "$INITRAMFS")"
# The guest runs exactly these bytes: Session and its authenticator (pinned or
# built here), the scenario's guest tools, and the host's /usr/bin/bwrap for
# its protection domains. One decompression unpacks them for comparison.
{
    echo "$(sha256sum "$SOPHIA_BIN" | cut -d' ' -f1)  usr/bin/sophia"
    echo "$(sha256sum "$FACTOTUM_BIN" | cut -d' ' -f1)  usr/bin/sophia-factotum"
    echo "$(sha256sum "$FACTOTUM_PAM_BIN" | cut -d' ' -f1)  usr/bin/sophia-factotum-pam"
    while read -r digest name; do
        echo "$digest  usr/bin/$name"
    done < "$GUEST_TOOLS/SHA256SUMS"
    echo "$(sha256sum /usr/bin/bwrap | cut -d' ' -f1)  usr/bin/bwrap"
} > "$OUT_DIR/IMAGE-EXPECTED.SHA256SUMS"
"$ROOT_DIR/tools/qemu_image_identity.sh" "$INITRAMFS" "$OUT_DIR/IMAGE-EXPECTED.SHA256SUMS" \
    "$OUT_DIR/IMAGE-IDENTITY.SHA256SUMS" \
    || { echo "initramfs does not hold the expected bytes" >&2; exit 1; }
echo "guest_bwrap_sha256=$(sed -n 's|  usr/bin/bwrap$||p' "$OUT_DIR/IMAGE-IDENTITY.SHA256SUMS")" \
    >> "$GUEST_TOOLS/FIXTURE.txt"
for guest_path in "${required_guest_paths[@]}"; do
    if ! grep -Fq " ${guest_path#/}" <<<"$initramfs_listing"; then
        echo "initramfs is missing required path: $guest_path" >&2
        exit 1
    fi
done

echo "Sophia QEMU guest initramfs built"
echo "Kernel: $KERNEL_IMAGE"
echo "Initramfs: $INITRAMFS"

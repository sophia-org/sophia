#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cat > "$work/gpu.log" <<'EOF'
lom_gpu_admission schema=1 status=ready grant_epoch=1 render_node=/dev/dri/renderD128 device_major=226 device_minor=128 pci_bus_id=0000:01:00.0 backend=Vulkan device_type=DiscreteGpu adapter_name="fixture" driver="fixture" visible_dri_entries=renderD128
sophia_shell_gpu_content_hardware_proof schema=1 status=complete protected=true revision=6 capabilities=0x283 grant_epoch=1 render_node=/dev/dri/renderD128 device_major=226 device_minor=128 pci_bus_id=0000:01:00.0 width=256 height=24 bytes=24576 checksum=0123456789abcdef renderer_outcome=10 backing_bytes=0 native_presentation=false
EOF
"$ROOT_DIR/tools/verify_lom_gpu_content_hardware_proof.sh" "$work/gpu.log" >/dev/null
for mutation in extra_drm cpu zero_checksum native identity; do
    cp "$work/gpu.log" "$work/$mutation.log"
    case "$mutation" in
        extra_drm) sed -i 's/visible_dri_entries=renderD128/visible_dri_entries=card0,renderD128/' "$work/$mutation.log" ;;
        cpu) sed -i 's/device_type=DiscreteGpu/device_type=Cpu/' "$work/$mutation.log" ;;
        zero_checksum) sed -i 's/checksum=0123456789abcdef/checksum=0000000000000000/' "$work/$mutation.log" ;;
        native) sed -i 's/native_presentation=false/native_presentation=true/' "$work/$mutation.log" ;;
        identity) sed -i '/^lom_gpu_admission /s/device_minor=128/device_minor=129/' "$work/$mutation.log" ;;
    esac
    if "$ROOT_DIR/tools/verify_lom_gpu_content_hardware_proof.sh" "$work/$mutation.log" >/dev/null 2>&1; then
        echo "verifier accepted $mutation mutation" >&2
        exit 1
    fi
done

cat > "$work/native.log" <<'EOF'
sophia_live_shell_gpu schema=1 status=granted mode=direct peer_pid=42 grant_epoch=1 device_major=226 device_minor=128 pci_bus_id=0000:01:00.0
sophia_live_shell_content schema=1 status=outputs facts_generation=1 outputs=1
sophia_live_shell_content schema=1 status=presented output=1 candidate_generation=1 presentation_epoch=11 staging_bytes=0 resident_bytes=24576 retiring_bytes=0 backing_bytes=24576
sophia_live_shell_content schema=1 status=presented output=1 candidate_generation=2 presentation_epoch=12 staging_bytes=0 resident_bytes=49152 retiring_bytes=24576 backing_bytes=49152
EOF
"$ROOT_DIR/tools/verify_lom_panel_native_gate.sh" "$work/native.log" >/dev/null
awk '{ printf "%d\t%d\t%d\t%s\n", NR, 1000 + NR, 2000 + NR, $0 }' \
    "$work/native.log" > "$work/native-events.log"
"$ROOT_DIR/tools/verify_lom_panel_native_gate.sh" "$work/native-events.log" >/dev/null
cp "$work/native.log" "$work/native-full.log"
sed -i '/candidate_generation=2/d' "$work/native.log"
if "$ROOT_DIR/tools/verify_lom_panel_native_gate.sh" "$work/native.log" >/dev/null 2>&1; then
    echo "native verifier accepted one generation" >&2
    exit 1
fi

sed 's/outputs=1/outputs=2/' "$work/native-full.log" > "$work/native-missing-output.log"
if "$ROOT_DIR/tools/verify_lom_panel_native_gate.sh" "$work/native-missing-output.log" >/dev/null 2>&1; then
    echo "native verifier accepted a missing output" >&2
    exit 1
fi

cp "$work/native-events.log" "$work/native-events-fatal.log"
printf '99\t9999\t9999\truntime_fatal phase=owner_loop\n' >> "$work/native-events-fatal.log"
if "$ROOT_DIR/tools/verify_lom_panel_native_gate.sh" "$work/native-events-fatal.log" >/dev/null 2>&1; then
    echo "native verifier accepted a fatal structured event" >&2
    exit 1
fi

cat "$work/native-events.log" "$work/native-events.log" > "$work/native-events-restarted.log"
if "$ROOT_DIR/tools/verify_lom_panel_native_gate.sh" "$work/native-events-restarted.log" >/dev/null 2>&1; then
    echo "native verifier accepted a restarted shell grant" >&2
    exit 1
fi

for failure in \
    'sophia_live_shell_gpu schema=1 status=revoked grant_epoch=1' \
    'sophia_live_shell_content schema=1 status=transport_failed'; do
    cp "$work/native-events.log" "$work/native-events-lifecycle-failure.log"
    printf '100\t10000\t10000\t%s\n' "$failure" >> "$work/native-events-lifecycle-failure.log"
    if "$ROOT_DIR/tools/verify_lom_panel_native_gate.sh" "$work/native-events-lifecycle-failure.log" >/dev/null 2>&1; then
        echo "native verifier accepted a shell lifecycle failure" >&2
        exit 1
    fi
done

runner="$ROOT_DIR/tools/run_current_lom_panel_gate_tty4.sh"
preflight_line=$(grep -n 'lom_gpu_content_hardware_proof.sh' "$runner" | cut -d: -f1)
takeover_line=$(grep -n 'run_sophia_session.sh' "$runner" | cut -d: -f1)
[[ -n "$preflight_line" && -n "$takeover_line" && "$preflight_line" -lt "$takeover_line" ]] || {
    echo "native runner does not prove GPU/content before graphics takeover" >&2
    exit 1
}
grep -q '^SOPHIA_SESSION_STARTUP=none ' "$runner" || {
    echo "native runner unexpectedly starts an application" >&2
    exit 1
}
if grep -Eq '^[[:space:]]*(bind|pointer-bind|session)[[:space:]]' \
    "$ROOT_DIR/tools/fixtures/lom_panel_desktop.kdl"; then
    echo "native panel profile carries an unrelated session action" >&2
    exit 1
fi

echo "lom_gpu_content_verifiers schema=1 status=pass mutations=11 structured_events=true pre_takeover_proof=true"

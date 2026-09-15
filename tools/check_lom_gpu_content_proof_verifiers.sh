#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cat > "$work/gpu.log" <<'EOF'
lom_gpu_admission schema=2 status=ready grant_epoch=1 render_node=/dev/dri/renderD128 device_major=226 device_minor=128 selection_method=drm_dev_t adapter_render_major=226 adapter_render_minor=128 adapter_has_render=true pci_bus_id=0000:01:00.0 pci_vendor_id=1002 pci_device_id=744c backend=Vulkan device_type=DiscreteGpu adapter_name="fixture" driver="fixture" visible_dri_entries=renderD128
sophia_shell_gpu_content_hardware_proof schema=2 status=complete protected=true revision=6 capabilities=0x783 grant_epoch=1 render_node=/dev/dri/renderD128 device_major=226 device_minor=128 pci_bus_id=0000:01:00.0 width=256 height=24 renders=2 first_bytes=24576 first_checksum=0123456789abcdef second_bytes=24576 second_checksum=fedcba9876543210 first_outcome=presented_synthetic second_renderer_outcome=9 backing_bytes=0 native_presentation=false
EOF
"$ROOT_DIR/tools/verify_lom_gpu_content_hardware_proof.sh" "$work/gpu.log" >/dev/null
for mutation in \
    extra_drm cpu zero_checksum native identity adapter_identity method has_render \
    missing_identity missing_backend missing_device_type duplicate_epoch malformed_major \
    overflow_major zero_epoch non_vulkan missing_content_input_capability wrong_renderer_outcome \
    one_render missing_second_checksum claimed_native_first; do
    cp "$work/gpu.log" "$work/$mutation.log"
    case "$mutation" in
        extra_drm) sed -i 's/visible_dri_entries=renderD128/visible_dri_entries=card0,renderD128/' "$work/$mutation.log" ;;
        cpu) sed -i 's/device_type=DiscreteGpu/device_type=Cpu/' "$work/$mutation.log" ;;
        zero_checksum) sed -i 's/first_checksum=0123456789abcdef/first_checksum=0000000000000000/' "$work/$mutation.log" ;;
        native) sed -i 's/native_presentation=false/native_presentation=true/' "$work/$mutation.log" ;;
        identity) sed -i '/^lom_gpu_admission /s/device_minor=128/device_minor=129/' "$work/$mutation.log" ;;
        adapter_identity) sed -i '/^lom_gpu_admission /s/adapter_render_minor=128/adapter_render_minor=129/' "$work/$mutation.log" ;;
        method) sed -i '/^lom_gpu_admission /s/selection_method=drm_dev_t/selection_method=pci/' "$work/$mutation.log" ;;
        has_render) sed -i '/^lom_gpu_admission /s/adapter_has_render=true/adapter_has_render=false/' "$work/$mutation.log" ;;
        missing_identity)
            sed -i 's/ device_major=226//g' "$work/$mutation.log"
            sed -i '/^lom_gpu_admission /s/ adapter_render_major=226//' "$work/$mutation.log"
            ;;
        missing_backend) sed -i '/^lom_gpu_admission /s/ backend=Vulkan//' "$work/$mutation.log" ;;
        missing_device_type) sed -i '/^lom_gpu_admission /s/ device_type=DiscreteGpu//' "$work/$mutation.log" ;;
        duplicate_epoch) sed -i '/^lom_gpu_admission /s/grant_epoch=1/grant_epoch=1 grant_epoch=1/' "$work/$mutation.log" ;;
        malformed_major)
            sed -i 's/device_major=226/device_major=invalid/g' "$work/$mutation.log"
            sed -i '/^lom_gpu_admission /s/adapter_render_major=226/adapter_render_major=invalid/' "$work/$mutation.log"
            ;;
        overflow_major)
            sed -i 's/device_major=226/device_major=4294967296/g' "$work/$mutation.log"
            sed -i '/^lom_gpu_admission /s/adapter_render_major=226/adapter_render_major=4294967296/' "$work/$mutation.log"
            ;;
        zero_epoch) sed -i 's/grant_epoch=1/grant_epoch=0/g' "$work/$mutation.log" ;;
        non_vulkan) sed -i '/^lom_gpu_admission /s/backend=Vulkan/backend=Gl/' "$work/$mutation.log" ;;
        missing_content_input_capability) sed -i 's/capabilities=0x783/capabilities=0x683/' "$work/$mutation.log" ;;
        wrong_renderer_outcome) sed -i 's/second_renderer_outcome=9/second_renderer_outcome=10/' "$work/$mutation.log" ;;
        one_render) sed -i 's/renders=2/renders=1/' "$work/$mutation.log" ;;
        missing_second_checksum) sed -i 's/ second_checksum=fedcba9876543210//' "$work/$mutation.log" ;;
        claimed_native_first) sed -i 's/first_outcome=presented_synthetic/first_outcome=presented_native/' "$work/$mutation.log" ;;
    esac
    if "$ROOT_DIR/tools/verify_lom_gpu_content_hardware_proof.sh" "$work/$mutation.log" >/dev/null 2>&1; then
        echo "verifier accepted $mutation mutation" >&2
        exit 1
    fi
done

cat > "$work/native.log" <<'EOF'
sophia_live_shell_gpu schema=1 status=granted mode=direct peer_pid=42 grant_epoch=1 device_major=226 device_minor=128 pci_bus_id=0000:01:00.0
sophia_live_wm_configuration schema=2 status=committed catalog_generation=1 session_operation_count=7
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
    'sophia_live_shell_content schema=1 status=transport_failed' \
    'sophia_live_wm_configuration schema=2 status=rejected reason=unavailable_session_slot catalog_generation=1 missing_slot_count=1 missing_slots=7'; do
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
grep -q 'SOPHIA_CORE_CONFIG="$LOM_CORE_CONFIG"' "$runner" || {
    echo "native runner does not pass its bounded application catalog to Sophia" >&2
    exit 1
}
if grep -Eq '^[[:space:]]*(bind|pointer-bind)[[:space:]]' \
    "$ROOT_DIR/tools/fixtures/lom_panel_desktop.kdl"; then
    echo "native panel profile carries an unrelated session action" >&2
    exit 1
fi
grep -q '^[[:space:]]*application-catalog "lom-panel-gate"$' \
    "$ROOT_DIR/tools/fixtures/lom_panel_desktop.kdl" || {
    echo "native panel profile does not select its application catalog" >&2
    exit 1
}
grep -q '^[[:space:]]*startup$' \
    "$ROOT_DIR/tools/fixtures/lom_panel_desktop.kdl" || {
    echo "native panel profile does not explicitly select an empty startup set" >&2
    exit 1
}
grep -q '^[[:space:]]*content-input #true$' \
    "$ROOT_DIR/tools/fixtures/lom_panel_desktop.kdl" || {
    echo "native panel profile does not admit discrete content input" >&2
    exit 1
}
grep -q 'application-catalog "lom-panel-gate" launch-policy="trusted-host"' \
    "$ROOT_DIR/tools/fixtures/lom_panel_core.kdl" || {
    echo "native panel core fixture does not define its admitted catalog" >&2
    exit 1
}

# These transcript controls launch only Python, never a native/GPU fixture.
python3 -B -m unittest discover -s "$ROOT_DIR/tools/probes/lom_workload/tests"

echo "lom_gpu_content_verifiers schema=1 status=pass mutations=28 structured_events=true pre_takeover_proof=true sequential_renders=2 workload_verifier=true"

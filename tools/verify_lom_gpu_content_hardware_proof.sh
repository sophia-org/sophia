#!/usr/bin/env bash
set -euo pipefail
set -f
export LC_ALL=C

if [[ $# -ne 1 || ! -f "$1" ]]; then
    echo "usage: $0 EVIDENCE_LOG" >&2
    exit 2
fi

log=$1
ready_count=$(grep -c '^lom_gpu_admission schema=2 status=ready ' "$log" || true)
complete_count=$(grep -c '^sophia_shell_gpu_content_hardware_proof schema=1 status=complete ' "$log" || true)
[[ "$ready_count" -eq 1 ]] || { echo "expected one Lom GPU admission record" >&2; exit 1; }
[[ "$complete_count" -eq 1 ]] || { echo "expected one Sophia GPU/content completion record" >&2; exit 1; }
ready=$(grep '^lom_gpu_admission schema=2 status=ready ' "$log")
complete=$(grep '^sophia_shell_gpu_content_hardware_proof schema=1 status=complete ' "$log")
field() {
    local record=$1 name=$2
    local token value='' count=0
    for token in $record; do
        if [[ "$token" == "$name="* ]]; then
            value=${token#*=}
            count=$((count + 1))
        fi
    done
    if [[ "$count" -ne 1 || -z "$value" ]]; then
        echo "expected exactly one nonempty $name field" >&2
        return 1
    fi
    printf '%s\n' "$value"
}

bounded_unsigned() {
    local value=$1 maximum=$2 label=$3
    if [[ ! "$value" =~ ^(0|[1-9][0-9]*)$ ]] \
        || [[ ${#value} -gt ${#maximum} ]] \
        || { [[ ${#value} -eq ${#maximum} ]] && [[ "$value" > "$maximum" ]]; }; then
        echo "$label is not a canonical bounded unsigned integer" >&2
        return 1
    fi
}

ready_schema=$(field "$ready" schema)
ready_status=$(field "$ready" status)
grant_epoch=$(field "$ready" grant_epoch)
render_node=$(field "$ready" render_node)
device_major=$(field "$ready" device_major)
device_minor=$(field "$ready" device_minor)
selection_method=$(field "$ready" selection_method)
adapter_render_major=$(field "$ready" adapter_render_major)
adapter_render_minor=$(field "$ready" adapter_render_minor)
adapter_has_render=$(field "$ready" adapter_has_render)
backend=$(field "$ready" backend)
device_type=$(field "$ready" device_type)
visible_dri_entries=$(field "$ready" visible_dri_entries)
ready_pci_bus_id=$(field "$ready" pci_bus_id)
field "$ready" pci_vendor_id >/dev/null
field "$ready" pci_device_id >/dev/null

complete_schema=$(field "$complete" schema)
complete_status=$(field "$complete" status)
protected=$(field "$complete" protected)
revision=$(field "$complete" revision)
capabilities=$(field "$complete" capabilities)
complete_epoch=$(field "$complete" grant_epoch)
complete_render_node=$(field "$complete" render_node)
complete_major=$(field "$complete" device_major)
complete_minor=$(field "$complete" device_minor)
complete_pci_bus_id=$(field "$complete" pci_bus_id)
width=$(field "$complete" width)
height=$(field "$complete" height)
bytes=$(field "$complete" bytes)
checksum=$(field "$complete" checksum)
renderer_outcome=$(field "$complete" renderer_outcome)
backing_bytes=$(field "$complete" backing_bytes)
native_presentation=$(field "$complete" native_presentation)

[[ "$ready_schema" == 2 && "$ready_status" == ready ]] || { echo "invalid Lom GPU admission record identity" >&2; exit 1; }
[[ "$complete_schema" == 1 && "$complete_status" == complete ]] || { echo "invalid Sophia GPU proof record identity" >&2; exit 1; }
bounded_unsigned "$grant_epoch" 18446744073709551615 "GPU grant epoch"
bounded_unsigned "$complete_epoch" 18446744073709551615 "Sophia grant epoch"
[[ "$grant_epoch" != 0 && "$complete_epoch" != 0 ]] || { echo "GPU grant epoch must be nonzero" >&2; exit 1; }
for identity in \
    "$device_major:GPU device major" \
    "$device_minor:GPU device minor" \
    "$adapter_render_major:adapter render major" \
    "$adapter_render_minor:adapter render minor" \
    "$complete_major:Sophia device major" \
    "$complete_minor:Sophia device minor"; do
    bounded_unsigned "${identity%%:*}" 4294967295 "${identity#*:}"
done

[[ "$render_node" == "/dev/dri/renderD${device_minor}" ]] || { echo "Lom render-node path disagrees with its kernel minor" >&2; exit 1; }
[[ "$visible_dri_entries" == "renderD${device_minor}" ]] || { echo "Lom observed extra or missing DRM nodes" >&2; exit 1; }
[[ "$selection_method" == drm_dev_t ]] || { echo "Lom did not select by DRM device identity" >&2; exit 1; }
[[ "$adapter_has_render" == true ]] || { echo "Lom adapter has no render-node identity" >&2; exit 1; }
[[ "$backend" == Vulkan ]] || { echo "Lom did not select a Vulkan adapter" >&2; exit 1; }
case "$device_type" in
    DiscreteGpu|IntegratedGpu|VirtualGpu|Other) ;;
    *) echo "Lom did not report an explicitly permitted non-CPU adapter type" >&2; exit 1 ;;
esac
[[ "$adapter_render_major" == "$device_major" ]] || { echo "Lom adapter render major disagrees with the grant" >&2; exit 1; }
[[ "$adapter_render_minor" == "$device_minor" ]] || { echo "Lom adapter render minor disagrees with the grant" >&2; exit 1; }
[[ "$protected" == true ]] || { echo "proof did not establish protected launch" >&2; exit 1; }
[[ "$revision" == 6 && "$capabilities" == 0x283 ]] || { echo "proof negotiated the wrong shell contract" >&2; exit 1; }
[[ "$width" == 256 && "$height" == 24 && "$bytes" == 24576 ]] || { echo "proof panel dimensions changed" >&2; exit 1; }
[[ "$renderer_outcome" == 9 ]] || { echo "proof did not settle through RendererFailed" >&2; exit 1; }
[[ "$backing_bytes" == 0 && "$native_presentation" == false ]] || { echo "proof did not release backing or overclaimed native presentation" >&2; exit 1; }
[[ "$checksum" =~ ^[0-9a-f]{16}$ ]] || { echo "proof omitted its pixel checksum" >&2; exit 1; }
[[ "$checksum" != 0000000000000000 ]] || { echo "proof reported a zero checksum" >&2; exit 1; }
compare_identity() {
    [[ "$1" == "$2" ]] || { echo "Lom and Sophia disagree on $3" >&2; exit 1; }
}
compare_identity "$grant_epoch" "$complete_epoch" grant_epoch
compare_identity "$render_node" "$complete_render_node" render_node
compare_identity "$device_major" "$complete_major" device_major
compare_identity "$device_minor" "$complete_minor" device_minor
compare_identity "$ready_pci_bus_id" "$complete_pci_bus_id" pci_bus_id
if grep -Eq 'runtime_fatal|panic|deadline expired|deadline_exceeded' "$log"; then
    echo "proof log contains a fatal or deadline failure" >&2
    exit 1
fi

printf 'lom_gpu_content_verification schema=1 status=pass native_presentation=false\n'

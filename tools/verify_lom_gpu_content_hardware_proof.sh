#!/usr/bin/env bash
set -euo pipefail

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
    sed -n "s/.* ${name}=\\([^ ]*\\).*/\\1/p" <<<"$record"
}

device_minor=$(field "$ready" device_minor)
render_node=$(field "$ready" render_node)
[[ "$render_node" == "/dev/dri/renderD${device_minor}" ]] || { echo "Lom render-node path disagrees with its kernel minor" >&2; exit 1; }
[[ "$ready" == *" visible_dri_entries=renderD${device_minor}" ]] || { echo "Lom observed extra or missing DRM nodes" >&2; exit 1; }
[[ "$ready" != *'device_type=Cpu'* ]] || { echo "Lom selected a CPU adapter" >&2; exit 1; }
[[ "$(field "$ready" selection_method)" == drm_dev_t ]] || { echo "Lom did not select by DRM device identity" >&2; exit 1; }
[[ "$(field "$ready" adapter_has_render)" == true ]] || { echo "Lom adapter has no render-node identity" >&2; exit 1; }
[[ "$(field "$ready" adapter_render_major)" == "$(field "$ready" device_major)" ]] || { echo "Lom adapter render major disagrees with the grant" >&2; exit 1; }
[[ "$(field "$ready" adapter_render_minor)" == "$device_minor" ]] || { echo "Lom adapter render minor disagrees with the grant" >&2; exit 1; }
[[ "$complete" == *'protected=true '* ]] || { echo "proof did not establish protected launch" >&2; exit 1; }
[[ "$complete" == *'width=256 height=24 bytes=24576 '* ]] || { echo "proof panel dimensions changed" >&2; exit 1; }
[[ "$complete" == *'backing_bytes=0 native_presentation=false'* ]] || { echo "proof did not release backing or overclaimed native presentation" >&2; exit 1; }
[[ "$complete" =~ checksum=([0-9a-f]{16}) ]] || { echo "proof omitted its pixel checksum" >&2; exit 1; }
[[ "${BASH_REMATCH[1]}" != 0000000000000000 ]] || { echo "proof reported a zero checksum" >&2; exit 1; }
for identity in grant_epoch render_node device_major device_minor pci_bus_id; do
    [[ "$(field "$ready" "$identity")" == "$(field "$complete" "$identity")" ]] || {
        echo "Lom and Sophia disagree on $identity" >&2; exit 1;
    }
done
if grep -Eq 'runtime_fatal|panic|deadline expired|deadline_exceeded' "$log"; then
    echo "proof log contains a fatal or deadline failure" >&2
    exit 1
fi

printf 'lom_gpu_content_verification schema=1 status=pass native_presentation=false\n'

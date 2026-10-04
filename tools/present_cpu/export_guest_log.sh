#!/bin/sh
# Run after measurement, including on a failed workload. Export failures must
# return a status to init, never terminate PID 1 through its set -e setting.
set +e
directory=$1
status=0
if [ -s "$directory/workload.json" ]; then
    printf 'sophia_present_cpu_result '
    cat "$directory/workload.json" || status=1
    printf '\n'
fi
bytes=$(wc -c < "$directory/session.log")
size_status=$?
hash=$(sha256sum "$directory/session.log")
hash_status=$?
usage=$(du -sk "$directory")
usage_status=$?
if [ "$size_status" -ne 0 ] || [ "$hash_status" -ne 0 ] || [ "$usage_status" -ne 0 ]; then
    echo 'sophia_qemu_cpu_log schema=1 status=failed reason=metadata'
    exit 1
fi
read -r export_start unused < /proc/uptime
echo "sophia_qemu_cpu_log schema=1 transport=tmpfs export=gzip_base64_after_measurement bytes=$bytes sha256=${hash%% *} tmpfs_used_kib=${usage%%[[:space:]]*} export_start_uptime=$export_start"
echo 'sophia_qemu_cpu_log_gzip_begin'
# No compressed copy. POSIX sh has no pipefail: retain the producer's status
# separately, so an encoder that accepts a truncated stream cannot hide ENOSPC
# or a read error. If even this tiny status file fails, the export fails closed.
(
    gzip -1 -c "$directory/session.log"
    printf '%s\n' "$?" > "$directory/compress.status"
) | base64
encode_status=$?
compress_status=1
if [ -r "$directory/compress.status" ]; then
    read -r compress_status < "$directory/compress.status"
fi
if [ "$encode_status" -eq 0 ] && [ "$compress_status" = 0 ]; then
    echo 'sophia_qemu_cpu_log_gzip_end'
else
    echo 'sophia_qemu_cpu_log schema=1 status=failed reason=export'
    status=1
fi
read -r export_end unused < /proc/uptime
echo "sophia_qemu_cpu_log schema=1 export_end_uptime=$export_end status=$status"
echo 'sophia_qemu_cpu_interrupts schema=1 boundary=before_session'
cat "$directory/interrupts-before" || status=1
echo 'sophia_qemu_cpu_interrupts schema=1 boundary=after_session_before_export'
cat "$directory/interrupts-after" || status=1
exit "$status"

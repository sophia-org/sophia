#!/bin/sh
# Sophia's check.sh for a separate XTS5 checkout, copied over the one the
# checkout's configure generates. That one regenerates tetexec.cfg through
# xts-config, which runs `xset q` and `xdpyinfo` against the display; the
# fixture host decodes neither the font-path nor the keyboard-control
# requests those need, so the configuration is written here from the
# suite's own template with the display named and every font path empty.
#
# The adapter runs `bash ./check.sh <scenario>` inside its sandbox with
# cwd = TET_ROOT, DISPLAY=:99, HOME=/home/test, no /etc and
# PATH=/usr/bin:/bin, after deleting tetexec.cfg and results/ from its
# private copy; nothing from an earlier run can supply a pass.
set -eu

scenario="${1:?usage: check.sh SCENARIO}"
case "$scenario" in
    *[!A-Za-z0-9-]*|'')
        echo "scenario must be letters, digits and dashes: $scenario" >&2
        exit 2
        ;;
esac
root="$PWD"
suite="$root/xts5"
[ -d "$suite" ] || {
    echo "built suite directory is missing: $suite" >&2
    exit 2
}
grep -q "^$scenario\$" "$suite/tet_scen" || {
    echo "scenario $scenario is not in $suite/tet_scen; run xts_select.py --install" >&2
    exit 2
}
tcc="$root/src/tet3/tcc/tcc"
[ -x "$tcc" ] || tcc="$(find "$root" -type f -name tcc -perm -u+x 2>/dev/null | head -n 1)"
[ -n "$tcc" ] && [ -x "$tcc" ] || {
    echo "no executable tcc under $root" >&2
    exit 2
}
template="$suite/tetexec.cfg.in"
[ -f "$template" ] || {
    echo "the suite's configuration template is missing: $template" >&2
    exit 2
}

# The template's keys, with what this display can honestly say: its name,
# no font path (font opcodes are not decoded), no reset delay (the host
# persists across cases), and the vendor fields left for the suite to read.
config="$suite/tetexec.cfg"
{
    grep -v '^XT_DISPLAY=\|^XT_FONTPATH=\|^XT_FONTPATH_GOOD=\|^XT_RESET_DELAY=' "$template"
    printf 'XT_DISPLAY=%s\n' "${DISPLAY:-:99}"
    printf 'XT_FONTPATH=\nXT_FONTPATH_GOOD=\nXT_RESET_DELAY=0\n'
} >"$config"

results="$root/results"
mkdir -p "$results"
export TET_ROOT="$root"
export TET_EXECUTE="$suite"
# The same invocation the checkout's xts-run makes: execute, journal under
# results, this configuration, the xts5 suite, the named scenario.
"$tcc" -e -i "$results" -x "$config" xts5 "$scenario"
status=$?
[ -n "$(find "$results" -mindepth 2 -maxdepth 2 -name journal -type f 2>/dev/null)" ] || {
    echo "tcc wrote no journal under $results" >&2
    exit 1
}
exit "$status"

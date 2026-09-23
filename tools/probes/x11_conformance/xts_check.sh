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
# no font path (font opcodes are not decoded), a one-second reset delay, and
# the vendor fields left for the suite to read.
config="$suite/tetexec.cfg"
{
    grep -v '^DISPLAY=\|^XT_FONTPATH=\|^XT_FONTPATH_GOOD=\|^XT_RESET_DELAY=' "$template"
    # The suite reads DISPLAY (the config var xts-config derives from
    # xdpyinfo); XT_DISPLAY is not a key it knows. Empty font paths because
    # no font opcode is decoded.
    #
    # The reset delay is what the suite waits after closing its last
    # connection, and it was zero here because the host persists across
    # cases. That is true of the process and not of the state: the atom
    # lifetime assertion closes the display and immediately reopens it to
    # check that the atoms went, and with no delay it can reconnect before
    # the server has finished tearing the old connection down. One second is
    # the suite's own default and costs one second per case that resets.
    printf 'DISPLAY=%s\n' "${DISPLAY:-:99}"
    printf 'XT_FONTPATH=\nXT_FONTPATH_GOOD=\nXT_RESET_DELAY=1\n'
} >"$config"

# tcc writes its journal directly into the directory it is given; the
# adapter reads exactly one results/*/journal, so the run gets a directory
# of its own under results.
results="$root/results/$scenario"
mkdir -p "$results"
export TET_ROOT="$root"
export TET_EXECUTE="$suite"
# Every program libtool built is a wrapper: it finds its binary beside
# itself but names the checkout's original absolute path for its libraries,
# and the adapter runs a private copy elsewhere. The loader skips a missing
# directory, so naming every built library directory of this copy is enough.
libdirs="$(find "$root" -type d -name .libs 2>/dev/null | tr '\n' ':')"
export LD_LIBRARY_PATH="${libdirs}${LD_LIBRARY_PATH:-}"
# The same invocation the checkout's xts-run makes: execute, journal under
# results, this configuration, the xts5 suite, the named scenario -- with
# one addition, a per-test-case timeout. A purpose that deadlocks its
# connection (t165) used to hold the whole run to the adapter's deadline and
# report nothing for the cases behind it; now it costs its case two minutes
# and the journal records the rest.
"$tcc" -e -t 120 -i "$results" -x "$config" xts5 "$scenario"
status=$?
[ -s "$results/journal" ] || {
    echo "tcc wrote no journal at $results/journal" >&2
    exit 1
}
exit "$status"

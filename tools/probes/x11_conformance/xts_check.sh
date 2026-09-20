#!/bin/sh
# Sophia's check.sh for a separate XTS5 checkout. Copy it to the checkout's
# root as check.sh; the adapter runs `bash ./check.sh <scenario>` inside its
# sandbox with cwd = TET_ROOT, DISPLAY=:99, HOME=/home/test, no /etc, and
# PATH=/usr/bin:/bin, after deleting tetexec.cfg and results/ from its private
# copy. Everything a run needs is regenerated here; nothing from an earlier
# run can supply a pass.
set -eu

scenario="${1:?usage: check.sh SCENARIO}"
case "$scenario" in
    *[!a-z0-9-]*|'')
        echo "scenario must be lowercase letters, digits and dashes: $scenario" >&2
        exit 2
        ;;
esac
suite="${XTS_SUITE:-xts5}"
scenario_file="$suite/tet_scen.$scenario"
[ -f "$scenario_file" ] || {
    echo "selected scenario file is missing: $scenario_file" >&2
    exit 2
}
[ -d "$suite" ] || {
    echo "built suite directory is missing: $suite" >&2
    exit 2
}

# The adapter's PATH is /usr/bin:/bin, which never holds tcc; the checkout's
# own build is the one that ran the suite.
tcc="$(find "$PWD" -type f -name tcc -perm -u+x 2>/dev/null | head -n 1)"
if [ -z "$tcc" ]; then
    tcc="$(command -v tcc 2>/dev/null || true)"
fi
[ -n "$tcc" ] || {
    echo "no executable tcc under $PWD or on PATH" >&2
    exit 2
}

# The execution configuration, written fresh. The fixture host decodes no
# font opcodes, so the font paths stay empty and no text purpose belongs in
# a selected scenario; the host persists across cases, so no reset delay.
cat >"$suite/tetexec.cfg" <<CFG
TET_OUTPUT_CAPTURE=False
TET_EXEC_IN_PLACE=True
TET_SAVE_FILES=
TET_TRANSFER_SAVE_FILES=
XT_DISPLAY=${DISPLAY:-:99}
XT_LOCAL=Yes
XT_FONTPATH=
XT_FONTPATH_GOOD=
XT_FONTPATH_BAD=
XT_RESET_DELAY=0
XT_SPEEDFACTOR=1
XT_DEBUG=0
XT_SAVE_SERVER_IMAGE=No
XT_EXTENSIONS=
CFG

results="$PWD/results"
mkdir -p "$results/0001e"
journal="$results/0001e/journal"
export TET_ROOT="${TET_ROOT:-$PWD}"
export TET_SUITE_ROOT="${TET_SUITE_ROOT:-$PWD}"
# -e executes, -s names the scenario file, -j names the journal so it lands
# where the adapter looks (exactly one results/*/journal).
"$tcc" -e -s "$scenario_file" -j "$journal" "$suite" "$scenario"
status=$?
[ -s "$journal" ] || {
    echo "tcc wrote no journal at $journal" >&2
    exit 1
}
exit "$status"

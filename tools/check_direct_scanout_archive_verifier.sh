#!/usr/bin/env bash
set -euo pipefail

# Proves the direct-scanout archive verifier rejects what it claims to.
#
# An archive is a claim that a specific commit and a specific binary produced a
# specific result. Each mutation below breaks one link in that chain and must
# be refused, because an archive nobody has watched fail is an archive that
# accepts anything.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
verifier="$ROOT_DIR/tools/verify_direct_scanout_archive.sh"
archiver="$ROOT_DIR/tools/archive_direct_scanout_run.sh"
temp_dir="$(mktemp -d)"
trap 'rm -rf -- "$temp_dir"' EXIT

# A passing session, synthesised rather than borrowed: the checks here are
# about the archive's identity chain, and the evidence verifier has its own
# controls in check_direct_scanout_verifier.sh.
evidence="$temp_dir/session.log"
{
    printf 'sophia_live_session schema=16 status=bounded_complete display=:77 runtime_surfaces=0 wm_policy=disabled wm_restarts=0\n'
    printf 'sophia_live_session_present schema=2 status=retired transaction=242 surface=2097166 source=2560x1440 target=2560x1440_0_0 clip=2560x1440_0_0 unit_scale=true\n'
    printf 'sophia_live_native_resources schema=12 status=complete direct_scanout_attempts=30 direct_scanout_flips=30 direct_scanout_tests=1 direct_scanout_test_rejections=0 direct_scanout_refusals=0 direct_scanout_unsupported=0 direct_scanout_fallbacks=0\n'
    printf 'sophia_live_direct_scanout_verdicts schema=2 status=complete eligible=32 layer_count=26 layer_not_active=0 layer_resampled=0 layer_offset=0 layer_not_head_sized=0 layer_clipped=0 layer_not_dma_buf=0 layer_translucent=0 composition_required=0 composed_cursor=0\n'
    printf 'sophia_live_direct_scanout schema=1 status=exported output=1 scene_generation=299 reason=none\n'
    printf 'sophia_live_direct_scanout schema=1 status=test_passed output=1 scene_generation=299 reason=none\n'
    printf 'sophia_live_direct_scanout schema=1 status=flipped output=1 scene_generation=299 reason=none\n'
} >"$evidence"

commit="$(git -C "$ROOT_DIR" rev-parse HEAD)"
# This verifier-only fixture hashes a real built executable; it does not run
# Sophia or assert that the synthesized scanout log came from that executable.
# Canonical checks build the debug profile, possibly in a dedicated target dir.
sophia_bin="${SOPHIA_DIRECT_SCANOUT_SOPHIA_BIN-${CARGO_TARGET_DIR:-$ROOT_DIR/target}/debug/sophia}"
if [[ ! -f "$sophia_bin" || ! -x "$sophia_bin" ]]; then
    echo "archive verifier fixture needs a built Sophia executable: $sophia_bin" >&2
    exit 1
fi
# This synthetic identity is hashed only; no installed desktop client is needed.
client_bin="${SOPHIA_DIRECT_SCANOUT_CLIENT_BIN:-/usr/bin/true}"
printf 'sophia_direct_scanout_identity schema=1 status=bound source_commit=%s sophia_sha256=%s client=%s client_sha256=%s core_sha256=%s desktop_sha256=%s\n' \
    "$commit" "$(sha256sum "$sophia_bin" | awk '{ print $1 }')" \
    "$(basename "$client_bin")" "$(sha256sum "$client_bin" | awk '{ print $1 }')" \
    "$(sha256sum "$ROOT_DIR/tools/fixtures/direct_scanout_core.kdl" | awk '{ print $1 }')" \
    "$(sha256sum "$ROOT_DIR/tools/fixtures/direct_scanout_desktop.kdl" | awk '{ print $1 }')" \
    >>"$evidence"

run_root="$temp_dir/runs"
SOPHIA_DIRECT_SCANOUT_RUN_ROOT="$run_root" \
    SOPHIA_DIRECT_SCANOUT_SOPHIA_BIN="$sophia_bin" \
    SOPHIA_DIRECT_SCANOUT_CLIENT_BIN="$client_bin" \
    "$archiver" "$evidence" >/dev/null
archive="$run_root/0001"
"$verifier" "$archive" >/dev/null || {
    echo "the archive verifier rejected an archive it just wrote" >&2
    exit 1
}

reject() {
    local name="$1" candidate="$2" expected="$3" output
    if output="$("$verifier" "$candidate" 2>&1)"; then
        echo "the archive verifier accepted $name" >&2
        exit 1
    fi
    printf '%s\n' "$output" | grep -Fq "$expected" || {
        echo "the archive verifier refused $name for the wrong reason:" >&2
        printf '%s\n' "$output" >&2
        exit 1
    }
}

# A wrong executable is a rejection candidate only, never a substitute for the
# actual Sophia binary used by the positive fixture. Neither binary is run.
wrong_binary="$temp_dir/wrong-sophia"
printf '#!/bin/sh\nexit 1\n' >"$wrong_binary"
chmod 700 "$wrong_binary"
mismatch_root="$temp_dir/mismatched-binary-runs"
if mismatch_output="$(
    SOPHIA_DIRECT_SCANOUT_RUN_ROOT="$mismatch_root" \
    SOPHIA_DIRECT_SCANOUT_SOPHIA_BIN="$wrong_binary" \
    SOPHIA_DIRECT_SCANOUT_CLIENT_BIN="$client_bin" \
        "$archiver" "$evidence" 2>&1
)"; then
    echo "the archiver accepted a Sophia binary that differs from the bound hash" >&2
    exit 1
fi
if [[ "$mismatch_output" != *"the Sophia binary no longer matches the verified run"* ]]; then
    echo "the archiver refused the changed Sophia binary for the wrong reason:" >&2
    printf '%s\n' "$mismatch_output" >&2
    exit 1
fi
if [[ -e "$mismatch_root" ]]; then
    echo "the archiver created output for a mismatched Sophia binary" >&2
    exit 1
fi

# Evidence edited after the fact. The checksums exist for this.
tampered="$temp_dir/tampered"
cp -r "$archive" "$tampered"
printf 'sophia_live_direct_scanout schema=1 status=flipped output=1 scene_generation=999 reason=none\n' \
    >>"$tampered/session.log"
reject "an archive whose evidence was edited" "$tampered" "checksum verification failed"

# A manifest describing a different run than the one beside it.
mismatched="$temp_dir/mismatched"
cp -r "$archive" "$mismatched"
sed -i 's/^evidence_sha256=.*/evidence_sha256=0000000000000000000000000000000000000000000000000000000000000000/' \
    "$mismatched/manifest"
(cd "$mismatched" && sha256sum manifest result.kdl session.log >SHA256SUMS)
reject "a manifest that does not describe its own evidence" "$mismatched" \
    "does not describe its own evidence"

# A commit this checkout has never seen.
unknown="$temp_dir/unknown"
cp -r "$archive" "$unknown"
sed -i 's/^source_commit=.*/source_commit=0123456789012345678901234567890123456789/' \
    "$unknown/manifest"
(cd "$unknown" && sha256sum manifest result.kdl session.log >SHA256SUMS)
reject "an archive naming a commit this checkout does not have" "$unknown" \
    "does not have"

# A manifest and its evidence disagreeing about which commit ran.
disagreed="$temp_dir/disagreed"
cp -r "$archive" "$disagreed"
other="$(git -C "$ROOT_DIR" rev-parse HEAD~1)"
sed -i "s/^source_commit=.*/source_commit=$other/" "$disagreed/manifest"
(cd "$disagreed" && sha256sum manifest result.kdl session.log >SHA256SUMS)
reject "a manifest and evidence that disagree on the commit" "$disagreed" \
    "disagree on the source commit"

# An unsigned commit, in a scratch clone. Every commit in this repository is
# signed, so the only way to exercise the signature guard is to build a history
# that has one that is not -- and doing that here rather than in the working
# tree keeps the check from depending on a commit someone might later sign.
unsigned_repo="$temp_dir/unsigned"
git init -q "$unsigned_repo"
git -C "$unsigned_repo" -c user.email=check@example.invalid -c user.name=check \
    -c commit.gpgsign=false commit -q --allow-empty -m 'unsigned'
unsigned_commit="$(git -C "$unsigned_repo" rev-parse HEAD)"
if git -C "$ROOT_DIR" cat-file -e "$unsigned_commit^{commit}" 2>/dev/null; then
    echo "the scratch commit collided with this repository's history" >&2
    exit 1
fi
# Fetched in so the commit exists here but carries no signature.
git -C "$ROOT_DIR" fetch -q "$unsigned_repo" HEAD
unsigned="$temp_dir/unsigned-archive"
cp -r "$archive" "$unsigned"
sed -i "s/^source_commit=.*/source_commit=$unsigned_commit/" "$unsigned/manifest"
sed -i "s/source_commit=$commit /source_commit=$unsigned_commit /" "$unsigned/session.log"
(cd "$unsigned" && sha256sum manifest result.kdl session.log >SHA256SUMS)
sed -i "s/^evidence_sha256=.*/evidence_sha256=$(sha256sum "$unsigned/session.log" | awk '{ print $1 }')/" \
    "$unsigned/manifest"
(cd "$unsigned" && sha256sum manifest result.kdl session.log >SHA256SUMS)
reject "an archive naming an unsigned commit" "$unsigned" "without a valid signature"

# A run that recorded a different kind of proof.
wrong_kind="$temp_dir/wrong-kind"
cp -r "$archive" "$wrong_kind"
sed -i 's/^record_kind=.*/record_kind=hagia_native_session/' "$wrong_kind/manifest"
(cd "$wrong_kind" && sha256sum manifest result.kdl session.log >SHA256SUMS)
reject "an archive recording another kind of run" "$wrong_kind" "another kind of run"

# An archive re-verifies under the rules that promoted it. A manifest may
# declare `proof=overlay`, and evidence carrying overlay records says the same
# thing for archives written before the field existed. Both are consulted:
# without the manifest an older overlay run would quietly re-verify as an
# ordinary one, and without the evidence check a field could outlive the proof
# it names.
declared_only="$temp_dir/declared-without-evidence"
cp -r "$archive" "$declared_only"
printf 'proof=overlay\n' >>"$declared_only/manifest"
(cd "$declared_only" && sha256sum manifest result.kdl session.log >SHA256SUMS)
reject "a manifest declaring an overlay proof its evidence lacks" "$declared_only" \
    "does not contain"

# Evidence carrying overlay records must face the overlay rules even with no
# field to declare them -- this is how archive 0002 keeps being held to the
# proof it actually made. An activation with no withdrawal is the cheapest
# violation of those rules that a plain verification would happily accept.
observed="$temp_dir/observed-overlay"
cp -r "$archive" "$observed"
printf 'sophia_live_direct_scanout_overlay_proof schema=1 status=activated output=1 flips_before=10\n' \
    >>"$observed/session.log"
sed -i "s/^evidence_sha256=.*/evidence_sha256=$(sha256sum "$observed/session.log" | awk '{ print $1 }')/" \
    "$observed/manifest"
(cd "$observed" && sha256sum manifest result.kdl session.log >SHA256SUMS)
reject "evidence with overlay records verified as an ordinary run" "$observed" \
    "never withdrew"

# The same in the other direction, for the cursor proof: a run whose evidence
# carries cursor-proof records faces the cursor rules whether or not the
# manifest names them. Without this, archive 0004 re-verifies as an ordinary
# direct-scanout run and the claim it was written to test stops being checked.
declared_cursor="$temp_dir/declared-cursor"
cp -r "$archive" "$declared_cursor"
printf 'proof=cursor\n' >>"$declared_cursor/manifest"
(cd "$declared_cursor" && sha256sum manifest result.kdl session.log >SHA256SUMS)
reject "a manifest declaring a cursor proof its evidence lacks" "$declared_cursor" \
    "does not contain"

observed_cursor="$temp_dir/observed-cursor"
cp -r "$archive" "$observed_cursor"
printf 'sophia_live_direct_scanout_cursor_proof schema=1 status=started output=1 flips_before=10\n' \
    >>"$observed_cursor/session.log"
sed -i "s/^evidence_sha256=.*/evidence_sha256=$(sha256sum "$observed_cursor/session.log" | awk '{ print $1 }')/" \
    "$observed_cursor/manifest"
(cd "$observed_cursor" && sha256sum manifest result.kdl session.log >SHA256SUMS)
reject "evidence with cursor records verified as an ordinary run" "$observed_cursor" \
    "never finished"

echo "direct scanout archive verifier checks passed"

#!/usr/bin/env bash
set -euo pipefail

# Re-verifies an archived native Hagia session independently of the run that
# produced it: checksums, record identity, every signed commit, and the evidence
# itself through the same verifier the gate used.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/lib/proof_checkout.sh
source "$ROOT_DIR/tools/lib/proof_checkout.sh"
# shellcheck source=tools/lib/reference_capture.sh
source "$ROOT_DIR/tools/lib/reference_capture.sh"
# The kind a caller expects is the caller's statement, never read from the
# archive: every existing reader keeps the native default and so refuses a
# t018 reference capture. Only --expected-kind=reference reads one.
expected_kind=native
case "${1:-}" in
    --expected-kind=native) shift ;;
    --expected-kind=reference) expected_kind=reference; shift ;;
    --expected-kind=*) echo "expected kind must be native or reference: $1" >&2; exit 2 ;;
esac
hagia_root="${SOPHIA_HAGIA_ROOT:-$ROOT_DIR/../hagia}"
state_home="${XDG_STATE_HOME:-$HOME/.local/state}"
if [[ "$expected_kind" == reference ]]; then
    run_root="${SOPHIA_HAGIA_REFERENCE_RUN_ROOT:-$state_home/sophia/reference/hagia-tab-captures}"
    expected_record_kind=hagia_reference_capture
else
    run_root="${SOPHIA_HAGIA_NATIVE_RUN_ROOT:-$state_home/sophia/promotion/hagia-native-runs}"
    expected_record_kind=hagia_native_session
fi
run="${1:-}"
if [[ -z "$run" ]]; then
    run="$(find "$run_root" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort -V | tail -n 1 || true)"
fi
[[ -n "$run" && -s "$run/SHA256SUMS" ]] || {
    echo "Hagia native session archive is missing: ${run:-$run_root}" >&2
    exit 1
}
proof_checkout_root "$hagia_root" || {
    echo "Hagia checkout is unavailable: $hagia_root" >&2
    exit 1
}
(
    cd "$run"
    sha256sum -c --status SHA256SUMS
) || {
    echo "Hagia native session archive checksum verification failed: $run" >&2
    exit 1
}
# record_schema=2 binds the Narthex commit and names the shell binary narthex.
# record_schema=1 is the pre-split spelling; these archives are immutable
# history and every one of them is re-verified by `cargo xtask check`.
record_schema="$(sed -n 's/^record_schema=//p' "$run/manifest")"
[[ "$(grep -c '^record_kind=' "$run/manifest")" == 1 \
    && "$(sed -n 's/^record_kind=//p' "$run/manifest")" == "$expected_record_kind" ]] || {
    echo "Hagia archive is not the expected $expected_kind kind: $run" >&2
    exit 1
}
if [[ "$expected_kind" == reference ]]; then
    [[ "$record_schema" == 1 \
        && "$(cat "$run/result.kdl")" == "sophia_hagia_reference_capture schema=1 status=captured native_acceptance=false" ]] || {
        echo "Hagia reference capture archive has the wrong record: $run" >&2
        exit 1
    }
    # The identity line and Narthex bindings of a capture are schema 2.
    identity_schema=2
else
    [[ "$record_schema" == 1 || "$record_schema" == 2 ]] || {
        echo "Hagia native session archive has the wrong record identity: $run" >&2
        exit 1
    }
    [[ "$(cat "$run/result.kdl")" == "sophia_hagia_native_session schema=$record_schema status=passed" ]] || {
        echo "Hagia native session archive is not passing: $run" >&2
        exit 1
    }
    identity_schema="$record_schema"
fi
if [[ "$identity_schema" == 2 ]]; then
    shell_manifest_key=narthex_binary_sha256
    shell_identity_key=narthex_sha256
    extra_keys=(narthex_commit)
else
    shell_manifest_key=hagia_shell_binary_sha256
    shell_identity_key=hagia_shell_sha256
    extra_keys=()
fi
for key in source_commit hagia_commit proof_text evidence_sha256 \
    sophia_binary_sha256 hagia_binary_sha256 "$shell_manifest_key" \
    "${extra_keys[@]}" desktop_profile_sha256; do
    [[ "$(grep -c "^${key}=" "$run/manifest")" == 1 ]] || {
        echo "Hagia native session archive has invalid $key cardinality: $run" >&2
        exit 1
    }
done

source_commit="$(sed -n 's/^source_commit=//p' "$run/manifest")"
hagia_commit="$(sed -n 's/^hagia_commit=//p' "$run/manifest")"
for repo_and_commit in "$ROOT_DIR:$source_commit" "$hagia_root:$hagia_commit"; do
    repo="${repo_and_commit%:*}"
    commit="${repo_and_commit##*:}"
    [[ "$commit" =~ ^[0-9a-f]{40}$ ]] && git -C "$repo" cat-file -e "$commit^{commit}" || {
        echo "Hagia native session archive has an invalid source commit: $repo" >&2
        exit 1
    }
    git -C "$repo" verify-commit "$commit" >/dev/null 2>&1 || {
        echo "Hagia native session archive source commit lacks a valid signature: $repo" >&2
        exit 1
    }
done

evidence_sha256="$(sha256sum "$run/session.log" | awk '{ print $1 }')"
[[ "$(sed -n 's/^evidence_sha256=//p' "$run/manifest")" == "$evidence_sha256" ]] || {
    echo "Hagia native session evidence digest does not match its manifest: $run" >&2
    exit 1
}
# The identity schema tracks the record schema one-for-one here.
identity="$(grep -E "^sophia_hagia_native_identity schema=$identity_schema status=bound " "$run/session.log")"
[[ "$(sed -n 's/.* sophia_commit=\([0-9a-f]\{40\}\) .*/\1/p' <<<"$identity")" == "$source_commit" \
    && "$(sed -n 's/.* hagia_commit=\([0-9a-f]\{40\}\) .*/\1/p' <<<"$identity")" == "$hagia_commit" \
    && "$(sed -n 's/.* sophia_sha256=\([0-9a-f]\{64\}\) .*/\1/p' <<<"$identity")" == \
        "$(sed -n 's/^sophia_binary_sha256=//p' "$run/manifest")" \
    && "$(sed -n 's/.* hagia_sha256=\([0-9a-f]\{64\}\) .*/\1/p' <<<"$identity")" == \
        "$(sed -n 's/^hagia_binary_sha256=//p' "$run/manifest")" \
    && "$(sed -n "s/.* $shell_identity_key=\\([0-9a-f]\\{64\\}\\) .*/\\1/p" <<<"$identity")" == \
        "$(sed -n "s/^$shell_manifest_key=//p" "$run/manifest")" \
    && "$(sed -n 's/.* desktop_profile_sha256=\([0-9a-f]\{64\}\)$/\1/p' <<<"$identity")" == \
        "$(sed -n 's/^desktop_profile_sha256=//p' "$run/manifest")" ]] || {
    echo "Hagia native session evidence and manifest have different identities: $run" >&2
    exit 1
}
# Schema 1 predates the Narthex split and has no Narthex source identity.
# Schema 2 must retain all three signature and manifest/evidence bindings on
# re-verification, not only when the archive is first written.
if [[ "$identity_schema" == 2 ]]; then
    narthex_root="${SOPHIA_NARTHEX_ROOT:-$ROOT_DIR/../narthex}"
    narthex_commit="$(sed -n 's/^narthex_commit=//p' "$run/manifest")"
    [[ "$(sed -n 's/.* narthex_commit=\([0-9a-f]\{40\}\) .*/\1/p' <<<"$identity")" == "$narthex_commit" ]] || {
        echo "Hagia native session evidence and manifest have different Narthex identities: $run" >&2
        exit 1
    }
    proof_checkout_root "$narthex_root" || {
        echo "Narthex checkout is unavailable: $narthex_root" >&2
        exit 1
    }
    [[ "$narthex_commit" =~ ^[0-9a-f]{40}$ ]] \
        && git -C "$narthex_root" cat-file -e "$narthex_commit^{commit}" || {
        echo "Hagia native session archive has an invalid Narthex source commit: $narthex_root" >&2
        exit 1
    }
    git -C "$narthex_root" verify-commit "$narthex_commit" >/dev/null 2>&1 || {
        echo "Hagia native session archive Narthex source commit lacks a valid signature: $narthex_root" >&2
        exit 1
    }
fi
proof_text="$(sed -n 's/^proof_text=//p' "$run/manifest")"
if [[ "$expected_kind" == reference ]]; then
    reference_capture_validate "$run/session.log" "$proof_text" || exit 1
    # Retained observations must be exactly the ones the manifest names; they
    # remain unverified operator notes.
    observation_keys="$(grep -c '^observations_sha256=' "$run/manifest" || true)"
    if [[ -e "$run/observations.txt" || "$observation_keys" != 0 ]]; then
        [[ "$observation_keys" == 1 && -f "$run/observations.txt" && ! -L "$run/observations.txt" \
            && "$(sed -n 's/^observations_sha256=//p' "$run/manifest")" \
                == "$(sha256sum "$run/observations.txt" | awk '{ print $1 }')" \
            && "$(grep -c '^observations=unverified$' "$run/manifest")" == 1 \
            && "$(grep -c ' observations.txt$' "$run/SHA256SUMS")" == 1 ]] || {
            echo "Hagia reference capture observations do not match the manifest: $run" >&2
            exit 1
        }
    fi
    echo "Hagia reference capture archive verified (native_acceptance=false, tab observations unverified): run=$run sophia=$source_commit hagia=$hagia_commit"
    exit 0
fi
"$ROOT_DIR/tools/verify_hagia_native_session.sh" \
    "$run/session.log" "$proof_text" >/dev/null

echo "Hagia native session archive verified: run=$run sophia=$source_commit hagia=$hagia_commit"

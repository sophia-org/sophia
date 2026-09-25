"""Exercise production proof preflights without builds, TTYs, or DRM takeover."""
from pathlib import Path
import os
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


def block(filename, start, end=None):
    source = (ROOT / "tools" / filename).read_text()
    if source.count(start) != 1:
        raise AssertionError(f"ambiguous preflight boundary in {filename}: {start!r}")
    beginning = source.index(start)
    finish = source.index(end, beginning) if end is not None else len(source)
    return source[beginning:finish]


FRAME = "run_frame_fed_output_gate_tty4.sh"
POLICY = "run_current_hagia_policy_gate_tty4.sh"
CRITICAL = "run_current_critical_path_tty4.sh"

# Like the Rust session_application_arguments tests, extract production sections
# rather than copy their decisions. No hardware or build section is evaluated.
PREFLIGHTS = {
    "frame": "\n".join([
        block(FRAME, "refuse() {\n", '[[ "${SOPHIA_FRAME_FED_OUTPUT_ARM'),
        block(FRAME, "verify_repo() {\n", "check_reference_connectors() {"),
        "PHASE=after",
        block(FRAME, 'verify_repo "$ROOT_DIR" Sophia\nverify_repo "$HAGIA_ROOT" Hagia\n[[',
              'sophia_sha256='),
    ]),
    "critical": "\n".join([
        'sophia_commit="$(git -C "$ROOT_DIR" rev-parse HEAD)"',
        'hagia_commit="$(git -C "$HAGIA_ROOT" rev-parse HEAD)"',
        block(CRITICAL, "verify_identity() {\n", "connected_connectors() {"),
        "verify_identity",
        "PHASE=after",
        "verify_identity",
    ]),
    "policy": "\n".join([
        block(POLICY, 'if [[ ! -d "$HAGIA_ROOT/.git" ]]', 'hagia_bin='),
        "PHASE=after",
        block(POLICY, 'if [[ -n "$(git -C "$ROOT_DIR" status --short)" \\\n',
              'sophia_bin='),
    ]),
    "reporter": block("check_proof_preconditions.sh", "status=0\n"),
}

GIT_RESPONSES = r'''
set -euo pipefail
PHASE=before
git() {
    local repo="$2" fault=
    [[ "$1" == -C ]] || return 98
    shift 2
    if [[ "$repo" == "$BAD_REPO" && "$PHASE" == "$FAULT_PHASE" ]]; then
        fault="$FAULT"
    fi
    case "$*" in
        'status --short'|'status --porcelain --untracked-files=all')
            case "$fault" in
                dirty) printf ' M tracked\n' ;;
                untracked) printf '?? untracked\n' ;;
            esac
            ;;
        'rev-parse HEAD')
            if [[ "$fault" == drift ]]; then
                printf '%040d\n' 2
            else
                printf '%040d\n' 1
            fi
            ;;
        'rev-parse --verify refs/remotes/origin/master')
            case "$UPSTREAM" in
                matching) printf '%040d\n' 1 ;;
                divergent) printf '%040d\n' 3 ;;
                missing) return 1 ;;
            esac
            ;;
        verify-commit\ *)
            [[ "$fault" != unsigned ]]
            ;;
        *)
            printf '%s\n' "$*" >>"$UNEXPECTED_GIT"
            return 98
            ;;
    esac
}
'''


class PhysicalGateIdentity(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="sophia-proof-identity-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.repos = {name: self.directory / name for name in ("Sophia", "Hagia", "Narthex")}
        for repo in self.repos.values():
            (repo / ".git").mkdir(parents=True)

    def run_preflight(self, gate, repo="Sophia", fault="", phase="before", upstream="missing"):
        unexpected = self.directory / "unexpected-git"
        environment = {
            "PATH": os.environ["PATH"],
            "ROOT_DIR": str(self.repos["Sophia"]),
            "HAGIA_ROOT": str(self.repos["Hagia"]),
            "NARTHEX_ROOT": str(self.repos["Narthex"]),
            "BAD_REPO": str(self.repos[repo]),
            "FAULT": fault,
            "FAULT_PHASE": phase,
            "UPSTREAM": upstream,
            "UNEXPECTED_GIT": str(unexpected),
        }
        result = subprocess.run(
            ["bash", "-c", GIT_RESPONSES + PREFLIGHTS[gate]],
            env=environment, stdin=subprocess.DEVNULL, capture_output=True,
            text=True, timeout=5, check=False,
        )
        self.assertFalse(unexpected.exists(), unexpected.read_text() if unexpected.exists() else "")
        return result

    def test_signed_clean_commits_accept_every_upstream_state(self):
        for gate in PREFLIGHTS:
            for upstream in ("matching", "missing", "divergent"):
                with self.subTest(gate=gate, upstream=upstream):
                    result = self.run_preflight(gate, upstream=upstream)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    if gate == "reporter":
                        self.assertIn("status=ready repositories=3", result.stdout)
                        self.assertIn({"matching": "upstream=ok", "missing": "NO origin/master",
                                       "divergent": "AHEAD/BEHIND origin/master"}[upstream], result.stdout)

    def test_dirty_untracked_and_unsigned_repositories_are_refused(self):
        for gate in PREFLIGHTS:
            repos = ("Sophia", "Hagia", "Narthex") if gate in ("policy", "reporter") else ("Sophia", "Hagia")
            for repo in repos:
                for fault in ("dirty", "untracked", "unsigned"):
                    with self.subTest(gate=gate, repo=repo, fault=fault):
                        result = self.run_preflight(gate, repo=repo, fault=fault)
                        self.assertNotEqual(result.returncode, 0)
                        self.assertNotIn("status=ready", result.stdout)
                        self.assertRegex(result.stdout + result.stderr, "clean|changed|signature|DIRTY|UNSIGNED")

    def test_source_changes_between_checks_are_refused(self):
        for gate in ("frame", "critical", "policy"):
            repos = ("Sophia", "Hagia", "Narthex") if gate == "policy" else ("Sophia", "Hagia")
            for repo in repos:
                for fault in ("dirty", "untracked", "drift"):
                    with self.subTest(gate=gate, repo=repo, fault=fault):
                        result = self.run_preflight(gate, repo=repo, fault=fault, phase="after")
                        self.assertNotEqual(result.returncode, 0)
                        self.assertRegex(result.stderr, "clean|changed")

    def test_reporter_refuses_missing_checkout(self):
        for repo in self.repos:
            with self.subTest(repo=repo):
                git_dir = self.repos[repo] / ".git"
                git_dir.rmdir()
                result = self.run_preflight("reporter")
                git_dir.mkdir()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("MISSING checkout", result.stdout)
                self.assertNotIn("status=ready", result.stdout)


if __name__ == "__main__":
    unittest.main()

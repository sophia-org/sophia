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
        (ROOT / "tools/lib/proof_checkout.sh").read_text(),
        block(POLICY, 'if ! proof_checkout_root "$HAGIA_ROOT"', 'hagia_bin='),
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
        'rev-parse --show-toplevel')
            [[ "$fault" != missing ]] && printf '%s\n' "$repo"
            ;;
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
        for name, repo in self.repos.items():
            (repo / ".git").mkdir(parents=True)
            self.repos[name] = repo.resolve()

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



# Real repositories and scripts, with only what a headless run cannot have
# replaced: signing (verify-commit), the tty4 check, the Nim and Cargo builds,
# and everything from the session start on. Nothing here opens a device.
REAL_GIT = subprocess.run(["bash", "-c", "command -v git"], capture_output=True,
                          text=True, check=True).stdout.strip()
GIT_ENV = {"GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1",
           "GIT_AUTHOR_NAME": "proof", "GIT_AUTHOR_EMAIL": "proof@example.invalid",
           "GIT_COMMITTER_NAME": "proof", "GIT_COMMITTER_EMAIL": "proof@example.invalid"}
PREDICATES = (ROOT / "tools/lib/proof_checkout.sh").read_text()
COPIED = (
    "tools/run_current_hagia_native_gate_tty4.sh",
    "tools/run_current_hagia_policy_gate_tty4.sh",
    "tools/hagia_native_session_gate.sh",
    "tools/hagia_policy_physical_gate.sh",
    "tools/lib/proof_checkout.sh",
    "tools/fixtures/t018_tab_reference.kdl",
)


def executable(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    path.chmod(0o755)


def git(repo, *args):
    subprocess.run([REAL_GIT, "-C", str(repo), *args], check=True, capture_output=True,
                   env={**os.environ, **GIT_ENV})


def commit_tree(repo, files):
    repo.mkdir(parents=True, exist_ok=True)
    git(repo, "init", "-q", "-b", "master")
    for relative, (text, mode) in files.items():
        path = repo / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        path.chmod(mode)
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "-m", "fixture")
    return repo.resolve()


class ProofCheckoutPredicates(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="sophia-proof-checkout-")
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name).resolve()
        self.repo = commit_tree(self.directory / "repo", {
            "profile.kdl": ("schema 1\n", 0o644), "sub/inner.txt": ("x\n", 0o644)})

    def predicate(self, call):
        return subprocess.run(["bash", "-c", PREDICATES + call], capture_output=True,
                              text=True, env={**os.environ, **GIT_ENV}, timeout=10).returncode

    def test_checkout_root_accepts_roots_and_worktrees_only(self):
        git(self.repo, "worktree", "add", "-q", str(self.directory / "linked"))
        plain = self.directory / "plain"
        plain.mkdir()
        (self.repo / "link").symlink_to(self.repo / "sub")
        cases = {
            str(self.repo): 0,
            str(self.directory / "linked"): 0,
            str(self.repo / "sub"): 1,
            str(self.repo / "link"): 1,
            str(plain): 1,
            str(self.directory / "missing"): 1,
        }
        for path, expected in cases.items():
            with self.subTest(path=path):
                self.assertEqual(self.predicate(f' proof_checkout_root "{path}"'), expected)

    def test_tracked_file_requires_an_unmodified_file_of_a_named_root(self):
        other = commit_tree(self.directory / "other", {"profile.kdl": ("schema 1\n", 0o644)})
        (self.repo / "untracked.kdl").write_text("schema 1\n")
        call = ' proof_tracked_file "{path}" "{root}"'
        self.assertEqual(self.predicate(call.format(path=self.repo / "profile.kdl", root=self.repo)), 0)
        for path, root in (
            ("profile.kdl", self.repo),
            (self.repo / "untracked.kdl", self.repo),
            (other / "profile.kdl", self.repo),
            (self.repo / "absent.kdl", self.repo),
        ):
            with self.subTest(path=path):
                self.assertEqual(self.predicate(call.format(path=path, root=root)), 1)
        (self.repo / "profile.kdl").write_text("schema 1\n// changed\n")
        self.assertEqual(self.predicate(call.format(path=self.repo / "profile.kdl", root=self.repo)), 1)

    def test_tracked_file_refuses_a_symlink_whatever_its_target(self):
        # A tracked, clean link does not fix the bytes it points at: an
        # external target can change while git status stays empty.
        external = self.directory / "external.kdl"
        external.write_text("schema 1\n")
        (self.repo / "linked-external.kdl").symlink_to(external)
        (self.repo / "linked-internal.kdl").symlink_to(self.repo / "profile.kdl")
        (self.repo / "dirty-target.kdl").write_text("schema 1\n")
        (self.repo / "linked-dirty.kdl").symlink_to(self.repo / "dirty-target.kdl")
        git(self.repo, "add", "linked-external.kdl", "linked-internal.kdl", "linked-dirty.kdl",
            "dirty-target.kdl")
        git(self.repo, "commit", "-q", "-m", "links")
        (self.repo / "dirty-target.kdl").write_text("schema 1\n// dirty\n")
        call = ' proof_tracked_file "{path}" "{root}"'
        for name in ("linked-external.kdl", "linked-internal.kdl", "linked-dirty.kdl"):
            with self.subTest(link=name):
                self.assertEqual(self.predicate(call.format(path=self.repo / name, root=self.repo)), 1)
        external.write_text("schema 1\n// changed outside the checkout\n")
        self.assertEqual(self.predicate(call.format(path=self.repo / "linked-external.kdl",
                                                    root=self.repo)), 1)
        # The ordinary tracked, unmodified regular file is still accepted.
        self.assertEqual(self.predicate(call.format(path=self.repo / "profile.kdl", root=self.repo)), 0)


class NativeReferenceDryRun(unittest.TestCase):
    """The native wrapper and gate, end to end up to the session start."""

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="sophia-native-dry-")
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name).resolve()
        self.marks = self.directory / "marks"
        self.marks.mkdir()
        files = {relative: ((ROOT / relative).read_text(), (ROOT / relative).stat().st_mode & 0o777)
                 for relative in COPIED}
        stub = "#!/bin/sh\nprintf '%s\\n' \"$0 $*\" >>\"$MARKS/{name}\"\nexit {code}\n"
        for name, code in (("atomic_scanout_preflight.sh", 0),
                           ("live_session_persistent_hardware_proof.sh", 3)):
            files[f"tools/{name}"] = (stub.format(name=name, code=code), 0o755)
        # The runners record, then execute, the Sophia binary they were handed,
        # as the real ones do; the policy wrapper's hand-off records the digest
        # it bound.
        files["tools/start_sophia_tty3.sh"] = ("""#!/bin/sh
printf '%s\\n' "$0 $*" >>"$MARKS/start_sophia_tty3.sh"
"${SOPHIA_BIN:-$(dirname "$0")/../target/release/sophia}" runner
exit 3
""", 0o755)
        files["tools/start_sophia_hagia_policy_tty4.sh"] = ("""#!/bin/sh
printf '%s\\n' "$SOPHIA_HAGIA_PHYSICAL_SOPHIA_SHA256" >>"$MARKS/policy-bound-sophia"
exit 3
""", 0o755)
        files["tools/fixtures/hagia_native_session_guide.sh"] = ("#!/bin/sh\n", 0o755)
        files["tools/fixtures/hagia_physical_guide.sh"] = ("#!/bin/sh\n", 0o755)
        files[".gitignore"] = ("target/\n", 0o644)
        self.sophia = commit_tree(self.directory / "sophia", files)
        self.hagia = commit_tree(self.directory / "hagia-main", {
            "examples/config/default.kdl": ("schema 1\n", 0o644), "src/hagia.nim": ("\n", 0o644)})
        # Hagia is a linked worktree: .git is a file there.
        git(self.hagia, "worktree", "add", "-q", str(self.directory / "hagia"))
        self.hagia = (self.directory / "hagia").resolve()
        self.narthex = commit_tree(self.directory / "narthex", {"src/narthex.nim": ("\n", 0o644)})
        self.bin = self.directory / "bin"
        executable(self.bin / "git", f"""#!/bin/bash
if [[ "$1" == -C && "$3" == verify-commit ]]; then
    [[ "$2" != "${{UNSIGNED_REPO:-}}" ]]
    exit
fi
exec {REAL_GIT} "$@"
""")
        executable(self.bin / "tty", "#!/bin/sh\necho /dev/tty4\n")
        executable(self.bin / "nim", """#!/bin/bash
echo nim >>"$MARKS/build"
for argument; do
    case "$argument" in -o:*) out="${argument#-o:}" ;; esac
done
printf '#!/bin/sh\\n# %s\\nexit 0\\n' "$out" >"$out"
chmod 755 "$out"
""")
        # Cargo honours --target-dir, then an inherited CARGO_TARGET_DIR.
        executable(self.bin / "cargo", """#!/bin/bash
echo cargo >>"$MARKS/build"
out="${CARGO_TARGET_DIR:-target}"
previous=
for argument; do
    case "$argument" in --target-dir=*) out="${argument#--target-dir=}" ;; esac
    [[ "$previous" == --target-dir ]] && out="$argument"
    previous="$argument"
done
mkdir -p "$out/release"
printf '#!/bin/sh\\n# fresh build\\necho "fresh $*" >>"$MARKS/sophia-invoked"\\nexit 0\\n' >"$out/release/sophia"
chmod 755 "$out/release/sophia"
""")
        for name in ("kitty", "browser"):
            executable(self.bin / name, "#!/bin/sh\n")

    def environment(self, **extra):
        return {
            "PATH": f"{self.bin}:{os.environ['PATH']}", "HOME": str(self.directory),
            "XDG_STATE_HOME": str(self.directory / "state"), "TMPDIR": str(self.directory),
            "MARKS": str(self.marks), "SOPHIA_HAGIA_ROOT": str(self.hagia),
            "SOPHIA_NARTHEX_ROOT": str(self.narthex),
            "SOPHIA_TERMINAL_BIN": str(self.bin / "kitty"),
            "SOPHIA_HAGIA_BROWSER_BIN": str(self.bin / "browser"),
            "SOPHIA_BROWSER_BIN": str(self.bin / "browser"),
            **GIT_ENV, **extra,
        }

    def run_script(self, script, environment, terminal=True):
        import pty
        primary, secondary = pty.openpty()
        try:
            return subprocess.run(
                ["bash", str(self.sophia / "tools" / script)], env=environment,
                stdin=secondary if terminal else subprocess.DEVNULL,
                capture_output=True, text=True, timeout=60, check=False)
        finally:
            os.close(primary)
            os.close(secondary)

    def mark(self, name):
        path = self.marks / name
        return path.read_text() if path.exists() else ""

    def test_reference_profile_reaches_the_session_start_with_every_identity_bound(self):
        profile = self.sophia / "tools/fixtures/t018_tab_reference.kdl"
        result = self.run_script("run_current_hagia_native_gate_tty4.sh",
                                 self.environment(SOPHIA_HAGIA_NATIVE_PROFILE=str(profile)))
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn("did not return cleanly (exit 3)", result.stderr)
        self.assertNotIn("unbound variable", result.stderr)
        self.assertIn("atomic_scanout_preflight.sh", self.mark("atomic_scanout_preflight.sh"))
        self.assertIn(str(self.sophia / "tools/start_sophia_tty3.sh"), self.mark("start_sophia_tty3.sh"))

    def test_profile_refusals_happen_before_any_build(self):
        # Ignored, so the tree stays clean and only the tracked-file check can
        # refuse it; an untracked file elsewhere is already refused as dirt.
        (self.sophia / "target").mkdir()
        (self.sophia / "target/untracked.kdl").write_text("schema 1\n")
        outside = self.directory / "outside.kdl"
        outside.write_text("schema 1\n")
        for profile in ("tools/fixtures/t018_tab_reference.kdl",
                        str(self.sophia / "target/untracked.kdl"), str(outside)):
            with self.subTest(profile=profile):
                result = self.run_script("run_current_hagia_native_gate_tty4.sh",
                                         self.environment(SOPHIA_HAGIA_NATIVE_PROFILE=profile))
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn("desktop profile must be", result.stderr)
                self.assertEqual(self.mark("build"), "")
                self.assertEqual(self.mark("start_sophia_tty3.sh"), "")

    def inherited_alternatives(self):
        """An inherited SOPHIA_BIN sentinel, an inherited Cargo target and a
        stale executable where the wrappers hash and run Sophia."""
        sentinel = self.directory / "alternate-sophia"
        executable(sentinel, '#!/bin/sh\necho "alternate $*" >>"$MARKS/sophia-invoked"\n')
        stale = self.sophia / "target/release/sophia"
        executable(stale, '#!/bin/sh\necho "stale $*" >>"$MARKS/sophia-invoked"\n')
        return {"SOPHIA_BIN": str(sentinel),
                "CARGO_TARGET_DIR": str(self.directory / "alternate-target")}

    def test_the_native_chain_runs_the_binary_it_built_and_bound(self):
        profile = self.sophia / "tools/fixtures/t018_tab_reference.kdl"
        result = self.run_script("run_current_hagia_native_gate_tty4.sh", self.environment(
            SOPHIA_HAGIA_NATIVE_PROFILE=str(profile), **self.inherited_alternatives()))
        self.assertEqual(result.returncode, 3, result.stderr)
        invoked = self.mark("sophia-invoked").splitlines()
        # The wrapper's config check and the runner both run Sophia; every run
        # must be the fresh, hashed build, never the stale or inherited binary.
        self.assertEqual({line.split()[0] for line in invoked}, {"fresh"}, invoked)
        self.assertEqual(invoked[-1], "fresh runner", invoked)

    def test_the_policy_wrapper_binds_the_binary_it_built(self):
        result = self.run_script("run_current_hagia_policy_gate_tty4.sh",
                                 self.environment(**self.inherited_alternatives()))
        self.assertEqual(result.returncode, 3, result.stderr)
        built = (self.sophia / "target/release/sophia").read_bytes()
        self.assertIn(b"# fresh build", built)
        self.assertEqual(self.mark("policy-bound-sophia").strip(),
                         __import__("hashlib").sha256(built).hexdigest())

    def test_a_cross_compilation_target_refuses_before_any_build(self):
        # CARGO_BUILD_TARGET moves the output below target/<triple>/, which is
        # not the executable these host-native proofs hash and run.
        profile = self.sophia / "tools/fixtures/t018_tab_reference.kdl"
        for script, extra in (("run_current_hagia_native_gate_tty4.sh",
                               {"SOPHIA_HAGIA_NATIVE_PROFILE": str(profile)}),
                              ("run_current_hagia_policy_gate_tty4.sh", {})):
            with self.subTest(script=script):
                result = self.run_script(script, self.environment(
                    CARGO_BUILD_TARGET="aarch64-unknown-linux-gnu", **extra))
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn("CARGO_BUILD_TARGET", result.stderr)
                self.assertEqual(self.mark("build"), "")

    def test_the_policy_launcher_runs_its_literal_bound_path(self):
        # Documentation control: the policy gate's launcher runs the literal
        # target/release/sophia it hashed and never reads SOPHIA_BIN, so it has
        # no inherited-binary hand-off to pin.
        launcher = (ROOT / "tools/live_session_persistent_hardware_proof.sh").read_text()
        self.assertIn('"$ROOT_DIR/target/release/sophia"', launcher)
        self.assertNotIn("SOPHIA_BIN", launcher)

    def gate_environment(self, **overrides):
        commits = {name: subprocess.run([REAL_GIT, "-C", str(root), "rev-parse", "HEAD"],
                                        capture_output=True, text=True, check=True).stdout.strip()
                   for name, root in (("sophia", self.sophia), ("hagia", self.hagia),
                                      ("narthex", self.narthex))}
        release = self.sophia / "target/release/sophia"
        executable(release, "#!/bin/sh\n# sophia\n")
        hagia_bin, narthex_bin = self.directory / "hagia-bin", self.directory / "narthex-bin"
        executable(hagia_bin, "#!/bin/sh\n# hagia\n")
        executable(narthex_bin, "#!/bin/sh\n# narthex\n")
        digest = lambda path: __import__("hashlib").sha256(path.read_bytes()).hexdigest()
        profile = self.sophia / "tools/fixtures/t018_tab_reference.kdl"
        values = {
            "SOPHIA_HAGIA_BIN": str(hagia_bin), "SOPHIA_HAGIA_SHELL_BIN": str(narthex_bin),
            "SOPHIA_DESKTOP_PROFILE": str(profile), "SOPHIA_DESKTOP_PROFILE_SHA256": digest(profile),
            "SOPHIA_HAGIA_NATIVE_ARM": "1", "SOPHIA_HAGIA_NATIVE_SEAT": "seat0",
            "SOPHIA_HAGIA_NATIVE_SOURCE_COMMIT": commits["sophia"],
            "SOPHIA_HAGIA_NATIVE_HAGIA_COMMIT": commits["hagia"],
            "SOPHIA_HAGIA_NATIVE_NARTHEX_COMMIT": commits["narthex"],
            "SOPHIA_HAGIA_NATIVE_SOPHIA_SHA256": digest(release),
            "SOPHIA_HAGIA_NATIVE_HAGIA_SHA256": digest(hagia_bin),
            "SOPHIA_HAGIA_NATIVE_NARTHEX_SHA256": digest(narthex_bin),
            "SOPHIA_HAGIA_PHYSICAL_ARM": "1", "SOPHIA_HAGIA_PHYSICAL_SEAT": "seat0",
            "SOPHIA_HAGIA_PHYSICAL_SOURCE_COMMIT": commits["sophia"],
            "SOPHIA_HAGIA_PHYSICAL_HAGIA_COMMIT": commits["hagia"],
            "SOPHIA_HAGIA_PHYSICAL_NARTHEX_COMMIT": commits["narthex"],
            "SOPHIA_HAGIA_PHYSICAL_SOPHIA_SHA256": digest(release),
            "SOPHIA_HAGIA_PHYSICAL_HAGIA_SHA256": digest(hagia_bin),
            "SOPHIA_HAGIA_PHYSICAL_NARTHEX_SHA256": digest(narthex_bin),
        }
        values.update(overrides)
        return self.environment(**values)

    def test_armed_gates_refuse_any_unbound_identity_before_the_session(self):
        for gate, start, prefix in (
            ("hagia_native_session_gate.sh", "start_sophia_tty3.sh", "SOPHIA_HAGIA_NATIVE_"),
            ("hagia_policy_physical_gate.sh", "live_session_persistent_hardware_proof.sh",
             "SOPHIA_HAGIA_PHYSICAL_"),
        ):
            accepted = self.run_script(gate, self.gate_environment(), terminal=False)
            with self.subTest(gate=gate, case="bound"):
                self.assertEqual(accepted.returncode, 3, accepted.stderr)
                self.assertNotIn("unbound variable", accepted.stderr)
                self.assertNotEqual(self.mark(start), "")
            (self.marks / start).unlink()
            for binary in ("SOPHIA_SHA256", "HAGIA_SHA256", "NARTHEX_SHA256"):
                with self.subTest(gate=gate, case=binary):
                    result = self.run_script(gate, self.gate_environment(**{prefix + binary: "0" * 64}),
                                             terminal=False)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertIn("does not match its bound physical-proof identity", result.stderr)
                    self.assertEqual(self.mark(start), "")
            for fault in ("dirty", "unsigned"):
                with self.subTest(gate=gate, case=f"narthex-{fault}"):
                    extra = {}
                    if fault == "dirty":
                        (self.narthex / "stray").write_text("x\n")
                    else:
                        extra["UNSIGNED_REPO"] = str(self.narthex)
                    result = self.run_script(gate, self.gate_environment(**extra), terminal=False)
                    (self.narthex / "stray").unlink(missing_ok=True)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertRegex(result.stderr, "changed|signature")
                    self.assertEqual(self.mark(start), "")


if __name__ == "__main__":
    unittest.main()

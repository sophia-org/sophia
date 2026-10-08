"""No installed PAM stack, logind session, GPU, or root action is used."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest


SOURCE = Path(__file__).resolve().parents[1] / "development_seat"


def module(name):
    spec = importlib.util.spec_from_file_location("development_" + name, SOURCE / f"{name}.py")
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


config, login, pam = module("config"), module("login"), module("pam")


def fixture_config():
    bundle = "/opt/sophia-development/frozen"
    names = ("bootstrap.py", "owner.py", "worker.py", "config.py", "login.py", "pam.py", "sandbox.py", "audit.py",
             "custody.py", "inventory.py", "policy.py", "elf.py", "deployment.py", "profile.kdl")
    return {"schema": 1, "user": "sophia-dev", "uid": 1200, "gid": 1200,
            "daily_uids": [1000], "seat": "seat-sophia-dev", "pci": "0000:16:00.0",
            "runtime_ms": 300000, "outer_seconds": 360, "bundle": bundle,
            "output_root": "/var/lib/sophia-development/runs", "pam_service": "sophia-development",
            "profile": bundle + "/profile.kdl",
            "tools": {name: {"path": "/usr/bin/" + name, "sha256": "0" * 64} for name in config.TOOLS},
            "files": [{"path": path, "sha256": "0" * 64} for path in
                      [*(bundle + "/" + name for name in names), "/etc/pam.d/sophia-development",
                       "/etc/polkit-1/rules.d/00-000-sophia-development.rules"]]}


class FixedConfiguration(unittest.TestCase):
    def test_valid_config_has_a_dedicated_identity_and_bounded_runtime(self):
        value = fixture_config()
        self.assertEqual(config.parse(value), value)

    def test_root_shared_user_seat0_missing_pins_and_bound_changes_refuse(self):
        for key, replacement in (("schema", True), ("uid", 0), ("gid", 0), ("uid", 1000), ("daily_uids", []),
                                 ("seat", "seat0"), ("seat", "../seat-dev"), ("runtime_ms", 300001),
                                 ("outer_seconds", 301), ("runtime_ms", True), ("pci", "card1"),
                                 ("bundle", "/home/niltempus/dev"), ("profile", "/tmp/user.kdl"),
                                 ("pam_service", "login"), ("tools", {}), ("files", [])):
            value = fixture_config()
            value[key] = replacement
            with self.subTest(key=key, value=replacement), self.assertRaises(ValueError):
                config.parse(value)

    def test_all_bundle_and_tool_hashes_are_mandatory(self):
        for name in config.TOOLS:
            value = fixture_config()
            value["tools"][name]["sha256"] = "wrong"
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "hash"):
                config.parse(value)
        value = fixture_config()
        value["files"].append(value["files"][0])
        with self.assertRaisesRegex(ValueError, "duplicate"):
            config.parse(value)

    def test_mutable_checkout_can_never_be_a_privileged_bundle(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "program"
            path.write_text("inert")
            with self.assertRaisesRegex(ValueError, "untrusted"):
                config.trusted(path, regular=True)


class LoginChecks(unittest.TestCase):
    def row(self):
        return {"seat": "seat-sophia-dev", "uid": 1200, "service": "sophia-development",
                "class": "user", "type": "wayland", "state": "active", "active": 1,
                "remote": 0, "can_tty": 0, "tty": None, "vt": None, "session": "c29",
                "process": {"pid": 10, "start_ticks": 99, "cgroup": "0::/c29\n"}}

    def test_every_login_authority_field_is_required(self):
        original = self.row()
        self.assertEqual(login.validate(original, fixture_config()), original)
        for field, wrong in (("seat", "seat0"), ("uid", 1000), ("service", "sshd"),
                             ("class", "greeter"), ("type", "tty"), ("state", "closing"),
                             ("active", 0), ("remote", 1), ("can_tty", 1), ("tty", "tty3"), ("vt", 3)):
            row = dict(original, **{field: wrong})
            with self.subTest(field=field), self.assertRaises(ValueError):
                login.validate(row, fixture_config())

    def test_a_changed_identity_between_observations_refuses(self):
        first, second = self.row(), self.row()
        second["process"]["start_ticks"] += 1
        class Fake:
            def __init__(self): self.rows = iter([first, second])
            def observe(self, pid): return next(self.rows)
        with self.assertRaisesRegex(ValueError, "changed"):
            login.attest(Fake(), fixture_config(), 10)


PAM_MODULE = r'''
#include <security/pam_appl.h>
#include <security/pam_modules.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static int record(const char *what, int argc, const char **argv) {
    if (argc < 1) return PAM_SERVICE_ERR;
    FILE *out = fopen(argv[0], "a");
    if (!out) return PAM_SERVICE_ERR;
    fprintf(out, "%s\n", what); fclose(out);
    return PAM_SUCCESS;
}
static void cleanup(pam_handle_t *p, void *data, int status) {
    (void)p; (void)status;
    const char *args[] = { data };
    record("end", 1, args);
    free(data);
}
int pam_sm_acct_mgmt(pam_handle_t *p, int flags, int argc, const char **argv) {
    (void)flags;
    const char *seat = pam_getenv(p, "XDG_SEAT");
    if (!seat || strcmp(seat, "seat-fixture")) return PAM_PERM_DENIED;
    return record("account", argc, argv);
}
int pam_sm_open_session(pam_handle_t *p, int flags, int argc, const char **argv) {
    (void)flags;
    int result = record("open", argc, argv);
    if (result == PAM_SUCCESS) {
        char *data = strdup(argv[0]);
        if (!data) return PAM_BUF_ERR;
        int stored = pam_set_data(p, "development-fixture", data, cleanup);
        if (stored != PAM_SUCCESS) { free(data); return stored; }
    }
    return argc > 1 ? PAM_SESSION_ERR : result;
}
int pam_sm_close_session(pam_handle_t *p, int flags, int argc, const char **argv) {
    (void)p; (void)flags;
    return record("close", argc, argv);
}
'''


class PrivatePam(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="development-private-pam-")
        cls.addClassCleanup(cls.temporary.cleanup)
        cls.directory = Path(cls.temporary.name)
        cls.library = str(Path("/usr/lib/libpam.so.0").resolve(strict=True))
        source = cls.directory / "fixture.c"
        source.write_text(PAM_MODULE)
        cls.plugin = cls.directory / "fixture.so"
        subprocess.run(["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", str(source),
                        "-o", str(cls.plugin), cls.library], capture_output=True, check=True, timeout=30)

    def start(self, fail_open=False):
        journal = self.directory / ("failed.log" if fail_open else "success.log")
        stack = self.directory / "private"
        stack.write_text(f"account required {self.plugin} {journal}\n"
                         f"session required {self.plugin} {journal}" + (" refuse" if fail_open else "") + "\n")
        return pam.Pam(self.library, "private", "fixture-user", {"XDG_SEAT": "seat-fixture"},
                       fixture_directory=self.directory), journal

    def test_a_private_stack_opens_and_closes_once_without_authenticating(self):
        handle, journal = self.start()
        try:
            self.assertEqual(handle.getenv("XDG_SEAT"), "seat-fixture")
            self.assertIsNone(handle.getenv("XDG_SESSION_ID"))
            handle.open()
        finally:
            handle.close()
        handle.close()
        self.assertEqual(journal.read_text().splitlines(), ["account", "open", "close", "end"])

    def test_failed_open_still_closes_partial_session_and_ends_handle(self):
        handle, journal = self.start(fail_open=True)
        with self.assertRaisesRegex(ValueError, "pam_open_session"):
            handle.open()
        self.assertIsNone(handle.handle.value)
        # Close may report an error after a failed open. pam_end must still
        # run the registered cleanup; both failures remain in the exception.
        self.assertEqual(journal.read_text().splitlines(), ["account", "open", "close", "end"])


if __name__ == "__main__":
    unittest.main()

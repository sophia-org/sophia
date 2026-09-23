"""Source-level schema tripwire, not a shell/Rust parser or evidence verifier.

Only reviewed message identities and expression forms are understood. A new
emitter form or reader needs review instead of disappearing from coverage.
End-to-end verifier fixtures own field semantics and historical compatibility.
"""
from pathlib import Path
import re
import sys

SESSION = 'sophia_live_session'
WM = 'sophia_live_wm'
SINGLE_SCHEMA = ('sophia_live_native_resources', 'sophia_live_rendering_efficiency')

# Every session completion consumer has one purpose. Archive exceptions name
# exact files, rather than exempting a directory that might acquire live gates.
PROOF_READERS = (
    'report_sophia_glxgears_performance.sh',
    'report_sophia_rendering_performance.sh',
    'verify_frame_fed_output_evidence.sh',
    'verify_hagia_native_session.sh',
    'verify_hagia_policy_physical.sh',
    'verify_installed_fallback_session.sh',
    'verify_installed_login_cycle.sh',
    'verify_installed_native_chrome_session.sh',
    'verify_installed_truecolor_session.sh',
    'verify_installed_xterm_session.sh',
    'verify_keyboard_independence_physical.sh',
    'verify_live_session_milestone4_evidence.sh',
    'verify_live_session_persistent_evidence.sh',
    'verify_mirror_group_physical.sh',
    'verify_qemu_session_evidence.sh',
    'verify_sophia_firefox_physical.sh',
    'verify_sophia_native_chrome.sh',
    'verify_sophia_standalone_vkcube.sh',
)
NORMAL_READERS = (
    # Frontend pixel checks plus a nonempty-scene witness; no startup-proof claim.
    'run_gtk_redraw_probe.py',
    'run_qt_popup_probe.py',
    'report_sophia_terminal_performance.sh',
    'verify_installed_hagia_session.sh',
    'verify_qemu_emergency_recovery_evidence.sh',
    'verify_qemu_xtest_selection_evidence.sh',
)
ARCHIVE_READERS = {
    'verify_live_session_two_xterm_evidence.sh': 'historical two-xterm startup/CPU budgets',
    'verify_live_session_milestone3_evidence.sh': 'historical paired namespace proof',
}
# These consumers parse a completion before testing its schema. Match their
# actual acceptance condition, not an unrelated per-field version branch.
PARSED_READERS = {
    'verify_live_session_milestone4_evidence.sh': r'^\s*\[\[ "\$\{observed\[schema\]\}" =~ \^(\([0-9|]+\))\$ \]\]',
    'verify_live_session_persistent_evidence.sh': r'^\s*\[\[ "\$\{observed\[schema\]\}" =~ \^(\([0-9|]+\))\$ \]\]',
    'verify_qemu_session_evidence.sh': r'^if \[\[ ! "\$completion_schema" =~ \^(\([0-9|]+\))\$ \]\]',
}
FIELD_ONLY_READERS = {
    'verify_qemu_emergency_recovery_evidence.sh': 'cleanup fields, no startup-proof claim',
    'verify_qemu_xtest_selection_evidence.sh': 'one bounded completion, no startup-proof claim',
}
WM_READERS = ('verify_hagia_native_session.sh', 'verify_sophia_firefox_physical.sh')
RUST_PROOF_READER = 'crates/sophia-conformance/src/direct_scanout.rs'
TOKEN = r'''([^\s\\/'"]+)'''


def emitter_schemas(sources):
    """Return all literal versions and the two reviewed completion branches."""
    versions = {name: set() for name in (*SINGLE_SCHEMA, WM)}
    completions = []
    for path, source in sources.items():
        for name in SINGLE_SCHEMA:
            versions[name].update(map(int, re.findall(re.escape(name) + r' schema=(\d+)', source)))
        versions[WM].update(map(int, re.findall(WM + r' schema=(\d+) status=ready(?:\s|\\|"|$)', source)))
        for match in re.finditer(SESSION + r' schema=([^ ]+) status=bounded_complete ', source):
            if match.group(1) != '{}':
                raise ValueError(f'{path}: unexpected completion emitter; review its proof semantics')
            # The expression is the first argument after the record's format
            # string. Do not fish for two unrelated numbers later in the file.
            tail = source[match.end():]
            branch = re.match(r'[^"\n]*",\s*if startup_proof_requested\s*\{\s*(\d+)\s*\}\s*else\s*\{\s*(\d+)\s*\}\s*,', tail)
            if branch is None:
                raise ValueError(f'{path}: cannot resolve conditional completion schema')
            completions.append(tuple(map(int, branch.groups())))
    if len(completions) != 1 or completions[0][0] == completions[0][1]:
        raise ValueError('expected one completion emitter with distinct proof and normal schemas')
    for name, values in versions.items():
        if not values:
            raise ValueError(f'no emitter found for {name}')
    if len(versions[WM]) != 1:
        raise ValueError('WM readiness emitters disagree on schema')
    return {name: max(values) for name, values in versions.items()}, completions[0]


def schema_patterns(source, name, status=None):
    expression = re.escape(name) + r' schema=' + TOKEN
    if status is not None:
        expression += r' status=' + re.escape(status) + r'(?=\s|\\|\$|[\'"\)])'
    return [(source.count('\n', 0, m.start()) + 1, m.group(1))
            for m in re.finditer(expression, source)]


def compatible(pattern, version):
    return re.fullmatch(pattern, str(version)) is not None


def check_readers(readers, versions, completion):
    failures = []
    checked = 0
    proof, normal = completion
    inventory = {name: (proof,) for name in PROOF_READERS}
    inventory.update({name: (proof, normal) for name in NORMAL_READERS})
    inventory.update({name: () for name in ARCHIVE_READERS})

    def check_patterns(path, patterns, required, proof_only=False):
        nonlocal checked
        if not patterns:
            failures.append(f'{path}: cannot locate reviewed schema acceptance')
        if proof_only and patterns:
            try:
                if all(compatible(pattern, normal) for _, pattern in patterns):
                    failures.append(f'{path}: proof reader accepts the unrequested-proof schema {normal}')
            except re.error:
                pass  # The per-pattern diagnostic below names the malformed regex.
        for line, pattern in patterns:
            checked += 1
            try:
                missing = [v for v in required if not compatible(pattern, v)]
            except re.error:
                failures.append(f'{path}:{line}: unsupported schema regex {pattern}')
                continue
            if missing:
                failures.append(f'{path}:{line}: schema={pattern} cannot read emitted schema(s) {missing}')

    seen_session = set()
    seen_wm = set()
    for path, source in readers.items():
        name = Path(path).name
        for record in SINGLE_SCHEMA:
            patterns = schema_patterns(source, record)
            if patterns:
                check_patterns(path, patterns, (versions[record],))
        # The Rust reader selects the record before examining its fields.
        if path == RUST_PROOF_READER:
            check_patterns(path, schema_patterns(source, SESSION), (proof,), proof_only=True)
            continue
        if SESSION in source and 'status=bounded_complete' in source:
            seen_session.add(name)
            if path != 'tools/' + name or name not in inventory:
                failures.append(f'{path}: completion reader has no reviewed purpose')
            elif name in ARCHIVE_READERS:
                if '# Archive-only:' not in source:
                    failures.append(f'{path}: archive exception lacks its archive-only declaration')
            elif name in PARSED_READERS:
                found = list(re.finditer(PARSED_READERS[name], source, re.MULTILINE))
                if len(found) != 1:
                    failures.append(f'{path}: expected one reviewed parsed schema check')
                else:
                    check_patterns(path, [(source.count('\n', 0, found[0].start()) + 1, found[0].group(1))], inventory[name], proof_only=True)
            elif name in FIELD_ONLY_READERS:
                if not re.search(SESSION + r' \.\*status=bounded_complete ', source):
                    failures.append(f'{path}: schema-independent completion selector changed')
                if schema_patterns(source, SESSION, 'bounded_complete'):
                    failures.append(f'{path}: field-only reader acquired a schema restriction')
            else:
                check_patterns(path, schema_patterns(source, SESSION, 'bounded_complete'), inventory[name], proof_only=name in PROOF_READERS)
        patterns = schema_patterns(source, WM, 'ready')
        if patterns:
            seen_wm.add(name)
            if path != 'tools/' + name or name not in WM_READERS:
                failures.append(f'{path}: WM readiness reader has no reviewed purpose')
            else:
                # Firefox has two complete compatibility branches. A generic
                # record selector is not evidence that either branch accepts it.
                literal = [(line, pat) for line, pat in patterns if pat.isdecimal()]
                checked += 1
                if not any(int(pat) == versions[WM] for _, pat in literal):
                    failures.append(f'{path}: no readiness branch accepts schema {versions[WM]}')
    for missing in sorted(set(inventory) - seen_session):
        failures.append(f'tools/{missing}: registered completion reader disappeared')
    for missing in sorted(set(WM_READERS) - seen_wm):
        failures.append(f'tools/{missing}: registered readiness reader disappeared')
    if RUST_PROOF_READER not in readers:
        failures.append(f'{RUST_PROOF_READER}: registered completion reader disappeared')
    return checked, failures


def audit(root):
    sources = {str(p.relative_to(root)): p.read_text() for p in (root / 'crates').rglob('*.rs')
               if 'src' in p.parts and 'target' not in p.parts}
    versions, completion = emitter_schemas(sources)
    readers = {str(p.relative_to(root)): p.read_text() for p in (root / 'tools').rglob('*')
               if p.suffix in ('.sh', '.py') and 'fixtures' not in p.parts
               and 'tests' not in p.parts and not p.name.startswith('check_')}
    rust = root / RUST_PROOF_READER
    if rust.exists():
        readers[RUST_PROOF_READER] = rust.read_text()
    return check_readers(readers, versions, completion)


def main():
    if len(sys.argv) != 1:
        raise ValueError('usage: check_live_record_schema_readers.sh [--self-test]')
    count, failures = audit(Path(__file__).resolve().parent.parent)
    for failure in failures:
        print(failure, file=sys.stderr)
    print(f'checked {count} schema acceptance sites across four message identities; '
          f'{len(ARCHIVE_READERS)} historical readers retained')
    return bool(failures)


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError, OSError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)

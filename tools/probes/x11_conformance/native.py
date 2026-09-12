#!/usr/bin/env python3
"""Execute named native obligations; zero matching Rust tests is NORESULT."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

from run import ROOT, HERE, bounded, clean_environment, digest


def verdict(status, output, test):
    if status == 124:
        return {'status': 'TIMEOUT', 'detail': 'native test absolute process deadline'}
    if status != 0:
        return {'status': 'FAIL', 'detail': f'cargo test exit {status}'}
    named = re.findall(r'^test (\S+) \.\.\. (\S+)\s*$', output, re.MULTILINE)
    summaries = re.findall(
        r'^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; '
        r'(\d+) measured; (\d+) filtered out; finished in [^\n]+$', output, re.MULTILINE)
    if named != [(test, 'ok')] or len(summaries) != 1:
        return {'status': 'NORESULT', 'detail': 'exact named execution and one summary required'}
    if summaries[0][:5] != ('ok', '1', '0', '0', '0'):
        return {'status': 'NORESULT', 'detail': 'one passed, zero ignored native tests required'}
    return {'status': 'PASS'}


def load(path):
    manifest = json.loads(path.read_text())
    obligations = manifest['obligations']
    ids = [item['id'] for item in obligations]
    if manifest['schema'] != 1 or not ids or len(ids) != len(set(ids)):
        raise ValueError('empty, duplicate or unsupported native obligations')
    for item in obligations:
        if item.get('mandatory') is not True:
            raise ValueError('native obligations may not be silently optional')
        implementation = item.get('implementation')
        if implementation is not None:
            if set(implementation) != {'package', 'target', 'test'}:
                raise ValueError('native implementation requires package, target and exact test')
            if not all(re.fullmatch(r'[A-Za-z0-9_:-]+', implementation[key])
                       for key in ('package', 'test')):
                raise ValueError('invalid native implementation identity')
            target = implementation['target']
            if target != {'kind': 'lib'} and not (
                    set(target) == {'kind', 'name'} and target['kind'] == 'test'
                    and re.fullmatch(r'[A-Za-z0-9_-]+', target['name'])):
                raise ValueError('native target must be lib or a named integration test')
    return manifest


def test_command(implementation):
    target = implementation['target']
    flags = ['--lib'] if target['kind'] == 'lib' else ['--test', target['name']]
    return ['cargo', 'test', '--offline', '-p', implementation['package'], *flags,
            implementation['test'], '--', '--exact', '--test-threads=1']


def evaluate(manifest, results):
    required = {item['id'] for item in manifest['obligations']}
    seen, failures = set(), []
    for result in results:
        name = result['case']
        if name not in required:
            failures.append(f'foreign native obligation: {name}')
        if name in seen:
            failures.append(f'duplicate native obligation: {name}')
        seen.add(name)
        if result['status'] != 'PASS':
            failures.append(f'{name}: {result["status"]}: {result.get("detail", "")}')
    failures.extend(f'{name}: MISSING' for name in sorted(required - seen))
    return {'status': 'FAIL' if failures else 'PASS', 'required': len(required),
            'executed': len(seen), 'failures': failures, 'results': results}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, default=HERE / 'native_manifest.json')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--target-dir', type=Path, required=True)
    parser.add_argument('--timeout', type=float, default=600)
    args = parser.parse_args()
    if not 0 < args.timeout <= 1800:
        parser.error('timeout must be in (0, 1800]')
    manifest = load(args.manifest)
    args.output.mkdir(parents=True, exist_ok=False)
    environment = clean_environment()
    environment['CARGO_TARGET_DIR'] = str(args.target_dir.resolve())
    results = []
    for item in manifest['obligations']:
        implementation = item.get('implementation')
        if implementation is None:
            result = {'status': 'NORESULT', 'detail': 'mandatory obligation has no implementation'}
        else:
            command = test_command(implementation)
            status, stdout, stderr = bounded(command, args.timeout, cwd=ROOT, env=environment,
                                             stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            log = stdout + stderr
            (args.output / f'{item["id"]}.log').write_bytes(log)
            result = verdict(status, stdout.decode(errors='replace'), implementation['test'])
            result['command'] = command
            result['output_sha256'] = digest(args.output / f'{item["id"]}.log')
        result['case'] = item['id']
        results.append(result)
        print(f'{item["id"]}: {result["status"]}', flush=True)
    report = evaluate(manifest, results)
    report['manifest_sha256'] = digest(args.manifest)
    report['source_commit'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT,
                                                     text=True).strip()
    report['source_dirty'] = bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT))
    report['scope'] = 'Native unit/headless evidence; no hardware or physical acceptance.'
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    return 0 if report['status'] == 'PASS' else 1


if __name__ == '__main__':
    raise SystemExit(main())

"""Compatibility adapter to the existing, kernel-validated isolation boundary.

All M3 build, inventory, process, evidence and verdict logic belongs to xtask.
This adapter only calls the existing launcher/entry validator and execs xtask.
Remove it when that established namespace boundary is migrated to Rust.
"""
import json
import os
from pathlib import Path
import sys

if sys.argv[1] == '--enter':
    sys.path.insert(0, '/work/isolation')
    import isolation
    delegated = isolation.validate_entry(int(sys.argv[2]))
    record = {'validated': True, 'namespaces': isolation.namespace_ids(),
              'delegated_descriptors': len(delegated),
              'render_devices_present': Path('/dev/dri').exists(),
              'input_devices_present': Path('/dev/input').exists()}
    Path('/work/evidence/containment.json').write_text(json.dumps(record) + '\n')
    os.execv('/work/xtask', ['xtask', 'check', 'm3-acceptance', '--contained'])
else:
    plan = json.loads(Path(sys.argv[1]).read_text())
    sys.path.insert(0, plan['isolation_directory'])
    import isolation
    result = isolation.launch(
        ['/usr/bin/python3', '-B', '/work/containment.py', '--enter', '{activation_fd}'],
        mounts=[isolation.Mount(Path(row['source']), row['destination'], row['writable'])
                for row in plan['mounts']], timeout=plan['timeout'], bwrap=plan['bwrap'])
    sys.stdout.buffer.write(result.stdout)
    sys.stderr.buffer.write(result.stderr)
    raise SystemExit(result.returncode)

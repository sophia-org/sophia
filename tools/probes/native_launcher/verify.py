#!/usr/bin/env python3
"""Strict first-run transcript checks, not visual/focus or latency acceptance."""
import json
from pathlib import Path
import sys

U64 = (1 << 64) - 1


class Invalid(ValueError):
    pass


def number(fields, key, low=1, high=U64):
    value = fields.get(key, '')
    if not value or not value.isascii() or not value.isdecimal():
        raise Invalid(f'missing/invalid {key}')
    result = int(value)
    if not low <= result <= high:
        raise Invalid(f'out-of-range {key}')
    return result


def grant(fields):
    return number(fields, 'connection_epoch'), number(fields, 'content_grant_epoch')


def decode(text):
    if len(text.encode()) > 64 * 1024 * 1024:
        raise Invalid('log exceeds bound')
    lines = text.splitlines()
    if len(lines) > 100_000:
        raise Invalid('record bound exceeded')
    for line in lines:
        if '\t' in line:
            parts = line.split('\t')
            if len(parts) != 4:
                raise Invalid('invalid structured record')
            line = parts[3]
        words = line.split()
        if not words:
            continue
        fields = {}
        for field in words[1:]:
            if '=' not in field:
                continue
            key, value = field.split('=', 1)
            if key in fields:
                raise Invalid('duplicate evidence field')
            fields[key] = value
        yield words[0], fields


def verify(text):
    roles, facts, presented, slots = {}, {}, {}, {}
    catalog = committed = launched = shutdown = 0
    for name, fields in decode(text):
        status = fields.get('status')
        if shutdown and name in {'sophia_shell_component', 'sophia_live_shell_content',
                                 'sophia_shell_component_catalog', 'sophia_native_launcher'}:
            raise Invalid('component work after quiescent shutdown')
        if 'runtime_fatal' in name or 'failure_code' in fields or status in {
            'transport_failed', 'presentation_failed', 'deadline_exceeded', 'retained',
        }:
            raise Invalid('runtime failure or retained shutdown owner')
        if name == 'sophia_shell_component':
            if number(fields, 'schema') != 1:
                raise Invalid('component schema')
            if status == 'process_retired':
                if fields.get('endpoint_released') != 'true' or grant(fields) not in roles.values():
                    raise Invalid('unknown/unreleased component retirement')
                role = next(r for r, g in roles.items() if g == grant(fields))
                if number(fields, 'slot', 0, 1) != slots[role]:
                    raise Invalid('retirement slot identity mismatch')
                if set(fields) != {'schema', 'status', 'slot', 'connection_epoch', 'content_grant_epoch', 'endpoint_released'}:
                    raise Invalid('retirement fields differ')
                continue
            if status != 'negotiated':
                raise Invalid('component failed or refused service')
            role = fields.get('role')
            if role not in {'bar', 'application_launcher'} or role in roles:
                raise Invalid('missing role or restarted component')
            expected = {'schema', 'status', 'role', 'slot', 'revision', 'gpu_mode',
                        'gpu_grant_epoch', 'device_major', 'device_minor',
                        'connection_epoch', 'content_grant_epoch'}
            if set(fields) != expected:
                raise Invalid('component fields differ')
            slot = number(fields, 'slot', 0, 1)
            identity = grant(fields)
            if identity in roles.values() or any(identity[i] == g[i] for g in roles.values() for i in (0, 1)):
                raise Invalid('component identities alias')
            if any(slot == v for v in slots.values()):
                raise Invalid('component slots alias')
            revision = number(fields, 'revision', 1, 65535)
            gpu = number(fields, 'gpu_grant_epoch', 0)
            major = number(fields, 'device_major', 0, (1 << 32) - 1)
            minor = number(fields, 'device_minor', 0, (1 << 32) - 1)
            if role == 'bar':
                if revision != 6 or fields['gpu_mode'] != 'direct' or gpu != identity[0] or major == 0:
                    raise Invalid('bar has no exact direct grant')
            elif revision != 7 or fields['gpu_mode'] != 'denied' or (gpu, major, minor) != (0, 0, 0):
                raise Invalid('launcher permission/revision mismatch')
            roles[role] = identity
            slots[role] = slot
        elif name == 'sophia_live_wm_configuration':
            if number(fields, 'schema') != 2 or status != 'committed':
                raise Invalid('WM configuration rejected')
            committed += 1
        elif name == 'sophia_shell_component_catalog':
            if number(fields, 'schema') != 1 or status != 'built' or number(fields, 'generation') != 1:
                raise Invalid('invalid catalog publication')
            number(fields, 'entries')
            catalog += 1
        elif name == 'sophia_live_shell_content':
            if number(fields, 'schema') != 1 or status not in {'outputs', 'prepared', 'presented'}:
                raise Invalid('content service failed')
            if status == 'prepared':
                continue
            identity = grant(fields)
            if identity not in roles.values():
                raise Invalid('content from unknown grant')
            if status == 'outputs':
                number(fields, 'facts_generation')
                if number(fields, 'outputs') != 2:
                    raise Invalid('two-output smoke requires exactly two outputs')
                facts[identity] = 2
            else:
                output = number(fields, 'output')
                candidate = number(fields, 'candidate_generation')
                number(fields, 'presentation_epoch')
                presented.setdefault(identity, {}).setdefault(output, set()).add(candidate)
        elif name == 'sophia_native_launcher':
            if number(fields, 'schema') != 1 or status != 'process_started':
                raise Invalid('launcher operation failed or expired')
            number(fields, 'transaction')
            launched += 1
        elif name == 'sophia_shell_components_shutdown':
            if fields != {'schema': '1', 'status': 'quiescent'}:
                raise Invalid('component accounting not quiescent')
            if len(roles) != 2 or launched < 1:
                raise Invalid('shutdown precedes workload')
            shutdown += 1
    if len(roles) != 2 or catalog != 1 or committed != 1 or launched != 1 or shutdown != 1:
        raise Invalid('missing/repeated required lifecycle evidence')
    for role, identity in roles.items():
        outputs = presented.get(identity, {})
        if facts.get(identity) != 2 or len(outputs) != 2 or any(len(v) < 2 for v in outputs.values()):
            raise Invalid(f'{role} did not present two generations on each output')
    if set(presented[roles['bar']]) != set(presented[roles['application_launcher']]):
        raise Invalid('bar and launcher output coverage differs')
    return {'status': 'pass', 'scope': 'native-launcher-smoke-transcript',
            'outputs': 2, 'started_processes': launched,
            'visual_focus_acceptance': 'OPERATOR_REQUIRED', 'latency_acceptance': 'NOT_RUN'}


if __name__ == '__main__':
    try:
        if len(sys.argv) != 2:
            raise Invalid('usage: verify.py HOST_SESSION_LOG')
        path = Path(sys.argv[1])
        if path.stat().st_size > 64 * 1024 * 1024:
            raise Invalid('log exceeds bound')
        result = verify(path.read_text())
    except (Invalid, OSError, UnicodeError) as error:
        print(json.dumps({'status': 'fail', 'reason': str(error)}))
        sys.exit(1)
    print(json.dumps(result))

"""Coverage inventory audit, not a source-derived behavioral oracle.

Wire assertions use their own specification constants. This separate check
only prevents newly declared requests disappearing from the coverage ledger.
"""
import re


def declared_core_requests(root):
    directory = root / 'crates/sophia-x-authority/src'
    # Every constants file the wire keeps, not the one it had when this was
    # written: the decoder's arms name constants from all of them, and a
    # constant that moved to its own file is not a request that vanished.
    constants = {}
    for path in sorted((directory / 'wire').glob('constants*.rs')):
        constants.update((name, int(value)) for name, value in re.findall(
            r'const (X_\w+): u8 = (\d+);', path.read_text()))
    decoder = (directory / 'wire.rs').read_text().split('pub fn decode_x11_core_request(', 1)[1]
    arms = re.findall(r'^        (X_\w+) =>', decoder, re.MULTILINE)
    if len(arms) < 50:
        raise ValueError('decoder shape changed; audit the inventory reader')
    return {str(constants[name]): name for name in arms if constants[name] < 128}


def check_inventory(root, manifest):
    declared = declared_core_requests(root)
    inventory = manifest['core_request_inventory']
    missing = set(declared) - set(inventory)
    if missing:
        raise ValueError(f'new decoded core requests lack manifest coverage/debt entries: {missing}')
    ids = {case['id'] for case in manifest['cases']}
    for opcode, row in inventory.items():
        if not row['cases'] and not row.get('coverage_debt'):
            raise ValueError(f'core request {opcode} has neither cases nor explicit debt')
        if not set(row['cases']) <= ids:
            raise ValueError(f'core request {opcode} references unknown case')
    return {'declared_core_requests': len(declared),
            'declared_with_mandatory_cases': sum(bool(inventory[key]['cases']) for key in declared),
            'coverage_debt': [inventory[key]['name'] for key in declared if not inventory[key]['cases']]}

"""Externally observed obligations, independent of Sophia dispatch internals."""
import socket
import time
from wire import Client
from drawing_cases import CASES as DRAWING_CASES


def client(context):
    return Client(context['socket'], context['order'], context['deadline'])


def peer_client(context):
    order = '>' if context['order'] == '<' else '<'
    return Client(context['socket'], order, context['deadline'])


def setup(context):
    with client(context) as a, peer_client(context) as b:
        assert a.base != b.base and a.mask == b.mask
        assert a.root == b.root
        a.sync()
        b.sync()


def setup_containment(context):
    with client(context) as healthy:
        marker = healthy.window()
        prefix = bytes([ord('l' if context['order'] == '<' else 'B'), 0])
        valid = prefix + healthy.pack('HHHHH', 11, 0, 0, 0, 0)
        auth = prefix + healthy.pack('HHHHH', 11, 0, 3, 5, 0)
        auth_body = b'abc' + bytes(1) + b'12345' + bytes(3)
        payloads = {
            'setup_empty': [b''],
            'setup_truncated_prefix': [valid[:n] for n in range(1, 12)],
            # Include truncation in the name, its padding, data and its padding.
            'setup_truncated_auth': [auth + auth_body[:n] for n in range(12)],
            'setup_invalid_order': [b'?' + valid[1:]],
            'setup_version_containment': [prefix + healthy.pack('HHHHH', 12, 0, 0, 0, 0)],
        }[context['case']]
        for payload in payloads:
            with socket.socket(socket.AF_UNIX) as peer:
                peer.settimeout(healthy.remaining())
                peer.connect(str(context['socket']))
                peer.sendall(payload)
                # EOF at the server, while retaining the read half to observe
                # its close. A successful send alone is not an ordering barrier.
                peer.shutdown(socket.SHUT_WR)
                response = bytearray()
                while True:
                    peer.settimeout(healthy.remaining())
                    try:
                        part = peer.recv(4096)
                    except ConnectionResetError:
                        break
                    if not part:
                        break
                    response.extend(part)
                    assert len(response) <= 4096, 'unbounded setup refusal'
                assert not response or response[0] == 0, 'invalid setup was accepted'
            # Existing state survives; a fresh client must also be admitted.
            assert healthy.u16(healthy.reply(14, healthy.pack('I', marker)), 16) == 80
            with peer_client(context) as newcomer:
                newcomer.sync()
                assert newcomer.u16(newcomer.reply(14, newcomer.pack('I', marker)), 16) == 80
            healthy.sync()


def window_tree(context):
    with client(context) as c:
        parent = c.window()
        child = c.window(parent)
        reply = c.reply(15, c.pack('I', parent))
        assert c.u32(reply, 8) == c.root
        assert c.u16(reply, 16) == 1 and c.u32(reply, 32) == child
        geometry = c.reply(14, c.pack('I', child))
        assert c.unpack('hhHH', geometry, 12) == (7, 9, 80, 60)


def window_transition(context):
    with client(context) as c:
        wid = c.window()
        step = context['case']
        if step in ('map', 'unmap'):
            c.send(8, c.pack('I', wid))
            c.sync()
            c.event(19, lambda e: c.u32(e, 8) == wid)
            reply = c.reply(3, c.pack('I', wid))
            assert reply[26] == 2, 'mapped window is not Viewable'
        if step == 'configure':
            c.send(12, c.pack('IHHIIII', wid, 15, 0, 21, 23, 101, 79))
            c.sync()
            event = c.event(22, lambda e: c.u32(e, 8) == wid)
            assert c.unpack('hhHH', event, 16) == (21, 23, 101, 79)
            assert c.unpack('hhHH', c.reply(14, c.pack('I', wid)), 12) == (21, 23, 101, 79)
        if step == 'unmap':
            c.send(10, c.pack('I', wid))
            c.sync()
            c.event(18, lambda e: c.u32(e, 8) == wid)
            assert c.reply(3, c.pack('I', wid))[26] == 0
        if step == 'destroy':
            sequence = c.send(4, c.pack('I', wid))
            c.sync()
            event = c.event(17, lambda e: c.u32(e, 8) == wid)
            assert c.u32(event, 4) == wid and c.u16(event, 2) == sequence
            c.completion(c.send(3, c.pack('I', wid)), error=3, opcode=3, resource=wid)
            c.sync()


def reply_errors(context):
    with client(context) as c:
        wid = c.window()
        bad = c.xid()
        c.completion(c.send(3, c.pack('I', bad)), error=3, opcode=3, resource=bad)
        c.completion(c.send(3), error=16, opcode=3)  # BadLength, then healthy request.
        c.completion(c.send(255), error=1, opcode=255)
        c.send(127)  # NoOperation has no reply; next sequence must complete.
        assert c.u16(c.reply(14, c.pack('I', wid)), 16) == 80


def get_geometry_errors(context):
    with client(context) as c:
        alive = c.window(events=0)
        destroyed = c.window(events=0)
        c.send(4, c.pack('I', destroyed))
        c.sync()
        for invalid in (c.xid(), destroyed):
            c.completion(c.send(14, c.pack('I', invalid)), error=9, opcode=14, resource=invalid)
            assert c.u16(c.reply(14, c.pack('I', alive)), 16) == 80


def destroy_subscribers(context):
    with client(context) as owner, peer_client(context) as watcher, client(context) as silent:
        parent = owner.window(events=1 << 19)
        wid = owner.window(parent, events=1 << 17)
        # Both masks on one subscriber; a different connection performs destroy.
        for target, mask in [(parent, 1 << 19), (wid, 1 << 17)]:
            watcher.send(2, watcher.pack('III', target, 1 << 11, mask))
        watcher.sync()
        silent.sync()
        owner.events.clear()
        owner.send(4, owner.pack('I', wid))
        owner.sync()
        forms = [watcher.event(17), watcher.event(17)]
        assert {(watcher.u32(e, 4), watcher.u32(e, 8)) for e in forms} == {(wid, wid), (parent, wid)}
        own_forms = [owner.event(17), owner.event(17)]
        assert {(owner.u32(e, 4), owner.u32(e, 8)) for e in own_forms} == {(wid, wid), (parent, wid)}
        # The subscriber event barrier proves routing has happened before the
        # no-subscription check; a quiet read alone could race dispatch.
        silent.sync()
        assert not any(e[0] & 127 == 17 for e in silent.events)
        watcher.sync()
        assert not any(e[0] & 127 == 17 for e in watcher.events), 'duplicate DestroyNotify'


def destroy_family(context):
    with client(context) as owner, peer_client(context) as watcher:
        parent = owner.window(events=1 << 17)
        child = owner.window(parent, events=1 << 17)
        grandchild = owner.window(child, events=1 << 17)
        if context['case'] == 'destroy_descendants':
            owner.send(4, owner.pack('I', parent))
            owner.sync()
            for wid in (child, grandchild):
                owner.completion(owner.send(3, owner.pack('I', wid)), error=3, opcode=3, resource=wid)
            observed = [owner.u32(owner.event(17), 8) for _ in range(3)]
            assert observed == [grandchild, child, parent], observed
        elif context['case'] == 'destroy_subwindows':
            owner.send(5, owner.pack('I', parent))
            owner.sync()
            observed = [owner.u32(owner.event(17), 8) for _ in range(2)]
            assert observed == [grandchild, child], observed
            assert owner.u16(owner.reply(15, owner.pack('I', parent)), 16) == 0
            assert owner.u16(owner.reply(14, owner.pack('I', parent)), 16) == 80
        elif context['case'] == 'destroy_peer_close':
            for wid in (parent, child, grandchild):
                watcher.send(2, watcher.pack('III', wid, 1 << 11, 1 << 17))
            watcher.sync()
            owner.close()
            observed = [watcher.u32(watcher.event(17), 8) for _ in range(3)]
            assert observed == [grandchild, child, parent], observed
            watcher.sync()
        else:
            owner.send(4, owner.pack('I', grandchild))
            owner.sync()
            owner.event(17)
            for wid in (grandchild, owner.xid()):
                owner.completion(owner.send(4, owner.pack('I', wid)), error=3, opcode=4, resource=wid)
                owner.sync()
                assert not any(e[0] & 127 == 17 for e in owner.events), 'phantom DestroyNotify after BadWindow'


def destroy_subwindows_order(context):
    with client(context) as owner, peer_client(context) as watcher:
        parent = owner.window(events=0)
        first = owner.window(parent, events=0)
        first_leaf = owner.window(first, events=0)
        second = owner.window(parent, events=0)
        second_leaf = owner.window(second, events=0)
        for wid in (first, first_leaf, second, second_leaf):
            watcher.send(2, watcher.pack('III', wid, 1 << 11, 1 << 17))
        watcher.sync()
        # Move the newer sibling below the older one: allocation order must
        # disagree with stack order or an id-sorted implementation would pass.
        owner.send(12, owner.pack('IHHII', second, (1 << 5) | (1 << 6), 0, first, 1))
        owner.sync()
        owner.send(5, owner.pack('I', parent))
        owner.sync()
        observed = [watcher.u32(watcher.event(17), 8) for _ in range(4)]
        assert observed == [second_leaf, second, first_leaf, first], observed
        for wid in (first, first_leaf, second, second_leaf):
            watcher.completion(watcher.send(3, watcher.pack('I', wid)), error=3, opcode=3, resource=wid)
        assert owner.u16(owner.reply(15, owner.pack('I', parent)), 16) == 0
        watcher.sync()
        assert not any(e[0] & 127 == 17 for e in watcher.events), 'duplicate subtree destruction'


def destroy_subwindows_invalid(context):
    with client(context) as owner:
        parent = owner.window(events=(1 << 17) | (1 << 19))
        for _ in range(2):
            owner.send(5, owner.pack('I', parent))  # Empty subtree is a no-op.
            owner.sync()
            assert not any(e[0] & 127 == 17 for e in owner.events), 'empty subtree destroyed its parent'
        child = owner.window(parent)
        owner.send(5, owner.pack('I', parent))
        owner.sync()
        forms = [owner.event(17), owner.event(17)]
        assert {(owner.u32(e, 4), owner.u32(e, 8)) for e in forms} == {(parent, child), (child, child)}
        for wid in (child, owner.xid()):
            owner.completion(owner.send(5, owner.pack('I', wid)), error=3, opcode=5, resource=wid)
            owner.sync()
            assert not any(e[0] & 127 == 17 for e in owner.events), 'phantom event after invalid DestroySubwindows'
        assert owner.u16(owner.reply(14, owner.pack('I', parent)), 16) == 80


def destroy_peer_close_subscribers(context):
    with client(context) as owner, peer_client(context) as watcher, client(context) as silent:
        parent = owner.window(events=0)
        child = owner.window(parent, events=0)
        # The watcher owns neither resource, and selects both addressed forms.
        for wid, mask in [(owner.root, 1 << 19),
                          (parent, (1 << 17) | (1 << 19)), (child, 1 << 17)]:
            watcher.send(2, watcher.pack('III', wid, 1 << 11, mask))
        watcher.sync()
        silent.sync()
        owner.close()  # No DestroyWindow request: this is solely disconnect cleanup.
        observed = []
        try:
            for _ in range(4):
                event = watcher.event(17)
                observed.append((watcher.u32(event, 4), watcher.u32(event, 8)))
        except TimeoutError as error:
            raise TimeoutError(f'disconnect DestroyNotify forms before deadline: {observed}') from error
        expected = {(child, child), (parent, child), (parent, parent), (watcher.root, parent)}
        assert len(set(observed)) == 4 and set(observed) == expected, observed
        # Check addressing separately from the existing chain-order case.
        for wid in (parent, child):
            watcher.completion(watcher.send(3, watcher.pack('I', wid)), error=3, opcode=3, resource=wid)
        watcher.sync()
        assert not any(e[0] & 127 == 17 for e in watcher.events), 'duplicate disconnect destruction'
        silent.sync()
        assert not any(e[0] & 127 == 17 for e in silent.events), 'unsubscribed peer notified'
        live = watcher.window()
        assert watcher.u16(watcher.reply(14, watcher.pack('I', live)), 16) == 80


def destroy_mapped(context):
    with client(context) as owner:
        wid = owner.window(events=1 << 17)
        owner.send(8, owner.pack('I', wid))
        owner.sync()
        owner.event(19)
        assert owner.reply(3, owner.pack('I', wid))[26] == 2
        owner.events.clear()
        owner.send(4, owner.pack('I', wid))
        owner.sync()
        # Both events precede the barrier reply. Do not filter away a missing
        # automatic UnmapNotify or permit it to arrive after DestroyNotify.
        events = [e for e in owner.events if e[0] & 127 in (17, 18)]
        assert [e[0] & 127 for e in events] == [18, 17], [e.hex() for e in events]
        assert all(owner.unpack('II', e, 4) == (wid, wid) for e in events)
        assert events[0][12] == 0  # from-configure = False
        owner.completion(owner.send(3, owner.pack('I', wid)), error=3, opcode=3, resource=wid)


def property_values(context):
    with client(context) as c:
        wid, atom = c.window(), c.atom('SOPHIA_CONFORMANCE_PROPERTY')
        name = b'SOPHIA_CONFORMANCE_PROPERTY'
        reply = c.reply(17, c.pack('I', atom))
        assert c.u16(reply, 8) == len(name) and reply[32:32+len(name)] == name
        for fmt, values in [(8, b'a\0bc'), (16, [0x1234, 0xabcd]), (32, [0x12345678, 0xfedcba98])]:
            data = values if fmt == 8 else c.pack(('H' if fmt == 16 else 'I') * len(values), *values)
            count = len(data) * 8 // fmt
            c.send(18, c.pack('IIIB3xI', wid, atom, 6, fmt, count) + data)
            reply = c.reply(20, c.pack('IIIII', wid, atom, 6, 0, 32))
            assert reply[1] == fmt and c.u32(reply, 8) == 6
            assert c.u32(reply, 12) == 0 and c.u32(reply, 16) == count
            assert reply[32:32+len(data)] == data
            event = c.event(28, lambda e: c.u32(e, 4) == wid and c.u32(e, 8) == atom)
            assert event[16] == 0
        listed = c.reply(21, c.pack('I', wid))
        assert atom in c.unpack('I' * c.u16(listed, 8), listed, 32)
        c.send(19, c.pack('II', wid, atom))
        c.sync()
        assert c.event(28, lambda e: c.u32(e, 8) == atom)[16] == 1
        assert c.reply(20, c.pack('IIIII', wid, atom, 0, 0, 32))[1] == 0


def selection_owner(context):
    with client(context) as a, peer_client(context) as b:
        wa, wb, selection = a.window(), b.window(), a.atom('SOPHIA_CONFORMANCE_SELECTION')
        a.send(22, a.pack('III', wa, selection, 0))
        a.sync()
        assert b.u32(b.reply(23, b.pack('I', selection)), 8) == wa
        b.send(22, b.pack('III', wb, selection, 0))
        b.sync()
        event = a.event(29)
        assert a.u32(event, 8) == wa and a.u32(event, 12) == selection
        assert a.u32(a.reply(23, a.pack('I', selection)), 8) == wb


def selection_transfer(context):
    with client(context) as a, peer_client(context) as b:
        wa, wb = a.window(), b.window()
        sel, prop = a.atom('SOPHIA_CONFORMANCE_SELECTION'), a.atom('SOPHIA_CONFORMANCE_TRANSFER')
        a.send(22, a.pack('III', wa, sel, 0))
        a.sync()
        b.send(24, b.pack('IIIII', wb, sel, 31, prop, 0))
        b.sync()
        request = a.event(30)
        assert a.unpack('IIIII', request, 8) == (wa, wb, sel, 31, prop)
        a.send(18, a.pack('IIIB3xI', wb, prop, 31, 8, 4) + b'data')
        notify = bytes([31, 0]) + a.pack('HIIIIII', 0, 0, wb, sel, 31, prop, 0) + bytes(4)
        a.send(25, a.pack('II', wb, 0) + notify)
        a.sync()
        event = b.event(31)
        assert b.unpack('IIII', event, 8) == (wb, sel, 31, prop)
        assert b.reply(20, b.pack('IIIII', wb, prop, 31, 0, 4))[32:36] == b'data'


def selection_absent(context):
    with client(context) as c:
        wid, sel = c.window(), c.atom('SOPHIA_CONFORMANCE_UNOWNED')
        c.send(24, c.pack('IIIII', wid, sel, 31, 0, 0))
        c.sync()
        event = c.event(31)
        assert c.unpack('IIII', event, 8) == (wid, sel, 31, 0)


def focus(context):
    with client(context) as c:
        wid = c.window()
        c.send(8, c.pack('I', wid))
        c.sync()
        c.send(42, c.pack('II', wid, 0), detail=1)
        result = c.sync()
        assert c.u32(result, 8) == wid and result[1] == 1
        c.event(9, lambda e: c.u32(e, 4) == wid)
        c.send(42, c.pack('II', 0, 0), detail=0)
        assert c.u32(c.sync(), 8) == 0
        c.event(10, lambda e: c.u32(e, 4) == wid)


def grab(context):
    with client(context) as a, peer_client(context) as b:
        wa, wb = a.window(), b.window()
        for c, w in [(a, wa), (b, wb)]:
            c.send(8, c.pack('I', w))
            c.sync()
        keyboard = context['case'] == 'keyboard_grab'
        def acquire(c, w):
            if keyboard:
                return c.reply(31, c.pack('IIBB2x', w, 0, 1, 1))[1]
            return c.reply(26, c.pack('IHBBIII', w, 0, 1, 1, 0, 0, 0))[1]
        assert acquire(a, wa) == 0, 'first grab did not succeed'
        assert acquire(b, wb) == 1, 'competing grab must report AlreadyGrabbed'
        a.send(32 if keyboard else 27, a.pack('I', 0))
        a.sync()
        assert acquire(b, wb) == 0, 'released grab remained owned'
        b.send(32 if keyboard else 27, b.pack('I', 0))
        b.sync()


def disconnect(context):
    with client(context) as healthy:
        peer = client(context)
        wid = peer.window()
        sel = peer.atom('SOPHIA_CONFORMANCE_DISCONNECT')
        peer.send(22, peer.pack('III', wid, sel, 0))
        peer.sync()
        assert healthy.u32(healthy.reply(23, healthy.pack('I', sel)), 8) == wid
        peer.close()
        # Disconnect is asynchronous. Poll a real reply until cleanup becomes
        # observable, bounded by the original absolute deadline (never reset).
        while healthy.u32(healthy.reply(23, healthy.pack('I', sel)), 8) != 0:
            time.sleep(min(0.01, healthy.remaining()))
        healthy.completion(healthy.send(3, healthy.pack('I', wid)), error=3, opcode=3, resource=wid)
        live = healthy.window()
        assert healthy.u16(healthy.reply(14, healthy.pack('I', live)), 16) == 80


def extensions(context):
    with client(context) as c:
        if context['case'] == 'policy_absence':
            for name in context['denied_extensions']:
                assert c.query_extension(name)[8] == 0, ('intentional absence changed', name)
            assert c.query_extension('SOPHIA-NONEXISTENT-CONFORMANCE')[8] == 0
            return
        if context['case'] == 'extension_discovery':
            opcodes = []
            for name in context['extensions']:
                reply = c.query_extension(name)
                assert reply[8] == 1 and reply[9] >= 128, (name, reply.hex())
                opcodes.append(reply[9])
            assert len(opcodes) == len(set(opcodes)), 'extension opcode collision'
            for name in context['fixture_absence']:
                assert c.query_extension(name)[8] == 0, ('fixture unexpectedly exposes device extension', name)
            return
        result = c.reply(99)
        names, offset = [], 32
        for _ in range(result[1]):
            size = result[offset]
            names.append(result[offset+1:offset+1+size].decode('ascii'))
            offset += size + 1
        expected = set(context['extensions'])
        assert len(names) == len(set(names)) and set(names) == expected, ('extension inventory drift', names)


def extension_versions(context):
    with client(context) as c:
        # Requests from the corresponding public extension specifications.
        versions = {'Present': (0, 'II', (1, 2)),
                    'XFIXES': (0, 'II', (6, 0)), 'RENDER': (0, 'II', (0, 11)),
                    'RANDR': (0, 'II', (1, 6)), 'GLX': (7, 'II', (1, 4)),
                    'XC-MISC': (0, 'HH', (1, 1)), 'XKEYBOARD': (0, 'HH', (1, 0)),
                    'XInputExtension': (47, 'HH', (2, 4)),
                    'Generic Event Extension': (0, 'HH', (1, 0))}
        for name, (minor, fmt, requested) in versions.items():
            op = c.query_extension(name)[9]
            reply = c.reply(op, c.pack(fmt, *requested), detail=minor)
            actual = c.unpack(fmt, reply, 8)
            assert (0, 0) < actual <= requested, (name, requested, actual)
        for name in ('SHAPE', 'MIT-SHM', 'XFree86-VidModeExtension'):
            reply = c.reply(c.query_extension(name)[9])
            assert c.u16(reply, 8) >= 1, (name, reply.hex())
        reply = c.reply(c.query_extension('SYNC')[9], c.pack('BB2x', 3, 1))
        assert (reply[8], reply[9]) == (3, 1)
        reply = c.reply(c.query_extension('BIG-REQUESTS')[9])
        assert c.u32(reply, 8) >= 65535


def extension_errors(context):
    with client(context) as c:
        for name in context['extensions']:
            op = c.query_extension(name)[9]
            assert op >= 128
            c.completion(c.send(op, detail=255), error=1, opcode=op, minor=255)
            c.sync()


def shape(context):
    with client(context) as c:
        op, wid = c.query_extension('SHAPE')[9], c.window()
        c.reply(op)  # QueryVersion
        c.send(op, c.pack('BBBBIhh', 0, 2, 0, 0, wid, 0, 0) +
               c.pack('hhHH', 3, 4, 17, 19), detail=1)
        reply = c.reply(op, c.pack('IB3x', wid, 2), detail=8)
        assert c.u32(reply, 8) == 1
        assert c.unpack('hhHH', reply, 32) == (3, 4, 17, 19)


def sync_counter(context):
    with client(context) as c:
        op = c.query_extension('SYNC')[9]
        c.reply(op, c.pack('BB2x', 3, 1))
        counter = c.xid()
        c.send(op, c.pack('IiI', counter, 0, 41), detail=2)
        reply = c.reply(op, c.pack('I', counter), detail=5)
        assert c.unpack('iI', reply, 8) == (0, 41)
        c.send(op, c.pack('IiI', counter, 0, 1), detail=4)
        assert c.unpack('iI', c.reply(op, c.pack('I', counter), detail=5), 8) == (0, 42)
        c.send(op, c.pack('I', counter), detail=6)
        c.sync()


def xkb_names(context):
    """Every keyboard name the server reports can be asked about by name.

    XKB permits atom None for an unnamed key type level, and we sent it for
    every level. A client is entitled to walk the names a GetNames reply
    carries and ask the server what each one is called; libxdo does exactly
    that on startup, was handed None, and libX11 exits a client whose request
    the server refuses. No real server sends None here, so nothing met it
    there. This asks the same question the client asks.
    """
    with client(context) as c:
        op = c.query_extension('XKEYBOARD')[9]
        c.reply(op, c.pack('HH', 1, 0))  # UseExtension, major 1 minor 0
        # All three masks libxdo asks for: key type names, their level names,
        # and virtual modifier names.
        reply = c.reply(op, c.pack('HHI', 0x0100, 0, 0x40 | 0x80 | 0x800), detail=17)
        types = reply[14]
        assert types > 0, 'the reply must advertise key types'
        body = reply[32:]
        names = [c.u32(body, index * 4) for index in range(types)]
        # Type names, then one level-count byte per type padded to a word,
        # then the level names those counts describe.
        counts = [body[types * 4 + index] for index in range(types)]
        levels_at = -(-(types * 4 + types) // 4) * 4
        names += [c.u32(body, levels_at + index * 4) for index in range(sum(counts))]
        for index, atom in enumerate(names):
            assert atom != 0, f'name {index} is None and a client may ask for it'
            named = c.reply(17, c.pack('I', atom))  # GetAtomName
            assert c.u16(named, 8) > 0, f'name {index} resolves to an empty string'
        # THE COUNT AND THE ATOMS MUST AGREE. This reply names no virtual
        # modifier, so there is nothing here to resolve and nothing to assert
        # about a name; what is asserted is that the virtualMods field and the
        # atoms following it describe the same reply. A field claiming names
        # that do not follow is the level-name defect one step earlier -- a
        # reply describing something the client will then ask about and be
        # refused for -- and reading past the body is how a client finds out.
        virtual_mods = c.u16(reply, 16)
        expected = (types * 4 + -(-types // 4) * 4 + sum(counts) * 4
                    + bin(virtual_mods).count('1') * 4)
        assert c.u32(reply, 4) * 4 == expected, (
            f'reply body is {c.u32(reply, 4) * 4} bytes, but nTypes={types}, '
            f'levels={counts} and virtualMods=0x{virtual_mods:04x} describe {expected}')


def xfixes_selection(context):
    with client(context) as owner, peer_client(context) as watcher:
        ext = watcher.query_extension('XFIXES')
        op, event_base = ext[9], ext[10]
        watcher.reply(op, watcher.pack('II', 5, 0))
        window, watched = owner.window(), watcher.window()
        selection = owner.atom('SOPHIA_CONFORMANCE_XFIXES')
        watcher.send(op, watcher.pack('III', watched, selection, 1), detail=2)
        watcher.sync()
        owner.send(22, owner.pack('III', window, selection, 0))
        owner.sync()
        event = watcher.event(event_base)
        assert event[1] == 0
        assert watcher.unpack('III', event, 4) == (watched, window, selection)
        assert watcher.u16(event, 2) == watcher.sequence, 'event used the sender sequence'
        assert watcher.u32(event, 16) != 0 and watcher.u32(event, 20) != 0, 'CurrentTime was not resolved'


def xfixes_listen(c, window, selection, mask):
    extension = c.query_extension('XFIXES')
    c.reply(extension[9], c.pack('II', 5, 0))
    c.send(extension[9], c.pack('III', window, selection, mask), detail=2)
    c.sync()
    return extension[9], extension[10]


def xfixes_notice(c, base, window, owner, selection, subtype=0):
    event = c.event(base)
    assert event[1] == subtype, ('wrong selection subtype', subtype, event.hex())
    assert c.unpack('III', event, 4) == (window, owner, selection), event.hex()
    return event


def xfixes_owner(c, window, selection):
    c.send(22, c.pack('III', window, selection, 0))
    c.sync()


def xfixes_selection_changes(context):
    with client(context) as owner, peer_client(context) as watcher:
        first, second, watched = owner.window(), owner.window(), watcher.window()
        selection = owner.atom('SOPHIA_XFIXES_CHANGES')
        _, base = xfixes_listen(watcher, watched, selection, 1)
        # Reassertion of the same owner can signal new contents. None is an
        # explicit SetSelectionOwner notification, not a destruction subtype.
        for window in (first, first, second, 0):
            xfixes_owner(owner, window, selection)
            event = xfixes_notice(watcher, base, watched, window, selection)
            assert watcher.u16(event, 2) == watcher.sequence
        # No intervening round trip: every transition must be retained in order.
        for window in (first, second, 0, first):
            owner.send(22, owner.pack('III', window, selection, 0))
        owner.sync()
        for window in (first, second, 0, first):
            xfixes_notice(watcher, base, watched, window, selection)
        watcher.sync()
        assert not any(e[0] & 127 == base for e in watcher.events), 'duplicate selection event'


def xfixes_selection_masks(context):
    with client(context) as owner, peer_client(context) as watcher, client(context) as silent:
        owned = owner.window()
        watched, control = watcher.window(), watcher.window()
        selection = owner.atom('SOPHIA_XFIXES_MASKS')
        other = owner.atom('SOPHIA_XFIXES_OTHER')
        op, base = xfixes_listen(watcher, watched, selection, 1)
        xfixes_listen(watcher, control, selection, 1)
        silent.sync()  # Connected, but never subscribed.
        for mask in (0, 2, 4):
            watcher.send(op, watcher.pack('III', watched, selection, mask), detail=2)
            watcher.sync()
            xfixes_owner(owner, owned, selection)
            xfixes_notice(watcher, base, control, owned, selection)
            watcher.sync()
            assert not any(e[0] & 127 == base for e in watcher.events), 'mask removal/replacement leaked an event'
        # An unrelated atom must not activate either subscription.
        xfixes_owner(owner, owned, other)
        watcher.sync()
        assert not any(e[0] & 127 == base for e in watcher.events), 'selection atom isolation failed'
        silent.sync()
        assert not any(e[0] & 127 == base for e in silent.events), 'unsubscribed peer got selection metadata'


def xfixes_selection_invalid(context):
    with client(context) as owner, peer_client(context) as watcher:
        owned, watched = owner.window(), watcher.window()
        selection = owner.atom('SOPHIA_XFIXES_INVALID')
        op, base = xfixes_listen(watcher, watched, selection, 1)
        for window, atom, mask, code, resource in (
            (watched, selection, 8, 2, 8),
            (watched, 0xffffffff, 1, 5, 0xffffffff),
            (watcher.xid(), selection, 1, 3, None),
        ):
            watcher.completion(watcher.send(op, watcher.pack('III', window, atom, mask), detail=2),
                               error=code, opcode=op, minor=2,
                               resource=window if resource is None else resource)
        watcher.sync()
        xfixes_owner(owner, owned, selection)
        xfixes_notice(watcher, base, watched, owned, selection)


def xfixes_selection_end(context):
    with client(context) as owner, peer_client(context) as watcher:
        owned, watched = owner.window(), watcher.window()
        selection = owner.atom('SOPHIA_XFIXES_END')
        _, base = xfixes_listen(watcher, watched, selection, 7)
        xfixes_owner(owner, owned, selection)
        first = xfixes_notice(watcher, base, watched, owned, selection)
        if context['case'] == 'xfixes_selection_destroy':
            owner.send(4, owner.pack('I', owned))
            owner.sync()
            subtype = 1
        else:
            owner.close()
            subtype = 2
        event = xfixes_notice(watcher, base, watched, 0, selection, subtype)
        assert watcher.u32(event, 20) == watcher.u32(first, 20), 'ownership timestamp changed on teardown'
        assert watcher.u32(watcher.reply(23, watcher.pack('I', selection)), 8) == 0
        watcher.sync()
        assert not any(e[0] & 127 == base for e in watcher.events), 'duplicate teardown subtype'


def xfixes_selection_reuse(context):
    with client(context) as owner, peer_client(context) as watcher:
        owned, watched, control = owner.window(), watcher.window(), watcher.window()
        selection = owner.atom('SOPHIA_XFIXES_REUSE')
        _, base = xfixes_listen(watcher, watched, selection, 1)
        xfixes_listen(watcher, control, selection, 1)
        watcher.send(4, watcher.pack('I', watched))
        watcher.sync()
        watcher.window(xid=watched)
        watcher.events.clear()
        xfixes_owner(owner, owned, selection)
        xfixes_notice(watcher, base, control, owned, selection)
        watcher.sync()
        assert not any(e[0] & 127 == base for e in watcher.events), 'reused XID inherited an old subscription'


def xfixes_selection_self(context):
    with client(context) as c:
        owned, watched = c.window(), c.window()
        selection = c.atom('SOPHIA_XFIXES_SELF')
        _, base = xfixes_listen(c, watched, selection, 1)
        sequence = c.send(22, c.pack('III', owned, selection, 0))
        event = xfixes_notice(c, base, watched, owned, selection)
        assert c.u16(event, 2) == sequence, 'self-notification has wrong sequence'
        c.sync()


def xfixes_selection_peer_descendant(context):
    with client(context) as owner, peer_client(context) as peer, client(context) as watcher:
        parent = owner.window(events=0)
        child = peer.window(parent, events=0)
        watched = watcher.window(events=0)
        parent_selection = owner.atom('SOPHIA_XFIXES_PARENT_CLOSE')
        child_selection = owner.atom('SOPHIA_XFIXES_PEER_CHILD')
        _, base = xfixes_listen(watcher, watched, parent_selection, 7)
        xfixes_listen(watcher, watched, child_selection, 7)
        xfixes_owner(owner, parent, parent_selection)
        first = xfixes_notice(watcher, base, watched, parent, parent_selection)
        xfixes_owner(peer, child, child_selection)
        second = xfixes_notice(watcher, base, watched, child, child_selection)
        owner.close()
        # The parent owner departed; the child's client remains. Both windows
        # disappear, but the two selection ownerships end for different causes.
        expected = {parent_selection: (2, watcher.u32(first, 20)),
                    child_selection: (1, watcher.u32(second, 20))}
        for _ in range(2):
            try:
                event = watcher.event(base)
            except TimeoutError as error:
                raise TimeoutError(f'missing descendant teardown notifications: {expected}') from error
            selection = watcher.u32(event, 12)
            assert selection in expected, 'duplicate or foreign selection teardown'
            subtype, timestamp = expected.pop(selection)
            assert event[1] == subtype, ('wrong descendant teardown cause', event.hex())
            assert watcher.unpack('II', event, 4) == (watched, 0)
            assert watcher.u32(event, 20) == timestamp
            if selection == parent_selection:
                # Parent teardown is an observed barrier. Its peer-owned child
                # must already be gone; announcing only the parent is not enough.
                peer.completion(peer.send(14, peer.pack('I', child)), error=9, opcode=14, resource=child)
        for selection in (parent_selection, child_selection):
            assert watcher.u32(watcher.reply(23, watcher.pack('I', selection)), 8) == 0
        peer.completion(peer.send(14, peer.pack('I', child)), error=9, opcode=14, resource=child)
        peer.sync()


# The authority's silence allowance for a connection that is owed output:
# `X_AUTHORITY_CLIENT_OUTPUT_SILENCE_LIMIT`, six seconds, in
# crates/sophia-x-authority/src/x11_socket/connection/output_spill.rs.
OUTPUT_SILENCE_ALLOWANCE = 6.0


def xfixes_selection_stalled(context):
    # A subscriber that stops reading is owed its notices, in order, up to a
    # bound (t165): what the kernel refuses is kept in its connection's spill
    # and delivered when it reads again. It is ended only when it has neither
    # read nor asked for the allowance while output is owed, or when more than
    # the byte bound is owed. Two subscribers stall through the same flood.
    # The laggard reads afterwards: it must receive every notice and stay.
    # The silent one never reads: past the allowance it must be ended, with
    # EOF, and not silently kept as if still subscribed. The reference server
    # keeps both; ending the silent one is this authority's stricter bound.
    with client(context) as owner, peer_client(context) as healthy, \
            client(context) as laggard, client(context) as silent:
        owned, watched = owner.window(), healthy.window()
        behind, stuck = laggard.window(), silent.window()
        selection = owner.atom('SOPHIA_XFIXES_STALLED')
        _, base = xfixes_listen(healthy, watched, selection, 1)
        _, behind_base = xfixes_listen(laggard, behind, selection, 1)
        xfixes_listen(silent, stuck, selection, 1)
        # Deliberately leave both peers unread through the flood. Bound both
        # work and elapsed time; the healthy peer drains every batch so this
        # is recipient-specific pressure, and far past a socket buffer's
        # worth of thirty-two-byte records. The laggard keeps asking while
        # it does not read -- one NoOperation a batch -- because a request
        # read is activity the allowance measures the absence of: on a
        # loaded machine a flood can outlast the allowance, and a client
        # that neither read nor asked for that long would be ended rightly,
        # which is the silent one's part, not this one's.
        count = 4096
        for _ in range(count // 16):
            for _ in range(16):
                owner.send(22, owner.pack('III', owned, selection, 0))
            laggard.send(127)
            owner.sync()
            for _ in range(16):
                xfixes_notice(healthy, base, watched, owned, selection)
        owner.sync()
        healthy.sync()
        # The laggard catches up: every notice, in order, then a round trip.
        for _ in range(count):
            xfixes_notice(laggard, behind_base, behind, owned, selection)
        laggard.sync()
        # The silent one is given the allowance, reading nothing and asking
        # nothing. The laggard is quiet too, but owes nothing, and is kept.
        time.sleep(OUTPUT_SILENCE_ALLOWANCE + 1)
        laggard.sync()
        received = 0
        while True:
            silent.sock.settimeout(silent.remaining())
            try:
                part = silent.sock.recv(65536)
            except ConnectionResetError:
                break
            except TimeoutError as error:
                raise TimeoutError(
                    f'silent subscriber stayed connected past the allowance after {count} '
                    f'notices; drained {received} bytes; the laggard and the healthy watcher '
                    f'received every notice') from error
            if not part:
                break
            received += len(part)
            assert received <= count * 32, 'unexpected output to the silent subscriber'
        # EOF is mandatory: keeping the peer alive after dropping what it was
        # owed is not a successful delivery or a policy denial. What the kernel
        # held still arrives; the spill it was owed beyond that does not.
        assert received < count * 32, 'the silent subscriber was ended with nothing owed'
        xfixes_owner(owner, owned, selection)
        xfixes_notice(healthy, base, watched, owned, selection)
        xfixes_notice(laggard, behind_base, behind, owned, selection)
        with peer_client(context) as newcomer:
            newcomer.sync()
            assert newcomer.u16(newcomer.reply(14, newcomer.pack('I', owned)), 16) == 80


def disconnect_grab(context):
    with client(context) as healthy:
        peer = client(context)
        a, b = peer.window(), healthy.window()
        for c, wid in ((peer, a), (healthy, b)):
            c.send(8, c.pack('I', wid))
            c.sync()
        def acquire(c, wid):
            return c.reply(26, c.pack('IHBBIII', wid, 0, 1, 1, 0, 0, 0))[1]
        assert acquire(peer, a) == 0
        assert acquire(healthy, b) == 1
        peer.close()
        while acquire(healthy, b) != 0:
            time.sleep(min(.01, healthy.remaining()))
        healthy.sync()


def truncated_peer(context):
    with client(context) as healthy:
        peer = client(context)
        wid = peer.window()
        # Declared two-word GetWindowAttributes, only half its body sent.
        peer.sock.sendall(bytes([3, 0]) + peer.pack('H', 2) + bytes(2))
        peer.close()
        # Ask about the old resource until its cleanup is visible; a failed
        # peer must not take down an already-connected healthy worker.
        while True:
            sequence = healthy.send(3, healthy.pack('I', wid))
            reply = healthy.record()
            if reply[0] == 0:
                assert reply[1] == 3 and healthy.u16(reply, 2) == sequence
                break
            assert reply[0] == 1 and healthy.u16(reply, 2) == sequence
            time.sleep(min(.01, healthy.remaining()))
        healthy.sync()


def destroy_xid_reuse(context):
    with client(context) as owner, peer_client(context) as old_watcher:
        wid = owner.window(events=1 << 17)
        old_watcher.send(2, old_watcher.pack('III', wid, 1 << 11, 1 << 17))
        old_watcher.sync()
        owner.send(4, owner.pack('I', wid))
        owner.sync()
        owner.event(17)
        old_watcher.event(17)
        assert owner.window(events=1 << 17, xid=wid) == wid
        owner.send(4, owner.pack('I', wid))
        owner.sync()
        owner.event(17)
        old_watcher.sync()
        assert not any(e[0] & 127 == 17 for e in old_watcher.events), 'old subscription survived XID reuse'


def force_screen_saver(context):
    with client(context) as c:
        # Reset (0) and Activate (1). This authority blanks nothing and keeps
        # no idle timer, so what is observable is that neither mode is
        # refused and the connection survives it. That is the whole of what
        # the request is for here: every XTS test's startup resets the screen
        # saver, and a refusal there ends the test before its assertions.
        for mode in (0, 1):
            c.send(115, detail=mode)
        c.sync()
        # Outside that pair the protocol requires a Value error reporting the
        # mode it refused, and the connection must survive that too.
        for mode in (2, 255):
            c.completion(c.send(115, detail=mode), error=2, opcode=115, resource=mode)
        c.sync()


def colormap_static_answers(context):
    with client(context) as c:
        default, copy = c.default_colormap, c.xid()
        # A request one unit long is BadLength before it is anything else:
        # the protocol frames each colormap request exactly.
        for opcode, body in ((81, c.pack('II', default, 0)), (82, c.pack('II', default, 0)),
                             (83, c.pack('II', c.root, 0)), (86, c.pack('III', default, 0, 0)),
                             (87, c.pack('IIII', default, 0, 0, 0)), (80, c.pack('III', copy, default, 0)),
                             (89, c.pack('II', default, 0)), (90, c.pack('IIII', default, 0, 0, 0))):
            c.completion(c.send(opcode, body), error=16, opcode=opcode)
        c.completion(c.send(88, c.pack('I', default)), error=16, opcode=88)
        # Framed right, a static visual answers: no cells or planes to
        # allocate, no writable cells to store into, nothing to free.
        c.completion(c.send(86, c.pack('IHBB', default, 1, 0, 0)), error=11, opcode=86)
        c.completion(c.send(87, c.pack('IHBBB', default, 1, 0, 0, 0)), error=11, opcode=87)
        c.completion(c.send(89, c.pack('IIHHHBx', default, 0, 0, 0, 0, 7)), error=10, opcode=89)
        c.completion(c.send(90, c.pack('IIHxx', default, 0, 3) + b'red\x00', detail=7), error=10, opcode=90)
        c.send(88, c.pack('III', default, 0, 1))
        c.send(81, c.pack('I', default))
        c.send(82, c.pack('I', default))
        c.sync()
        # CopyColormapAndFree is a new colormap on the source's visual, usable
        # and the client's to name once; ListInstalledColormaps is the default.
        c.send(80, c.pack('II', copy, default))
        assert c.reply(84, c.pack('IHHHxx', copy, 0x8000, 0x4000, 0x2000))[0] == 1
        c.completion(c.send(80, c.pack('II', copy, default)), error=14, opcode=80, resource=copy)
        c.completion(c.send(80, c.pack('II', 0x7ff00001, default)), error=14, opcode=80)
        reply = c.reply(83, c.pack('I', c.root))
        assert c.u16(reply, 8) == 1 and c.u32(reply, 32) == default, reply.hex()
        c.completion(c.send(83, c.pack('I', 0x7ff00001)), error=3, opcode=83)
        c.sync()


def server_controls_round_trip(context):
    with client(context) as c:
        # Pointer control: defaults, a change read back, a zero denominator.
        assert c.unpack('HHH', c.reply(106), 8) == (2, 1, 4)
        c.send(105, c.pack('hhhBB', 7, 3, 9, 1, 1))
        assert c.unpack('HHH', c.reply(106), 8) == (7, 3, 9)
        c.completion(c.send(105, c.pack('hhhBB', 1, 0, 4, 1, 0)), error=2, opcode=105)
        # Screen saver: defaults, a change with one mode kept, a bad mode.
        assert c.unpack('HHBB', c.reply(108), 8) == (600, 600, 1, 1)
        c.send(107, c.pack('hhBB2x', 120, 30, 0, 2))
        assert c.unpack('HHBB', c.reply(108), 8) == (120, 30, 0, 1)
        c.completion(c.send(107, c.pack('hhBB2x', 120, 30, 3, 0)), error=2, opcode=107)
        # Keyboard control: bell set and read back; an unused mask bit is
        # BadValue carrying the mask; a led without a mode is BadMatch.
        c.send(102, c.pack('III', 0x06, 75, 880))
        reply = c.reply(103)
        assert reply[13] == 75 and c.u16(reply, 14) == 880 and reply[1] == 1, reply.hex()
        c.completion(c.send(102, c.pack('II', 0x100, 0)), error=2, opcode=102, resource=0x100)
        c.completion(c.send(102, c.pack('II', 0x10, 3)), error=8, opcode=102)
        # No motion history; an unknown window is named.
        reply = c.reply(39, c.pack('III', c.root, 0, 0))
        assert c.u32(reply, 8) == 0 and c.u32(reply, 4) == 0
        c.completion(c.send(39, c.pack('III', 0x7ff00001, 0, 0)), error=3, opcode=39)
        # The host list is empty and enabled, and nobody may change it.
        reply = c.reply(110)
        assert reply[1] == 1 and c.u16(reply, 8) == 0 and c.u32(reply, 4) == 0
        c.completion(c.send(109, c.pack('BxH', 0, 4) + bytes([127, 0, 0, 1])), error=10, opcode=109)
        c.completion(c.send(109, c.pack('BxH', 0, 4) + bytes([127, 0, 0, 1]), detail=2), error=2, opcode=109)
        c.completion(c.send(111, detail=1), error=10, opcode=111)
        c.completion(c.send(111, detail=2), error=2, opcode=111)
        c.sync()


def unmap_subwindows(context):
    with client(context) as c:
        parent = c.window()
        a, b = c.window(parent), c.window(parent)
        for w in (parent, a, b):
            c.send(8, c.pack('I', w))
        c.sync()
        c.events.clear()
        c.send(11, c.pack('I', parent))
        c.sync()
        unmapped = [c.u32(e, 8) for e in c.events if e[0] & 127 == 18 and c.u32(e, 4) == c.u32(e, 8)]
        assert unmapped == [b, a], ('top to bottom, one each', unmapped)
        c.events.clear()
        c.send(11, c.pack('I', parent))
        c.sync()
        assert not any(e[0] & 127 == 18 for e in c.events), 'nothing to unmap twice'
        c.completion(c.send(11, c.pack('I', 0x7ff00001)), error=3, opcode=11)


def circulate_window(context):
    with client(context) as c, peer_client(context) as manager:
        parent = c.window()
        c.send(8, c.pack('I', parent))
        lower = c.xid()
        c.send(1, c.pack('IIhhHHHHII', lower, parent, 10, 10, 80, 80, 0, 1, 0, 0), detail=24)
        upper = c.xid()
        c.send(1, c.pack('IIhhHHHHII', upper, parent, 40, 40, 80, 80, 0, 1, 0, 0), detail=24)
        c.send(8, c.pack('I', lower))
        c.send(8, c.pack('I', upper))
        # StructureNotify on the children: CirculateNotify goes to whoever
        # selected it on the window, not to whoever asked.
        for child in (lower, upper):
            c.send(2, c.pack('II', child, 1 << 11) + c.pack('I', 1 << 17))
        c.send(2, c.pack('II', parent, 1 << 11) + c.pack('I', (1 << 17) | (1 << 19)))
        c.sync()
        c.events.clear()
        # RaiseLowest: the occluded lower child to the top, one notice.
        c.send(13, c.pack('I', parent), detail=0)
        c.sync()
        moved = [(c.u32(e, 8), e[16]) for e in c.events if e[0] & 127 == 26 and c.u32(e, 4) == c.u32(e, 8)]
        assert moved == [(lower, 0)], moved
        tree = c.reply(15, c.pack('I', parent))
        count = c.u16(tree, 16)
        assert c.unpack(f'{count}I', tree, 32) == (upper, lower), 'bottom to top after the raise'
        c.completion(c.send(13, c.pack('I', parent), detail=2), error=2, opcode=13, resource=2)
        # A manager selecting SubstructureRedirect on the parent is asked
        # instead: it reads a CirculateRequest naming the child, and the
        # stacking stays as it was.
        manager.send(2, manager.pack('II', parent, 1 << 11) + manager.pack('I', 1 << 20))
        manager.sync()
        c.send(13, c.pack('I', parent), detail=0)
        c.sync()
        request = manager.event(27)
        assert manager.unpack('II', request, 4) == (parent, upper) and request[16] == 0, request.hex()
        tree = c.reply(15, c.pack('I', parent))
        assert c.unpack('2I', tree, 32) == (upper, lower), 'a redirected circulate moves nothing'


def rotate_properties(context):
    with client(context) as c:
        window = c.window()
        atoms = [c.atom(f'SOPHIA_ROTATE_{name}') for name in 'ABC']
        for index, atom in enumerate(atoms):
            c.send(18, c.pack('IIIB3xI', window, atom, 6, 32, 1) + c.pack('I', index + 1))
        c.send(2, c.pack('II', window, 1 << 11) + c.pack('I', 1 << 22))
        c.sync()
        c.events.clear()
        c.send(114, c.pack('IHh', window, 3, 1) + c.pack('3I', *atoms))
        c.sync()
        notified = [c.u32(e, 8) for e in c.events if e[0] & 127 == 28]
        assert notified == atoms, notified
        assert c.unpack('I', c.reply(20, c.pack('IIIII', window, atoms[0], 6, 0, 1)), 32) == (2,), 'A holds what B held'
        assert c.unpack('I', c.reply(20, c.pack('IIIII', window, atoms[2], 6, 0, 1)), 32) == (1,), 'C holds what A held'
        d = c.atom('SOPHIA_ROTATE_D')
        c.completion(c.send(114, c.pack('IHh', window, 3, 1) + c.pack('3I', atoms[0], atoms[1], d)), error=8, opcode=114)
        c.completion(c.send(114, c.pack('IHh', window, 2, 1) + c.pack('2I', atoms[0], atoms[0])), error=8, opcode=114)
        c.completion(c.send(114, c.pack('IHh', 0x7ff00001, 1, 1) + c.pack('I', atoms[0])), error=3, opcode=114)


def change_active_pointer_grab(context):
    with client(context) as c:
        # No active grab: nothing. A bit outside the pointer events: BadValue
        # carrying the mask. An unknown cursor: BadCursor.
        c.send(30, c.pack('IIH2x', 0, 0, 0))
        c.sync()
        c.completion(c.send(30, c.pack('IIH2x', 0, 0, 0x8001)), error=2, opcode=30, resource=0x8001)
        c.completion(c.send(30, c.pack('IIH2x', 0x7ff00001, 0, 4)), error=6, opcode=30, resource=0x7ff00001)
        c.sync()


def kill_client(context):
    with client(context) as killer, peer_client(context) as victim, client(context) as witness:
        window = victim.window()
        victim.send(8, victim.pack('I', window))
        victim.sync()
        witness.reply(3, witness.pack('I', window))
        killer.completion(killer.send(113, killer.pack('I', 0x7ff00001)), error=2, opcode=113, resource=0x7ff00001)
        killer.send(113, killer.pack('I', window))
        killer.sync()
        victim.sock.settimeout(victim.remaining())
        while True:
            try:
                if not victim.sock.recv(4096):
                    break
            except ConnectionResetError:
                break
        deadline = time.monotonic() + 5
        while True:
            try:
                witness.completion(witness.send(3, witness.pack('I', window)), error=3, opcode=3, resource=window)
                break
            except AssertionError:
                assert time.monotonic() < deadline, 'the victim window outlived its client'
                time.sleep(0.02)


def set_close_down_mode(context):
    with client(context) as witness:
        permanent, temporary = client(context), client(context)
        permanent.completion(permanent.send(112, detail=3), error=2, opcode=112, resource=3)
        permanent.send(112, detail=1)
        kept = permanent.window()
        permanent.sync()
        temporary.send(112, detail=2)
        fleeting = temporary.window()
        temporary.sync()
        permanent.close()
        temporary.close()
        time.sleep(0.2)
        witness.reply(3, witness.pack('I', kept))
        witness.reply(3, witness.pack('I', fleeting))
        witness.send(113, witness.pack('I', 0))
        witness.sync()
        witness.completion(witness.send(3, witness.pack('I', fleeting)), error=3, opcode=3, resource=fleeting)
        witness.reply(3, witness.pack('I', kept))
        witness.send(113, witness.pack('I', kept))
        witness.sync()
        witness.completion(witness.send(3, witness.pack('I', kept)), error=3, opcode=3, resource=kept)


def change_save_set(context):
    with client(context) as peer:
        manager = client(context)
        frame = manager.window()
        manager.send(8, manager.pack('I', frame))
        window = peer.window()
        peer.send(2, peer.pack('II', window, 1 << 11) + peer.pack('I', 1 << 17))
        peer.sync()
        manager.send(7, manager.pack('IIhh', window, frame, 0, 0))
        manager.send(8, manager.pack('I', window))
        manager.completion(manager.send(6, manager.pack('I', frame)), error=8, opcode=6, resource=frame)
        manager.completion(manager.send(6, manager.pack('I', 0x7ff00001)), error=3, opcode=6, resource=0x7ff00001)
        manager.completion(manager.send(6, manager.pack('I', window), detail=2), error=2, opcode=6, resource=2)
        manager.send(6, manager.pack('I', window))
        manager.sync()
        peer.sync()
        peer.events.clear()
        manager.close()
        assert peer.event(18)[0] & 127 == 18
        reparent = peer.event(21)
        assert peer.u32(reparent, 12) == peer.root, reparent.hex()
        peer.event(19)
        tree = peer.reply(15, peer.pack('I', window))
        assert peer.u32(tree, 12) == peer.root, 'back under the root'
        peer.reply(3, peer.pack('I', window))


def set_pointer_mapping(context):
    with client(context) as c, peer_client(context) as peer:
        c.completion(c.send(116, bytes([1, 2, 3, 4, 5, 6, 7, 8]), detail=8), error=2, opcode=116, resource=8)
        c.completion(c.send(116, bytes([3, 2, 3, 4, 5, 6, 7, 8, 9]), detail=9), error=2, opcode=116, resource=3)
        reply = c.reply(116, bytes([3, 2, 1, 4, 5, 6, 7, 8, 9]), detail=9)
        assert reply[1] == 0, reply.hex()
        assert c.event(34)[4] == 2
        assert peer.event(34)[4] == 2, 'every client is told'
        reply = c.reply(117)
        assert reply[32:32 + reply[1]] == bytes([3, 2, 1, 4, 5, 6, 7, 8, 9]), reply.hex()
        assert c.reply(116, bytes([1, 2, 3, 4, 5, 6, 7, 8, 9]), detail=9)[1] == 0
        c.event(34)
        peer.event(34)


def change_keyboard_mapping(context):
    with client(context) as c:
        c.send(100, c.pack('BB2x', 38, 3) + c.pack('III', 0x61, 0x41, 0xe6), detail=1)
        notice = c.event(34)
        assert notice[4:7] == bytes([1, 38, 1]), notice.hex()
        reply = c.reply(101, c.pack('BB2x', 38, 1))
        assert reply[1] == 3 and c.unpack('III', reply, 32) == (0x61, 0x41, 0xe6), reply.hex()
        c.completion(c.send(100, c.pack('BB2x', 7, 1) + c.pack('I', 0), detail=1), error=2, opcode=100, resource=7)
        c.sync()


def set_modifier_mapping(context):
    with client(context) as c:
        current = c.reply(119)
        kpm = current[1]
        keycodes = current[32:32 + 8 * kpm]
        reply = c.reply(118, keycodes, detail=kpm)
        assert reply[1] == 0, reply.hex()
        assert c.event(34)[4] == 0
        other = bytes([9]) + keycodes[1:]
        reply = c.reply(118, other, detail=kpm)
        assert reply[1] == 2, reply.hex()
        assert not any(e[0] & 127 == 34 for e in c.events), 'no notice for a refused map'
        c.completion(c.send(118, bytes([3]) + keycodes[1:], detail=kpm), error=2, opcode=118, resource=3)


def query_keymap(context):
    with client(context) as c:
        reply = c.reply(44)
        assert c.u32(reply, 4) == 2 and all(b == 0 for b in reply[8:40]), reply.hex()
        c.completion(c.send(44, c.pack('I', 0)), error=16, opcode=44)


def warp_pointer(context):
    with client(context) as c:
        root, window = c.root, c.window()

        def warp(source, destination, src=(0, 0, 0, 0), dst=(0, 0)):
            return c.send(41, c.pack('IIhhHHhh', source, destination,
                                     src[0], src[1], src[2], src[3], dst[0], dst[1]))

        def position():
            reply = c.reply(38, c.pack('I', root))
            return c.unpack('hh', reply, 16)

        # An unconditional warp to a point on the root moves the pointer
        # there and says nothing, and a client sees the move.
        warp(0, root, dst=(40, 25))
        c.sync()
        assert position() == (40, 25), position()

        # With no destination window the offset is from where it already is.
        warp(0, 0, dst=(-10, 5))
        c.sync()
        assert position() == (30, 30), position()

        # A source rectangle that does not hold the pointer makes the warp
        # conditional, and it does not happen. That is not a refusal.
        warp(root, 0, src=(500, 500, 10, 10), dst=(1, 1))
        c.sync()
        assert position() == (30, 30), position()

        # The same warp with a rectangle that does hold it fires.
        warp(root, 0, src=(0, 0, 200, 200), dst=(1, 1))
        c.sync()
        assert position() == (31, 31), position()

        # A warp into a window is relative to that window's origin.
        warp(0, window, dst=(2, 3))
        c.sync()
        assert position() != (31, 31), 'a warp into a window moved nothing'

        # A window nobody created is a Window error naming it.
        c.completion(warp(0, 0x7fff0001), error=3, opcode=41)
        c.completion(warp(0x7fff0002, 0), error=3, opcode=41)
        c.sync()


CASES = {'setup': setup,
         'force_screen_saver': force_screen_saver,
         'colormap_static_answers': colormap_static_answers,
         'server_controls_round_trip': server_controls_round_trip,
         'unmap_subwindows': unmap_subwindows,
         'circulate_window': circulate_window,
         'rotate_properties': rotate_properties,
         'change_active_pointer_grab': change_active_pointer_grab,
         'kill_client': kill_client,
         'set_close_down_mode': set_close_down_mode,
         'change_save_set': change_save_set,
         'set_pointer_mapping': set_pointer_mapping,
         'change_keyboard_mapping': change_keyboard_mapping,
         'set_modifier_mapping': set_modifier_mapping,
         'query_keymap': query_keymap,
         'warp_pointer': warp_pointer,
         **{name: setup_containment for name in ('setup_empty', 'setup_truncated_prefix',
             'setup_truncated_auth', 'setup_invalid_order', 'setup_version_containment')},
         'window_tree': window_tree, 'map': window_transition,
         'configure': window_transition, 'unmap': window_transition, 'destroy': window_transition,
         'reply_errors': reply_errors,
         'get_geometry_errors': get_geometry_errors, 'property_values': property_values,
         'selection_owner': selection_owner, 'selection_transfer': selection_transfer,
         'selection_absent': selection_absent, 'focus': focus, 'pointer_grab': grab,
         'keyboard_grab': grab, 'disconnect': disconnect, 'extensions': extensions,
         'destroy_subscribers': destroy_subscribers, 'extension_discovery': extensions,
         'policy_absence': extensions, 'extension_versions': extension_versions,
         'extension_errors': extension_errors, 'shape': shape, 'sync_counter': sync_counter,
         'xkb_names': xkb_names,
         'xfixes_selection': xfixes_selection,
         'xfixes_selection_changes': xfixes_selection_changes,
         'xfixes_selection_masks': xfixes_selection_masks,
         'xfixes_selection_invalid': xfixes_selection_invalid,
         'xfixes_selection_destroy': xfixes_selection_end,
         'xfixes_selection_close': xfixes_selection_end,
         'xfixes_selection_reuse': xfixes_selection_reuse,
         'xfixes_selection_self': xfixes_selection_self,
         'xfixes_selection_stalled': xfixes_selection_stalled,
         'xfixes_selection_peer_descendant': xfixes_selection_peer_descendant,
         'disconnect_grab': disconnect_grab,
         'truncated_peer': truncated_peer, 'destroy_descendants': destroy_family,
         'destroy_subwindows': destroy_family, 'destroy_peer_close': destroy_family,
         'destroy_invalid': destroy_family, 'destroy_xid_reuse': destroy_xid_reuse,
         'destroy_subwindows_order': destroy_subwindows_order,
         'destroy_subwindows_invalid': destroy_subwindows_invalid,
         'destroy_peer_close_subscribers': destroy_peer_close_subscribers,
         'destroy_mapped': destroy_mapped,
         **DRAWING_CASES}

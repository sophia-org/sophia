"""Independent XTEST 2.1 wire obligations against an owned private instance.

Layouts follow XTEST 2.1 and xtestproto.h, not Sophia's codec. The final
FakeInput byte is padding in 2.1. These cases prove X-visible effects; they do
not certify physical delivery, namespace effect confinement or native debt.
"""
import select
import socket
import struct
import time

from wire import Client


def client(context, denied=False):
    prefix = 'denied_' if denied else ''
    return Client(context['socket'], context['order'], context['deadline'],
                  auth_name=context.get(prefix + 'auth_name', b''),
                  auth_data=context.get(prefix + 'auth_data', b''))


def extension_names(c):
    reply = c.reply(99)
    names, offset = [], 32
    for _ in range(reply[1]):
        assert offset < len(reply), 'truncated ListExtensions name length'
        size = reply[offset]
        offset += 1
        assert offset + size <= len(reply), 'truncated ListExtensions name'
        names.append(reply[offset:offset + size].decode('ascii'))
        offset += size
    assert len(names) == len(set(names)), 'duplicate extension names'
    return names


def major(c):
    reply = c.query_extension('XTEST')
    assert reply[8] == 1, 'authorized private connection has no XTEST'
    assert reply[9] >= 128, 'extension repurposed a core opcode'
    assert reply[10:12] == b'\0\0', 'XTEST invents events or errors'
    assert 'XTEST' in extension_names(c), 'QueryExtension/ListExtensions disagree'
    return reply[9]


def fake_body(c, kind, detail=0, delay=0, root=0, x=0, y=0, padding=0):
    return c.pack('BBHI I 8x hh 7x B', kind, detail, 0, delay, root, x, y, padding)


def fake(c, opcode, kind, **fields):
    return c.send(opcode, fake_body(c, kind, **fields), detail=2)


def query_pointer(c):
    reply = c.reply(38, c.pack('I', c.root))
    assert reply[1] == 1, 'pointer is not on the private test screen'
    return c.unpack('hh', reply, 16)


def target(c):
    window = c.window(events=(1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 6))
    c.send(8, c.pack('I', window))
    c.send(42, c.pack('II', window, 0), detail=0)
    assert c.u32(c.sync(), 8) == window, 'private Session did not commit requested focus'
    return window


def key_event(c, kind, window, key=38):
    event = ordered_input_event(c, kind, window)
    assert event[0] == kind, 'FakeInput became a SendEvent synthetic event'
    assert event[1] == key, ('wrong keycode', event.hex())
    assert c.u32(event, 8) == c.root
    return event


def ordered_input_event(c, kind, window):
    family = (2, 3) if kind in (2, 3) else (4, 5)
    while True:
        for index, data in enumerate(c.events):
            if data[0] & 127 in family and c.u32(data, 12) == window:
                event = c.events.pop(index)
                assert event[0] == kind, 'input transition was out of order or SendEvent'
                return event
        data = c.record()
        assert data[0] >= 2, 'unexpected completion while awaiting input'
        c.events.append(data)
        assert len(c.events) <= 256, 'unbounded input backlog'


def no_input_yet(c, window, interval, kinds=(2, 3, 4, 5)):
    """Check buffered and newly arriving input against one absolute deadline."""
    until = min(c.deadline, time.monotonic() + interval)
    while True:
        assert not any(data[0] & 127 in kinds and c.u32(data, 12) == window
                       for data in c.events), 'unexpected early or duplicate input transition'
        remaining = until - time.monotonic()
        if remaining <= 0:
            c.remaining()  # Exhausting the case deadline is not a quiet PASS.
            return
        ready, _, _ = select.select([c.sock], [], [], remaining)
        if not ready:
            c.remaining()
            return
        original = c.deadline
        c.deadline = until
        try:
            data = c.record()
        finally:
            c.deadline = original
        assert data[0] >= 2, 'unexpected completion during input silence check'
        c.events.append(data)
        assert len(c.events) <= 256, 'unbounded input backlog'


def sync_before(c, deadline):
    original = c.deadline
    c.deadline = min(original, deadline)
    try:
        c.sync()
        assert time.monotonic() < deadline, 'healthy peer waited for another client delay'
    finally:
        c.deadline = original


def no_reply_yet(c, interval):
    ready, _, _ = select.select([c.sock], [], [], min(interval, c.remaining()))
    assert not ready, 'later request completed before the delayed input/server grab'


def discovery(context):
    with client(context) as c:
        opcode = major(c)
        for requested in (1, 2, 65535):
            reply = c.completion(c.send(opcode, c.pack('BBH', 2, 0, requested), detail=0))
            assert reply[1] == 2 and c.u16(reply, 8) == 1, 'server must negotiate XTEST 2.1'
            assert c.u32(reply, 4) == 0


def denied(context):
    # The unauthorized connection still has core X access. The grant is
    # distinct from setup authentication and resource access.
    with client(context, denied=True) as c:
        reply = c.query_extension('XTEST')
        assert reply[8] == 0 and 'XTEST' not in extension_names(c)
        opcode = context.get('known_xtest_major', 146)
        before = query_pointer(c)
        x, y = (13, 14) if before == (11, 12) else (11, 12)
        for minor, body in ((0, c.pack('BBH', 2, 0, 1)),
                            (1, c.pack('II', c.root, 0)),
                            (2, fake_body(c, 6, x=x, y=y)),
                            (3, c.pack('B3x', 1))):
            c.completion(c.send(opcode, body, detail=minor), error=10,
                         opcode=opcode, minor=minor)
        c.sync()
        assert query_pointer(c) == before, 'denied FakeInput changed pointer despite BadAccess'


def setup_request(order, name, data):
    assert isinstance(name, bytes) and isinstance(data, bytes)
    assert len(name) <= 65535 and len(data) <= 65535
    prefix = bytes([ord('l' if order == '<' else 'B'), 0])
    return (prefix + struct.pack(order + 'HHHHH', 11, 0, len(name), len(data), 0)
            + name + bytes((-len(name)) % 4) + data + bytes((-len(data)) % 4))


def expect_setup_refused(sock, order, deadline):
    def read(size):
        result = bytearray()
        while len(result) < size:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError('absolute setup-refusal deadline expired')
            sock.settimeout(remaining)
            chunk = sock.recv(size - len(result))
            if not chunk:
                raise EOFError('setup refusal ended before the failure record')
            result.extend(chunk)
        return bytes(result)
    prefix = read(8)
    # Never include the reply or credential in an assertion message: a broken
    # server may echo the secret into its failure reason.
    assert prefix[0] == 0, 'supplied invalid credentials fell through to successful/continuing setup'
    assert struct.unpack_from(order + 'H', prefix, 2)[0] == 11, 'wrong setup-refusal version'
    size = struct.unpack_from(order + 'H', prefix, 6)[0] * 4
    assert prefix[1] <= size, 'truncated setup-refusal reason'
    read(size)


def setup_authorization(context):
    name, data = context.get('auth_name', b''), context.get('auth_data', b'')
    assert name and data, 'setup-authorization case needs the private runner credential'
    changed_name = bytes([name[0] ^ 1]) + name[1:]
    changed_data = bytes([data[0] ^ 1]) + data[1:]
    supplied_name, supplied_data = {
        'xtest_setup_wrong_name': (changed_name, data),
        'xtest_setup_wrong_data': (name, changed_data),
        'xtest_setup_missing_name': (b'', data),
        'xtest_setup_missing_data': (name, b''),
    }[context['case']]
    with client(context) as authorized:
        major(authorized)  # Positive control: exact credentials grant XTEST.
        with Client(context['socket'], context['order'], context['deadline'],
                    auth_name=b'', auth_data=b'') as anonymous:
            assert anonymous.query_extension('XTEST')[8] == 0
            assert 'XTEST' not in extension_names(anonymous)
            anonymous.sync()  # Empty/empty remains a healthy ungranted client.
        with socket.socket(socket.AF_UNIX) as rejected:
            rejected.settimeout(authorized.remaining())
            rejected.connect(str(context['socket']))
            rejected.sendall(setup_request(context['order'], supplied_name, supplied_data))
            expect_setup_refused(rejected, context['order'], context['deadline'])
        authorized.sync()  # An invalid setup must not kill existing clients.


def key_pair(context):
    with client(context) as observer, client(context) as injector:
        window, opcode = target(observer), major(injector)
        fake(injector, opcode, 2, detail=38)
        fake(injector, opcode, 3, detail=38)
        injector.sync()
        key_event(observer, 2, window)
        key_event(observer, 3, window)
        no_input_yet(observer, window, .04)


def padding(context):
    with client(context) as c:
        opcode = major(c)
        fake(c, opcode, 6, x=23, y=31, padding=255)
        assert query_pointer(c) == (23, 31), '2.1 padding was interpreted as device selection'


def motion(context):
    with client(context) as observer, client(context) as injector:
        opcode = major(injector)
        observer.send(2, observer.pack('III', observer.root, 1 << 11, 1 << 6))
        observer.sync()
        fake(injector, opcode, 6, root=0, x=23, y=31)
        assert query_pointer(injector) == (23, 31), 'Root None absolute motion failed'
        event = observer.event(6)
        assert observer.unpack('hh', event, 20) == (23, 31)
        fake(injector, opcode, 6, detail=1, root=injector.root, x=7, y=-5)
        assert query_pointer(injector) == (30, 26), 'relative motion was not relative'
        event = observer.event(6)
        assert observer.unpack('hh', event, 20) == (30, 26)
        geometry = injector.reply(14, injector.pack('I', injector.root))
        width, height = injector.unpack('HH', geometry, 16)
        assert width > 30 and height > 31, 'private topology is too small for motion fixture'
        fake(injector, opcode, 6, root=injector.root, x=-32768, y=-32768)
        assert query_pointer(injector) == (0, 0), 'negative off-screen motion was not clipped'
        fake(injector, opcode, 6, root=injector.root, x=32767, y=32767)
        assert query_pointer(injector) == (width - 1, height - 1), 'positive motion not clipped'


def button_pair(context):
    with client(context) as observer, client(context) as injector:
        window, opcode = target(observer), major(injector)
        translated = observer.reply(40, observer.pack('IIhh', window, observer.root, 10, 10))
        assert translated[1] == 1
        x, y = observer.unpack('hh', translated, 12)
        fake(injector, opcode, 6, x=x, y=y)
        fake(injector, opcode, 4, detail=1)
        fake(injector, opcode, 5, detail=1)
        injector.sync()
        press = ordered_input_event(observer, 4, window)
        release = ordered_input_event(observer, 5, window)
        assert press[0] == 4 and release[0] == 5
        assert press[1] == release[1] == 1
        assert not observer.u16(press, 28) & (1 << 8), 'press reports post-transition state'
        assert observer.u16(release, 28) & (1 << 8), 'release lost prior button state'
        no_input_yet(observer, window, .04)


def fake_errors(context):
    with client(context) as c:
        opcode, child = major(c), c.window()
        for body, error in ((fake_body(c, 0), 2), (fake_body(c, 35), 2),
                            (fake_body(c, 2, detail=0), 2),
                            (fake_body(c, 4, detail=0), 2),
                            (fake_body(c, 4, detail=255), 2),
                            (fake_body(c, 6, detail=2), 2),
                            (fake_body(c, 6, root=c.xid()), 3),
                            (fake_body(c, 6, root=child), 2),
                            (fake_body(c, 6)[:-4], 16),
                            (fake_body(c, 6) + bytes(4), 16)):
            c.completion(c.send(opcode, body, detail=2), error=error, opcode=opcode, minor=2)
            c.sync()
        fake(c, opcode, 6, x=17, y=19)
        assert query_pointer(c) == (17, 19), 'malformed request poisoned healthy continuation'


def request_errors(context):
    with client(context) as c:
        opcode = major(c)
        for minor, body in ((0, b''), (0, bytes(8)), (1, bytes(4)),
                            (1, bytes(12)), (3, b''), (3, bytes(8))):
            c.completion(c.send(opcode, body, detail=minor), error=16,
                         opcode=opcode, minor=minor)
            c.sync()
        c.completion(c.send(opcode, detail=255), error=1, opcode=opcode, minor=255)
        c.sync()


def compare_cursor(context):
    with client(context) as c:
        opcode, window, pixmap, cursor = major(c), c.window(), c.xid(), c.xid()
        c.send(53, c.pack('IIHH', pixmap, c.root, 1, 1), detail=1)
        c.send(93, c.pack('IIIHHHHHHHH', cursor, pixmap, 0, 0, 0, 0, 65535, 65535, 65535, 0, 0))
        c.send(2, c.pack('III', window, 1 << 14, cursor))
        c.sync()
        def same(win, cur):
            reply = c.completion(c.send(opcode, c.pack('II', win, cur), detail=1))
            assert reply[1] in (0, 1)
            return reply[1]
        assert same(window, cursor) == 1
        assert same(window, 0) == 0
        c.send(8, c.pack('I', window))
        c.sync()
        translated = c.reply(40, c.pack('IIhh', window, c.root, 10, 10))
        x, y = c.unpack('hh', translated, 12)
        fake(c, opcode, 6, x=x, y=y)
        c.sync()
        assert same(window, 1) == 1, 'CurrentCursor is not the pointer cursor'
        for win, cur, error in ((c.xid(), 0, 3), (c.xid(), 1, 3),
                                (window, c.xid(), 6)):
            c.completion(c.send(opcode, c.pack('II', win, cur), detail=1),
                         error=error, opcode=opcode, minor=1)
        c.sync()


def grab_control(context):
    with client(context) as owner, client(context) as impervious, client(context) as ordinary:
        opcode = major(impervious)
        impervious.send(opcode, impervious.pack('B3x', 1), detail=3)
        impervious.sync()
        owner.send(36)  # GrabServer
        owner.sync()
        try:
            ordinary_sequence = ordinary.send(43)
            # This reply proves processing continues for the impervious client.
            impervious.sync()
            no_reply_yet(ordinary, .04)
        finally:
            owner.send(37)
            owner.sync()
        ordinary.completion(ordinary_sequence)
        impervious.send(opcode, impervious.pack('B3x', 0), detail=3)
        impervious.sync()
        owner.send(36)
        owner.sync()
        try:
            sequence = impervious.send(43)
            no_reply_yet(impervious, .04)
        finally:
            owner.send(37)
            owner.sync()
        impervious.completion(sequence)
        impervious.completion(impervious.send(opcode, impervious.pack('B3x', 2), detail=3),
                              error=2, opcode=opcode, minor=3)


def delayed(context):
    with client(context) as observer, client(context) as injector, client(context) as peer:
        window, opcode = target(observer), major(injector)
        started = time.monotonic()
        # Leave a substantial wall-clock gap between peer progress and expiry.
        # This is socket scheduling evidence, not a native timing guarantee.
        fake(injector, opcode, 2, detail=38, delay=1000)
        sequence = injector.send(43)
        no_reply_yet(injector, .05)
        sync_before(peer, started + .60)
        no_input_yet(observer, window, max(0, started + .95 - time.monotonic()))
        injector.completion(sequence)
        assert time.monotonic() - started >= .95, 'FakeInput delay completed early'
        key_event(observer, 2, window)
        fake(injector, opcode, 3, detail=38)
        injector.sync()
        key_event(observer, 3, window)
        no_input_yet(observer, window, .04)


def half_close(context):
    with client(context) as observer, client(context) as injector:
        window, opcode = target(observer), major(injector)
        started = time.monotonic()
        fake(injector, opcode, 2, detail=38, delay=500)
        fake(injector, opcode, 3, detail=38)
        sequence = injector.send(43)
        injector.sock.shutdown(socket.SHUT_WR)
        no_reply_yet(injector, .05)
        no_input_yet(observer, window, max(0, started + .45 - time.monotonic()))
        injector.completion(sequence)
        assert time.monotonic() - started >= .45, 'half-close bypassed FakeInput delay'
        key_event(observer, 2, window)
        key_event(observer, 3, window)
        no_input_yet(observer, window, .04)
        injector.sock.settimeout(injector.remaining())
        assert injector.sock.recv(1) == b'', 'write-half-close never completed after buffered requests'


def full_delay_disconnect(context):
    with client(context) as observer, client(context) as injector:
        target(observer)
        opcode = major(injector)
        before = query_pointer(observer)
        fake(injector, opcode, 6, delay=0xffffffff, x=73, y=47)
        injector.send(43)
        no_reply_yet(injector, .05)
        injector.close()
        for _ in range(3):
            observer.sync()
        assert query_pointer(observer) == before, 'cancelled CARD32 delay changed pointer'
        with client(context) as replacement:
            replacement_opcode = major(replacement)
            fake(replacement, replacement_opcode, 6, x=29, y=37)
            assert query_pointer(replacement) == (29, 37), 'departed delayed client blocked replacement'


def disconnect_release(context):
    with client(context) as observer, client(context) as injector:
        window, opcode = target(observer), major(injector)
        fake(injector, opcode, 2, detail=38)
        injector.sync()
        key_event(observer, 2, window)
        injector.close()
        key_event(observer, 3, window)
        observer.sync()
        no_input_yet(observer, window, .04)


def two_injectors(context):
    with client(context) as observer, client(context) as first, client(context) as second:
        window, first_opcode, second_opcode = target(observer), major(first), major(second)
        fake(first, first_opcode, 2, detail=50)
        first.sync()
        key_event(observer, 2, window, key=50)
        fake(second, second_opcode, 2, detail=50)
        second.sync()
        first.close()
        # This is a bounded observation of extra aggregate transitions, not a
        # disconnect-cleanup barrier. The watcher's round trip cannot establish
        # another connection's teardown order; deterministic native teardown
        # tests must prove that ordering independently.
        observer.sync()
        no_reply_yet(observer, .05)
        assert not observer.events, 'joining/retiring source changed aggregate key state'
        fake(second, second_opcode, 3, detail=50)
        second.sync()
        key_event(observer, 3, window, key=50)
        observer.sync()
        assert not observer.events, 'duplicate aggregate transition'
        no_reply_yet(observer, .04)


CASES = {
    'xtest_discovery': discovery,
    'xtest_disabled': denied,
    'xtest_unauthorized': denied,
    'xtest_setup_wrong_name': setup_authorization,
    'xtest_setup_wrong_data': setup_authorization,
    'xtest_setup_missing_name': setup_authorization,
    'xtest_setup_missing_data': setup_authorization,
    'xtest_key_pair': key_pair,
    'xtest_button_pair': button_pair,
    'xtest_motion': motion,
    'xtest_padding': padding,
    'xtest_fake_errors': fake_errors,
    'xtest_request_errors': request_errors,
    'xtest_compare_cursor': compare_cursor,
    'xtest_grab_control': grab_control,
    'xtest_delay_order': delayed,
    'xtest_half_close': half_close,
    'xtest_full_delay_disconnect': full_delay_disconnect,
    'xtest_disconnect_release': disconnect_release,
    'xtest_two_injectors': two_injectors,
}

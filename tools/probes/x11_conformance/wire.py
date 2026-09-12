"""Independent X11 core client, written from the X11 protocol specification.

No Sophia encoder, decoder, constants, generated bindings or observation API.
All waits use an absolute case deadline, including asynchronous events.
"""
import socket
import struct
import time


class Client:
    def __init__(self, path, order, deadline, *, auth_name=b'', auth_data=b''):
        if len(auth_name) > 65535 or len(auth_data) > 65535:
            raise ValueError('X11 setup authorization field exceeds CARD16')
        self.order, self.deadline = order, deadline
        self.sock = socket.socket(socket.AF_UNIX)
        try:
            self.setup(path, auth_name, auth_data)
        except BaseException:
            self.sock.close()
            raise

    def setup(self, path, auth_name, auth_data):
        self.sock.settimeout(self.remaining())
        self.sock.connect(str(path))
        self.sequence, self.events = 0, []
        self.sock.sendall(bytes([ord('l' if self.order == '<' else 'B'), 0]) +
                          self.pack('HHHHH', 11, 0, len(auth_name), len(auth_data), 0) +
                          auth_name + bytes(-len(auth_name) % 4) +
                          auth_data + bytes(-len(auth_data) % 4))
        prefix = self.read(8)
        data = self.read(self.u16(prefix, 6) * 4)
        assert prefix[0] == 1, f'X11 setup failed with status {prefix[0]}'
        assert self.u16(prefix, 2) == 11
        self.base, self.mask = self.unpack('II', data, 4)
        vendor_len = self.u16(data, 16)
        assert data[20] > 0, 'setup has no root'
        screen = 32 + ((vendor_len + 3) & ~3) + data[21] * 8
        self.root = self.u32(data, screen)
        self.depth = data[screen + 38]
        self.next_id = 1

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()

    def close(self):
        self.sock.close()

    def remaining(self):
        left = self.deadline - time.monotonic()
        if left <= 0:
            raise TimeoutError('absolute case deadline expired')
        return left

    def pack(self, fmt, *values):
        return struct.pack(self.order + fmt, *values)

    def unpack(self, fmt, data, offset=0):
        return struct.unpack_from(self.order + fmt, data, offset)

    def u16(self, data, offset):
        return self.unpack('H', data, offset)[0]

    def u32(self, data, offset):
        return self.unpack('I', data, offset)[0]

    def read(self, size):
        assert size <= 1024 * 1024, f'unbounded X11 record: {size}'
        result = bytearray()
        while len(result) < size:
            self.sock.settimeout(self.remaining())
            chunk = self.sock.recv(size - len(result))
            if not chunk:
                raise EOFError('peer closed before mandatory completion')
            result.extend(chunk)
        return bytes(result)

    def send(self, opcode, body=b'', detail=0):
        body += bytes((-len(body)) % 4)
        self.sequence = (self.sequence + 1) & 0xffff
        self.sock.settimeout(self.remaining())
        self.sock.sendall(bytes([opcode, detail]) + self.pack('H', 1 + len(body)//4) + body)
        return self.sequence

    def record(self):
        data = self.read(32)
        if data[0] == 1 or data[0] & 127 == 35:
            data += self.read(self.u32(data, 4) * 4)
        return data

    def completion(self, sequence, error=None, opcode=None, resource=None, minor=None):
        while True:
            data = self.record()
            if data[0] >= 2:
                self.events.append(data)
                assert len(self.events) <= 256, 'unbounded events before completion'
                continue
            assert self.u16(data, 2) == sequence, ('wrong completion sequence', sequence, data.hex())
            if error is None:
                assert data[0] == 1, f'unexpected X error: {data.hex()}'
            else:
                assert data[0] == 0 and data[1] == error, ('wrong error', error, data.hex())
                assert data[10] == opcode, ('wrong error major opcode', data.hex())
                if minor is not None:
                    assert self.u16(data, 8) == minor, ('wrong error minor opcode', data.hex())
                if resource is not None:
                    assert self.u32(data, 4) == resource, ('wrong error resource', data.hex())
            return data

    def reply(self, opcode, body=b'', detail=0):
        return self.completion(self.send(opcode, body, detail))

    def sync(self):
        return self.reply(43)  # GetInputFocus is a real protocol round trip.

    def event(self, kind, predicate=lambda _: True):
        while True:
            for index, data in enumerate(self.events):
                if data[0] & 127 == kind and predicate(data):
                    return self.events.pop(index)
            data = self.record()
            assert data[0] >= 2, f'unexpected completion while awaiting event {kind}: {data.hex()}'
            self.events.append(data)
            assert len(self.events) <= 256, 'unbounded event backlog'

    def xid(self):
        value = self.base | self.next_id
        assert self.next_id & ~self.mask == 0
        self.next_id += 1
        return value

    def window(self, parent=None, events=(1 << 17) | (1 << 22) | (1 << 21), xid=None):
        wid = self.xid() if xid is None else xid
        body = self.pack('IIhhHHHHII', wid, parent or self.root, 7, 9, 80, 60, 0, 1, 0,
                         (1 << 9) | (1 << 11)) + self.pack('II', 1, events)
        self.send(1, body)
        self.sync()
        return wid

    def atom(self, name):
        value = name.encode('ascii')
        reply = self.reply(16, self.pack('HH', len(value), 0) + value)
        atom = self.u32(reply, 8)
        assert atom != 0, 'InternAtom returned None'
        return atom

    def query_extension(self, name):
        value = name.encode('ascii')
        return self.reply(98, self.pack('HH', len(value), 0) + value)

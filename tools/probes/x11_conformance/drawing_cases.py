"""Pixmap, graphics-context, drawing and image obligations.

Every pixel assertion reads the drawable back through GetImage, so a request
is judged by what it left in the drawable, never by the absence of an error.
Expectations come from the X11 protocol specification's own pixelization
rules: filled rectangles cover [x, x+width) by [y, y+height), thin horizontal,
vertical and 45-degree lines pass through pixel centers, and an arc is
inscribed in its rectangle so the box corners lie outside a full disc while
its center lies inside.
"""
from wire import Client

# Core request numbers and error codes, from the protocol encoding.
CREATE_WINDOW, MAP_WINDOW, GET_GEOMETRY = 1, 8, 14
CREATE_PIXMAP, FREE_PIXMAP, CREATE_GC, CHANGE_GC, COPY_GC, SET_DASHES = 53, 54, 55, 56, 57, 58
SET_CLIP_RECTANGLES, FREE_GC, CLEAR_AREA, COPY_AREA, COPY_PLANE = 59, 60, 61, 62, 63
POLY_POINT, POLY_LINE, POLY_SEGMENT, POLY_RECTANGLE, POLY_ARC = 64, 65, 66, 67, 68
FILL_POLY, POLY_FILL_RECTANGLE, POLY_FILL_ARC, PUT_IMAGE, GET_IMAGE = 69, 70, 71, 72, 73
QUERY_BEST_SIZE = 97
BAD_VALUE, BAD_WINDOW, BAD_PIXMAP, BAD_FONT, BAD_MATCH = 2, 3, 4, 7, 8
BAD_DRAWABLE, BAD_GC, BAD_ID_CHOICE = 9, 13, 14
EXPOSE, GRAPHICS_EXPOSURE, NO_EXPOSURE, MAP_NOTIFY = 12, 13, 14, 19
GC_FOREGROUND, GC_BACKGROUND, GC_LINE_STYLE, GC_TILE, GC_FONT = 1 << 2, 1 << 3, 1 << 5, 1 << 10, 1 << 14
GC_GRAPHICS_EXPOSURES, GC_CLIP_MASK = 1 << 16, 1 << 19
GC_LINE_WIDTH, GC_CAP_STYLE, GC_JOIN_STYLE = 1 << 4, 1 << 6, 1 << 7
CAP_BUTT, CAP_PROJECTING, JOIN_MITER, JOIN_BEVEL = 1, 3, 0, 2
BITMAP, XY_PIXMAP, Z_PIXMAP = 0, 1, 2
CW_BACKGROUND_PIXEL, CW_OVERRIDE_REDIRECT, CW_EVENT_MASK = 1 << 1, 1 << 9, 1 << 11
INPUT_OUTPUT, INPUT_ONLY = 1, 2
EXPOSURE_MASK, STRUCTURE_NOTIFY_MASK = 1 << 15, 1 << 17


def client(context):
    return Client(context['socket'], context['order'], context['deadline'])


def pixmap(c, width, height, depth=None, drawable=None, xid=None):
    pid = c.xid() if xid is None else xid
    c.send(CREATE_PIXMAP, c.pack('IIHH', pid, drawable or c.root, width, height),
           detail=c.depth if depth is None else depth)
    c.sync()
    return pid


def gc(c, drawable, foreground, background=0):
    cid = c.xid()
    c.send(CREATE_GC, c.pack('III', cid, drawable, GC_FOREGROUND | GC_BACKGROUND)
           + c.pack('II', foreground, background))
    c.sync()
    return cid


def set_foreground(c, gcid, foreground):
    c.send(CHANGE_GC, c.pack('II', gcid, GC_FOREGROUND) + c.pack('I', foreground))


def fill(c, drawable, gcid, *rectangles):
    c.send(POLY_FILL_RECTANGLE, c.pack('II', drawable, gcid)
           + b''.join(c.pack('hhHH', *rectangle) for rectangle in rectangles))


def input_only_window(c):
    wid = c.xid()
    c.send(CREATE_WINDOW, c.pack('IIhhHHHHII', wid, c.root, 0, 0, 10, 10, 0, INPUT_ONLY, 0, 0))
    c.sync()
    return wid


def mapped_window(c, background, width=8, height=4, events=EXPOSURE_MASK | STRUCTURE_NOTIFY_MASK):
    wid = c.xid()
    mask = CW_BACKGROUND_PIXEL | CW_OVERRIDE_REDIRECT | CW_EVENT_MASK
    c.send(CREATE_WINDOW, c.pack('IIhhHHHHII', wid, c.root, 7, 9, width, height, 0, INPUT_OUTPUT, 0, mask)
           + c.pack('III', background, 1, events), detail=c.depth)
    c.send(MAP_WINDOW, c.pack('I', wid))
    c.sync()
    c.event(MAP_NOTIFY, lambda e: c.u32(e, 8) == wid)
    # Mapping may expose the whole window. Those events are the map's, not a
    # later request's, so they are set aside without being required.
    c.sync()
    c.events = [event for event in c.events if event[0] & 127 != EXPOSE]
    return wid


def pixel_bytes(c, values):
    return b''.join(value.to_bytes(4, 'little' if c.image_byte_order == 0 else 'big') for value in values)


def pixels(c, drawable, width, height, plane_mask=0xffffffff, x=0, y=0):
    """Rows of pixel values in the drawable's depth, read back as ZPixmap."""
    reply = c.reply(GET_IMAGE, c.pack('IhhHHI', drawable, x, y, width, height, plane_mask), detail=Z_PIXMAP)
    bits_per_pixel, scanline_pad = c.formats[reply[1]]
    assert bits_per_pixel == 32 and scanline_pad == 32, ('this oracle reads 32-bit rows', bits_per_pixel)
    data = reply[32:32 + c.u32(reply, 4) * 4]
    assert len(data) >= width * height * 4, 'GetImage returned fewer bytes than the rectangle holds'
    mask = (1 << reply[1]) - 1
    order = 'little' if c.image_byte_order == 0 else 'big'
    rows = [[int.from_bytes(data[(row * width + column) * 4:][:4], order) & mask
             for column in range(width)] for row in range(height)]
    return rows, reply


def painted(c, drawable, width, height):
    return {(x, y) for y, row in enumerate(pixels(c, drawable, width, height)[0])
            for x, value in enumerate(row) if value}


def exposed(c, kind, drawable, major=None):
    """The union of one contiguous run of exposure events, ended by count zero."""
    covered = set()
    while True:
        event = c.event(kind, lambda e: c.u32(e, 4) == drawable)
        x, y, width, height = c.unpack('HHHH', event, 8)
        covered |= {(x + i, y + j) for i in range(width) for j in range(height)}
        if kind == GRAPHICS_EXPOSURE:
            assert event[20] == major, ('GraphicsExposure names its request', event.hex())
            count = c.u16(event, 18)
        else:
            count = c.u16(event, 16)
        if count == 0:
            return covered


def no_events(c, *kinds):
    c.sync()
    return not any(event[0] & 127 in kinds for event in c.events)


def pixmap_lifecycle(context):
    with client(context) as c:
        alive = c.window(events=0)
        pid = pixmap(c, 8, 4)
        paint = gc(c, pid, 0x123456)
        fill(c, pid, paint, (0, 0, 8, 4))
        set_foreground(c, paint, 0x00ff00)
        fill(c, pid, paint, (2, 1, 3, 2))
        rows, reply = pixels(c, pid, 8, 4)
        assert reply[1] == c.depth and c.u32(reply, 8) == 0, 'a pixmap reports its depth and visual None'
        assert rows == [[0x00ff00 if 2 <= x < 5 and 1 <= y < 3 else 0x123456 for x in range(8)]
                        for y in range(4)]
        # Zero extents and an unsupported depth are Value errors.
        for width, height, depth in ((0, 4, c.depth), (8, 0, c.depth)):
            c.completion(c.send(CREATE_PIXMAP, c.pack('IIHH', c.xid(), c.root, width, height), detail=depth),
                         error=BAD_VALUE, opcode=CREATE_PIXMAP)
        c.completion(c.send(CREATE_PIXMAP, c.pack('IIHH', c.xid(), c.root, 8, 4), detail=99),
                     error=BAD_VALUE, opcode=CREATE_PIXMAP, resource=99)
        bad = c.xid()
        c.completion(c.send(CREATE_PIXMAP, c.pack('IIHH', c.xid(), bad, 8, 4), detail=c.depth),
                     error=BAD_DRAWABLE, opcode=CREATE_PIXMAP, resource=bad)
        c.completion(c.send(CREATE_PIXMAP, c.pack('IIHH', pid, c.root, 8, 4), detail=c.depth),
                     error=BAD_ID_CHOICE, opcode=CREATE_PIXMAP, resource=pid)
        # An InputOnly window is a legal drawable argument here, and depth-one
        # pixmaps are the bitmaps every cursor and mask is built from.
        via_input_only = pixmap(c, 4, 4, drawable=input_only_window(c))
        bitmap = pixmap(c, 4, 4, depth=1)
        for freed in (pid, via_input_only, bitmap):
            c.send(FREE_PIXMAP, c.pack('I', freed))
        c.sync()
        c.completion(c.send(GET_IMAGE, c.pack('IhhHHI', pid, 0, 0, 1, 1, 0xffffffff), detail=Z_PIXMAP),
                     error=BAD_DRAWABLE, opcode=GET_IMAGE, resource=pid)
        c.completion(c.send(FREE_PIXMAP, c.pack('I', pid)), error=BAD_PIXMAP, opcode=FREE_PIXMAP, resource=pid)
        # The freed identifier may be chosen again.
        pixmap(c, 2, 2, xid=pid)
        assert c.u16(c.reply(GET_GEOMETRY, c.pack('I', alive)), 16) == 80


def gc_lifecycle(context):
    with client(context) as c:
        alive = c.window(events=0)
        pid = pixmap(c, 4, 2)
        first, second = gc(c, pid, 0x111111), gc(c, pid, 0x222222)
        fill(c, pid, first, (0, 0, 4, 2))
        assert pixels(c, pid, 4, 2)[0] == [[0x111111] * 4] * 2
        set_foreground(c, first, 0x333333)
        fill(c, pid, first, (0, 0, 4, 1))
        # CopyGC carries only the selected components; an empty mask is legal.
        c.send(COPY_GC, c.pack('III', first, second, GC_FOREGROUND))
        fill(c, pid, second, (0, 1, 4, 1))
        assert pixels(c, pid, 4, 2)[0] == [[0x333333] * 4] * 2
        c.send(COPY_GC, c.pack('III', first, second, 0))
        c.sync()
        # A gcontext is bound to its drawable's root and depth.
        bitmap = pixmap(c, 4, 2, depth=1)
        shallow = gc(c, bitmap, 1)
        c.completion(c.send(POLY_FILL_RECTANGLE, c.pack('II', pid, shallow) + c.pack('hhHH', 0, 0, 1, 1)),
                     error=BAD_MATCH, opcode=POLY_FILL_RECTANGLE)
        c.completion(c.send(COPY_GC, c.pack('III', shallow, first, GC_FOREGROUND)),
                     error=BAD_MATCH, opcode=COPY_GC)
        bad = c.xid()
        c.completion(c.send(CREATE_GC, c.pack('III', c.xid(), bad, 0)),
                     error=BAD_DRAWABLE, opcode=CREATE_GC, resource=bad)
        c.completion(c.send(CREATE_GC, c.pack('III', first, pid, 0)),
                     error=BAD_ID_CHOICE, opcode=CREATE_GC, resource=first)
        c.completion(c.send(CREATE_GC, c.pack('III', c.xid(), pid, 1) + c.pack('I', 16)),
                     error=BAD_VALUE, opcode=CREATE_GC, resource=16)
        c.completion(c.send(CHANGE_GC, c.pack('II', first, GC_LINE_STYLE) + c.pack('I', 3)),
                     error=BAD_VALUE, opcode=CHANGE_GC, resource=3)
        c.completion(c.send(CHANGE_GC, c.pack('II', first, GC_TILE) + c.pack('I', bad)),
                     error=BAD_PIXMAP, opcode=CHANGE_GC, resource=bad)
        c.completion(c.send(CHANGE_GC, c.pack('II', first, GC_FONT) + c.pack('I', bad)),
                     error=BAD_FONT, opcode=CHANGE_GC, resource=bad)
        c.completion(c.send(CHANGE_GC, c.pack('II', first, GC_TILE) + c.pack('I', bitmap)),
                     error=BAD_MATCH, opcode=CHANGE_GC)
        # A freed context refuses use and a second free.
        c.send(FREE_GC, c.pack('I', first))
        c.sync()
        c.completion(c.send(POLY_FILL_RECTANGLE, c.pack('II', pid, first) + c.pack('hhHH', 0, 0, 1, 1)),
                     error=BAD_GC, opcode=POLY_FILL_RECTANGLE, resource=first)
        c.completion(c.send(FREE_GC, c.pack('I', first)), error=BAD_GC, opcode=FREE_GC, resource=first)
        assert c.u16(c.reply(GET_GEOMETRY, c.pack('I', alive)), 16) == 80


def gc_dashes_clip(context):
    with client(context) as c:
        pid = pixmap(c, 8, 4)
        paint, back = gc(c, pid, 0xabcdef), gc(c, pid, 0)
        c.completion(c.send(SET_DASHES, c.pack('IHH', paint, 0, 0)), error=BAD_VALUE, opcode=SET_DASHES)
        c.completion(c.send(SET_DASHES, c.pack('IHH', paint, 0, 3) + bytes([4, 0, 2])),
                     error=BAD_VALUE, opcode=SET_DASHES)
        c.send(SET_DASHES, c.pack('IHH', paint, 1, 2) + bytes([4, 2]))
        c.sync()
        # A clip list confines output. The rectangle is relative to the clip origin.
        fill(c, pid, back, (0, 0, 8, 4))
        c.send(SET_CLIP_RECTANGLES, c.pack('Ihh', paint, 1, 1) + c.pack('hhHH', 1, 0, 3, 2), detail=0)
        fill(c, pid, paint, (0, 0, 8, 4))
        clipped = [[0xabcdef if 2 <= x < 5 and 1 <= y < 3 else 0 for x in range(8)] for y in range(4)]
        assert pixels(c, pid, 8, 4)[0] == clipped
        # An empty list disables output entirely, under every ordering.
        for ordering in (1, 2, 3):
            c.send(SET_CLIP_RECTANGLES, c.pack('Ihh', paint, 0, 0), detail=ordering)
            fill(c, pid, paint, (0, 0, 8, 4))
        assert pixels(c, pid, 8, 4)[0] == clipped
        c.completion(c.send(SET_CLIP_RECTANGLES, c.pack('Ihh', paint, 0, 0), detail=4),
                     error=BAD_VALUE, opcode=SET_CLIP_RECTANGLES, resource=4)
        # clip-mask None restores unclipped output.
        c.send(CHANGE_GC, c.pack('II', paint, GC_CLIP_MASK) + c.pack('I', 0))
        fill(c, pid, paint, (0, 0, 8, 4))
        assert pixels(c, pid, 8, 4)[0] == [[0xabcdef] * 8] * 4
        # A clip pixmap confines every primitive, not only fills: of each
        # request, only the mask's set pixels, (1, 0) and (2, 1), may land.
        mask = pixmap(c, 8, 4, depth=1)
        bit = gc(c, mask, 0)
        fill(c, mask, bit, (0, 0, 8, 4))
        set_foreground(c, bit, 1)
        fill(c, mask, bit, (1, 0, 1, 1), (2, 1, 1, 1))
        source = pixmap(c, 8, 4)
        fill(c, source, paint, (0, 0, 8, 4))
        c.send(CHANGE_GC, c.pack('II', paint, GC_GRAPHICS_EXPOSURES | GC_CLIP_MASK) + c.pack('II', 0, mask))
        admitted = {(1, 0), (2, 1)}
        rows = b''.join(c.pack('hhhh', 0, y, 7, y) for y in range(4))
        image = pixel_bytes(c, [0xabcdef] * 32)
        for name, draw, lands in (
                ('fill', lambda: fill(c, pid, paint, (0, 0, 8, 4)), admitted),
                ('segments', lambda: c.send(POLY_SEGMENT, c.pack('II', pid, paint) + rows), admitted),
                ('outline', lambda: c.send(POLY_RECTANGLE, c.pack('II', pid, paint) + c.pack('hhHH', 0, 0, 7, 3)),
                 {(1, 0)}),
                ('copy', lambda: c.send(COPY_AREA, c.pack('IIIhhhhHH', source, pid, paint, 0, 0, 0, 0, 8, 4)),
                 admitted),
                ('image', lambda: c.send(PUT_IMAGE, c.pack('IIHHhhBBH', pid, paint, 8, 4, 0, 0, 0, c.depth, 0)
                                         + image, detail=Z_PIXMAP), admitted)):
            fill(c, pid, back, (0, 0, 8, 4))
            draw()
            expected = [[0xabcdef if (x, y) in lands else 0 for x in range(8)] for y in range(4)]
            assert pixels(c, pid, 8, 4)[0] == expected, f'clip pixmap did not confine {name}'
        unknown = c.xid()
        c.completion(c.send(SET_DASHES, c.pack('IHH', unknown, 0, 1) + bytes([1])),
                     error=BAD_GC, opcode=SET_DASHES, resource=unknown)
        c.completion(c.send(SET_CLIP_RECTANGLES, c.pack('Ihh', unknown, 0, 0), detail=0),
                     error=BAD_GC, opcode=SET_CLIP_RECTANGLES, resource=unknown)


def clear_area(context):
    with client(context) as c:
        wid = mapped_window(c, background=0x0f0f0f)
        paint = gc(c, wid, 0xf0f0f0)
        fill(c, wid, paint, (0, 0, 8, 4))
        # exposures False clears to the background pixel and reports nothing.
        c.send(CLEAR_AREA, c.pack('IhhHH', wid, 2, 1, 3, 2), detail=0)
        assert pixels(c, wid, 8, 4)[0] == [[0x0f0f0f if 2 <= x < 5 and 1 <= y < 3 else 0xf0f0f0 for x in range(8)]
                                           for y in range(4)]
        assert no_events(c, EXPOSE), 'exposures False generates no Expose'
        # Zero width and height reach the window's edges; exposures True reports the region.
        c.send(CLEAR_AREA, c.pack('IhhHH', wid, 6, 3, 0, 0), detail=1)
        assert exposed(c, EXPOSE, wid) == {(6, 3), (7, 3)}
        assert pixels(c, wid, 8, 4)[0][3][6:] == [0x0f0f0f, 0x0f0f0f]
        c.completion(c.send(CLEAR_AREA, c.pack('IhhHH', wid, 0, 0, 1, 1), detail=2),
                     error=BAD_VALUE, opcode=CLEAR_AREA, resource=2)
        pid = pixmap(c, 2, 2)
        c.completion(c.send(CLEAR_AREA, c.pack('IhhHH', pid, 0, 0, 1, 1), detail=0),
                     error=BAD_WINDOW, opcode=CLEAR_AREA, resource=pid)
        c.completion(c.send(CLEAR_AREA, c.pack('IhhHH', input_only_window(c), 0, 0, 1, 1), detail=0),
                     error=BAD_MATCH, opcode=CLEAR_AREA)


def copy_area(context):
    with client(context) as c:
        source, destination = pixmap(c, 4, 2), pixmap(c, 6, 3)
        paint = gc(c, source, 0x101010)
        fill(c, source, paint, (0, 0, 4, 2))
        set_foreground(c, paint, 0x202020)
        fill(c, source, paint, (1, 0, 2, 1))
        set_foreground(c, paint, 0x303030)
        fill(c, destination, paint, (0, 0, 6, 3))
        c.send(COPY_AREA, c.pack('IIIhhhhHH', source, destination, paint, 0, 0, 1, 1, 4, 2))
        expected = [[0x303030] * 6,
                    [0x303030, 0x101010, 0x202020, 0x202020, 0x101010, 0x303030],
                    [0x303030] + [0x101010] * 4 + [0x303030]]
        assert pixels(c, destination, 6, 3)[0] == expected
        # graphics-exposures defaults to True: a wholly available source reports NoExposure.
        no_exposure = c.event(NO_EXPOSURE, lambda e: c.u32(e, 4) == destination)
        assert c.u16(no_exposure, 8) == 0 and no_exposure[10] == COPY_AREA, no_exposure.hex()
        assert no_events(c, GRAPHICS_EXPOSURE)
        # Source outside the pixmap is not copied and is reported exposed at the destination.
        c.send(COPY_AREA, c.pack('IIIhhhhHH', source, destination, paint, 2, 0, 0, 0, 4, 2))
        assert pixels(c, destination, 6, 3)[0] == [[0x202020, 0x101010] + expected[0][2:],
                                                   [0x101010, 0x101010] + expected[1][2:],
                                                   expected[2]]
        assert exposed(c, GRAPHICS_EXPOSURE, destination, COPY_AREA) == {(x, y) for x in (2, 3) for y in (0, 1)}
        assert no_events(c, NO_EXPOSURE)
        # graphics-exposures False silences both events.
        c.send(CHANGE_GC, c.pack('II', paint, GC_GRAPHICS_EXPOSURES) + c.pack('I', 0))
        c.send(COPY_AREA, c.pack('IIIhhhhHH', source, destination, paint, 2, 0, 0, 0, 4, 2))
        assert no_events(c, NO_EXPOSURE, GRAPHICS_EXPOSURE)
        # Depth, gcontext and drawable refusals.
        bitmap = pixmap(c, 4, 2, depth=1)
        c.completion(c.send(COPY_AREA, c.pack('IIIhhhhHH', bitmap, destination, paint, 0, 0, 0, 0, 1, 1)),
                     error=BAD_MATCH, opcode=COPY_AREA)
        bad = c.xid()
        c.completion(c.send(COPY_AREA, c.pack('IIIhhhhHH', source, destination, bad, 0, 0, 0, 0, 1, 1)),
                     error=BAD_GC, opcode=COPY_AREA, resource=bad)
        c.completion(c.send(COPY_AREA, c.pack('IIIhhhhHH', bad, destination, paint, 0, 0, 0, 0, 1, 1)),
                     error=BAD_DRAWABLE, opcode=COPY_AREA, resource=bad)


def copy_plane(context):
    with client(context) as c:
        source, destination = pixmap(c, 4, 1), pixmap(c, 4, 1)
        paint = gc(c, source, 0)
        for x, value in enumerate((0x010000, 0x000100, 0x030000, 0x000000)):
            set_foreground(c, paint, value)
            fill(c, source, paint, (x, 0, 1, 1))
        copier = gc(c, destination, 0xaaaaaa, 0x555555)
        c.send(COPY_PLANE, c.pack('IIIhhhhHHI', source, destination, copier, 0, 0, 0, 0, 4, 1, 0x010000))
        assert pixels(c, destination, 4, 1)[0] == [[0xaaaaaa, 0x555555, 0xaaaaaa, 0x555555]]
        assert c.event(NO_EXPOSURE, lambda e: c.u32(e, 4) == destination)[10] == COPY_PLANE
        # The source need not share the destination's depth.
        bitmap = pixmap(c, 4, 1, depth=1)
        ink = gc(c, bitmap, 0)
        fill(c, bitmap, ink, (0, 0, 4, 1))
        set_foreground(c, ink, 1)
        fill(c, bitmap, ink, (1, 0, 2, 1))
        c.send(COPY_PLANE, c.pack('IIIhhhhHHI', bitmap, destination, copier, 0, 0, 0, 0, 4, 1, 1))
        assert pixels(c, destination, 4, 1)[0] == [[0x555555, 0xaaaaaa, 0xaaaaaa, 0x555555]]
        # bit-plane must be exactly one bit below the source depth.
        for plane, drawable in ((0, source), (0x3, source), (1 << c.depth, source), (2, bitmap)):
            c.completion(c.send(COPY_PLANE, c.pack('IIIhhhhHHI', drawable, destination, copier, 0, 0, 0, 0, 1, 1, plane)),
                         error=BAD_VALUE, opcode=COPY_PLANE, resource=plane)


def poly_primitives(context):
    with client(context) as c:
        pid = pixmap(c, 8, 6)
        back, paint = gc(c, pid, 0), gc(c, pid, 0xffffff)

        def fresh():
            fill(c, pid, back, (0, 0, 8, 6))

        fresh()
        c.send(POLY_POINT, c.pack('II', pid, paint) + c.pack('hhhh', 1, 1, 3, 2), detail=0)
        assert painted(c, pid, 8, 6) == {(1, 1), (3, 2)}
        fresh()
        c.send(POLY_POINT, c.pack('II', pid, paint) + c.pack('hhhh', 1, 1, 2, 1), detail=1)
        assert painted(c, pid, 8, 6) == {(1, 1), (3, 2)}, 'Previous coordinates accumulate'
        fresh()
        c.send(POLY_LINE, c.pack('II', pid, paint) + c.pack('hhhh', 1, 1, 5, 1), detail=0)
        assert painted(c, pid, 8, 6) == {(x, 1) for x in range(1, 6)}
        fresh()
        c.send(POLY_LINE, c.pack('II', pid, paint) + c.pack('hhhhhh', 2, 0, 2, 4, 5, 4), detail=0)
        assert painted(c, pid, 8, 6) == {(2, y) for y in range(5)} | {(x, 4) for x in range(2, 6)}
        fresh()
        c.send(POLY_LINE, c.pack('II', pid, paint) + c.pack('hhhh', 0, 0, 3, 3), detail=0)
        assert painted(c, pid, 8, 6) == {(i, i) for i in range(4)}
        fresh()
        c.send(POLY_SEGMENT, c.pack('II', pid, paint) + c.pack('hhhhhhhh', 0, 0, 2, 0, 5, 5, 5, 3))
        assert painted(c, pid, 8, 6) == {(0, 0), (1, 0), (2, 0), (5, 3), (5, 4), (5, 5)}
        fresh()
        c.send(POLY_RECTANGLE, c.pack('II', pid, paint) + c.pack('hhHH', 1, 1, 4, 3))
        assert painted(c, pid, 8, 6) == {(x, y) for x in range(1, 6) for y in range(1, 5)
                                         if x in (1, 5) or y in (1, 4)}
        # A wide line is the polygon its width sweeps, with the GC's caps and
        # joins; a pixel is drawn when its centre lies inside.
        wide = pixmap(c, 16, 14)
        wide_back, wide_paint = gc(c, wide, 0), gc(c, wide, 0xffffff)

        def stroke(width, cap, join, *points):
            fill(c, wide, wide_back, (0, 0, 16, 14))
            c.send(CHANGE_GC, c.pack('II', wide_paint, GC_LINE_WIDTH | GC_CAP_STYLE | GC_JOIN_STYLE)
                   + c.pack('III', width, cap, join))
            c.send(POLY_LINE, c.pack('II', wide, wide_paint) + c.pack('h' * len(points), *points), detail=0)
            return painted(c, wide, 16, 14)

        def block(xs, ys):
            return {(x, y) for x in xs for y in ys}

        assert stroke(4, CAP_BUTT, JOIN_MITER, 2, 5, 10, 5) == block(range(2, 10), range(3, 7))
        assert stroke(4, CAP_PROJECTING, JOIN_MITER, 2, 5, 10, 5) == block(range(0, 12), range(3, 7))
        # A right angle at (10, 2): the outer corner's far pixel (11, 0) is
        # inside the miter and outside the bevel.
        bodies = block(range(2, 10), range(0, 4)) | block(range(8, 12), range(2, 10))
        miter = stroke(4, CAP_BUTT, JOIN_MITER, 2, 2, 10, 2, 10, 10)
        bevel = stroke(4, CAP_BUTT, JOIN_BEVEL, 2, 2, 10, 2, 10, 10)
        assert bodies <= miter and (11, 0) in miter, sorted(miter - bodies)
        assert bodies <= bevel and (11, 0) not in bevel, sorted(bevel - bodies)
        for opcode in (POLY_POINT, POLY_LINE):
            c.completion(c.send(opcode, c.pack('II', pid, paint) + c.pack('hh', 0, 0), detail=2),
                         error=BAD_VALUE, opcode=opcode, resource=2)
        c.completion(c.send(POLY_POINT, c.pack('II', input_only_window(c), paint) + c.pack('hh', 0, 0), detail=0),
                     error=BAD_MATCH, opcode=POLY_POINT)


def fill_primitives(context):
    with client(context) as c:
        pid = pixmap(c, 9, 9)
        back, paint = gc(c, pid, 0), gc(c, pid, 0xffffff)

        def fresh():
            fill(c, pid, back, (0, 0, 9, 9))

        fresh()
        fill(c, pid, paint, (1, 1, 3, 2), (5, 4, 2, 2))
        assert painted(c, pid, 9, 9) == ({(x, y) for x in range(1, 4) for y in range(1, 3)}
                                         | {(x, y) for x in range(5, 7) for y in range(4, 6)})
        rectangle = {(x, y) for x in range(1, 4) for y in range(1, 3)}
        fresh()
        c.send(FILL_POLY, c.pack('II', pid, paint) + c.pack('BBH', 2, 0, 0) + c.pack('hhhhhhhh', 1, 1, 4, 1, 4, 3, 1, 3))
        assert painted(c, pid, 9, 9) == rectangle
        fresh()
        c.send(FILL_POLY, c.pack('II', pid, paint) + c.pack('BBH', 0, 1, 0) + c.pack('hhhhhhhh', 1, 1, 3, 0, 0, 2, -3, 0))
        assert painted(c, pid, 9, 9) == rectangle, 'Previous coordinates close the same path'
        # Pixel centres are integral and a pixel is inside when its centre
        # is. The slanted edge from (9, 0) to (0, 3) passes through centres
        # (6, 1) and (3, 2) with the interior to their left, so neither is
        # drawn; every centre left of them is.
        fresh()
        c.send(FILL_POLY, c.pack('II', pid, paint) + c.pack('BBH', 0, 0, 0) + c.pack('hhhhhh', 0, 0, 9, 0, 0, 3))
        assert painted(c, pid, 9, 9) == ({(x, 0) for x in range(9)} | {(x, 1) for x in range(6)}
                                         | {(x, 2) for x in range(3)}), sorted(painted(c, pid, 9, 9))
        c.completion(c.send(FILL_POLY, c.pack('II', pid, paint) + c.pack('BBH', 3, 0, 0) + c.pack('hh', 0, 0)),
                     error=BAD_VALUE, opcode=FILL_POLY, resource=3)
        c.completion(c.send(FILL_POLY, c.pack('II', pid, paint) + c.pack('BBH', 0, 2, 0) + c.pack('hh', 0, 0)),
                     error=BAD_VALUE, opcode=FILL_POLY, resource=2)
        # A full disc inscribed in a 7x7 box holds its center and none of the box corners.
        box = {(x, y) for x in range(1, 8) for y in range(1, 8)}
        corners = {(1, 1), (7, 1), (1, 7), (7, 7)}
        fresh()
        c.send(POLY_FILL_ARC, c.pack('II', pid, paint) + c.pack('hhHHhh', 1, 1, 7, 7, 0, 360 * 64))
        disc = painted(c, pid, 9, 9)
        assert (4, 4) in disc and not (corners & disc) and disc <= box, sorted(disc)
        fresh()
        # Angles are INT16 in 64ths of a degree, so the largest overshoot a
        # request can carry is just under 512 degrees.
        c.send(POLY_FILL_ARC, c.pack('II', pid, paint) + c.pack('hhHHhh', 1, 1, 7, 7, 0, 500 * 64))
        assert painted(c, pid, 9, 9) == disc, 'angles beyond 360 degrees are truncated'
        fresh()
        # The thin outline follows the path through [x, y+height/2] and
        # [x+width, y+height/2], so it spans the eight pixel columns and rows
        # from 1 to 8, touching each side of that box and none of its corners.
        c.send(POLY_ARC, c.pack('II', pid, paint) + c.pack('hhHHhh', 1, 1, 7, 7, 0, 360 * 64))
        ring = painted(c, pid, 9, 9)
        outline_box = {(x, y) for x in range(1, 9) for y in range(1, 9)}
        outline_corners = {(1, 1), (8, 1), (1, 8), (8, 8)}
        assert ring and (4, 4) not in ring and not (outline_corners & ring) and ring <= outline_box, sorted(ring)
        assert {(1, 4), (8, 4), (4, 1), (4, 8)} <= ring, sorted(ring)


def put_get_image(context):
    with client(context) as c:
        pid = pixmap(c, 4, 2)
        paint = gc(c, pid, 0xff0000, 0x0000ff)
        assert c.formats[c.depth][0] == 32, 'this case sends 32-bit ZPixmap rows'
        values = [0x112233, 0x445566, 0x778899, 0xaabbcc, 0xddeeff, 0x102030, 0x405060, 0x708090]
        c.send(PUT_IMAGE, c.pack('IIHHhhBBH', pid, paint, 4, 2, 0, 0, 0, c.depth, 0) + pixel_bytes(c, values), detail=Z_PIXMAP)
        rows, reply = pixels(c, pid, 4, 2)
        assert rows == [values[:4], values[4:]] and c.u32(reply, 8) == 0
        assert pixels(c, pid, 4, 2, plane_mask=0xff0000)[0] == [[v & 0xff0000 for v in values[:4]],
                                                                 [v & 0xff0000 for v in values[4:]]]
        # A partly outside image is clipped, not refused.
        c.send(PUT_IMAGE, c.pack('IIHHhhBBH', pid, paint, 2, 1, 3, 1, 0, c.depth, 0) + pixel_bytes(c, [1, 2]), detail=Z_PIXMAP)
        assert pixels(c, pid, 4, 2)[0] == [values[:4], values[4:7] + [1]]
        # Bitmap format paints foreground for set bits and background for clear ones.
        bits = 0b10100000 if c.bitmap_bit_order == 1 else 0b00000101
        c.send(PUT_IMAGE, c.pack('IIHHhhBBH', pid, paint, 4, 1, 0, 0, 0, 1, 0) + bytes([bits, 0, 0, 0]), detail=BITMAP)
        assert pixels(c, pid, 4, 2)[0][0] == [0xff0000, 0x0000ff, 0xff0000, 0x0000ff]
        pixel = pixel_bytes(c, [0])
        c.completion(c.send(PUT_IMAGE, c.pack('IIHHhhBBH', pid, paint, 1, 1, 0, 0, 0, 8, 0) + pixel, detail=Z_PIXMAP),
                     error=BAD_MATCH, opcode=PUT_IMAGE)
        c.completion(c.send(PUT_IMAGE, c.pack('IIHHhhBBH', pid, paint, 1, 1, 0, 0, 0, c.depth, 0) + pixel, detail=BITMAP),
                     error=BAD_MATCH, opcode=PUT_IMAGE)
        c.completion(c.send(PUT_IMAGE, c.pack('IIHHhhBBH', pid, paint, 1, 1, 0, 0, 4, c.depth, 0) + pixel, detail=Z_PIXMAP),
                     error=BAD_MATCH, opcode=PUT_IMAGE)
        c.completion(c.send(PUT_IMAGE, c.pack('IIHHhhBBH', pid, paint, 1, 1, 0, 0, 0, c.depth, 0) + pixel, detail=3),
                     error=BAD_VALUE, opcode=PUT_IMAGE, resource=3)
        c.completion(c.send(GET_IMAGE, c.pack('IhhHHI', pid, 0, 0, 1, 1, 0xffffffff), detail=3),
                     error=BAD_VALUE, opcode=GET_IMAGE, resource=3)
        c.completion(c.send(GET_IMAGE, c.pack('IhhHHI', pid, 2, 0, 4, 2, 0xffffffff), detail=Z_PIXMAP),
                     error=BAD_MATCH, opcode=GET_IMAGE)
        # A viewable window reports its visual; an unmapped one or an outside rectangle is a Match error.
        wid = mapped_window(c, background=0x000000)
        rows, reply = pixels(c, wid, 8, 4)
        assert reply[1] == c.depth and c.u32(reply, 8) == c.root_visual
        c.completion(c.send(GET_IMAGE, c.pack('IhhHHI', wid, 0, 0, 9, 1, 0xffffffff), detail=Z_PIXMAP),
                     error=BAD_MATCH, opcode=GET_IMAGE)
        unmapped = c.window(events=0)
        c.completion(c.send(GET_IMAGE, c.pack('IhhHHI', unmapped, 0, 0, 1, 1, 0xffffffff), detail=Z_PIXMAP),
                     error=BAD_MATCH, opcode=GET_IMAGE)


def query_best_size(context):
    with client(context) as c:
        pid = pixmap(c, 4, 4)
        wid = c.window(events=0)
        for shape in (0, 1, 2):
            for drawable in (c.root, wid, pid):
                if shape == 0 and drawable == pid:
                    continue
                width, height = c.unpack('HH', c.reply(QUERY_BEST_SIZE, c.pack('IHH', drawable, 16, 16), detail=shape), 8)
                assert width > 0 and height > 0, (shape, width, height)
        c.completion(c.send(QUERY_BEST_SIZE, c.pack('IHH', wid, 16, 16), detail=3),
                     error=BAD_VALUE, opcode=QUERY_BEST_SIZE, resource=3)
        input_only = input_only_window(c)
        for shape in (1, 2):
            c.completion(c.send(QUERY_BEST_SIZE, c.pack('IHH', input_only, 16, 16), detail=shape),
                         error=BAD_MATCH, opcode=QUERY_BEST_SIZE)
        bad = c.xid()
        c.completion(c.send(QUERY_BEST_SIZE, c.pack('IHH', bad, 16, 16), detail=1),
                     error=BAD_DRAWABLE, opcode=QUERY_BEST_SIZE, resource=bad)


CASES = {'pixmap_lifecycle': pixmap_lifecycle, 'gc_lifecycle': gc_lifecycle,
         'gc_dashes_clip': gc_dashes_clip, 'clear_area': clear_area, 'copy_area': copy_area,
         'copy_plane': copy_plane, 'poly_primitives': poly_primitives,
         'fill_primitives': fill_primitives, 'put_get_image': put_get_image,
         'query_best_size': query_best_size}

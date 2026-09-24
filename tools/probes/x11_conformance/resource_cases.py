"""Text, colour and cursor obligations.

Text is judged by the pixels it leaves, read back through GetImage, against
the font's own metrics from QueryFont: ImageText fills the box from the
origin across the string's width and from the font's ascent above the
baseline to its descent below, and PolyText paints exactly the glyph ink
ImageText paints in the foreground. Colours are judged against the root
visual's own masks, as the protocol's TrueColor rule states them. Cursors
have no pixels a client can read, so they are judged by acceptance, by the
errors the protocol assigns, and by whether a freed cursor still names
anything.
"""
from drawing_cases import (BAD_DRAWABLE, BAD_GC, BAD_FONT, BAD_MATCH, BAD_VALUE, GC_FONT,
                           client, fill, gc, pixels, pixmap)

OPEN_FONT, CLOSE_FONT, QUERY_FONT, QUERY_TEXT_EXTENTS = 45, 46, 47, 48
LIST_FONTS, LIST_FONTS_WITH_INFO = 49, 50
CHANGE_WINDOW_ATTRIBUTES, CHANGE_GC = 2, 56
POLY_TEXT8, POLY_TEXT16, IMAGE_TEXT8, IMAGE_TEXT16 = 74, 75, 76, 77
CREATE_COLORMAP, FREE_COLORMAP, ALLOC_COLOR, ALLOC_NAMED_COLOR = 78, 79, 84, 85
QUERY_COLORS, LOOKUP_COLOR = 91, 92
CREATE_CURSOR, CREATE_GLYPH_CURSOR, FREE_CURSOR, RECOLOR_CURSOR = 93, 94, 95, 96
BAD_CURSOR, BAD_COLOR, BAD_NAME = 6, 12, 15
TRUE_COLOR = 4
CW_CURSOR = 1 << 14


def open_font(c, name):
    fid = c.xid()
    value = name.encode('ascii')
    c.send(OPEN_FONT, c.pack('IHH', fid, len(value), 0) + value)
    c.sync()
    return fid


def font_metrics(c, fid):
    """Ascent, descent and each character's advance, from QueryFont."""
    reply = c.reply(QUERY_FONT, c.pack('I', fid))
    first, last = c.unpack('HH', reply, 40)
    properties = c.u16(reply, 46)
    ascent, descent = c.unpack('hh', reply, 52)
    count = c.u32(reply, 56)
    infos = 60 + properties * 8
    # A font whose characters all share one set of metrics may send no
    # per-character infos, and then every character has the max bounds.
    uniform = c.unpack('h', reply, 24 + 4)[0]
    widths = {first + index: c.unpack('h', reply, infos + index * 12 + 4)[0] if count else uniform
              for index in range(last - first + 1)}
    assert count in (0, last - first + 1), ('char infos span the range', first, last, count)
    return ascent, descent, widths


def text_gc(c, drawable, fid, foreground, background):
    cid = gc(c, drawable, foreground, background)
    c.send(CHANGE_GC, c.pack('II', cid, GC_FONT) + c.pack('I', fid))
    return cid


def ink(c, drawable, width, height, foreground):
    return {(x, y) for y, row in enumerate(pixels(c, drawable, width, height)[0])
            for x, value in enumerate(row) if value == foreground}


def poly_items(c, *items):
    """PolyText8 items: (delta, text) runs, or a font id to switch to.

    A font switch is 255 followed by the font id, most significant byte
    first whatever the connection's byte order."""
    out = b''
    for item in items:
        if isinstance(item, int):
            out += bytes([255]) + item.to_bytes(4, 'big')
        else:
            delta, text = item
            out += bytes([len(text), delta & 0xff]) + text
    return out


def wide(text):
    return b''.join(bytes([0, byte]) for byte in text)


def text_primitives(context):
    width, height = 64, 20
    foreground, background = 0xffffff, 0x0000ff
    with client(context) as c:
        fid = open_font(c, 'fixed')
        ascent, descent, widths = font_metrics(c, fid)
        assert ascent > 0 and descent >= 0 and ascent + descent < height, (ascent, descent)
        text, x, y = b'AB', 3, ascent + 2
        advance = sum(widths[byte] for byte in text)
        box = {(i, j) for i in range(x, x + advance) for j in range(y - ascent, y + descent)}

        # ImageText8 fills exactly the box and paints glyphs over it.
        image = pixmap(c, width, height)
        paint = text_gc(c, image, fid, foreground, background)
        blank = gc(c, image, 0)
        fill(c, image, blank, (0, 0, width, height))
        c.send(IMAGE_TEXT8, c.pack('IIhh', image, paint, x, y) + text, detail=len(text))
        rows = pixels(c, image, width, height)[0]
        covered = {(i, j) for j, row in enumerate(rows) for i, value in enumerate(row) if value}
        assert covered == box, ('ImageText8 covers exactly its box', sorted(covered ^ box)[:8])
        glyphs = {(i, j) for j, row in enumerate(rows) for i, value in enumerate(row)
                  if value == foreground}
        assert glyphs and glyphs < box, 'ImageText8 paints glyph ink in the foreground'
        assert all(rows[j][i] in (foreground, background) for i, j in box)

        # ImageText16 with the same characters leaves the same pixels.
        image16 = pixmap(c, width, height)
        fill(c, image16, blank, (0, 0, width, height))
        c.send(IMAGE_TEXT16, c.pack('IIhh', image16, paint, x, y) + wide(text), detail=len(text))
        assert pixels(c, image16, width, height)[0] == rows, 'ImageText16 matches ImageText8'

        # PolyText8 paints the same ink and nothing else; a delta moves the
        # next run, and a font switch is honoured before the run after it.
        reference = pixmap(c, width, height)
        fill(c, reference, blank, (0, 0, width, height))
        gap = 5
        c.send(IMAGE_TEXT8, c.pack('IIhh', reference, paint, x, y) + text, detail=len(text))
        c.send(IMAGE_TEXT8, c.pack('IIhh', reference, paint, x + advance + gap, y) + b'C', detail=1)
        expected = ink(c, reference, width, height, foreground)
        poly = pixmap(c, width, height)
        fill(c, poly, blank, (0, 0, width, height))
        c.send(POLY_TEXT8, c.pack('IIhh', poly, paint, x, y)
               + poly_items(c, (0, text), fid, (gap, b'C')))
        assert ink(c, poly, width, height, foreground) == expected, 'PolyText8 paints the runs'
        assert not {(i, j) for j, row in enumerate(pixels(c, poly, width, height)[0])
                    for i, value in enumerate(row) if value not in (0, foreground)}, \
            'PolyText8 paints no background'

        # PolyText16 carries the same runs in two-byte characters.
        poly16 = pixmap(c, width, height)
        fill(c, poly16, blank, (0, 0, width, height))
        items16 = (bytes([len(text), 0]) + wide(text) + bytes([255]) + fid.to_bytes(4, 'big')
                   + bytes([1, gap]) + wide(b'C'))
        c.send(POLY_TEXT16, c.pack('IIhh', poly16, paint, x, y) + items16)
        assert ink(c, poly16, width, height, foreground) == expected, 'PolyText16 paints the runs'

        # Errors name what was wrong.
        bad = c.xid()
        c.completion(c.send(IMAGE_TEXT8, c.pack('IIhh', bad, paint, x, y) + text, detail=len(text)),
                     error=BAD_DRAWABLE, opcode=IMAGE_TEXT8, resource=bad)
        c.completion(c.send(POLY_TEXT8, c.pack('IIhh', poly, bad, x, y) + poly_items(c, (0, text))),
                     error=BAD_GC, opcode=POLY_TEXT8, resource=bad)
        # Xorg names no resource for a font switch it cannot make.
        c.completion(c.send(POLY_TEXT8, c.pack('IIhh', poly, paint, x, y) + poly_items(c, bad, (0, text))),
                     error=BAD_FONT, opcode=POLY_TEXT8)
        c.send(CLOSE_FONT, c.pack('I', fid))
        c.sync()


def colormaps(context):
    with client(context) as c:
        visual = c.visuals[c.root_visual]
        assert visual['class'] == TRUE_COLOR, 'this oracle reads a TrueColor root visual'
        red_mask, green_mask, blue_mask = visual['masks']

        def expected(red, green, blue):
            """The TrueColor pixel for a 16-bit colour, and the colour it shows."""
            pixel, shown = 0, []
            for value, mask in ((red, red_mask), (green, green_mask), (blue, blue_mask)):
                shift = (mask & -mask).bit_length() - 1
                bits = bin(mask).count('1')
                level = value >> (16 - bits)
                pixel |= level << shift
                shown.append(level * 0xffff // ((1 << bits) - 1))
            return pixel, tuple(shown)

        cmap = c.xid()
        c.send(CREATE_COLORMAP, c.pack('III', cmap, c.root, c.root_visual), detail=0)
        c.sync()
        # AllocAll has no meaning for a visual with no writable cells.
        c.completion(c.send(CREATE_COLORMAP, c.pack('III', c.xid(), c.root, c.root_visual), detail=1),
                     error=BAD_MATCH, opcode=CREATE_COLORMAP)

        colour = (0x1234, 0xabcd, 0xfedc)
        pixel, shown = expected(*colour)
        reply = c.reply(ALLOC_COLOR, c.pack('IHHHH', cmap, *colour, 0))
        assert c.unpack('HHH', reply, 8) == shown and c.u32(reply, 16) == pixel, \
            ('AllocColor answers the TrueColor pixel', reply.hex())

        reply = c.reply(QUERY_COLORS, c.pack('I', cmap) + c.pack('II', pixel, 0))
        assert c.u16(reply, 8) == 2
        assert c.unpack('HHH', reply, 32) == shown and c.unpack('HHH', reply, 40) == (0, 0, 0), \
            ('QueryColors reads the pixels back', reply.hex())

        for query in (ALLOC_NAMED_COLOR, LOOKUP_COLOR):
            name = b'Red'
            reply = c.reply(query, c.pack('IHH', cmap, len(name), 0) + name)
            exact = c.unpack('HHH', reply, 12 if query == ALLOC_NAMED_COLOR else 8)
            assert exact == (0xffff, 0, 0), ('the database names red, case-insensitively', reply.hex())
            if query == ALLOC_NAMED_COLOR:
                assert c.u32(reply, 8) == expected(0xffff, 0, 0)[0]
            missing = b'no such colour'
            c.completion(c.send(query, c.pack('IHH', cmap, len(missing), 0) + missing),
                         error=BAD_NAME, opcode=query)

        c.send(FREE_COLORMAP, c.pack('I', cmap))
        c.sync()
        c.completion(c.send(ALLOC_COLOR, c.pack('IHHHH', cmap, 0, 0, 0, 0)),
                     error=BAD_COLOR, opcode=ALLOC_COLOR, resource=cmap)
        c.completion(c.send(FREE_COLORMAP, c.pack('I', cmap)),
                     error=BAD_COLOR, opcode=FREE_COLORMAP, resource=cmap)
        # Freeing the default colormap is no error and frees nothing.
        c.send(FREE_COLORMAP, c.pack('I', c.default_colormap))
        c.reply(ALLOC_COLOR, c.pack('IHHHH', c.default_colormap, 0, 0, 0, 0))


def cursors(context):
    white, black = (0xffff, 0xffff, 0xffff), (0, 0, 0)
    with client(context) as c:
        wid = c.window(events=0)
        source = pixmap(c, 16, 16, depth=1)
        mask = pixmap(c, 16, 16, depth=1)
        shape = gc(c, source, 1)
        fill(c, source, shape, (0, 0, 16, 16))
        fill(c, mask, shape, (0, 0, 16, 16))

        cursor = c.xid()
        c.send(CREATE_CURSOR, c.pack('IIIHHHHHHHH', cursor, source, mask, *white, *black, 3, 4))
        c.send(CHANGE_WINDOW_ATTRIBUTES, c.pack('II', wid, CW_CURSOR) + c.pack('I', cursor))
        c.sync()
        # The hotspot has to lie on the source -- Xorg admits one past each
        # edge, and so does this oracle; a colour pixmap is no shape; and the
        # mask has to be the source's size.
        c.send(CREATE_CURSOR, c.pack('IIIHHHHHHHH', c.xid(), source, 0, *white, *black, 16, 16))
        c.sync()
        c.completion(c.send(CREATE_CURSOR, c.pack('IIIHHHHHHHH', c.xid(), source, 0, *white, *black, 17, 0)),
                     error=BAD_MATCH, opcode=CREATE_CURSOR)
        colour = pixmap(c, 16, 16)
        c.completion(c.send(CREATE_CURSOR, c.pack('IIIHHHHHHHH', c.xid(), colour, 0, *white, *black, 0, 0)),
                     error=BAD_MATCH, opcode=CREATE_CURSOR)
        small = pixmap(c, 8, 8, depth=1)
        c.completion(c.send(CREATE_CURSOR, c.pack('IIIHHHHHHHH', c.xid(), source, small, *white, *black, 0, 0)),
                     error=BAD_MATCH, opcode=CREATE_CURSOR)

        fid = open_font(c, 'cursor')
        glyph = c.xid()
        c.send(CREATE_GLYPH_CURSOR, c.pack('IIIHHHHHHHH', glyph, fid, fid, 68, 69, *black, *white))
        c.sync()
        bad = c.xid()
        c.completion(c.send(CREATE_GLYPH_CURSOR, c.pack('IIIHHHHHHHH', c.xid(), bad, 0, 68, 0, *black, *white)),
                     error=BAD_FONT, opcode=CREATE_GLYPH_CURSOR, resource=bad)
        c.completion(c.send(CREATE_GLYPH_CURSOR, c.pack('IIIHHHHHHHH', c.xid(), fid, 0, 0xfff0, 0, *black, *white)),
                     error=BAD_VALUE, opcode=CREATE_GLYPH_CURSOR)

        c.send(RECOLOR_CURSOR, c.pack('IHHHHHH', glyph, *white, *black))
        c.sync()
        c.completion(c.send(RECOLOR_CURSOR, c.pack('IHHHHHH', bad, *white, *black)),
                     error=BAD_CURSOR, opcode=RECOLOR_CURSOR, resource=bad)

        # A freed cursor names nothing, while the window it was set on keeps
        # working; freeing it twice is an error.
        c.send(FREE_CURSOR, c.pack('I', cursor))
        c.sync()
        c.completion(c.send(CHANGE_WINDOW_ATTRIBUTES, c.pack('II', wid, CW_CURSOR) + c.pack('I', cursor)),
                     error=BAD_CURSOR, opcode=CHANGE_WINDOW_ATTRIBUTES, resource=cursor)
        c.completion(c.send(FREE_CURSOR, c.pack('I', cursor)),
                     error=BAD_CURSOR, opcode=FREE_CURSOR, resource=cursor)
        c.send(CHANGE_WINDOW_ATTRIBUTES, c.pack('II', wid, CW_CURSOR) + c.pack('I', glyph))
        c.send(FREE_CURSOR, c.pack('I', glyph))
        c.sync()


def font_names(c, opcode, pattern, maximum):
    """ListFonts's names, or ListFontsWithInfo's, with the info replies."""
    value = pattern.encode('ascii')
    sequence = c.send(opcode, c.pack('HH', maximum, len(value)) + value)
    if opcode == LIST_FONTS:
        reply = c.completion(sequence)
        count, names, offset = c.u16(reply, 8), [], 32
        for _ in range(count):
            size = reply[offset]
            names.append(reply[offset + 1:offset + 1 + size].decode('latin-1'))
            offset += 1 + size
        return names, []
    names, infos = [], []
    while True:
        reply = c.completion(sequence)
        size = reply[1]
        if size == 0:
            return names, infos
        properties = c.u16(reply, 46)
        start = 60 + properties * 8
        names.append(reply[start:start + size].decode('latin-1'))
        infos.append(reply)


def fonts(context):
    with client(context) as c:
        fid = open_font(c, 'fixed')
        query = c.reply(QUERY_FONT, c.pack('I', fid))
        ascent, descent, widths = font_metrics(c, fid)
        first, last = c.unpack('HH', query, 40)
        assert first <= ord('A') <= last and ascent > 0, ('fixed covers ASCII', first, last)
        assert all(widths[code] <= c.unpack('h', query, 24 + 4)[0] for code in widths), \
            'no advance exceeds the max bounds'

        # QueryTextExtents agrees with QueryFont's advances and with the box
        # ImageText fills, through the font and through a GC naming it.
        text = b'Hello'
        advance = sum(widths[byte] for byte in text)
        for fontable in (fid, text_gc(c, c.root, fid, 1, 0)):
            reply = c.reply(QUERY_TEXT_EXTENTS, c.pack('I', fontable) + wide(text) + bytes(2),
                            detail=1)
            assert c.unpack('hh', reply, 8) == (ascent, descent), ('font extents', reply.hex())
            assert c.unpack('i', reply, 16)[0] == advance, ('overall width', reply.hex())
        width, height = advance + 4, ascent + descent + 4
        image = pixmap(c, width, height)
        paint = text_gc(c, image, fid, 0xffffff, 0x0000ff)
        fill(c, image, gc(c, image, 0), (0, 0, width, height))
        c.send(IMAGE_TEXT8, c.pack('IIhh', image, paint, 2, 2 + ascent) + text, detail=len(text))
        covered = {(i, j) for j, row in enumerate(pixels(c, image, width, height)[0])
                   for i, value in enumerate(row) if value}
        assert covered == {(i, j) for i in range(2, 2 + advance) for j in range(2, 2 + ascent + descent)}, \
            'ImageText fills the box QueryTextExtents measures'

        # The lists name what OpenFont opens, honour the limit, and describe
        # each face as QueryFont does.
        # A name may be listed once per path element that provides it.
        names, _ = font_names(c, LIST_FONTS, 'fixed', 100)
        assert names and {name.lower() for name in names} == {'fixed'}, ('ListFonts fixed', names)
        names, _ = font_names(c, LIST_FONTS, '*', 1)
        assert len(names) == 1, ('ListFonts honours its limit', names)
        assert font_names(c, LIST_FONTS, 'no-such-font-*', 100) == ([], [])
        names, infos = font_names(c, LIST_FONTS_WITH_INFO, 'fixed', 100)
        # An alias may be answered with the names it resolves to; the first
        # face is the one OpenFont chose.
        assert names and len(infos) == len(names), ('ListFontsWithInfo fixed', names)
        assert c.unpack('hh', infos[0], 52) == (ascent, descent), 'the info is the face QueryFont reads'
        assert font_names(c, LIST_FONTS_WITH_INFO, 'no-such-font-*', 100) == ([], [])

        # An unknown name is a Name error; a closed font names nothing.
        missing = b'no-such-font-name'
        c.completion(c.send(OPEN_FONT, c.pack('IHH', c.xid(), len(missing), 0) + missing),
                     error=BAD_NAME, opcode=OPEN_FONT)
        c.send(CLOSE_FONT, c.pack('I', fid))
        c.sync()
        for opcode, body in ((QUERY_FONT, c.pack('I', fid)), (CLOSE_FONT, c.pack('I', fid)),
                             (QUERY_TEXT_EXTENTS, c.pack('I', fid) + wide(text) + bytes(2))):
            c.completion(c.send(opcode, body, detail=1 if opcode == QUERY_TEXT_EXTENTS else 0),
                         error=BAD_FONT, opcode=opcode, resource=fid)


CASES = {'fonts': fonts, 'text_primitives': text_primitives, 'colormaps': colormaps, 'cursors': cursors}

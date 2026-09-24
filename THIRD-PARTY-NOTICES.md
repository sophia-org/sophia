# Third-party notices

Sophia is BSD-3-Clause (see `LICENSE`). It carries the following third-party
material, each retained under its own licence.

## yserver — MIT

Copyright (c) 2026 Jos Dehaes. <https://github.com/joske/yserver>

Portions of the X authority's font handling are derived from yserver's:

- `crates/sophia-x-authority/src/font/metrics.rs` — the per-character and
  per-font metric shape, and the `char_info` and `text_extents` rules, from
  `crates/yserver-protocol/src/x11/mod.rs`.
- `crates/sophia-x-authority/src/font/xlfd.rs` — the XLFD wildcard matcher,
  from `crates/yserver/src/kms/core.rs`.
- `crates/sophia-x-authority/src/font/directory.rs` — the `fonts.dir` and
  `fonts.alias` formats and the quoted-alias rule, from the same file.

Each derived file carries the notice in its own header. The PCF reader is
Sophia's own: yserver reads a PCF's headers and leaves glyphs to FreeType,
which Sophia does not link.

    Permission is hereby granted, free of charge, to any person obtaining a
    copy of this software and associated documentation files (the "Software"),
    to deal in the Software without restriction, including without limitation
    the rights to use, copy, modify, merge, publish, distribute, sublicense,
    and/or sell copies of the Software, and to permit persons to whom the
    Software is furnished to do so, subject to the following conditions:

    The above copyright notice and this permission notice shall be included in
    all copies or substantial portions of the Software.

    THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
    IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
    FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
    THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
    LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
    FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
    DEALINGS IN THE SOFTWARE.

## X.Org fonts — public domain and MIT/X11

`crates/sophia-x-authority/src/font/fixed_6x13.rs` is derived from X.Org's
misc-fixed 6x13 ISO 8859-1 bitmap, whose upstream notice reads "Public domain
font. Share and enjoy."

`tools/fixtures/fonts/6x13-iso8859-1.pcf` is one unmodified file from
`font-misc-misc`, redistributed as test data under the MIT/X11 licence used by
the X.Org font packages. See that directory's README for provenance.

## X.Org X server `mi` — The Open Group and Digital Equipment Corporation

Three files are ports of the X server's machine-independent drawing code,
from `gitlab.freedesktop.org/xorg/xserver`; each names the routines it ports:

- `crates/sophia-x-authority/src/software/geometry/wide_line.rs`:
  `mi/miwideline.c` and `mi/miwideline.h` (author Keith Packard, MIT X
  Consortium), `miStepDash` from `mi/midash.c`, `ICEIL` from `mi/mifpoly.h`,
  and the wide branches of `mi/mipolyseg.c` and `mi/mipolyrect.c`.
  Copyright 1988, 1998 The Open Group; copyright 1989 Digital Equipment
  Corporation.
- `crates/sophia-x-authority/src/software/geometry/polygon.rs`:
  `mi/mipoly.c` and `mi/mipoly.h` (author Brian Kelleher) with the edge
  macros of `mi/miscanfill.h`. Copyright 1987, 1998 The Open Group;
  copyright 1987 Digital Equipment Corporation.
- `crates/sophia-x-authority/src/software/geometry/fill_arc.rs`:
  `mi/mifillarc.c` and `mi/mifillarc.h` (author Bob Scheifler, MIT X
  Consortium). Copyright 1989, 1998 The Open Group, under the first notice
  below alone.

The notices are reproduced here with the wide-line files' copyright lines;
the other files' differ only in their years.

    Copyright 1988, 1998  The Open Group

    Permission to use, copy, modify, distribute, and sell this software and its
    documentation for any purpose is hereby granted without fee, provided that
    the above copyright notice appear in all copies and that both that
    copyright notice and this permission notice appear in supporting
    documentation.

    The above copyright notice and this permission notice shall be included
    in all copies or substantial portions of the Software.

    THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
    OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
    MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
    IN NO EVENT SHALL THE OPEN GROUP BE LIABLE FOR ANY CLAIM, DAMAGES OR
    OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
    ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
    OTHER DEALINGS IN THE SOFTWARE.

    Except as contained in this notice, the name of The Open Group shall
    not be used in advertising or otherwise to promote the sale, use or
    other dealings in this Software without prior written authorization
    from The Open Group.

    Copyright 1989 by Digital Equipment Corporation, Maynard, Massachusetts.

                            All Rights Reserved

    Permission to use, copy, modify, and distribute this software and its
    documentation for any purpose and without fee is hereby granted,
    provided that the above copyright notice appear in all copies and that
    both that copyright notice and this permission notice appear in
    supporting documentation, and that the name of Digital not be
    used in advertising or publicity pertaining to distribution of the
    software without specific, written prior permission.

    DIGITAL DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE, INCLUDING
    ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS, IN NO EVENT SHALL
    DIGITAL BE LIABLE FOR ANY SPECIAL, INDIRECT OR CONSEQUENTIAL DAMAGES OR
    ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
    WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
    ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS
    SOFTWARE.

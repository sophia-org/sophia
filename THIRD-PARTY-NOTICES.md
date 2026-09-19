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

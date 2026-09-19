# Font fixtures

One real X11 Portable Compiled Font, so the PCF reader is tested against a file
a real `bdftopcf` produced rather than only against files the tests build
themselves. A synthetic file can only prove the reader matches the test's own
idea of the format; this one proves it matches the format.

## `6x13-iso8859-1.pcf`

The ISO 8859-1 subset of the misc-fixed 6x13 face, 19,628 bytes, decompressed
once from the host package so the tests need no gzip and no host font path:

    zcat /usr/share/fonts/X11/misc/6x13-ISO8859-1.pcf.gz > 6x13-iso8859-1.pcf

It exercises the variations that matter: compressed metrics, most-significant
byte and bit order, four-byte row padding for a six-pixel glyph, accelerators
carrying ink bounds, and a matrix with 223 of its 256 cells defined.

## Provenance and licence

Upstream `font-misc-misc` (X.Org), distributed under the MIT/X11 licence used
by the X.Org font packages. Sophia redistributes this one file unmodified as
test data.

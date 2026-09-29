# C desktop SDK

The shell codecs, sessions, and generic 9P client now live in
`sophia-org/sophia-desktop-sdk-c`. Sophia tests the exact offline source pin at
`vendor/c-desktop-sdk/source/`; see its manifest and update instructions.

The SDK provides WM file support. This directory retains frozen WM socket
bindings only because the current SDK snapshot still pins their bytes. Their
generator and socket test clients are retired; the bindings will be removed
with the SDK compatibility release.

# C desktop SDK

The shell codecs, sessions, and generic 9P client now live in
`sophia-org/sophia-desktop-sdk-c`. Sophia tests the exact offline source pin at
`vendor/c-desktop-sdk/source/`; see its manifest and update instructions.

This directory retains the generated WM socket binding and its generator tests.
WM file support will be integrated with the SDK in its own slice.

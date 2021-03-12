# mso5k_dumpfb

A small utility to dump the different framebuffer layers
on Rigol's MSO5000-series oscilloscopes.

You likely need a nightly toolchain. To run on the scope,
it's easiest to build a statically linked binary with MUSL:

```
$ cargo +nightly build --target=armv7-unknown-linux-musleabihf --release
```
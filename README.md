# LZ👌in Rust [![crates.io](https://img.shields.io/crates/v/lzokay-native?style=flat-square&logo=rust)](https://crates.io/crates/lzokay-native) [![docs.rs](https://img.shields.io/badge/docs.rs-lzokay--native-66c2a5.svg?logo=docs.rs&style=flat-square)](https://docs.rs/lzokay-native) [![build status](https://img.shields.io/github/actions/workflow/status/arma-tools/lzokay-native-rs/CI.yml?branch=main&style=flat-square)](https://github.com/arma-tools/lzokay-native-rs/actions?query=branch%3Amaster)

This crate includes a pure rust port of [lzokay](https://github.com/jackoalan/lzokay), which is a C++ implementation of the [LZO compression format](http://www.oberhumer.com/opensource/lzo/).

## Documentation
See [RustDoc Documentation](https://docs.rs/lzokay-native).

The documentation includes some examples.

## Installation

Add following lines to your Cargo.toml:
```toml
# Cargo.toml
[dependencies]
lzokay-native = "0.1"
```

## Features

### `compress`
This feature includes everything for compression.

### `decompress`
This feature includes everything for decompression
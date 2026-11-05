# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-11-05

### Added
- `Error::LookbehindOverrun`: returned by `decompress`, `decompress_all` and
  the bounded variants when a match reaches before the start of the output,
  which means the input is corrupt (#3).

### Changed
- **Breaking:** `Error` is `#[non_exhaustive]`, so future error variants
  aren't breaking changes. `match`es on it need a wildcard arm (#3).
- **Breaking:** requires Rust 1.85 (edition 2024) (#4).
- `expected_size` reserves at most 1 MiB up front. The output still grows to
  whatever size the data decompresses to (#3).

### Fixed
- Corrupt input no longer makes decompression panic with an index out of
  bounds. Matches before the start of the output wrapped the read position
  around, in the bounded and unbounded functions (#3).
- `expected_size` of `Some(usize::MAX)` no longer panics with a capacity
  overflow, and large sizes are no longer allocated before reading any
  input (#3).
- Docs build on docs.rs again: the `doc_auto_cfg` feature was removed from
  nightly Rust in 1.92, `doc_cfg` replaces it. The 0.1.1 docs failed to build.

## [0.1.1] - 2026-11-05

### Added
- `decompress_bounded` and `decompress_all_bounded`: they return
  `Error::OutputOverrun` if the decompressed output would exceed `max_size`,
  which guards against untrusted or malicious input (#2, thanks @fanyang89).

### Fixed
- Use `std::ptr::{copy_nonoverlapping, write_bytes}` instead of the
  `std::intrinsics` re-exports, which fixes builds on newer Rust toolchains (#2).
- Doc typos and clarified that `expected_size` is only an allocation hint.

## [0.1.0] - 2023-08-11

- Initial release.

[Unreleased]: https://github.com/arma-tools/lzokay-native-rs/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/arma-tools/lzokay-native-rs/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/arma-tools/lzokay-native-rs/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/arma-tools/lzokay-native-rs/releases/tag/v0.1.0

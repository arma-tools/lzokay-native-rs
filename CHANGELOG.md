# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.1.1]: https://github.com/arma-tools/lzokay-native-rs/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/arma-tools/lzokay-native-rs/releases/tag/v0.1.0

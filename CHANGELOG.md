# Changelog

## 0.1.0 — 2026-10-07

The first Rust release of WebSiteHealth.

### Added

- Interactive setup with arrow-key menus and selectable URL lists.
- Concurrent HTTP(S) checks with timeouts, redirect limits, and verified TLS.
- Continuous monitoring with local DOWN/RECOVERED alerts and an optional terminal bell.
- Saved URL files, text and JSON Lines output, and script-friendly exit codes.
- Native release builds for macOS (Apple Silicon and Intel), Linux x86-64, and Windows x86-64.
- Automated checks, SHA-256 checksums, and an MIT license.

### Fixed

- URL credentials and query values could appear in malformed argument errors.
- An unread output pipe could delay Ctrl-C shutdown.

### Changed

- Rebuilt the original Python monitor as a standalone Rust CLI.

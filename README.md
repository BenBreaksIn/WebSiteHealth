# WebSiteHealth

**Website uptime checks, without leaving your terminal.**

[![Rust checks](https://github.com/BenBreaksIn/WebSiteHealth/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/BenBreaksIn/WebSiteHealth/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/BenBreaksIn/WebSiteHealth)](https://github.com/BenBreaksIn/WebSiteHealth/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A small Rust CLI for checking one website or a whole list. Run a quick check,
watch continuously with local alerts, or pipe JSON into your own scripts.
Start with an arrow-key guide; save your URLs so the next run takes less typing.

```console
$ websitehealth https://example.com http://localhost:8080/health
2026-10-07T19:00:00.123Z UP   #1 https://example.com/ | HTTP 200 | 142 ms
2026-10-07T19:00:00.200Z DOWN #2 http://localhost:8080/health | HTTP 503 | 8 ms
```

*Illustrative output. Each check reports its completion time and response latency.*

[Download](https://github.com/BenBreaksIn/WebSiteHealth/releases/latest) ·
[Usage reference](docs/usage.md) · [Changelog](CHANGELOG.md) ·
[Report a bug](https://github.com/BenBreaksIn/WebSiteHealth/issues/new?template=bug_report.yml)

## What it does

- **Guided setup.** Arrow keys, Enter, and Space to select websites and settings.
- **Multiple websites.** Check concurrently, paste several URLs, or load a saved list.
- **Continuous monitoring.** Choose an interval and watch until Ctrl-C.
- **Local alerts.** DOWN and RECOVERED messages on state changes, with an optional beep.
- **Script-friendly output.** Text or JSON Lines, predictable exit codes, and separate diagnostics.
- **Sensible network limits.** Timeouts, bounded concurrency, verified TLS, and redirect limits.

No account, API key, or notification service required.

## Install

### Download a binary

Get the archive for your system from the
[latest release](https://github.com/BenBreaksIn/WebSiteHealth/releases/latest).
Each archive includes the executable, documentation, and license notices.

| System | Archive suffix | Build environment |
| --- | --- | --- |
| macOS, Apple Silicon | `aarch64-apple-darwin.tar.gz` | macOS 14 |
| macOS, Intel | `x86_64-apple-darwin.tar.gz` | macOS 15 |
| Linux, x86-64 | `x86_64-unknown-linux-gnu.tar.gz` | Ubuntu 22.04, glibc 2.35 |
| Windows, x86-64 | `x86_64-pc-windows-msvc.zip` | Windows Server 2022 |

Extract the archive and run `./websitehealth` on macOS/Linux or
`.\websitehealth.exe` in PowerShell. To run it from any directory, put the
executable in a directory on your `PATH`.

Releases include `SHA256SUMS`. To verify a downloaded archive, keep it beside that
file and compare its SHA-256 hash with the matching entry:

```sh
# macOS
shasum -a 256 websitehealth-*.tar.gz
# Linux
sha256sum websitehealth-*.tar.gz
```

```powershell
# Windows
Get-FileHash .\websitehealth-*.zip -Algorithm SHA256
```

Binaries are not code-signed or notarized. If your system prevents running a
downloaded binary, you can build from source with the instructions below.

### Build from source

Install [Rust and Cargo](https://rustup.rs/) and a native C/C++ build toolchain:
Xcode Command Line Tools on macOS, a C compiler on Linux, or Visual Studio C++
Build Tools and NASM on Windows. Release CI uses Rust 1.94.0.

```sh
git clone https://github.com/BenBreaksIn/WebSiteHealth.git
cd WebSiteHealth
cargo install --path . --locked
websitehealth --version
```

Releases are distributed on GitHub. The package is not on crates.io.

## Start checking

```sh
# Open the guide. Enter websites once, then use the menus.
websitehealth

# Check multiple URLs once.
websitehealth https://example.com https://example.org

# Monitor every 30 seconds with local transition alerts.
websitehealth -w -a -i 30 https://example.com

# Reuse URLs saved by the guide; add -b for the optional terminal bell.
websitehealth -w -a -f websitehealth.urls

# Structured results for scripts and CI.
websitehealth --json --timeout 5 https://example.com
```

A single check exits with `0` when all URLs are healthy and `1` when any fail.
Invalid input or runtime errors exit with `2`; Ctrl-C exits with `130`.

The guide can save URLs in the current folder with your explicit selection.
Saved URLs include query values; results and alerts hide query strings.
Settings are selected each run. See the [full reference](docs/usage.md) for all
options, JSON fields, saved lists, redaction details, and alert behavior.

## What a healthy check means

A final **HTTP 200–299** response is healthy. Checks use GET, follow at most ten
redirects, and measure time through receipt of response headers. They do not
validate page content or measure a full page load.

Monitoring runs while the process is open. Alerts stay in your terminal;
WebSiteHealth does not run as a background service or send external notifications.
The optional beep depends on your terminal's bell settings.

## Development

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
cargo test --locked --release --test cli
```

CI runs native builds and local HTTP/TLS fixture tests on Linux, macOS (Apple
Silicon and Intel), and Windows. Unix builds also test Ctrl-C and unread-pipe
cancellation. Guided terminal flows have been exercised locally on macOS.
Tests need no public websites; downloading dependencies requires network access.

See [CONTRIBUTING.md](CONTRIBUTING.md) for development and release instructions.
Python is not needed to run WebSiteHealth.

## License

[MIT](LICENSE) © 2026 BenBreaksIn. Third-party dependencies retain their own
licenses; release archives include their notices.

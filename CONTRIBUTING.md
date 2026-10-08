# Contributing

Bug reports and focused pull requests are welcome. For behavior changes, include
local fixture tests and update the usage reference. Keep the CLI small and easy
to use; discuss substantial new features in an issue first.

## Local checks

Install the build dependencies listed in the README, then run:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
cargo test --locked --release --test cli
```

Commit `Cargo.lock` when updating dependencies. Tests launch the actual CLI
against local loopback fixtures. Do not add tests that depend on public websites.
Do not include credentials or private monitoring URLs in issues or fixtures.

## Releases

1. Update the package version, lockfile, and changelog together.
2. Install `cargo-about 0.9.2` with its CLI feature
   (`cargo install cargo-about --version 0.9.2 --features cli --locked`), then regenerate dependency notices:
   `cargo about generate --locked about.hbs -o THIRD-PARTY-NOTICES.txt`.
3. Run the checks above and merge the release commit to `main`.
4. Tag that commit `v<package version>` and push the tag.
5. The release workflow builds and tests each platform, packages the executable
   and notices, creates checksums, and opens a **draft** GitHub release.
6. Review the draft, test a downloaded archive, add release notes, and publish it.

CI uses Rust 1.94.0. The artifact scripts require Python 3.8 or later;
Python is not a runtime dependency of the CLI. Release jobs have read-only
repository permissions; only the draft-release job can write releases.

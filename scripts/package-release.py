"""Package a tested native executable; run from the repository root."""

import argparse
import json
import pathlib
import platform
import shutil
import subprocess
import tarfile
import zipfile

parser = argparse.ArgumentParser()
parser.add_argument("tag")
parser.add_argument("target", choices=[
    "aarch64-apple-darwin", "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc",
])
args = parser.parse_args()
metadata = json.loads(subprocess.check_output([
    "cargo", "metadata", "--locked", "--format-version", "1",
    "--filter-platform", args.target,
], text=True))
version = next(crate["version"] for crate in metadata["packages"]
               if pathlib.Path(crate["manifest_path"]).resolve() == pathlib.Path("Cargo.toml").resolve())
if args.tag != f"v{version}":
    raise SystemExit("Release tag does not match Cargo.toml")
host = subprocess.check_output(["rustc", "-vV"], text=True)
if f"host: {args.target}" not in host.splitlines():
    raise SystemExit("Archive target does not match the native Rust toolchain")
windows = platform.system() == "Windows"
binary_name = "websitehealth.exe" if windows else "websitehealth"
binary = pathlib.Path("target/release") / binary_name
actual = subprocess.check_output([str(binary.resolve()), "--version"], text=True).strip()
if actual != f"websitehealth {version}":
    raise SystemExit(f"Executable version mismatch: {actual}")
name = f"websitehealth-{args.tag}-{args.target}"
staging = pathlib.Path("dist") / name
staging.mkdir(parents=True, exist_ok=False)
for source in [binary, "README.md", "LICENSE", "THIRD-PARTY-NOTICES.txt", "CHANGELOG.md", "CONTRIBUTING.md", "SECURITY.md"]:
    shutil.copy2(source, staging)
shutil.copytree("docs", staging / "docs")
# Preserve bundled C-library attributions in full, in addition to the SPDX
# license texts harvested by cargo-about.
license_dir = staging / "docs" / "licenses"
license_dir.mkdir()
for crate_name in ["aws-lc-rs", "aws-lc-sys"]:
    crates = [crate for crate in metadata["packages"] if crate["name"] == crate_name]
    if not crates:
        raise SystemExit(f"Missing dependency license source: {crate_name}")
    for crate in crates:
        source = pathlib.Path(crate["manifest_path"]).parent / "LICENSE"
        shutil.copy2(source, license_dir / f"{crate_name}-{crate['version']}.txt")
if windows:
    archive = staging.with_name(name + ".zip")
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as output:
        for path in sorted(staging.rglob("*")):
            if path.is_file():
                output.write(path, path.relative_to(staging.parent))
else:
    archive = staging.with_name(name + ".tar.gz")
    with tarfile.open(archive, "w:gz") as output:
        output.add(staging, arcname=name)
shutil.rmtree(staging)
print(archive)

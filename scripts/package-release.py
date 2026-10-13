#!/usr/bin/env python3
"""Package a native x86_64 GNU/Linux release and its third-party notices."""
import hashlib
import json
import platform
import shutil
import subprocess
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGET = "x86_64-unknown-linux-gnu"


def output(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def main():
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise SystemExit("Release packaging currently supports x86_64 Linux only")
    metadata = json.loads(output("cargo", "metadata", "--locked", "--format-version", "1",
                                 "--filter-platform", TARGET))
    package = next(p for p in metadata["packages"] if p["id"] == metadata["resolve"]["root"])
    version = package["version"]
    binary = ROOT / "target/release/flux-pro"
    if output(str(binary), "--version") != f"flux-pro {version}":
        raise SystemExit("Binary version differs from Cargo.toml; rebuild first")
    name = f"flux-pro-v{version}-{TARGET}"
    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    stage = dist / name
    if stage.exists():
        shutil.rmtree(stage)
    stage.mkdir()
    shutil.copy2(binary, stage / "flux-pro")
    for filename in ["install.sh", "uninstall.sh", "README.md", "LICENSE", "CHANGELOG.md", "CONTRIBUTING.md"]:
        shutil.copy2(ROOT / filename, stage / filename)
    for dirname in ["packaging", "docs"]:
        shutil.copytree(ROOT / dirname, stage / dirname)

    notices = stage / "third-party-licenses"
    notices.mkdir()
    index = ["# Third-party notices", "", "Resolved Cargo dependencies (including build/test tooling).",
             "NVIDIA's driver and NVML shared library are not bundled.", ""]
    for dependency in sorted(metadata["packages"], key=lambda p: p["name"]):
        if not dependency["source"]:
            continue
        label = f'{dependency["name"]}-{dependency["version"]}'
        index.append(f'- {label}: {dependency["license"] or "see bundled license files"}')
        source = Path(dependency["manifest_path"]).parent
        licenses = [p for p in source.iterdir()
                    if p.name.lower().startswith(("license", "copying", "copyright", "notice"))]
        if not licenses:
            raise SystemExit(f"No license files found for {label}")
        destination = notices / label
        destination.mkdir()
        for license_file in licenses:
            if license_file.is_dir():
                shutil.copytree(license_file, destination / license_file.name)
            else:
                shutil.copy2(license_file, destination / license_file.name)
    rust_docs = Path(output("rustc", "--print", "sysroot")) / "share/doc/rust"
    shutil.copytree(rust_docs / "licenses", notices / "rust/licenses")
    shutil.copy2(rust_docs / "COPYRIGHT-library.html", notices / "rust/COPYRIGHT-library.html")
    index.extend(["", "Rust standard-library notices are in rust/.", output("rustc", "--version"), ""])
    (notices / "README.md").write_text("\n".join(index))

    archive = dist / f"{name}.tar.gz"
    with tarfile.open(archive, "w:gz") as tar:
        tar.add(stage, arcname=name)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    (dist / "SHA256SUMS").write_text(f"{digest}  {archive.name}\n")
    changelog = (ROOT / "CHANGELOG.md").read_text()
    release_section = next(section for section in changelog.split("\n## ")[1:]
                           if section.splitlines()[0].startswith(f"{version} "))
    (dist / "RELEASE_NOTES.md").write_text("\n".join(release_section.splitlines()[1:]).strip() + "\n")
    print(archive)


if __name__ == "__main__":
    main()

# Contributing

Bug reports, hardware compatibility reports, and pull requests are welcome.

For hardware reports, include your distribution, kernel, project version, CPU
and GPU models, USB VID/PID, selected sensor labels, and relevant service logs.
Run `flux-pro --list` and `flux-pro --dry-run --once` with your selectors. Redact
GPU UUIDs, serial numbers, usernames, and any unrelated information before
posting. Say whether the physical display updates correctly; a successful USB
write alone cannot establish what appears on the panel.

For an unsupported controller descriptor, find its `hidrawN` using `--list`,
then include the output of:

```sh
od -An -tx1 /sys/class/hidraw/hidrawN/device/report_descriptor
```

Replace `hidrawN` with the actual node. Please do not upload Windows installers,
proprietary binaries, or full private diagnostic dumps.

## Local checks

Use a current stable Rust toolchain on Linux:

```sh
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo build --release --locked
bash -n install.sh uninstall.sh
./install.sh --destdir /tmp/flux-pro-stage
```

Tests use temporary hwmon fixtures and fake HID writers; they do not access a
real display. Sensor and packet changes should include a regression test for
the behavior being changed. Keep runtime work in-process, retain explicit GPU
selection, and preserve device-identity checks and missing-reading behavior.

## Releases (maintainers)

1. Update `Cargo.toml`, run `cargo check` to refresh `Cargo.lock`, and update
   `CHANGELOG.md`. Run the checks above.
2. Commit, then create and push an annotated `vVERSION` tag matching Cargo's
   version. Tags are release inputs: do not move a published tag.
3. The Release workflow builds on Ubuntu 22.04, packages the x86-64 GNU/Linux
   binary, includes dependency licenses, checks a staged installation, and
   creates a **draft** GitHub release with SHA-256 checksums.
4. Download the draft's archive, verify checksums, test `--version` and real
   sensor readings on supported hardware, review the notes, then publish.

For local packaging, run `python3 scripts/package-release.py` after a release
build. This maintainer helper needs Python 3; the daemon and installer do not.
Local archives inherit your system's glibc requirements. Publish the workflow's
Ubuntu-built artifact to keep the documented glibc baseline.

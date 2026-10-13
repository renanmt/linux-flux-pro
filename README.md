# Linux Flux Pro

[![CI](https://github.com/renanmt/linux-flux-pro/actions/workflows/ci.yml/badge.svg)](https://github.com/renanmt/linux-flux-pro/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Display CPU and GPU temperatures on your **Antec Flux Pro** case under Linux.

A small Rust daemon reads Linux temperature sensors and sends updates directly
to the case over USB HID. It uses the kernel's existing `hidraw` interface;
there is no custom kernel module, Python runtime, or `nvidia-smi` subprocess.

## Hardware support

| Component | Support |
| --- | --- |
| Case display | Antec Flux Pro, USB `2022:0522`, with the [documented HID descriptor](docs/PROTOCOL.md) |
| AMD CPU | Automatic `k10temp:Tctl` selection; hardware-tested |
| Intel CPU | Automatic `coretemp` package selection; needs community hardware testing |
| NVIDIA GPU | Direct NVML access; hardware-tested with an RTX 4080 |
| AMD / other hwmon GPU | Explicit sensor selection, e.g. `hwmon:amdgpu:edge`; needs community hardware testing |

Tested on Arch Linux / Omarchy with the controller revision described above.
Other revisions can share the same USB ID: the daemon checks the exact HID
report descriptor and refuses an unknown revision. Please report yours if it
is rejected. This initial release controls the temperature display only.
It does not control fans or RGB lighting. This is an independent project,
unaffiliated with Antec.

## Install

The packaged service requires **Linux with systemd and udev**. Prebuilt releases
are for **x86-64 Linux with glibc 2.35 or newer**. NVIDIA support additionally
requires your installed NVIDIA driver's `libnvidia-ml.so.1`; it is loaded only
when using an NVIDIA selector. The service uses the `video` and `render` groups
for GPU access, so both groups must exist on your distribution.

Download the `.tar.gz` binary archive and `SHA256SUMS` from
[Releases](https://github.com/renanmt/linux-flux-pro/releases/latest) into the
same directory. For v0.1.0:

```sh
sha256sum --check SHA256SUMS
tar -xzf flux-pro-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
cd flux-pro-v0.1.0-x86_64-unknown-linux-gnu
./flux-pro --list
./flux-pro --dry-run --once
sudo ./install.sh --system
```

The installer creates a dedicated `flux-display` account, installs a udev rule
for display access, and preserves an existing `/etc/default/flux-pro`. It does
not start the service. Edit the configuration as described below, then run:

```sh
sudo systemctl enable --now flux-pro.service
systemctl status flux-pro.service
```

Run only one display service at a time. If replacing another implementation,
stop and disable its service first. To upgrade this project, stop
`flux-pro.service`, run the new installer, then start it again.

### Build from source

Install a current stable Rust toolchain and your distribution's C linker/build
tools, then:

```sh
git clone https://github.com/renanmt/linux-flux-pro.git
cd linux-flux-pro
cargo build --release --locked
./target/release/flux-pro --list
./target/release/flux-pro --dry-run --once
sudo ./install.sh --system
```

Continue with configuration and service activation above. Python is not needed
to build or run the daemon. The maintainer's release packaging helper uses Python.
For a file-only installation preview, use `./install.sh --destdir /tmp/flux-pro-stage`.

## Configuration

Settings live in `/etc/default/flux-pro`:

```ini
CPU_SENSOR=auto
GPU_SENSOR=nvidia:auto
INTERVAL=1
```

`auto` selects exactly one AMD `k10temp:Tctl` or Intel `coretemp` package sensor.
`nvidia:auto` requires exactly one NVIDIA GPU and pins its UUID for the life of
the process. Multiple GPUs require a specific UUID or PCI address. NVIDIA
failures never switch the display to an integrated GPU.

Use `flux-pro --list` to discover your identifiers:

| Setting | Example |
| --- | --- |
| AMD CPU | `CPU_SENSOR=k10temp:Tctl` |
| Intel CPU | `CPU_SENSOR="coretemp:Package id 0"` |
| Specific NVIDIA GPU | `GPU_SENSOR=nvidia:0000:01:00.0` |
| NVIDIA GPU by UUID | `GPU_SENSOR=nvidia:GPU-YOUR-UUID` |
| AMD GPU edge sensor | `GPU_SENSOR=hwmon:amdgpu:edge` |

CPU selectors also accept an absolute `temp*_input` path; GPU hwmon paths use
`hwmon:/sys/class/hwmon/.../temp1_input`. Driver/label selectors are preferable
because numbered hwmon paths can change across boots. Ambiguous matches are
rejected. Once selected, hwmon sensors are pinned to their physical identity
for the life of the process, including after renumbering.

After changing settings:

```sh
sudo systemctl restart flux-pro.service
journalctl -u flux-pro.service -f
```

The update interval accepts 0.1–3600 seconds. The default is one second. The
daemon keeps NVML and the HID connection open between successful updates and
sleeps between reads. It retries failures and logs changes in health instead of
repeating identical errors every second.

### Manual use

```sh
flux-pro --help
flux-pro --gpu hwmon:amdgpu:edge --dry-run --once
flux-pro --cpu k10temp:Tctl --gpu nvidia:auto --dry-run --once
```

`--dry-run` prints readings and packet bytes without opening the display.
`--once` exits unsuccessfully if either sensor or the display write fails.
Without `--once`, the daemon continues and retries. To write manually, first
stop the service, then run `sudo flux-pro --once` or `sudo flux-pro`.
`--device /dev/hidrawN` can choose between multiple matching case displays.

Temperatures use Celsius and truncate to tenths. Missing, faulted, negative,
and above-99.9°C readings send the protocol's missing-value marker (`EE EE EE`)
instead of a misleading number. NVIDIA NVML provides whole-degree readings.
A graceful SIGINT/SIGTERM shutdown sends missing markers to both fields;
a forced kill or power loss may leave the last reading visible.

## Troubleshooting

- **No display found:** check the case's internal USB connection, run
  `lsusb -d 2022:0522`, and inspect `flux-pro --list`.
- **Permission denied:** rerun the installer to apply its udev rule, then
  restart the service. Check `journalctl -u flux-pro.service`.
- **Unknown HID descriptor:** your controller revision needs investigation.
  Open a hardware-support issue; see [Contributing](CONTRIBUTING.md).
- **NVIDIA unavailable:** confirm your NVIDIA driver is loaded and its NVML
  library is installed. On multi-GPU machines, select a UUID or PCI address.
- **Missing or ambiguous sensor:** use `--list`, then test your explicit
  selectors with `--dry-run --once`. Choose your discrete GPU deliberately.
- **Service fails at GROUP:** check `getent group video render`. Adapt
  `SupplementaryGroups` in a systemd override to the GPU-access groups used by
  your distribution, then reload systemd and restart.

The dry-run CLI uses its command-line defaults, **not** `/etc/default/flux-pro`.
Pass your configured selectors explicitly when diagnosing readings. The journal
records the service's sensor failures, recovery, and device errors.

## Uninstall

From an extracted release or source checkout:

```sh
sudo ./uninstall.sh --system
```

This stops/disables the service and removes the installed binary, unit, udev
rule, and sysusers definition. It preserves `/etc/default/flux-pro` and the
inactive `flux-display` account. Remove the saved configuration separately if
you no longer need it. It does not uninstall system Python or unrelated tools.

## Development and protocol

See [Contributing](CONTRIBUTING.md), [the protocol notes](docs/PROTOCOL.md), and
[the changelog](CHANGELOG.md). Hardware is not required for unit tests:

```sh
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```

Protocol investigation used REA to inspect the display path in iUnity 3.1.0.0,
followed by live Linux HID writes. Thanks to
[Nish Tahir's earlier protocol research](https://nishtahir.com/building-an-ubuntu-service-for-my-antec-flux-display/)
for documenting this hardware. Vendor installers, extracted proprietary
binaries, and local analysis dumps are not distributed here.

## License

[MIT](LICENSE). Release archives include third-party dependency license texts.

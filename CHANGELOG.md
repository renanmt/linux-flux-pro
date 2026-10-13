# Changelog

## 0.1.0 — 2026-10-13

Initial public release.

- Rust userspace daemon for the Antec Flux Pro temperature display.
- CPU readings from Linux hwmon and NVIDIA GPU readings through NVML.
- Explicit hwmon GPU selection for compatible sensors.
- Single-NVIDIA-GPU detection, with UUID/PCI selection for multi-GPU systems.
- Validated USB identity and HID descriptor before writing display reports.
- Persistent connections, reconnect attempts, and sensor identity tracking.
- Missing-reading markers, configurable update interval, and graceful shutdown.
- Dedicated systemd service account, udev permissions, and install/uninstall scripts.
- Protocol documentation, unit tests, and checksummed Linux release archives.

Hardware-tested on Arch Linux / Omarchy with an AMD CPU, NVIDIA RTX 4080, and
USB controller 2022:0522 with the documented 31-byte HID descriptor. Intel CPU,
AMD GPU, and other controller revisions need community hardware testing.

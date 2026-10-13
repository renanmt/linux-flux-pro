# Flux Pro display protocol

This project implements the display write path observed in iUnity 3.1.0.0 and
validated with Linux `hidraw` writes. Linux's existing HID driver is sufficient;
no firmware update or custom kernel driver is involved.

## Supported controller

- USB vendor/product: `2022:0522`.
- USB manufacturer/product strings observed: `HWCX` / `USB info.`.
- HID interface 0, 64-byte interrupt IN and OUT reports, without report IDs.
- Exact 31-byte report descriptor:

```text
05 01 09 00 a1 01 15 00 25 ff 19 01 29 08 95 40
75 08 81 02 19 01 29 08 95 40 75 08 91 02 c0
```

The daemon checks USB bus type, VID/PID, and this descriptor through ioctls on
the opened file descriptor before writing. Other descriptors need separate
validation, even if their USB IDs match.

## Output packet

Offsets refer to the USB payload, excluding the Linux API's report-ID prefix.

| Offset | Meaning |
| --- | --- |
| 0–1 | Header: `55 AA` |
| 2–4 | Fixed command: `01 01 06` |
| 5–7 | CPU tens, ones, tenths; binary digits `00`–`09` |
| 8–10 | GPU tens, ones, tenths; binary digits `00`–`09` |
| 11 | Sum of bytes 0–10 modulo 256 |
| 12–63 | Zero padding |

For CPU 42.5°C and GPU 63.2°C, the first 12 payload bytes are:

```text
55 aa 01 01 06 04 02 05 06 03 02 1d
```

The [Linux hidraw API](https://docs.kernel.org/hid/hidraw.html) requires a leading
zero report-ID byte for an unnumbered output report. The daemon writes 65 bytes
in one call: that zero prefix, the 12-byte packet, and 52 zero bytes. It treats
a short write as a failed report rather than sending the remainder separately.
No handshake or feature report was found in the inspected display write path.

`EE EE EE` is the missing-value marker for either temperature. This sender
encodes valid 0°C as zero digits, truncates millidegrees to tenths, and uses the
missing marker for readings outside 0–99.9°C. Those are deliberate choices:
the inspected Windows implementation converts sensor values to integers before
formatting and also uses the missing marker for zero. The marker's exact visual
appearance may depend on the controller revision.

## Investigation provenance

REA inspection of the managed display path in iUnity 3.1.0.0 recovered its
one-second timer, packet generator, VID/PID constants, and HID output buffer
padding. The service is a .NET single-file application with an embedded managed
assembly. Sensor SDK internals and unrelated Windows UI behavior were outside
the investigation.

| Artifact | SHA-256 |
| --- | --- |
| iUnity-3.1.0.0.msi | `72ba60d0e020b5e3bf0378b24a287d3b68217cb6a58fc83dc8fb73a970a581cf` |
| Embedded iUnityService.dll | `6fffb5a28d11e9f2cd0654ebded58e32666e999ff95048e9e1a52855b84a1fde` |

For that assembly, packet generation is method token `0x0600004a`; the USB ID
constants are in `0x06000049`; the one-second timer is in `0x0600004c`.
These coordinates identify the inspected version, not every iUnity release.
Vendor binaries and disassembly dumps are intentionally absent from this repo.

[Nish Tahir's earlier work](https://nishtahir.com/building-an-ubuntu-service-for-my-antec-flux-display/)
describes this packet family in iUnity 2.1. The findings here were checked
against the 3.1 display path and the locally tested controller revision.

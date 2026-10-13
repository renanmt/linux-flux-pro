#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 1 || "$1" != --system || $EUID != 0 ]]; then
  echo 'Usage: sudo ./uninstall.sh --system' >&2
  exit 2
fi
if systemctl cat flux-pro.service >/dev/null 2>&1; then
  systemctl disable --now flux-pro.service
fi
rm -f /usr/local/bin/flux-pro /etc/systemd/system/flux-pro.service \
  /etc/udev/rules.d/70-flux-pro.rules /usr/lib/sysusers.d/flux-pro.conf
systemctl daemon-reload
udevadm control --reload-rules
udevadm trigger --subsystem-match=hidraw --action=change
echo 'Uninstalled. Preserved /etc/default/flux-pro and the inactive flux-display account.'

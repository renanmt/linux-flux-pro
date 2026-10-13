#!/usr/bin/env bash
# Review first. This installs files but never starts or enables the service.
set -euo pipefail
source_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
case "${1:-}" in
  --destdir)
    [[ $# == 2 && "$2" == /* && "$2" != / ]] || { echo 'Usage: ./install.sh --destdir /absolute/staging/directory' >&2; exit 2; }
    destination=$(realpath -m -- "$2")
    [[ "$destination" != / ]] || { echo 'Staging destination must not resolve to /.' >&2; exit 2; }
    ;;
  --system)
    [[ $# == 1 && $EUID == 0 ]] || { echo 'Run sudo ./install.sh --system in your terminal.' >&2; exit 2; }
    destination=
    ;;
  *) echo 'Usage: ./install.sh --destdir /absolute/staging/directory | --system'; exit 2 ;;
esac
binary="$source_dir/flux-pro"
[[ -x "$binary" ]] || binary="$source_dir/target/release/flux-pro"
[[ -x "$binary" ]] || { echo 'Build first: cargo build --release --locked' >&2; exit 1; }
install -Dm755 "$binary" "$destination/usr/local/bin/flux-pro.new"
mv -f "$destination/usr/local/bin/flux-pro.new" "$destination/usr/local/bin/flux-pro"
install -Dm644 "$source_dir/packaging/flux-pro.service" "$destination/etc/systemd/system/flux-pro.service"
install -Dm644 "$source_dir/packaging/70-flux-pro.rules" "$destination/etc/udev/rules.d/70-flux-pro.rules"
install -Dm644 "$source_dir/packaging/flux-pro.sysusers" "$destination/usr/lib/sysusers.d/flux-pro.conf"
if [[ ! -e "$destination/etc/default/flux-pro" ]]; then
  install -Dm644 "$source_dir/packaging/flux-pro.default" "$destination/etc/default/flux-pro"
fi
if [[ -z "$destination" ]]; then
  systemd-sysusers /usr/lib/sysusers.d/flux-pro.conf
  udevadm control --reload-rules
  udevadm trigger --subsystem-match=hidraw --action=change
  systemctl daemon-reload
  echo 'Installed. Review /etc/default/flux-pro, then run: sudo systemctl enable --now flux-pro.service'
else
  echo "Staged files in $destination; no system settings changed."
fi

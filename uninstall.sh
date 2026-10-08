#!/usr/bin/env bash
# Stops the watcher and removes the binary and the two startup files that install.sh added.
set -euo pipefail

name="xfwm-zoom-share-fix"
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}"
unit_path="$config_dir/systemd/user/$name.service"
autostart_path="$config_dir/autostart/$name.desktop"

# systemctl fails for a unit that it does not know, and that unit has nothing to stop.
systemctl --user stop "$name.service" 2>/dev/null || true
systemctl --user reset-failed "$name.service" 2>/dev/null || true

rm -f "$unit_path" "$autostart_path"
systemctl --user daemon-reload

if [[ -x "$HOME/.cargo/bin/$name" ]]; then
	cargo uninstall --root "$HOME/.cargo" "$name"
fi

echo "Removed the watcher, $unit_path, and $autostart_path"

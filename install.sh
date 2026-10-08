#!/usr/bin/env bash
# Builds the watcher, installs it with its two startup files, and starts it in the current X
# session. Run it again after a change: it replaces each file and restarts the watcher.
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
name="xfwm-zoom-share-fix"
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}"
unit_path="$config_dir/systemd/user/$name.service"
autostart_path="$config_dir/autostart/$name.desktop"

# The unit starts %h/.cargo/bin/xfwm-zoom-share-fix, so the root stays fixed. Without --root,
# CARGO_INSTALL_ROOT or the cargo config can move the binary.
cargo install --locked --root "$HOME/.cargo" --path "$repo_dir"

install -D -m 0644 "$repo_dir/dist/$name.service" "$unit_path"
install -D -m 0644 "$repo_dir/dist/$name.desktop" "$autostart_path"
systemctl --user daemon-reload

echo "Installed $HOME/.cargo/bin/$name"
echo "Installed $unit_path"
echo "Installed $autostart_path"

# Ubuntu copies DISPLAY into the systemd user manager at each X login. Without it, the watcher
# cannot connect, so it starts at the next XFCE login instead.
if systemctl --user show-environment | grep -q '^DISPLAY='; then
	systemctl --user restart "$name.service"
	systemctl --user --no-pager --lines=5 status "$name.service"
else
	echo "No X session found. The watcher starts at the next XFCE login."
fi

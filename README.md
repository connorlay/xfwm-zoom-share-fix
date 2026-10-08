# xfwm-zoom-share-fix

A small X11 watcher that stops a full-screen Zoom share from freezing the screen on XFCE. It
sets `_NET_WM_BYPASS_COMPOSITOR` to 2 on the Zoom share frame, so that xfwm4 keeps compositing
the screen.

## The problem

For a full-screen share, the Zoom desktop app maps a window named `cpt_frame_xcb_window` over
the whole screen. The window draws only the border around the shared screen. It also sets
`_NET_WM_BYPASS_COMPOSITOR` to 1, which asks the compositor to skip the window.

- xfwm4 obeys the hint. A window that covers the screen and skips the compositor makes xfwm4
  stop drawing the screen.
- The other windows still draw, but only into their offscreen buffers. The screen keeps the last
  image that xfwm4 drew.
- Input still works, because the frame takes no mouse input. Only the Zoom toolbar seems to
  respond.

The value 2 asks the compositor to never skip the window. xfwm4 then composites the frame like
any other window, and the screen shows the border around the shared screen.

The [Extended Window Manager Hints](https://specifications.freedesktop.org/wm-spec/latest/)
define `_NET_WM_BYPASS_COMPOSITOR`.

## How the watcher works

The watcher uses X events and no polling, so it uses no CPU while no share runs.

1. It receives an event for each new top-level window, and it then watches the properties of
   that window.
2. When a window has the name `cpt_frame_xcb_window` and the hint value 1, the watcher sets the
   hint to 2.
3. If the frame was already visible with the value 1, the watcher unmaps the frame and maps it
   again, so that xfwm4 reads the new value.

In most cases the watcher sets the hint before Zoom maps the frame, and the screen does not
freeze at all.

## Install

The install uses a systemd user service and an XFCE autostart entry. XFCE does not start
`graphical-session.target`, so the autostart entry starts the service after the X session
starts. systemd restarts the watcher after a failure and keeps its log.

```sh
./install.sh
```

The script does these steps:

- It builds and installs the binary to `~/.cargo/bin/xfwm-zoom-share-fix`.
- It copies `dist/xfwm-zoom-share-fix.service` to `~/.config/systemd/user/`.
- It copies `dist/xfwm-zoom-share-fix.desktop` to `~/.config/autostart/`.
- It restarts the service when an X session runs.

Run the script again after a change. It replaces each file and restarts the watcher.

To remove each file and stop the watcher:

```sh
./uninstall.sh
```

## Use

```text
Usage: xfwm-zoom-share-fix [--name <NAME>] [--dry-run]

Options:
  --name <NAME>  The window name to fix [default: cpt_frame_xcb_window]
  --dry-run      Log each change, but change nothing
  -h, --help     Print this help
  -V, --version  Print the version
```

To see the log of the service:

```sh
journalctl --user -u xfwm-zoom-share-fix
```

A fix writes lines like these:

```text
0x5c00004: set _NET_WM_BYPASS_COMPOSITOR to 2
0x5c00004: mapped the frame again
```

### Exit codes

| Code | Meaning |
|---|---|
| 0 | The X session ended, so systemd does not restart the watcher. |
| 1 | The watcher could not connect to the X server or watch the screen. systemd retries. |
| 2 | An argument is not valid. |

## Limits

- **The match uses the window name only.** Zoom renamed this window once, from
  `cpt_frame_window`, around version 5.9.3. If Zoom renames it again, the freeze comes back,
  and the log shows no fix during a share. Pass the new name with `--name`.
- **X11 only.** A Wayland session needs no fix, because it does not use xfwm4.
- **Tested with** Zoom 7.2.1.5760, xfwm4 4.20.0, and Xorg 1.21.1.22 on Ubuntu 26.04.

## Tests

```sh
cargo test
```

The integration tests start their own Xvfb server for each test, so they never touch your
desktop. Install Xvfb first:

```sh
sudo apt install xvfb
```

Xvfb runs no compositor. The tests prove that the watcher finds the frame, sets the hint, and
maps the frame again. Only a real Zoom share proves that xfwm4 keeps drawing the screen.

## License

MIT. See [LICENSE](LICENSE).

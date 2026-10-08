//! Keeps the Zoom share frame inside the xfwm4 compositor.
//!
//! For a full-screen share, Zoom maps an override-redirect window named `cpt_frame_xcb_window`
//! over the whole screen, with `_NET_WM_BYPASS_COMPOSITOR` set to 1. xfwm4 then does not
//! composite the screen, and the screen shows a stale image. The watcher sets the hint to 2,
//! which asks the compositor to never skip the window.
//!
//! See the [Extended Window Manager Hints](https://specifications.freedesktop.org/wm-spec/latest/)
//! for `_NET_WM_BYPASS_COMPOSITOR`.

pub mod cli;
pub mod tracker;
pub mod watcher;

/// The configuration of the watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The window name to fix.
    pub name: String,
    /// When `true`, the watcher logs each change and changes nothing.
    pub dry_run: bool,
}

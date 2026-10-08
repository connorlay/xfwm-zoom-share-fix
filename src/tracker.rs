//! Decides when the watcher sets the hint on a frame and when it maps the frame again.
//!
//! The tracker holds no X connection. The watcher reports each X event to it, and the tracker
//! returns the next step. The X server sends the events of one connection in the order in which
//! it processes the requests. So the order of a `MapNotify` and the `PropertyNotify` of the own
//! change shows whether the compositor saw the frame with the old hint.

use std::collections::HashMap;

/// An X window id.
pub type WindowId = u32;

/// The value of `_NET_WM_BYPASS_COMPOSITOR` that asks the compositor to skip a window.
pub const BYPASS_REQUESTED: u32 = 1;

/// The value of `_NET_WM_BYPASS_COMPOSITOR` that asks the compositor to never skip a window.
pub const BYPASS_NEVER: u32 = 2;

/// Returns `true` when a window has the target name and asks the compositor to skip it.
///
/// A window without the hint, or with any other value, needs no fix.
pub fn needs_fix(name: Option<&str>, bypass: Option<u32>, target: &str) -> bool {
    name == Some(target) && bypass == Some(BYPASS_REQUESTED)
}

/// The meaning of a `PropertyNotify` event for `_NET_WM_BYPASS_COMPOSITOR`.
#[derive(Debug, PartialEq, Eq)]
pub enum HintChange {
    /// The event reports the change that the watcher made. When `remap` is `true`, the frame was
    /// visible with the old hint, so the watcher must unmap the frame and map it again.
    Own { remap: bool },
    /// Another client changed the hint. The watcher must read the window again.
    Other,
}

#[derive(Debug, Default)]
struct WindowState {
    mapped: bool,
    own_change_pending: bool,
    remap_after_change: bool,
}

/// Holds the state of each top-level window that the watcher follows.
#[derive(Debug, Default)]
pub struct Tracker {
    windows: HashMap<WindowId, WindowState>,
}

impl Tracker {
    /// Returns an empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts to follow a window. A window that the tracker already follows keeps its state.
    pub fn on_created(&mut self, window: WindowId, mapped: bool) {
        self.windows.entry(window).or_insert(WindowState {
            mapped,
            ..WindowState::default()
        });
    }

    /// Stops to follow a window that the X server destroyed or that left the root window.
    pub fn on_removed(&mut self, window: WindowId) {
        self.windows.remove(&window);
    }

    /// Records that a window is no longer visible.
    pub fn on_unmapped(&mut self, window: WindowId) {
        self.state(window).mapped = false;
    }

    /// Records that a window is visible, and returns `true` when the watcher must read it.
    ///
    /// While the own change is pending, the map came first. The compositor then saw the old hint,
    /// so the frame must map again after the change arrives.
    pub fn on_mapped(&mut self, window: WindowId) -> bool {
        let state = self.state(window);
        state.mapped = true;

        if state.own_change_pending {
            state.remap_after_change = true;
            false
        } else {
            true
        }
    }

    /// Returns the meaning of a change to `_NET_WM_BYPASS_COMPOSITOR` on a window.
    pub fn on_hint_changed(&mut self, window: WindowId) -> HintChange {
        let state = self.state(window);

        if state.own_change_pending {
            state.own_change_pending = false;
            HintChange::Own {
                remap: std::mem::take(&mut state.remap_after_change),
            }
        } else {
            HintChange::Other
        }
    }

    /// Returns `true` when the watcher must set the hint now.
    ///
    /// The caller reads the window and passes the result of [`needs_fix`]. While an own change is
    /// pending, the tracker returns `false`, because the `PropertyNotify` of that change comes
    /// next.
    pub fn on_read(&mut self, window: WindowId, needs_fix: bool) -> bool {
        let state = self.state(window);

        if !needs_fix || state.own_change_pending {
            return false;
        }

        state.own_change_pending = true;
        state.remap_after_change = state.mapped;
        true
    }

    /// Returns the number of windows that the tracker follows.
    pub fn len(&self) -> usize {
        self.windows.len()
    }

    /// Returns `true` when the tracker follows no window.
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    // An event can arrive for a window that the tracker does not follow yet, for example when the
    // window existed before the watcher started. A new default state is correct for that window.
    fn state(&mut self, window: WindowId) -> &mut WindowState {
        self.windows.entry(window).or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: WindowId = 0x5c0_0004;
    const TARGET: &str = "cpt_frame_xcb_window";

    mod needs_fix {
        use super::*;

        #[test]
        fn is_true_for_the_target_name_with_the_requested_value() {
            assert!(needs_fix(Some(TARGET), Some(BYPASS_REQUESTED), TARGET));
        }

        #[test]
        fn is_false_for_another_name() {
            assert!(!needs_fix(Some("Meeting"), Some(BYPASS_REQUESTED), TARGET));
        }

        #[test]
        fn is_false_for_a_window_without_a_name() {
            assert!(!needs_fix(None, Some(BYPASS_REQUESTED), TARGET));
        }

        #[test]
        fn is_false_for_the_never_value() {
            assert!(!needs_fix(Some(TARGET), Some(BYPASS_NEVER), TARGET));
        }

        #[test]
        fn is_false_for_a_window_without_the_hint() {
            assert!(!needs_fix(Some(TARGET), None, TARGET));
        }

        #[test]
        fn is_false_for_a_value_that_the_specification_does_not_define() {
            assert!(!needs_fix(Some(TARGET), Some(7), TARGET));
        }
    }

    mod tracker {
        use super::*;

        #[test]
        fn sets_the_hint_with_no_remap_when_the_change_arrives_before_the_map() {
            let mut tracker = Tracker::new();
            tracker.on_created(FRAME, false);

            assert!(tracker.on_read(FRAME, true));
            assert_eq!(
                tracker.on_hint_changed(FRAME),
                HintChange::Own { remap: false }
            );
            assert!(tracker.on_mapped(FRAME));
        }

        #[test]
        fn remaps_when_the_map_arrives_before_the_change() {
            let mut tracker = Tracker::new();
            tracker.on_created(FRAME, false);

            assert!(tracker.on_read(FRAME, true));
            assert!(!tracker.on_mapped(FRAME));
            assert_eq!(
                tracker.on_hint_changed(FRAME),
                HintChange::Own { remap: true }
            );
        }

        #[test]
        fn remaps_a_frame_that_is_already_visible() {
            let mut tracker = Tracker::new();
            tracker.on_created(FRAME, true);

            assert!(tracker.on_read(FRAME, true));
            assert_eq!(
                tracker.on_hint_changed(FRAME),
                HintChange::Own { remap: true }
            );
        }

        #[test]
        fn remaps_only_once_for_one_change() {
            let mut tracker = Tracker::new();
            tracker.on_created(FRAME, true);
            tracker.on_read(FRAME, true);
            tracker.on_hint_changed(FRAME);

            tracker.on_unmapped(FRAME);
            assert!(tracker.on_mapped(FRAME));
            assert!(!tracker.on_read(FRAME, false));
        }

        #[test]
        fn sends_no_second_change_while_one_is_pending() {
            let mut tracker = Tracker::new();
            tracker.on_created(FRAME, false);

            assert!(tracker.on_read(FRAME, true));
            assert!(!tracker.on_read(FRAME, true));
        }

        #[test]
        fn reports_a_change_by_another_client_after_the_own_change() {
            let mut tracker = Tracker::new();
            tracker.on_created(FRAME, false);
            tracker.on_read(FRAME, true);
            tracker.on_hint_changed(FRAME);

            assert_eq!(tracker.on_hint_changed(FRAME), HintChange::Other);
            assert!(tracker.on_read(FRAME, true));
        }

        #[test]
        fn does_nothing_for_a_window_that_needs_no_fix() {
            let mut tracker = Tracker::new();
            tracker.on_created(FRAME, true);

            assert!(!tracker.on_read(FRAME, false));
            assert_eq!(tracker.on_hint_changed(FRAME), HintChange::Other);
        }

        #[test]
        fn keeps_the_state_when_a_window_is_created_twice() {
            let mut tracker = Tracker::new();
            tracker.on_created(FRAME, false);
            tracker.on_read(FRAME, true);

            tracker.on_created(FRAME, true);

            assert_eq!(
                tracker.on_hint_changed(FRAME),
                HintChange::Own { remap: false }
            );
        }

        #[test]
        fn follows_a_window_that_it_did_not_see_created() {
            let mut tracker = Tracker::new();

            assert!(tracker.on_mapped(FRAME));
            assert!(tracker.on_read(FRAME, true));
            assert_eq!(
                tracker.on_hint_changed(FRAME),
                HintChange::Own { remap: true }
            );
        }

        #[test]
        fn forgets_a_removed_window() {
            let mut tracker = Tracker::new();
            tracker.on_created(FRAME, false);

            tracker.on_removed(FRAME);

            assert!(tracker.is_empty());
        }
    }
}

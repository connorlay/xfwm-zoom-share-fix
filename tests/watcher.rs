mod support;

use support::{Client, FRAME_NAME, MapEvent, Watcher, Xvfb};

#[test]
fn sets_the_hint_before_the_frame_maps_with_no_remap() {
    let xvfb = Xvfb::start();
    let _watcher = Watcher::start(&xvfb.display, &[]);
    let client = Client::connect(&xvfb.display);

    let frame = client.create_named_frame(FRAME_NAME, 1);
    assert!(client.wait_for_bypass(frame, 2));
    client.map(frame);
    client.sync_with_watcher(FRAME_NAME);

    assert!(client.is_viewable(frame));
    assert_eq!(client.map_events(frame), vec![MapEvent::Map]);
}

#[test]
fn fixes_a_frame_that_maps_together_with_the_hint() {
    let xvfb = Xvfb::start();
    let _watcher = Watcher::start(&xvfb.display, &[]);
    let client = Client::connect(&xvfb.display);

    let frame = client.create_named_frame(FRAME_NAME, 1);
    client.map(frame);

    assert!(client.wait_for_bypass(frame, 2));
    client.sync_with_watcher(FRAME_NAME);
    assert!(client.is_viewable(frame));
}

#[test]
fn maps_again_a_frame_that_was_visible_before_the_watcher_started() {
    let xvfb = Xvfb::start();
    let client = Client::connect(&xvfb.display);
    let frame = client.create_named_frame(FRAME_NAME, 1);
    client.map(frame);

    let mut watcher = Watcher::start(&xvfb.display, &[]);

    assert!(client.wait_for_bypass(frame, 2));
    watcher.wait_for_line("mapped the frame again");
    client.sync_with_watcher(FRAME_NAME);
    assert!(client.is_viewable(frame));
    assert_eq!(
        client.map_events(frame),
        vec![MapEvent::Map, MapEvent::Unmap, MapEvent::Map]
    );
}

#[test]
fn maps_again_a_visible_frame_that_gets_the_hint_late() {
    let xvfb = Xvfb::start();
    let mut watcher = Watcher::start(&xvfb.display, &[]);
    let client = Client::connect(&xvfb.display);

    let frame = client.create_frame();
    client.set_wm_name(frame, FRAME_NAME);
    client.map(frame);
    client.set_bypass(frame, 1);

    assert!(client.wait_for_bypass(frame, 2));
    watcher.wait_for_line("mapped the frame again");
    client.sync_with_watcher(FRAME_NAME);
    assert!(client.is_viewable(frame));
}

#[test]
fn fixes_the_hint_again_when_the_app_sets_it_back() {
    let xvfb = Xvfb::start();
    let _watcher = Watcher::start(&xvfb.display, &[]);
    let client = Client::connect(&xvfb.display);
    let frame = client.create_named_frame(FRAME_NAME, 1);
    assert!(client.wait_for_bypass(frame, 2));

    client.set_bypass(frame, 1);

    assert!(client.wait_for_bypass(frame, 2));
}

#[test]
fn fixes_a_frame_that_gets_its_name_after_the_hint() {
    let xvfb = Xvfb::start();
    let _watcher = Watcher::start(&xvfb.display, &[]);
    let client = Client::connect(&xvfb.display);

    let frame = client.create_frame();
    client.set_bypass(frame, 1);
    client.sync_with_watcher(FRAME_NAME);
    assert_eq!(client.bypass(frame), Some(1));

    client.set_net_wm_name(frame, FRAME_NAME);

    assert!(client.wait_for_bypass(frame, 2));
}

#[test]
fn leaves_a_window_with_another_name_unchanged() {
    let xvfb = Xvfb::start();
    let _watcher = Watcher::start(&xvfb.display, &[]);
    let client = Client::connect(&xvfb.display);

    let other = client.create_named_frame("Meeting", 1);
    client.map(other);
    client.sync_with_watcher(FRAME_NAME);

    assert_eq!(client.bypass(other), Some(1));
}

#[test]
fn leaves_a_frame_without_the_requested_value_unchanged() {
    let xvfb = Xvfb::start();
    let _watcher = Watcher::start(&xvfb.display, &[]);
    let client = Client::connect(&xvfb.display);

    let no_hint = client.create_frame();
    client.set_wm_name(no_hint, FRAME_NAME);
    let no_preference = client.create_named_frame(FRAME_NAME, 0);
    client.sync_with_watcher(FRAME_NAME);

    assert_eq!(client.bypass(no_hint), None);
    assert_eq!(client.bypass(no_preference), Some(0));
}

#[test]
fn fixes_only_the_name_that_the_name_option_gives() {
    let xvfb = Xvfb::start();
    let _watcher = Watcher::start(&xvfb.display, &["--name", "custom_frame"]);
    let client = Client::connect(&xvfb.display);

    let zoom_frame = client.create_named_frame(FRAME_NAME, 1);
    let custom_frame = client.create_named_frame("custom_frame", 1);

    assert!(client.wait_for_bypass(custom_frame, 2));
    assert_eq!(client.bypass(zoom_frame), Some(1));
}

#[test]
fn changes_nothing_in_a_dry_run() {
    let xvfb = Xvfb::start();
    let mut watcher = Watcher::start(&xvfb.display, &["--dry-run"]);
    let client = Client::connect(&xvfb.display);

    let frame = client.create_named_frame(FRAME_NAME, 1);
    client.map(frame);

    let line = watcher.wait_for_line("would set _NET_WM_BYPASS_COMPOSITOR to 2");
    assert!(line.starts_with(&format!("{frame:#x}")));
    assert_eq!(client.bypass(frame), Some(1));
    assert_eq!(client.map_events(frame), vec![MapEvent::Map]);
}

#[test]
fn exits_with_success_when_the_x_server_stops() {
    let mut xvfb = Xvfb::start();
    let mut watcher = Watcher::start(&xvfb.display, &[]);

    xvfb.stop();

    assert!(watcher.wait_for_exit().success());
    assert!(
        watcher
            .log()
            .iter()
            .any(|line| line.contains("lost the connection to the X server"))
    );
}

// Xvfb with `-displayfd` takes the lowest free display number. A parallel test can therefore take
// a number that another test just freed, so this test uses a number that no test takes. x11rb adds
// 6000 to the number for the TCP port, so the number must stay below 59536.
#[test]
fn exits_with_failure_when_no_x_server_runs() {
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_xfwm-zoom-share-fix"))
        .env("DISPLAY", ":4242")
        .stderr(std::process::Stdio::null())
        .status()
        .expect("run the watcher");

    assert_eq!(status.code(), Some(1));
}

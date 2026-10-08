//! Starts an Xvfb server and the watcher binary, and acts as the X client of a test.
//!
//! Each test gets its own Xvfb server, so the tests never touch the desktop and can run in
//! parallel. Xvfb runs no window manager and no compositor. The tests prove the X protocol work
//! of the watcher, not the drawing of the screen.

#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, MapState, PropMode, Window,
    WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

pub const FRAME_NAME: &str = "cpt_frame_xcb_window";

const TIMEOUT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(10);

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        _NET_WM_NAME,
        _NET_WM_BYPASS_COMPOSITOR,
        UTF8_STRING,
    }
}

/// An Xvfb server that stops when the value drops.
pub struct Xvfb {
    child: Child,
    pub display: String,
}

impl Xvfb {
    /// Starts Xvfb on a free display. Xvfb picks the display and writes its number to the file
    /// descriptor of `-displayfd` when it accepts connections.
    pub fn start() -> Self {
        let mut child = Command::new("Xvfb")
            .args([
                "-displayfd",
                "1",
                "-nolisten",
                "tcp",
                "-screen",
                "0",
                "1440x900x24",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start Xvfb; install it with `sudo apt install xvfb`");

        let mut line = String::new();
        BufReader::new(child.stdout.take().expect("Xvfb stdout"))
            .read_line(&mut line)
            .expect("read the display number from Xvfb");

        let number = line.trim();
        assert!(!number.is_empty(), "Xvfb exited before it chose a display");

        Self {
            child,
            display: format!(":{number}"),
        }
    }

    /// Stops the server. SIGTERM lets Xvfb delete its socket and its lock file, and SIGKILL
    /// follows when Xvfb does not exit in time. A server that already stopped stays stopped.
    pub fn stop(&mut self) {
        if let Ok(Some(_)) = self.child.try_wait() {
            return;
        }

        let _ = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status();

        let deadline = Instant::now() + TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            thread::sleep(POLL);
        }

        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The watcher binary, with its log lines. The process stops when the value drops.
pub struct Watcher {
    child: Child,
    lines: Receiver<String>,
    log: Vec<String>,
}

impl Watcher {
    /// Starts the watcher on `display` and waits until it watches the screen.
    pub fn start(display: &str, args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_xfwm-zoom-share-fix"))
            .args(args)
            .env("DISPLAY", display)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start the watcher");

        let stderr = child.stderr.take().expect("watcher stderr");
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });

        let mut watcher = Self {
            child,
            lines,
            log: Vec::new(),
        };
        watcher.wait_for_line("watching for windows");
        watcher
    }

    /// Returns the first new log line that holds `needle`. Panics after the timeout.
    pub fn wait_for_line(&mut self, needle: &str) -> String {
        let deadline = Instant::now() + TIMEOUT;

        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match self.lines.recv_timeout(left) {
                Ok(line) => {
                    self.log.push(line.clone());
                    if line.contains(needle) {
                        return line;
                    }
                }
                Err(_) => break,
            }
        }

        panic!("no log line holds {needle:?}; log: {:#?}", self.log);
    }

    /// Returns each log line so far.
    pub fn log(&mut self) -> Vec<String> {
        self.log.extend(self.lines.try_iter());
        self.log.clone()
    }

    /// Waits for the process to exit, and returns its status. Panics after the timeout.
    pub fn wait_for_exit(&mut self) -> ExitStatus {
        let deadline = Instant::now() + TIMEOUT;

        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().expect("poll the watcher") {
                return status;
            }
            thread::sleep(POLL);
        }

        panic!("the watcher did not exit; log: {:#?}", self.log());
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A map or an unmap of a window, as the X client of the test sees it.
#[derive(Debug, PartialEq, Eq)]
pub enum MapEvent {
    Map,
    Unmap,
}

/// The X client of a test. It plays the part of Zoom.
pub struct Client {
    conn: RustConnection,
    root: Window,
    atoms: Atoms,
}

impl Client {
    pub fn connect(display: &str) -> Self {
        let (conn, screen_num) = x11rb::connect(Some(display)).expect("connect to Xvfb");
        let root = conn.setup().roots[screen_num].root;
        let atoms = Atoms::new(&conn)
            .expect("intern atoms")
            .reply()
            .expect("intern atoms");

        Self { conn, root, atoms }
    }

    /// Creates an unmapped override-redirect window over the full screen, like the Zoom frame.
    /// The client receives the map events of the window.
    pub fn create_frame(&self) -> Window {
        let window = self.conn.generate_id().expect("window id");
        let attributes = CreateWindowAux::new()
            .override_redirect(1)
            .event_mask(EventMask::STRUCTURE_NOTIFY);

        self.conn
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                self.root,
                0,
                0,
                1440,
                900,
                0,
                WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &attributes,
            )
            .expect("create a window");
        self.flush();
        window
    }

    /// Creates a frame with `WM_NAME` and the hint set, and does not map it.
    pub fn create_named_frame(&self, name: &str, bypass: u32) -> Window {
        let window = self.create_frame();
        self.set_wm_name(window, name);
        self.set_bypass(window, bypass);
        window
    }

    pub fn set_wm_name(&self, window: Window, name: &str) {
        self.conn
            .change_property8(
                PropMode::REPLACE,
                window,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                name.as_bytes(),
            )
            .expect("set WM_NAME");
        self.flush();
    }

    pub fn set_net_wm_name(&self, window: Window, name: &str) {
        self.conn
            .change_property8(
                PropMode::REPLACE,
                window,
                self.atoms._NET_WM_NAME,
                self.atoms.UTF8_STRING,
                name.as_bytes(),
            )
            .expect("set _NET_WM_NAME");
        self.flush();
    }

    pub fn set_bypass(&self, window: Window, value: u32) {
        self.conn
            .change_property32(
                PropMode::REPLACE,
                window,
                self.atoms._NET_WM_BYPASS_COMPOSITOR,
                AtomEnum::CARDINAL,
                &[value],
            )
            .expect("set _NET_WM_BYPASS_COMPOSITOR");
        self.flush();
    }

    pub fn map(&self, window: Window) {
        self.conn.map_window(window).expect("map the window");
        self.flush();
    }

    pub fn bypass(&self, window: Window) -> Option<u32> {
        self.conn
            .get_property(
                false,
                window,
                self.atoms._NET_WM_BYPASS_COMPOSITOR,
                AtomEnum::CARDINAL,
                0,
                1,
            )
            .expect("read _NET_WM_BYPASS_COMPOSITOR")
            .reply()
            .expect("read _NET_WM_BYPASS_COMPOSITOR")
            .value32()
            .and_then(|mut values| values.next())
    }

    pub fn is_viewable(&self, window: Window) -> bool {
        self.conn
            .get_window_attributes(window)
            .expect("read the window attributes")
            .reply()
            .expect("read the window attributes")
            .map_state
            == MapState::VIEWABLE
    }

    /// Waits until the hint of `window` has `value`, and returns `false` after the timeout.
    pub fn wait_for_bypass(&self, window: Window, value: u32) -> bool {
        let deadline = Instant::now() + TIMEOUT;

        while Instant::now() < deadline {
            if self.bypass(window) == Some(value) {
                return true;
            }
            thread::sleep(POLL);
        }

        false
    }

    /// Waits until the watcher handled each event that the X server sent it before this call.
    ///
    /// The X server sends the events of one connection in order. So when the watcher fixes a new
    /// frame, it handled each earlier event, and the X server processed each earlier request of
    /// the watcher.
    pub fn sync_with_watcher(&self, name: &str) {
        let marker = self.create_named_frame(name, 1);
        assert!(
            self.wait_for_bypass(marker, 2),
            "the watcher did not fix the marker frame"
        );
    }

    /// Returns the map and unmap events of `window` that arrived so far.
    pub fn map_events(&self, window: Window) -> Vec<MapEvent> {
        let mut events = Vec::new();

        while let Some(event) = self.conn.poll_for_event().expect("poll for events") {
            match event {
                Event::MapNotify(event) if event.window == window => events.push(MapEvent::Map),
                Event::UnmapNotify(event) if event.window == window => events.push(MapEvent::Unmap),
                _ => {}
            }
        }

        events
    }

    fn flush(&self) {
        self.conn.flush().expect("flush the X connection");
    }
}

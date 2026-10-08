//! Follows the top-level windows of one X screen and sets the hint on each Zoom share frame.

use std::fmt;

use x11rb::connection::Connection;
use x11rb::errors::{ConnectionError, ReplyError};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, EventMask, MapState, PropMode,
    PropertyNotifyEvent, ReparentNotifyEvent, Window,
};
use x11rb::wrapper::ConnectionExt as _;

use crate::Config;
use crate::tracker::{BYPASS_NEVER, HintChange, Tracker, needs_fix};

// The longest name that the watcher reads, in 32-bit units. Zoom names its frame with 20 bytes.
const NAME_LENGTH: u32 = 64;

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        _NET_WM_NAME,
        _NET_WM_BYPASS_COMPOSITOR,
        UTF8_STRING,
    }
}

/// An error that stops the watcher before it can watch the screen.
#[derive(Debug)]
pub enum WatchError {
    /// The connection to the X server failed.
    Connection(ConnectionError),
    /// The X server refused a request at startup.
    Reply(ReplyError),
}

impl fmt::Display for WatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(error) => write!(f, "{error}"),
            Self::Reply(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for WatchError {}

impl From<ConnectionError> for WatchError {
    fn from(error: ConnectionError) -> Self {
        Self::Connection(error)
    }
}

impl From<ReplyError> for WatchError {
    fn from(error: ReplyError) -> Self {
        Self::Reply(error)
    }
}

struct WindowProperties {
    name: Option<String>,
    bypass: Option<u32>,
}

/// Watches one X screen for share frames.
pub struct Watcher<C: Connection> {
    conn: C,
    root: Window,
    atoms: Atoms,
    config: Config,
    tracker: Tracker,
}

impl<C: Connection> Watcher<C> {
    /// Returns a watcher that receives the window events of the root window of `screen_num`.
    ///
    /// The watcher sees no window until the caller calls [`Watcher::scan`] and [`Watcher::run`].
    pub fn new(conn: C, screen_num: usize, config: Config) -> Result<Self, WatchError> {
        let root = conn.setup().roots[screen_num].root;
        let atoms = Atoms::new(&conn)?.reply()?;
        let attributes =
            ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_NOTIFY);
        conn.change_window_attributes(root, &attributes)?.check()?;

        Ok(Self {
            conn,
            root,
            atoms,
            config,
            tracker: Tracker::new(),
        })
    }

    /// Follows each top-level window that existed before the watcher started.
    ///
    /// A frame that is already visible gets the fix at once.
    pub fn scan(&mut self) -> Result<(), WatchError> {
        let tree = self.conn.query_tree(self.root)?.reply()?;

        for window in tree.children {
            self.follow(window, None)?;
        }

        self.conn.flush()?;
        Ok(())
    }

    /// Handles X events until the connection fails, and returns the error.
    ///
    /// The connection fails when the X session ends.
    pub fn run(&mut self) -> ConnectionError {
        loop {
            let result = self
                .conn
                .wait_for_event()
                .and_then(|event| self.handle(event));

            if let Err(error) = result {
                return error;
            }
        }
    }

    fn handle(&mut self, event: Event) -> Result<(), ConnectionError> {
        match event {
            Event::CreateNotify(event) if event.parent == self.root => {
                self.follow(event.window, Some(false))?;
            }
            Event::DestroyNotify(event) => self.tracker.on_removed(event.window),
            Event::ReparentNotify(event) => self.on_reparent(&event)?,
            Event::MapNotify(event) => {
                if self.tracker.on_mapped(event.window) {
                    self.evaluate(event.window)?;
                }
            }
            Event::UnmapNotify(event) => self.tracker.on_unmapped(event.window),
            Event::PropertyNotify(event) => self.on_property(&event)?,
            Event::Error(error) => eprintln!("X error: {error:?}"),
            _ => {}
        }

        self.conn.flush()
    }

    // `mapped` is `None` when the watcher must ask the X server for the map state.
    fn follow(&mut self, window: Window, mapped: Option<bool>) -> Result<(), ConnectionError> {
        let attributes = ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE);
        self.conn
            .change_window_attributes(window, &attributes)?
            .ignore_error();

        let mapped = match mapped {
            Some(mapped) => mapped,
            None => match read_map_state(&self.conn, window)? {
                Some(mapped) => mapped,
                None => return Ok(()),
            },
        };

        self.tracker.on_created(window, mapped);
        self.evaluate(window)
    }

    // A window manager moves each managed window into a frame of its own. The Zoom share frame is
    // an override-redirect window, so it stays a child of the root window.
    fn on_reparent(&mut self, event: &ReparentNotifyEvent) -> Result<(), ConnectionError> {
        if event.parent == self.root {
            return self.follow(event.window, None);
        }

        self.tracker.on_removed(event.window);
        let attributes = ChangeWindowAttributesAux::new().event_mask(EventMask::NO_EVENT);
        self.conn
            .change_window_attributes(event.window, &attributes)?
            .ignore_error();
        Ok(())
    }

    fn on_property(&mut self, event: &PropertyNotifyEvent) -> Result<(), ConnectionError> {
        if event.atom == self.atoms._NET_WM_BYPASS_COMPOSITOR {
            match self.tracker.on_hint_changed(event.window) {
                HintChange::Own { remap: true } => self.remap(event.window)?,
                HintChange::Own { remap: false } => {}
                HintChange::Other => self.evaluate(event.window)?,
            }
        } else if event.atom == u32::from(AtomEnum::WM_NAME)
            || event.atom == self.atoms._NET_WM_NAME
        {
            self.evaluate(event.window)?;
        }

        Ok(())
    }

    fn evaluate(&mut self, window: Window) -> Result<(), ConnectionError> {
        let Some(properties) = self.read_properties(window)? else {
            self.tracker.on_removed(window);
            return Ok(());
        };

        if !needs_fix(
            properties.name.as_deref(),
            properties.bypass,
            &self.config.name,
        ) {
            return Ok(());
        }

        if self.config.dry_run {
            eprintln!("{window:#x}: would set _NET_WM_BYPASS_COMPOSITOR to {BYPASS_NEVER}");
            return Ok(());
        }

        if self.tracker.on_read(window, true) {
            self.conn
                .change_property32(
                    PropMode::REPLACE,
                    window,
                    self.atoms._NET_WM_BYPASS_COMPOSITOR,
                    AtomEnum::CARDINAL,
                    &[BYPASS_NEVER],
                )?
                .ignore_error();
            eprintln!("{window:#x}: set _NET_WM_BYPASS_COMPOSITOR to {BYPASS_NEVER}");
        }

        Ok(())
    }

    // The compositor reads the hint when a window maps, so a visible frame must map again.
    fn remap(&mut self, window: Window) -> Result<(), ConnectionError> {
        self.conn.unmap_window(window)?.ignore_error();
        self.conn.map_window(window)?.ignore_error();
        eprintln!("{window:#x}: mapped the frame again");
        Ok(())
    }

    // Returns `None` when the window no longer exists. The three requests go out before the first
    // reply, so the read costs one round trip.
    fn read_properties(&self, window: Window) -> Result<Option<WindowProperties>, ConnectionError> {
        let net_name = self.conn.get_property(
            false,
            window,
            self.atoms._NET_WM_NAME,
            self.atoms.UTF8_STRING,
            0,
            NAME_LENGTH,
        )?;
        let wm_name = self.conn.get_property(
            false,
            window,
            AtomEnum::WM_NAME,
            AtomEnum::ANY,
            0,
            NAME_LENGTH,
        )?;
        let bypass = self.conn.get_property(
            false,
            window,
            self.atoms._NET_WM_BYPASS_COMPOSITOR,
            AtomEnum::CARDINAL,
            0,
            1,
        )?;

        let (Some(net_name), Some(wm_name), Some(bypass)) = (
            gone_on_error(net_name.reply())?,
            gone_on_error(wm_name.reply())?,
            gone_on_error(bypass.reply())?,
        ) else {
            return Ok(None);
        };

        let name = [net_name, wm_name]
            .into_iter()
            .find(|reply| !reply.value.is_empty())
            .map(|reply| String::from_utf8_lossy(&reply.value).into_owned());

        Ok(Some(WindowProperties {
            name,
            bypass: bypass.value32().and_then(|mut values| values.next()),
        }))
    }
}

// Returns `None` when the window no longer exists.
fn read_map_state(conn: &impl Connection, window: Window) -> Result<Option<bool>, ConnectionError> {
    let reply = gone_on_error(conn.get_window_attributes(window)?.reply())?;
    Ok(reply.map(|attributes| attributes.map_state != MapState::UNMAPPED))
}

// A window can disappear between an event and the read of its properties. The X server then
// answers with an error, which means that the window is gone.
fn gone_on_error<T>(result: Result<T, ReplyError>) -> Result<Option<T>, ConnectionError> {
    match result {
        Ok(reply) => Ok(Some(reply)),
        Err(ReplyError::X11Error(_)) => Ok(None),
        Err(ReplyError::ConnectionError(error)) => Err(error),
    }
}

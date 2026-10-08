use std::process::ExitCode;

use xfwm_zoom_share_fix::cli::{Command, USAGE, parse_args};
use xfwm_zoom_share_fix::watcher::Watcher;

// A failure before the watcher sees the screen exits with an error, so that systemd starts the
// watcher again. The X connection fails when the session ends, so that exit is a success, and
// systemd does not try to start the watcher without an X server.
fn main() -> ExitCode {
    let config = match parse_args(std::env::args().skip(1)) {
        Ok(Command::Run(config)) => config,
        Ok(Command::Help) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(Command::Version) => {
            println!("xfwm-zoom-share-fix {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprint!("error: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let (conn, screen_num) = match x11rb::connect(None) {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("error: cannot connect to the X server: {error}");
            return ExitCode::FAILURE;
        }
    };

    let mode = if config.dry_run { " (dry run)" } else { "" };
    let name = config.name.clone();

    let result = Watcher::new(conn, screen_num, config).and_then(|mut watcher| {
        watcher.scan()?;
        Ok(watcher)
    });

    let mut watcher = match result {
        Ok(watcher) => watcher,
        Err(error) => {
            eprintln!("error: cannot watch the X screen: {error}");
            return ExitCode::FAILURE;
        }
    };

    eprintln!("watching for windows named {name:?}{mode}");
    let error = watcher.run();
    eprintln!("lost the connection to the X server: {error}");
    ExitCode::SUCCESS
}

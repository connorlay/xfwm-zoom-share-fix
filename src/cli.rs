//! Reads the command-line arguments.

use crate::Config;

/// The window name of the Zoom share frame.
pub const DEFAULT_NAME: &str = "cpt_frame_xcb_window";

/// The help text.
pub const USAGE: &str = "\
Usage: xfwm-zoom-share-fix [--name <NAME>] [--dry-run]

Keeps the Zoom share frame inside the xfwm4 compositor, so that a full-screen
share does not freeze the screen.

Options:
  --name <NAME>  The window name to fix [default: cpt_frame_xcb_window]
  --dry-run      Log each change, but change nothing
  -h, --help     Print this help
  -V, --version  Print the version
";

/// The task that the command line asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Watch the X screen with this configuration.
    Run(Config),
    /// Print the help text.
    Help,
    /// Print the version.
    Version,
}

/// Returns the task for the arguments, without the program name.
///
/// Returns an error message for an unknown option or for a `--name` without a value.
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut config = Config {
        name: DEFAULT_NAME.to_owned(),
        dry_run: false,
    };
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--name" => {
                config.name = args
                    .next()
                    .filter(|name| !name.is_empty())
                    .ok_or("--name needs a value")?;
            }
            "--dry-run" => config.dry_run = true,
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Command::Run(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command, String> {
        parse_args(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn uses_the_zoom_frame_name_with_no_arguments() {
        assert_eq!(
            parse(&[]),
            Ok(Command::Run(Config {
                name: DEFAULT_NAME.to_owned(),
                dry_run: false,
            }))
        );
    }

    #[test]
    fn reads_a_custom_name_and_dry_run() {
        assert_eq!(
            parse(&["--name", "frame", "--dry-run"]),
            Ok(Command::Run(Config {
                name: "frame".to_owned(),
                dry_run: true,
            }))
        );
    }

    #[test]
    fn refuses_a_name_option_without_a_value() {
        assert_eq!(parse(&["--name"]), Err("--name needs a value".to_owned()));
    }

    #[test]
    fn refuses_an_empty_name() {
        assert_eq!(
            parse(&["--name", ""]),
            Err("--name needs a value".to_owned())
        );
    }

    #[test]
    fn refuses_an_unknown_argument() {
        assert_eq!(
            parse(&["--verbose"]),
            Err("unknown argument: --verbose".to_owned())
        );
    }

    #[test]
    fn returns_help_before_it_reads_the_other_arguments() {
        assert_eq!(parse(&["--help", "--verbose"]), Ok(Command::Help));
    }

    #[test]
    fn returns_the_version() {
        assert_eq!(parse(&["-V"]), Ok(Command::Version));
    }
}

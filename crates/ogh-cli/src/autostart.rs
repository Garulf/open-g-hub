//! `ogh autostart`: start `ogh-daemon` and `ogh-tray` at logon through the per-user Run key on
//! Windows.

use std::io::Write;
use std::path::Path;

use clap::Subcommand;

use crate::error::CliError;

#[cfg(not(windows))]
const NOT_WINDOWS: &str = "use the systemd user unit (or the NixOS/Home Manager module)";

#[cfg(windows)]
mod registry;

#[cfg(windows)]
const DAEMON_VALUE: &str = "open-g-hub";
#[cfg(windows)]
const TRAY_VALUE: &str = "open-g-hub-tray";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Subcommand)]
pub enum AutostartCommand {
    /// Start ogh-daemon.exe, and ogh-tray.exe if present, from ogh.exe's folder at logon
    On,
    /// Stop starting ogh-daemon and ogh-tray at logon
    Off,
    /// Print on or off for ogh-daemon and for ogh-tray
    Status,
}

/// The Run key value for a daemon at `daemon_exe`: the path in double quotes, so a
/// folder with spaces is not split into a program and arguments.
pub fn autostart_command(daemon_exe: &Path) -> String {
    format!("\"{}\"", daemon_exe.display())
}

/// The two lines `ogh autostart status` prints, without a trailing newline.
pub fn status_text(daemon: bool, tray: bool) -> String {
    let word = |on: bool| if on { "on" } else { "off" };
    format!("ogh-daemon: {}\nogh-tray: {}", word(daemon), word(tray))
}

#[cfg(not(windows))]
pub fn run(_command: AutostartCommand, out: &mut impl Write) -> Result<(), CliError> {
    writeln!(out, "{NOT_WINDOWS}")?;
    out.flush()?;
    Ok(())
}

#[cfg(windows)]
pub fn run(command: AutostartCommand, out: &mut impl Write) -> Result<(), CliError> {
    let text = match command {
        AutostartCommand::On => turn_on()?,
        AutostartCommand::Off => {
            registry::remove(DAEMON_VALUE)?;
            registry::remove(TRAY_VALUE)?;
            "auto-start off".to_string()
        }
        AutostartCommand::Status => status_text(
            registry::is_set(DAEMON_VALUE)?,
            registry::is_set(TRAY_VALUE)?,
        ),
    };
    writeln!(out, "{text}")?;
    out.flush()?;
    Ok(())
}

#[cfg(windows)]
fn turn_on() -> Result<String, CliError> {
    let own = std::env::current_exe()
        .map_err(|e| CliError::Local(format!("cannot find ogh.exe's own path: {e}")))?;
    let daemon_exe = own.with_file_name("ogh-daemon.exe");
    if !daemon_exe.is_file() {
        return Err(CliError::Local(format!(
            "{} does not exist; put ogh-daemon.exe next to ogh.exe",
            daemon_exe.display()
        )));
    }
    registry::set(DAEMON_VALUE, &autostart_command(&daemon_exe))?;

    let tray_exe = own.with_file_name("ogh-tray.exe");
    if tray_exe.is_file() {
        registry::set(TRAY_VALUE, &autostart_command(&tray_exe))?;
        Ok("auto-start on for ogh-daemon and ogh-tray".to_string())
    } else {
        registry::remove(TRAY_VALUE)?;
        Ok("auto-start on for ogh-daemon; ogh-tray.exe is not next to ogh.exe".to_string())
    }
}
